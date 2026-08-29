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

/// Pixiv 主页地址。
pub const BROWSE_HOME: &str = "https://www.pixiv.net/";

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
/// 将本地 Cookie Map 注入到子 Webview 中，返回成功写入的个数。
///
/// 前提：目标域页面已完成过至少一次加载（WKWebView 网络进程会话已建立），
/// 注入后需 navigate 刷新才会携带新登录态——load → set → reload 是
/// WKWebView cookie 注入的可靠顺序；首次导航前批量注入不可靠（访客加载后
/// pixiv 会用访客 PHPSESSID 覆盖同名登录 cookie）。
///
/// 仅 PHPSESSID 设置为 HttpOnly，其他 Cookie 允许 JS 读取；
/// 过滤非 Cookie 字段（如 x-csrf-token）；只写 .pixiv.net 单域。
/// set_cookie 自身阻塞等待 WebKit completion 回调，无需额外 sleep。
pub fn inject_cookies_to_webview(
    webview: &Webview,
    cookies: &std::collections::HashMap<String, String>,
) -> usize {
    // 先清空 store 中现存 pixiv 域 cookie：历史双域注入残留与 pixiv 访客
    // Set-Cookie 会与登录版同名并存，请求与回读可能取到旧值（访客版）
    if let Ok(existing) = webview.cookies() {
        for c in existing {
            let domain = c.domain().unwrap_or("").trim_start_matches('.');
            let is_pixiv = domain == "pixiv.net" || domain.ends_with(".pixiv.net");
            if is_pixiv {
                let name = c.name().to_string();
                if let Err(err) = webview.delete_cookie(c) {
                    log::warn!("清理旧 Cookie 失败: {name} ({err})");
                }
            }
        }
    }

    let mut count = 0usize;
    for (k, v) in cookies {
        if k.is_empty() || v.is_empty() || k == "x-csrf-token" {
            continue;
        }
        let cookie = tauri::webview::Cookie::build((k.clone(), v.clone()))
            .domain(".pixiv.net")
            .path("/")
            .secure(true)
            .http_only(k == "PHPSESSID")
            .build();
        match webview.set_cookie(cookie) {
            Ok(()) => count += 1,
            Err(err) => log::warn!("注入 Cookie 失败: {k} ({err})"),
        }
    }
    count
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
        if !saved.get("PHPSESSID").is_some_and(|v| !v.is_empty()) {
            log::info!("本地登录态缺 PHPSESSID，跳过自动注入");
            return;
        }

        let wv_inject = wv.clone();
        let count = tauri::async_runtime::spawn_blocking(move || {
            let n = inject_cookies_to_webview(&wv_inject, &saved);
            let home: Url = BROWSE_HOME.parse().expect("Pixiv 主页 URL 常量必然合法");
            let _ = wv_inject.navigate(home);
            n
        })
        .await
        .unwrap_or(0);
        log::info!("首次加载完成，自动注入登录态 {count} 个 Cookie 并刷新首页");
    });
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
    tokio::task::spawn_blocking(move || {
        let _ = wv_init.show();
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
                let current_url =
                    tokio::task::spawn_blocking(move || wv.url().map(|u| u.to_string()))
                        .await
                        .ok()
                        .and_then(|r| r.ok());

                if let Some(url_str) = current_url {
                    if url_str != last_url {
                        last_url = url_str.clone();
                        let _ = app_handle
                            .emit("browse://url-changed", serde_json::json!({ "url": url_str }));
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
}
