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

/// 检查目标 host 是否在 Pixiv 白名单内（允许 pixiv.net 及其所有子域名）。
pub fn is_allowed_host(host: &str) -> bool {
    let host = host.trim();
    if host.is_empty() {
        return false;
    }
    host == "pixiv.net" || host.ends_with(".pixiv.net")
}

/// 确保子 webview 已创建（幂等）。
///
/// 若未创建，则在主窗口添加子 webview（默认隐藏，等待前端同步 bounds 后显示），
/// 并启动后台 URL 变化轮询任务。
pub async fn ensure_browse_webview(app: &AppHandle, state: &AppState) -> Result<Webview, String> {
    if let Some(wv) = app.get_webview(BROWSE_LABEL) {
        return Ok(wv);
    }

    let main_window = app
        .get_window("main")
        .ok_or_else(|| "未找到主窗口".to_string())?;

    let profile_dir = state.paths.config_dir.join("browse-webview-profile");
    let initial_url: Url = "about:blank"
        .parse()
        .map_err(|e| format!("URL 解析失败: {e}"))?;

    let builder = WebviewBuilder::new(BROWSE_LABEL, WebviewUrl::External(initial_url))
        .data_directory(profile_dir)
        .on_navigation(|url| is_allowed_host(url.host_str().unwrap_or("")));

    let webview = main_window
        .add_child(
            builder,
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(1.0, 1.0),
        )
        .map_err(|e| format!("添加子 Webview 失败: {e}"))?;

    let _ = webview.hide();

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
        assert!(is_allowed_host("touch.pixiv.net"));

        assert!(!is_allowed_host("evil-pixiv.net"));
        assert!(!is_allowed_host("pixiv.net.evil.com"));
        assert!(!is_allowed_host("notpixiv.net"));
        assert!(!is_allowed_host("example.com"));
        assert!(!is_allowed_host(""));
        assert!(!is_allowed_host("   "));
    }
}
