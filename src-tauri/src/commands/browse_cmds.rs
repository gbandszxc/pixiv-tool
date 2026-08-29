//! 内嵌浏览命令。

use tauri::{AppHandle, Manager, State, Url};

use crate::browse::{BROWSE_LABEL, ensure_browse_webview, is_allowed_host};
use crate::state::AppState;

/// 打开 Pixiv 浏览页（确保子 webview 创建）。
#[tauri::command]
pub async fn browse_open(
    app: AppHandle,
    state: State<'_, AppState>,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
) -> Result<(), String> {
    let px = x.unwrap_or(180.0);
    let py = y.unwrap_or(42.0);
    let pw = w.unwrap_or(800.0);
    let ph = h.unwrap_or(600.0);
    log::info!("执行 browse_open, 目标区域: ({px}, {py}, {pw}, {ph})");
    let wv = ensure_browse_webview(&app, &state, px, py, pw, ph).await?;
    if pw > 0.0 && ph > 0.0 {
        let rect = tauri::Rect {
            position: tauri::Position::Logical(tauri::LogicalPosition::new(px, py)),
            size: tauri::Size::Logical(tauri::LogicalSize::new(pw, ph)),
        };
        let _ = wv.set_bounds(rect);
        let _ = wv.show();
    }
    Ok(())
}

/// 将客户端保存的登录态注入子 Webview 并刷新首页（load 后注入，可靠路径）。
///
/// 返回是否执行了注入（本地存在非空 PHPSESSID 才注入）。
async fn inject_saved_and_reload(
    wv: &tauri::Webview,
    cookies: &crate::cookies::CookieStore,
) -> Result<bool, String> {
    let Ok(Some(saved)) = cookies.load() else {
        return Ok(false);
    };
    if !saved.get("PHPSESSID").is_some_and(|s| !s.is_empty()) {
        return Ok(false);
    }
    let home_url: Url = crate::browse::BROWSE_HOME
        .parse()
        .map_err(|e| format!("Pixiv 主页 URL 解析失败: {e}"))?;

    let wv_inject = wv.clone();
    tokio::task::spawn_blocking(move || {
        let n = crate::browse::inject_cookies_to_webview(&wv_inject, &saved);
        let _ = wv_inject.navigate(home_url);
        log::info!("同步登录回退注入 {n} 个 Cookie 并刷新首页");
    })
    .await
    .map_err(|e| format!("异步执行异常: {e}"))?;
    Ok(true)
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

    let scale_factor = app
        .get_window("main")
        .and_then(|win| win.scale_factor().ok())
        .unwrap_or(1.0);

    let rect = tauri::Rect {
        position: tauri::Position::Logical(tauri::LogicalPosition::new(x, y)),
        size: tauri::Size::Logical(tauri::LogicalSize::new(w, h)),
    };

    tokio::task::spawn_blocking(move || {
        wv.set_bounds(rect)
            .map_err(|e| format!("设置 bounds 失败: {e}"))?;
        // 回读校准：确认 setFrame 实际生效（bounds 错位问题的诊断信号）
        match wv.bounds() {
            Ok(rb) => {
                let pos = rb.position.to_logical::<f64>(scale_factor);
                let size = rb.size.to_logical::<f64>(scale_factor);
                log::info!(
                    "边界同步: sent=({x},{y},{w},{h}) readback=({:.0},{:.0},{:.0},{:.0})",
                    pos.x,
                    pos.y,
                    size.width,
                    size.height
                );
            }
            Err(e) => log::warn!("边界回读失败: {e}"),
        }
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
            // Webview 中无登录态：尝试将本地保存的登录态注入并刷新
            if inject_saved_and_reload(&wv, &state.cookies).await? {
                return Ok(serde_json::json!({
                    "status": "injected",
                }));
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
            // Webview 中提取到的 PHPSESSID 为未登录/匿名访客 session：
            // 注入本地保存的真实登录态并刷新
            if inject_saved_and_reload(&wv, &state.cookies).await? {
                return Ok(serde_json::json!({
                    "status": "injected",
                }));
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
    inject_saved_and_reload(&wv, &state.cookies).await
}
