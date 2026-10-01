//! 应用级命令。
//!
//! 退出确认流程：Rust 拦截关闭/退出请求（prevent_close / prevent_exit）后
//! emit `app://confirm-exit`，前端弹原生 dialog 确认框，确认后回调本命令退出。

/// 用户在前端确认退出后调用；`exit(0)` 走 code=Some 的退出路径，
/// 不再触发 `ExitRequested`（prevent_exit 只拦 code=None），无死循环。
#[tauri::command]
pub fn app_exit(app: tauri::AppHandle) {
    app.exit(0);
}

/// 显示菜单栏并进入菜单循环（前端 Alt 松开时调用）。
///
/// Windows 上菜单栏默认隐藏（装配期 `menu_bar::install`），退出菜单循环后
/// 由 `menu_bar` 的子类化过程自动隐藏。macOS 菜单在系统顶栏恒显，此处 no-op。
#[tauri::command]
pub fn app_menu_show(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    crate::menu_bar::show(&window).map_err(|e| format!("显示菜单栏失败: {e}"))?;
    #[cfg(not(windows))]
    drop(window);
    Ok(())
}
