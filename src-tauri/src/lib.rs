//! pixiv-tool Tauri 2 桌面应用（lib 形态，main.rs 只做薄壳）。
//!
//! 本文件是**装配层**，一次写全，后续 phase 只填充桩模块内容、不改本文件：
//! - 插件：tauri-plugin-log（文件 + dev stdout）、tauri-plugin-dialog（退出确认/目录选择）
//! - setup：AppPaths（dev/release 分平台）→ Settings 加载 → Db 打开 → AppState
//! - 全部命令注册（见 invoke_handler，与 commands/ 一一对应）
//! - 主窗口关闭确认（读 settings.language 决定中英文文案）

pub mod auth;
pub mod commands;
pub mod browse;
pub mod cookies;
pub mod core;
pub mod db;
pub mod logging;
pub mod paths;
pub mod pixiv;
pub mod platform;
pub mod settings;
pub mod state;

use tauri::Manager;

use crate::db::Db;
use crate::paths::app_paths;
use crate::settings::Settings;
use crate::state::AppState;

/// 组装并运行 Tauri 应用。
pub fn run() {
    // dev 判定用 debug_assertions（cargo build --release 即 release 语义）。
    let dev: bool = cfg!(debug_assertions);
    let paths = app_paths(dev);
    // 日志插件必须先于 setup 注册，AppPaths 在这里提前计算。
    let log_plugin = logging::build_log_plugin(&paths, dev);

    tauri::Builder::default()
        .plugin(log_plugin)
        // 头像回显协议：从 data/cache 读取后端代下的头像文件。
        // i.pximg.net 防盗链导致 webview 直连 403，故走本地自定义协议。
        .register_uri_scheme_protocol("pixiv-avatar", |ctx, request| {
            use std::borrow::Cow;
            // 只取最后一段文件名，天然免疫路径穿越
            let name = std::path::Path::new(request.uri().path())
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let state = ctx.app_handle().state::<crate::state::AppState>();
            let path = state.paths.data_dir.join("cache").join(name);
            log::info!("头像协议请求: {name} → {}", path.display());
            match std::fs::read(path) {
                Ok(bytes) => {
                    let mime = match name.rsplit('.').next() {
                        Some("png") => "image/png",
                        Some("webp") => "image/webp",
                        Some("gif") => "image/gif",
                        _ => "image/jpeg",
                    };
                    tauri::http::Response::builder()
                        .header("Content-Type", mime)
                        .body(Cow::Owned(bytes))
                        .expect("带 Content-Type 的响应构造不会失败")
                }
                Err(err) => {
                    log::warn!("头像协议 404: {name} ({err})");
                    tauri::http::Response::builder()
                        .status(404)
                        .body(Cow::Borrowed(&[][..]))
                        .expect("静态 404 响应构造不会失败")
                }
            }
        })
        .setup(move |app| {
            let settings = Settings::load_or_init(&paths.config_dir);
            let db = Db::open(&paths.data_dir.join("app.db"))?;
            // 显式主题须在任何子 webview 创建前同步设置：窗口默认外观跟随
            // 系统，与持久化主题不一致时，pixiv 首次加载会按浅色完成 JS
            // 初始化，之后窗口转深色也只有 CSS 媒体查询部分跟随，出现
            // 白块。auto 不设置，保持跟随系统（首载天然正确）。
            let explicit_theme = match settings.theme.as_str() {
                "dark" => Some(tauri::Theme::Dark),
                "light" => Some(tauri::Theme::Light),
                _ => None,
            };
            app.manage(AppState::new(paths.clone(), settings, db));
            if let Some(theme) = explicit_theme {
                if let Some(win) = app.get_window("main") {
                    let _ = win.set_theme(Some(theme));
                }
            }
            logging::init(app.handle());
            register_close_confirmation(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // auth
            commands::auth_cmds::auth_status,
            commands::auth_cmds::auth_login,
            commands::auth_cmds::auth_login_manual,
            commands::auth_cmds::auth_logout,
            // browse
            commands::browse_cmds::browse_open,
            commands::browse_cmds::browse_set_bounds,
            commands::browse_cmds::browse_hide,
            commands::browse_cmds::browse_set_theme,
            commands::browse_cmds::browse_show,
            commands::browse_cmds::browse_navigate,
            commands::browse_cmds::browse_go_back,
            commands::browse_cmds::browse_sync_login,
            commands::browse_cmds::browse_inject_login,
            // tasks
            commands::task_cmds::tasks_list,
            commands::task_cmds::task_create,
            commands::task_cmds::task_pause,
            commands::task_cmds::task_resume,
            commands::task_cmds::task_cancel,
            commands::task_cmds::task_retry_failed,
            commands::task_cmds::task_delete,
            commands::task_cmds::tasks_delete,
            commands::task_cmds::tasks_delete_completed,
            // settings
            commands::settings_cmds::settings_get,
            commands::settings_cmds::settings_save,
            commands::settings_cmds::clear_logs,
            // history
            commands::history_cmds::history_list,
            // novels / illustrations
            commands::misc_cmds::novel_delete,
            commands::misc_cmds::novels_batch_delete,
            commands::misc_cmds::novels_delete_all,
            commands::misc_cmds::illustration_delete,
            commands::misc_cmds::illustrations_batch_delete,
            commands::misc_cmds::illustrations_delete_all,
            commands::misc_cmds::open_novel_file,
            commands::misc_cmds::open_illustration_folder,
        ])
        .run(tauri::generate_context!())
        .expect("Pixiv Tool 运行失败");
}

/// 主窗口关闭确认：先 prevent_close，弹 ask 对话框，确认后 destroy。
///
/// 注意：handler 跑在主线程事件循环里，不能用 blocking_show（会在主线程上
/// 死锁等到超时），必须用回调式 show。文案语言实时读 settings.language。
/// （Tauri 2 没有 v1 的 on_close_requested 便捷方法，走 on_window_event。）
fn register_close_confirmation(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let app_handle = app.handle().clone();
    let confirmed_window = window.clone();
    window.on_window_event(move |event| {
        let tauri::WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };
        api.prevent_close();
        let language = app_handle
            .try_state::<AppState>()
            .and_then(|state| state.settings.lock().ok().map(|s| s.language.clone()))
            .unwrap_or_else(|| "zh-CN".to_string());
        let (message, ok_text, cancel_text) = if language == "en-US" {
            ("Quit Pixiv Tool?", "Quit", "Cancel")
        } else {
            ("确认退出 Pixiv Tool 吗？", "退出", "取消")
        };
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
        let close_target = confirmed_window.clone();
        app_handle
            .dialog()
            .message(message)
            .buttons(MessageDialogButtons::OkCancelCustom(
                ok_text.to_string(),
                cancel_text.to_string(),
            ))
            .show(move |confirmed| {
                if confirmed {
                    // destroy 不再走 CloseRequested，直接关窗
                    let _ = close_target.destroy();
                }
            });
    });
}
