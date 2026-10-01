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
