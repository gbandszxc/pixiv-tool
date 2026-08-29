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
    log::info!("执行 browse_open");
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

    log::info!("设置浏览页边界: x={x}, y={y}, w={w}, h={h}");

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
        return Err("仅允许访问 pixiv 相关域名".to_string());
    }

    let Some(wv) = app.get_webview(BROWSE_LABEL) else {
        return Ok(());
    };

    tokio::task::spawn_blocking(move || {
        wv.navigate(parsed_url)
            .map_err(|e| format!("导航失败: {e}"))?;
        let _ = wv.show();
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("异步执行异常: {e}"))?
}

/// 从子 webview 提取并同步登录态到系统凭据存储。
#[tauri::command]
pub async fn browse_sync_login(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let wv = app
        .get_webview(BROWSE_LABEL)
        .ok_or_else(|| "未找到 Pixiv 浏览窗口".to_string())?;

    let wv_cookies = wv.clone();
    let raw_cookies = tokio::task::spawn_blocking(move || wv_cookies.cookies())
        .await
        .map_err(|e| format!("异步执行异常: {e}"))?
        .map_err(|e| format!("读取浏览器 Cookie 失败: {e}"))?;

    let mut cookies = crate::auth::webview_login::extract_pixiv_cookies_from_store(&raw_cookies);

    let phpsessid = match cookies.get("PHPSESSID").filter(|v| !v.is_empty()) {
        Some(s) => s.clone(),
        None => {
            // 若 Webview 中未检测到登录态，但客户端本地已保存登录态，则主动注入到 Webview 并刷新
            if let Ok(Some(saved)) = state.cookies.load() {
                if saved.get("PHPSESSID").is_some_and(|s| !s.is_empty()) {
                    let wv_inject = wv.clone();
                    let home_url: Url = crate::browse::BROWSE_HOME
                        .parse()
                        .map_err(|e| format!("Pixiv 主页 URL 解析失败: {e}"))?;
                    tokio::task::spawn_blocking(move || {
                        crate::browse::inject_cookies_to_webview(&wv_inject, &saved);
                        let _ = wv_inject.navigate(home_url);
                    })
                    .await
                    .map_err(|e| format!("异步执行异常: {e}"))?;

                    return Ok(serde_json::json!({
                        "status": "injected",
                    }));
                }
            }
            return Ok(serde_json::json!({
                "status": "no_session",
            }));
        }
    };

    match crate::pixiv::csrf::fetch_session_probe(&phpsessid).await {
        Ok(probe) => {
            cookies.insert("x-csrf-token".to_string(), probe.csrf_token);
            state
                .cookies
                .save(&cookies)
                .map_err(|e| format!("保存登录态失败: {e}"))?;

            Ok(serde_json::json!({
                "status": "success",
            }))
        }
        Err(crate::pixiv::csrf::ProbeError::Invalid(_)) => {
            // Webview 中提取到的 PHPSESSID 为未登录/匿名访客 session
            // 尝试读取客户端本地保存的真实登录态并注入到 Webview
            if let Ok(Some(saved)) = state.cookies.load() {
                if saved.get("PHPSESSID").is_some_and(|s| !s.is_empty()) {
                    let wv_inject = wv.clone();
                    let home_url: Url = crate::browse::BROWSE_HOME
                        .parse()
                        .map_err(|e| format!("Pixiv 主页 URL 解析失败: {e}"))?;
                    tokio::task::spawn_blocking(move || {
                        crate::browse::inject_cookies_to_webview(&wv_inject, &saved);
                        let _ = wv_inject.navigate(home_url);
                    })
                    .await
                    .map_err(|e| format!("异步执行异常: {e}"))?;

                    return Ok(serde_json::json!({
                        "status": "injected",
                    }));
                }
            }
            Ok(serde_json::json!({
                "status": "no_session",
            }))
        }
        Err(e) => Ok(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        })),
    }
}

/// 主动将客户端系统凭据中的登录态注入到子 Webview 并刷新。
#[tauri::command]
pub async fn browse_inject_login(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let Some(wv) = app.get_webview(BROWSE_LABEL) else {
        return Ok(false);
    };

    let cookies = match state.cookies.load() {
        Ok(Some(c)) if !c.is_empty() => c,
        _ => return Ok(false),
    };
    let home_url: Url = crate::browse::BROWSE_HOME
        .parse()
        .map_err(|e| format!("Pixiv 主页 URL 解析失败: {e}"))?;

    let wv_inject = wv.clone();
    tokio::task::spawn_blocking(move || {
        crate::browse::inject_cookies_to_webview(&wv_inject, &cookies);
        let _ = wv_inject.navigate(home_url);
        let _ = wv_inject.show();
    })
    .await
    .map_err(|e| format!("异步执行异常: {e}"))?;
    Ok(true)
}
