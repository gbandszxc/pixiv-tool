//! Pixiv 内嵌浏览页子 webview 管理。
//!
//! 负责子 webview 的生命周期、域白名单控制、URL 轮询与事件派发。

use std::sync::OnceLock;
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

/// 将本地 Cookie Map 注入到子 Webview 中。
///
/// 严格区分 HttpOnly 属性：仅 PHPSESSID 设置为 HttpOnly，其他 Cookie 允许 JS 读取；
/// 过滤非 Cookie 字段（如 x-csrf-token）；
pub fn inject_cookies_to_webview(
    webview: &Webview,
    cookies: &std::collections::HashMap<String, String>,
) {
    for (k, v) in cookies {
        if k.is_empty() || v.is_empty() || k == "x-csrf-token" {
            continue;
        }
        let is_http_only = k == "PHPSESSID";
        // 同时写入 .pixiv.net 与 pixiv.net，兼容 WebKit 各版本域名匹配机制
        for domain in [".pixiv.net", "pixiv.net"] {
            let cookie = tauri::webview::Cookie::build((k.clone(), v.clone()))
                .domain(domain)
                .path("/")
                .secure(true)
                .http_only(is_http_only)
                .build();
            let _ = webview.set_cookie(cookie);
        }
    }
    // 等待底层 WebKit Cookie Jar 跨进程异步落库
    std::thread::sleep(std::time::Duration::from_millis(200));
}

/// 确保子 webview 已创建（幂等）。
///
/// 若未创建，则在主窗口添加子 webview（默认隐藏，等待前端同步 bounds 后显示），
/// 并启动后台 URL 变化轮询任务。
pub async fn ensure_browse_webview(
    app: &AppHandle,
    state: &AppState,
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

    let profile_dir = state.paths.config_dir.join("browse-webview-profile");
    let blank_url: Url = "about:blank".parse().unwrap();
    let home_url: Url = BROWSE_HOME
        .parse()
        .map_err(|e| format!("Pixiv 主页 URL 解析失败: {e}"))?;

    log::info!("创建 Pixiv 浏览页 Webview，Profile 目录: {:?}", profile_dir);

    let builder = WebviewBuilder::new(BROWSE_LABEL, WebviewUrl::External(blank_url))
        .data_directory(profile_dir)
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
        .on_navigation(|url| {
            let host = url.host_str().unwrap_or("");
            let allowed = is_allowed_host(host);
            if !allowed {
                log::warn!("拦截非白名单域名导航: {url}");
            }
            allowed
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

    let cookies = match state.cookies.load() {
        Ok(c) => c,
        Err(err) => {
            log::warn!("读取本地登录态失败: {err}");
            None
        }
    };

    let wv_init = webview.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(map) = cookies {
            if !map.is_empty() {
                log::info!("初始化注入本地已保存的 Cookie: {} 个", map.len());
                inject_cookies_to_webview(&wv_init, &map);
            }
        }
        let _ = wv_init.navigate(home_url);
        let _ = wv_init.show();
    })
    .await
    .map_err(|e| format!("异步执行异常: {e}"))?;

    log::info!("Pixiv 浏览页 Webview 创建完成并导航至首页");

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
