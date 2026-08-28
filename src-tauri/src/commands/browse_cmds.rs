//! 内嵌浏览命令。

use tauri::{AppHandle, Manager, State, Url};

use crate::browse::{BROWSE_LABEL, ensure_browse_webview, is_allowed_host};
use crate::state::AppState;

/// 打开 Pixiv 浏览页（确保子 webview 创建）。
#[tauri::command]
pub async fn browse_open(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    ensure_browse_webview(&app, &state).await?;
    Ok(())
}

/// 同步子 webview 的边界位置与尺寸。
///
/// 前端根据占位 DOM 的 bounding rect 传入逻辑坐标。
/// 传入宽高 <= 0 时直接忽略。
#[tauri::command]
pub async fn browse_set_bounds(
    app: AppHandle,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), String> {
    if w <= 0.0 || h <= 0.0 {
        return Ok(());
    }

    let Some(wv) = app.get_webview(BROWSE_LABEL) else {
        return Ok(());
    };

    let rect = tauri::Rect {
        position: tauri::Position::Logical(tauri::LogicalPosition::new(x, y)),
        size: tauri::Size::Logical(tauri::LogicalSize::new(w, h)),
    };

    tokio::task::spawn_blocking(move || {
        wv.set_bounds(rect)
            .map_err(|e| format!("设置 bounds 失败: {e}"))?;
        let _ = wv.show();
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("异步执行异常: {e}"))?
}

/// 隐藏子 webview。
#[tauri::command]
pub async fn browse_hide(app: AppHandle) -> Result<(), String> {
    let Some(wv) = app.get_webview(BROWSE_LABEL) else {
        return Ok(());
    };

    tokio::task::spawn_blocking(move || {
        let _ = wv.hide();
    })
    .await
    .map_err(|e| format!("异步执行异常: {e}"))
}

/// 显示子 webview。
#[tauri::command]
pub async fn browse_show(app: AppHandle) -> Result<(), String> {
    let Some(wv) = app.get_webview(BROWSE_LABEL) else {
        return Ok(());
    };

    tokio::task::spawn_blocking(move || {
        let _ = wv.show();
    })
    .await
    .map_err(|e| format!("异步执行异常: {e}"))
}

/// 导航子 webview 到指定 URL。
///
/// 校验目标域名是否在白名单内，非白名单域名拒绝导航。
#[tauri::command]
pub async fn browse_navigate(app: AppHandle, url: String) -> Result<(), String> {
    let parsed_url: Url = url.parse().map_err(|e| format!("URL 无效: {e}"))?;
    let host = parsed_url.host_str().unwrap_or("");
    if !is_allowed_host(host) {
        return Err("仅允许访问 pixiv.net 域名".to_string());
    }

    let Some(wv) = app.get_webview(BROWSE_LABEL) else {
        return Ok(());
    };

    tokio::task::spawn_blocking(move || {
        wv.navigate(parsed_url)
            .map_err(|e| format!("导航失败: {e}"))
    })
    .await
    .map_err(|e| format!("异步执行异常: {e}"))?
}
