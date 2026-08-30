//! Pixiv 内嵌浏览页子 webview 管理。
//!
//! 负责子 webview 的生命周期、域白名单控制、URL 轮询与事件派发。

use std::sync::LazyLock;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewBuilder,
    WebviewUrl,
};

use crate::state::AppState;

/// 子 webview 标识符。
pub const BROWSE_LABEL: &str = "pixiv-browse";

/// 子 webview 底色：深色取应用深色表面（DESIGN.md dark surface #101014），
/// 浅色纯白。用于消除深色主题下 webview 加载期白闪与 overscroll 露白。
pub fn webview_surface_color(dark: bool) -> tauri::window::Color {
    if dark {
        tauri::window::Color(0x10, 0x10, 0x14, 0xFF)
    } else {
        tauri::window::Color(0xFF, 0xFF, 0xFF, 0xFF)
    }
}

/// Pixiv 主页地址。
pub const BROWSE_HOME: &str = "https://www.pixiv.net/";

#[cfg(windows)]
const WEBVIEW2_COOKIE_DOMAIN: &str = ".pixiv.net";
#[cfg(windows)]
const WEBVIEW2_COOKIE_TTL_SECS: i64 = 90 * 24 * 60 * 60;

#[cfg(windows)]
fn webview2_cookie_expires_at(now_unix: i64) -> f64 {
    now_unix.saturating_add(WEBVIEW2_COOKIE_TTL_SECS) as f64
}

static POLL_STARTED: OnceLock<()> = OnceLock::new();

/// 自动注入登录态是否已尝试过（每个 webview 进程实例只尝试一次）。
static AUTO_INJECT_DONE: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));

/// 检查目标 host 是否在 Pixiv 白名单内（允许 pixiv、CDN、静态资源、第三方验证与空白页）。
pub fn is_allowed_host(host: &str) -> bool {
    let host = host.trim().to_lowercase();
    if host.is_empty() || host == "about:blank" {
        return true;
    }
    host == "pixiv.net"
        || host.ends_with(".pixiv.net")
        || host == "pximg.net"
        || host.ends_with(".pximg.net")
        || host == "pixiv.org"
        || host.ends_with(".pixiv.org")
        || host == "fanbox.cc"
        || host.ends_with(".fanbox.cc")
        || host == "booth.pm"
        || host.ends_with(".booth.pm")
        || host == "recaptcha.net"
        || host.ends_with(".recaptcha.net")
        || host == "google.com"
        || host.ends_with(".google.com")
        || host == "gstatic.com"
        || host.ends_with(".gstatic.com")
}

#[cfg(any(not(windows), test))]
fn is_pixiv_session_cookie(cookie: &tauri::webview::Cookie<'_>) -> bool {
    let domain = cookie.domain().unwrap_or_default().trim_start_matches('.');
    cookie.name() == "PHPSESSID" && (domain == "pixiv.net" || domain.ends_with(".pixiv.net"))
}

/// 清掉内嵌 Pixiv 的旧账号会话，保留 device_token / Cloudflare 等设备态。
/// 返回成功删除的会话 Cookie 数。
pub fn clear_session_from_webview(webview: &Webview) -> usize {
    #[cfg(not(windows))]
    {
        let Ok(cookies) = webview.cookies() else {
            return 0;
        };
        cookies
            .into_iter()
            .filter(is_pixiv_session_cookie)
            .filter(|cookie| webview.delete_cookie(cookie.clone()).is_ok())
            .count()
    }

    #[cfg(windows)]
    {
        use std::sync::mpsc;
        use std::time::Duration;
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2;
        use windows_core::{HSTRING, Interface};

        let (tx, rx) = mpsc::sync_channel(1);
        if webview
            .with_webview(move |platform| {
                let result = unsafe {
                    (|| -> windows_core::Result<()> {
                        let core = platform.controller().CoreWebView2()?;
                        let core: ICoreWebView2_2 = core.cast()?;
                        let manager = core.CookieManager()?;
                        manager.DeleteCookies(
                            &HSTRING::from("PHPSESSID"),
                            &HSTRING::from(BROWSE_HOME),
                        )?;
                        Ok(())
                    })()
                };
                let _ = tx.send(result);
            })
            .is_err()
        {
            return 0;
        }
        matches!(rx.recv_timeout(Duration::from_secs(5)), Ok(Ok(()))) as usize
    }
}
/// 将本地 Cookie Map 注入到子 Webview 中，返回成功写入的个数。
///
/// Windows（WebView2）：绕过 wry `set_cookie`，直接调用原生 CookieManager。
/// cookie crate 的 `domain()` 会剥掉前导点，使 `.pixiv.net` 退化为
/// host-only `pixiv.net`；`document.cookie` 又无法覆盖 HttpOnly 访客会话。
/// 原生 API 保留前导点，并在写入前删除 www.pixiv.net 可见的同名旧条目。
///
/// macOS（WKWebView）：保留 `set_cookie` 路径。NSHTTPCookie 对无点域仍做
/// 子域匹配，原三域覆盖策略有效（历史实证）。
///
/// 前提：目标域页面已完成过至少一次加载（网络进程会话已建立），
/// 注入后需 navigate 刷新才会携带新登录态——load → set → reload 是
/// cookie 注入的可靠顺序；首次导航前批量注入不可靠（访客加载后
/// pixiv 会用访客 PHPSESSID 覆盖同名登录 cookie）。
///
/// 过滤非 Cookie 字段（如 x-csrf-token）与 CF 凭证（绑定客户端指纹）。
pub fn inject_cookies_to_webview(
    webview: &Webview,
    cookies: &std::collections::HashMap<String, String>,
) -> usize {
    // Cloudflare 凭证绑定获取它的客户端 TLS/浏览器指纹：keyring 里的
    // cf_clearance 等来自 Chrome（CDP 登录），注入 webview 无效且会挤掉
    // webview 自己协商的凭证，导致 reload 被 CF 判为访客。注入一律跳过。
    fn is_cf_cookie(name: &str) -> bool {
        matches!(name, "cf_clearance" | "__cf_bm" | "__cf_ob")
    }

    let injectable: Vec<(String, String)> = cookies
        .iter()
        .filter(|(k, v)| {
            !k.is_empty() && !v.is_empty() && k.as_str() != "x-csrf-token" && !is_cf_cookie(k)
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    #[cfg(not(windows))]
    {
        clear_session_from_webview(webview);
        // 先按原始 cookie 对象删除全部旧 PHPSESSID，再写三种域形式覆盖
        // pixiv 服务器的 host-only 与历史 domain 版本；避免切换时同名旧会话
        // 因 Cookie 排序在前而继续生效。
        let mut count = 0usize;
        for (k, v) in &injectable {
            // 显式 Expires：WKHTTPCookieStore 对无过期时间的 session cookie
            // 存在不随后续请求发送的已知行为；真实过期以 keyring 真相源为准
            let expires = tauri::webview::cookie::time::OffsetDateTime::now_utc()
                + tauri::webview::cookie::time::Duration::days(90);
            for domain in [".pixiv.net", "pixiv.net", "www.pixiv.net"] {
                let cookie = tauri::webview::Cookie::build((k.clone(), v.clone()))
                    .domain(domain)
                    .path("/")
                    .secure(true)
                    .http_only(k.as_str() == "PHPSESSID")
                    .expires(expires)
                    .build();
                match webview.set_cookie(cookie) {
                    Ok(()) => count += 1,
                    Err(err) => log::warn!("注入 Cookie 失败: {k}@{domain} ({err})"),
                }
            }
        }

        // 回读校验：确认 cookie 真实落库（只打名字与域，不打值）
        if let Ok(after) = webview.cookies() {
            let pixiv: Vec<String> = after
                .iter()
                .filter(|c| {
                    let d = c.domain().unwrap_or("").trim_start_matches('.');
                    d == "pixiv.net" || d.ends_with(".pixiv.net")
                })
                .map(|c| format!("{}@{}", c.name(), c.domain().unwrap_or("?")))
                .collect();
            log::info!(
                "注入后回读: 写入 {count} 个, store 现有 pixiv 域 cookie {} 个: {pixiv:?}",
                pixiv.len()
            );
        }
        count
    }

    #[cfg(windows)]
    {
        use std::sync::mpsc;
        use std::time::Duration;
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2;
        use windows_core::{HSTRING, Interface};

        let (tx, rx) = mpsc::sync_channel(1);
        if let Err(err) = webview.with_webview(move |platform| {
            let result = unsafe {
                (|| -> windows_core::Result<usize> {
                    let core = platform.controller().CoreWebView2()?;
                    let core: ICoreWebView2_2 = core.cast()?;
                    let manager = core.CookieManager()?;
                    let uri = HSTRING::from(BROWSE_HOME);
                    let domain = HSTRING::from(WEBVIEW2_COOKIE_DOMAIN);
                    let path = HSTRING::from("/");
                    let expires = webview2_cookie_expires_at(
                        tauri::webview::cookie::time::OffsetDateTime::now_utc().unix_timestamp(),
                    );

                    for (name, value) in &injectable {
                        let http_only = name == "PHPSESSID";
                        let name = HSTRING::from(name.as_str());
                        manager.DeleteCookies(&name, &uri)?;
                        let cookie = manager.CreateCookie(
                            &name,
                            &HSTRING::from(value.as_str()),
                            &domain,
                            &path,
                        )?;
                        cookie.SetIsSecure(true)?;
                        cookie.SetIsHttpOnly(http_only)?;
                        cookie.SetExpires(expires)?;
                        manager.AddOrUpdateCookie(&cookie)?;
                    }
                    Ok(injectable.len())
                })()
            };
            let _ = tx.send(result.map_err(|e| e.to_string()));
        }) {
            log::warn!("派发 WebView2 Cookie 注入失败: {err}");
            return 0;
        }

        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(count)) => count,
            Ok(Err(err)) => {
                log::warn!("WebView2 Cookie 注入失败: {err}");
                0
            }
            Err(err) => {
                log::warn!("等待 WebView2 Cookie 注入完成失败: {err}");
                0
            }
        }
    }
}

/// 首次加载完成后自动注入本地登录态并刷新（幂等：每个进程实例只尝试一次）。
///
/// 由 WebviewBuilder::on_page_load(Finished) 触发；实际注入放到
/// async_runtime 的阻塞线程执行（set_cookie 阻塞等待 completion，不卡主线程）。
fn auto_inject_on_first_load(app: &AppHandle, wv: &Webview, url: &Url) {
    let host = url.host_str().unwrap_or("");
    let is_pixiv_main = host == "pixiv.net" || host.ends_with(".pixiv.net");
    if !is_pixiv_main || AUTO_INJECT_DONE.swap(true, Ordering::SeqCst) {
        return;
    }

    let app = app.clone();
    let wv = wv.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let saved = match state.cookies.load() {
            Ok(Some(c)) => c,
            Ok(None) => {
                log::info!("本地无登录态，跳过自动注入");
                return;
            }
            Err(e) => {
                log::warn!("读取本地登录态失败，跳过自动注入: {e}");
                return;
            }
        };
        if saved.get("PHPSESSID").is_none_or(|v| v.is_empty()) {
            log::info!("本地登录态缺 PHPSESSID，跳过自动注入");
            return;
        }

        // 只读取当前 URL 真正会携带的 Cookie：Windows 不能用 cookies()，
        // 否则 host-only pixiv.net 条目会被误判为可发送到 www.pixiv.net。
        // 已有会话有效时不注入、不刷新；过期或失效才回填 App 登录态。
        let wv_probe = wv.clone();
        let raw = tauri::async_runtime::spawn_blocking(move || {
            #[cfg(windows)]
            {
                let home: Url = BROWSE_HOME.parse().expect("Pixiv 主页 URL 常量必然合法");
                wv_probe.cookies_for_url(home).ok()
            }
            #[cfg(not(windows))]
            {
                wv_probe.cookies().ok()
            }
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
        let existing = crate::auth::webview_login::extract_pixiv_cookies_from_store(&raw);
        if let Some(sid) = existing.get("PHPSESSID").filter(|v| !v.is_empty()) {
            match crate::pixiv::csrf::fetch_session_probe(sid).await {
                Ok(_) => {
                    log::info!("webview 已有有效登录态，跳过注入与刷新");
                    return;
                }
                Err(e) => {
                    log::info!("webview 登录态已失效（{e}），重新注入");
                }
            }
        }

        let wv_inject = wv.clone();
        let count = tauri::async_runtime::spawn_blocking(move || {
            let count = inject_cookies_to_webview(&wv_inject, &saved);
            #[cfg(not(windows))]
            {
                let home: Url = BROWSE_HOME.parse().expect("Pixiv 主页 URL 常量必然合法");
                let _ = wv_inject.navigate(home);
            }
            count
        })
        .await
        .unwrap_or(0);
        #[cfg(windows)]
        {
            let home: Url = BROWSE_HOME.parse().expect("Pixiv 主页 URL 常量必然合法");
            let _ = wv.navigate(home);
        }
        log::info!("首次加载完成，自动注入登录态 {count} 个 Cookie 并刷新首页");
    });
}

/// 深色补丁 JS：pixiv 登录深色模式下 novel/show.php 的正文与评论区卡片
/// 写死白底（styled-components 组件未适配深色，站内 bug，浏览器复现一致）。
/// 启发式定位：非透明、近纯白、大面积（≥400×160）的 div/section 染成
/// pixiv 深色卡片色 #1f1f1f，深色文字分级翻浅（正文 #f5f5f5 / 次级 #d6d6dc，
/// 均取自 pixiv 深色体系实测值）。MutationObserver 常驻补染（200ms debounce）
/// + 16 轮初始轮询，覆盖 hydration 延迟与二次渲染；脚本幂等，重复注入无害。
// ponytail: 启发式补丁依赖 pixiv 深色体系与白卡片特征，pixiv 改版后若
// 失效/误染，调整面积与亮度阈值即可；切回浅色主题不回滚，刷新页面即恢复。
const DARK_NOVEL_PATCH_JS: &str = r#"(function(){
    // 同时经 initialization_script（document start，消除闪白）与
    // on_page_load(Finished) eval（兜底）注入；__dkPatchInstalled 保证幂等。
    // 深色判定读页面真实 prefers-color-scheme（WKWebView effective 外观）。
    // 不能用窗口 theme()：未显式 set_theme（auto）时 tao 记录默认 Light，
    // 与实际跟随系统的深色外观不符（release 白块复现根因）
    if (!/novel\/show\.php/.test(location.href)) return;
    if (!window.matchMedia('(prefers-color-scheme: dark)').matches) return;
    if (window.__dkPatchInstalled) return;
    window.__dkPatchInstalled = true;
    function lumaOf(c){ var m=/^rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(c); return m ? (0.299*m[1]+0.587*m[2]+0.114*m[3])/255 : 1; }
    function fix(){
        document.querySelectorAll('div,section').forEach(function(el){
            if (el.dataset.dkPatched) return;
            var cs = getComputedStyle(el);
            var bg = cs.backgroundColor;
            if (bg.indexOf('rgba') === 0) return;
            if (lumaOf(bg) < 0.92) return;
            var r = el.getBoundingClientRect();
            if (r.width < 400 || r.height < 160) return;
            el.dataset.dkPatched = '1';
            el.style.backgroundColor = '#1f1f1f';
            if (lumaOf(cs.color) < 0.5) el.style.color = '#f5f5f5';
            el.querySelectorAll('*').forEach(function(ch){
                if (ch.dataset.dkText) return;
                var l2 = lumaOf(getComputedStyle(ch).color);
                if (l2 < 0.2) { ch.dataset.dkText='1'; ch.style.color = '#f5f5f5'; }
                else if (l2 < 0.5) { ch.dataset.dkText='1'; ch.style.color = '#d6d6dc'; }
            });
        });
    }
    var t = null;
    function schedule(){
        if (t) return;
        t = setTimeout(function(){ t = null; fix(); }, 200);
    }
    for (var i = 0; i < 16; i++) setTimeout(fix, i * 500);
    // document start 时 documentElement 可能尚未解析，轮询挂载 observer
    function install(){
        if (document.documentElement) {
            new MutationObserver(schedule).observe(document.documentElement, {childList: true, subtree: true});
        } else {
            setTimeout(install, 10);
        }
    }
    install();
})()"#;

/// novel/show.php 页面深色补丁兜底：Finished 时 eval 注入同一脚本（幂等）。
/// initialization_script 理论上在 document start 即生效，此路径保证
/// 任何情况下补丁最终注入。
fn inject_dark_page_patch(wv: &Webview, url: &Url) {
    if !url.path().starts_with("/novel/show.php") {
        return;
    }
    if let Err(e) = wv.eval(DARK_NOVEL_PATCH_JS) {
        log::warn!("深色补丁注入失败: {e}");
    }
}

/// 确保子 webview 已创建（幂等）。
///
/// 直接以 Pixiv 首页创建并显示；登录态注入不在创建时做——首次加载完成后
/// 由 `auto_inject_on_first_load`（on_page_load Finished）注入并刷新。
/// 并启动后台 URL 变化轮询任务。
pub async fn ensure_browse_webview(
    app: &AppHandle,
    _state: &AppState,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<Webview, String> {
    if let Some(wv) = app.get_webview(BROWSE_LABEL) {
        return Ok(wv);
    }

    let main_window = app
        .get_window("main")
        .ok_or_else(|| "未找到主窗口".to_string())?;

    let home_url: Url = BROWSE_HOME
        .parse()
        .map_err(|e| format!("Pixiv 主页 URL 解析失败: {e}"))?;

    log::info!("创建 Pixiv 浏览页 Webview");

    let app_for_load = app.clone();
    let builder = WebviewBuilder::new(BROWSE_LABEL, WebviewUrl::External(home_url))
        .initialization_script(DARK_NOVEL_PATCH_JS)
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
        .on_navigation(|url| {
            let host = url.host_str().unwrap_or("");
            let allowed = is_allowed_host(host);
            if !allowed {
                log::warn!("拦截非白名单域名导航: {url}");
            }
            allowed
        })
        .on_page_load(move |wv, payload| {
            if let tauri::webview::PageLoadEvent::Finished = payload.event() {
                auto_inject_on_first_load(&app_for_load, &wv, payload.url());
                inject_dark_page_patch(&wv, payload.url());
            }
        });

    let initial_pos = if x >= 0.0 && y >= 0.0 {
        LogicalPosition::new(x, y)
    } else {
        LogicalPosition::new(180.0, 42.0)
    };
    let initial_size = if w > 0.0 && h > 0.0 {
        LogicalSize::new(w, h)
    } else {
        LogicalSize::new(800.0, 600.0)
    };

    let webview = main_window
        .add_child(builder, initial_pos, initial_size)
        .map_err(|e| format!("添加子 Webview 失败: {e}"))?;

    let wv_init = webview.clone();
    // 按窗口当前生效主题设底色：窗口外观在 setup 已按持久化主题预设，
    // 这里读取的是 webview 首次加载时的真实 prefers-color-scheme
    let init_dark = main_window
        .theme()
        .map(|t| t == tauri::Theme::Dark)
        .unwrap_or(false);
    tokio::task::spawn_blocking(move || {
        let _ = wv_init.show();
        let _ = wv_init.set_background_color(Some(webview_surface_color(init_dark)));
    })
    .await
    .map_err(|e| format!("异步执行异常: {e}"))?;

    log::info!("Pixiv 浏览页 Webview 创建完成，首次加载后将注入登录态");

    if POLL_STARTED.set(()).is_ok() {
        let app_handle = app.clone();
        tokio::spawn(async move {
            let mut last_url = String::new();
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                let Some(wv) = app_handle.get_webview(BROWSE_LABEL) else {
                    continue;
                };
                let probe = tokio::task::spawn_blocking(move || {
                    let url = wv.url().ok().map(|u| u.to_string());
                    (wv, url)
                })
                .await
                .ok();

                if let Some((wv, Some(url_str))) = probe {
                    if url_str != last_url {
                        last_url = url_str.clone();
                        let _ = app_handle.emit(
                            "browse://url-changed",
                            serde_json::json!({ "url": url_str }),
                        );
                        // SPA 路由（pushState）不触发 on_page_load，也不重新执行
                        // initialization_script——URL 轮询是唯一可靠覆盖 SPA 跳转的
                        // 注入通道；脚本幂等，整页导航场景重复注入无副作用
                        if url_str.contains("/novel/show.php") {
                            let _ = wv.eval(DARK_NOVEL_PATCH_JS);
                        }
                    }
                }
            }
        });
    }

    Ok(webview)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_allowed_host() {
        assert!(is_allowed_host("pixiv.net"));
        assert!(is_allowed_host("www.pixiv.net"));
        assert!(is_allowed_host("accounts.pixiv.net"));
        assert!(is_allowed_host("sketch.pixiv.net"));
        assert!(is_allowed_host("s.pximg.net"));
        assert!(is_allowed_host("i.pximg.net"));
        assert!(is_allowed_host("fanbox.cc"));
        assert!(is_allowed_host("booth.pm"));
        assert!(is_allowed_host(""));
        assert!(is_allowed_host("about:blank"));

        assert!(!is_allowed_host("evil-pixiv.net"));
        assert!(!is_allowed_host("pixiv.net.evil.com"));
        assert!(!is_allowed_host("notpixiv.net"));
        assert!(!is_allowed_host("example.com"));
        assert!(!is_allowed_host("evil-pximg.net"));
    }

    #[test]
    fn identifies_only_pixiv_session_cookies() {
        let session = tauri::webview::Cookie::build(("PHPSESSID", "old"))
            .domain(".pixiv.net")
            .path("/")
            .build();
        let device = tauri::webview::Cookie::build(("device_token", "keep"))
            .domain(".pixiv.net")
            .path("/")
            .build();
        let foreign = tauri::webview::Cookie::build(("PHPSESSID", "keep"))
            .domain("example.com")
            .path("/")
            .build();

        assert!(is_pixiv_session_cookie(&session));
        assert!(!is_pixiv_session_cookie(&device));
        assert!(!is_pixiv_session_cookie(&foreign));
    }

    #[cfg(windows)]
    #[test]
    fn webview2_cookie_domain_must_cover_pixiv_subdomains() {
        let cookie = tauri::webview::Cookie::build(("PHPSESSID", "test-session"))
            .domain(".pixiv.net")
            .path("/")
            .http_only(true)
            .build();

        assert_eq!(cookie.domain(), Some("pixiv.net"));
        assert_ne!(cookie.domain(), Some(WEBVIEW2_COOKIE_DOMAIN));
        assert_eq!(WEBVIEW2_COOKIE_DOMAIN, ".pixiv.net");
    }

    #[cfg(windows)]
    #[test]
    fn webview2_cookie_is_persistent_for_90_days() {
        assert_eq!(webview2_cookie_expires_at(1_000), 7_777_000.0);
    }
}
