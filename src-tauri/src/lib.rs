//! pixiv-tool Tauri 2 桌面应用（lib 形态，main.rs 只做薄壳）。
//!
//! 本文件是**装配层**，一次写全，后续 phase 只填充桩模块内容、不改本文件：
//! - 插件：tauri-plugin-log（文件 + dev stdout）、tauri-plugin-dialog（退出确认/目录选择）
//! - setup：AppPaths（dev/release 分平台）→ Settings 加载 → Db 打开 → AppState
//! - 全部命令注册（见 invoke_handler，与 commands/ 一一对应）
//! - 主窗口关闭确认（读 settings.language 决定中英文文案）

pub mod accounts;
pub mod auth;
pub mod browse;
pub mod commands;
pub mod cookies;
pub mod core;
pub mod db;
pub mod image_proxy;
pub mod logging;
pub mod paths;
pub mod pixiv;
pub mod platform;
pub mod settings;
pub mod state;

use tauri::{Emitter, Manager};

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
        // 图片代理协议：前端 convertFileSrc(pximgUrl, "pixiv-img")——传原始 URL，
        // 不要预编码（convertFileSrc 在 Windows 侧会编码一次，预编码会双重编码 403）。
        // pximg 防盗链需 Referer，走后端代下 + 磁盘缓存（data/cache/img）。
        // 异步注册不阻塞主线程；缓存目录从 AppState.paths 取（AppState 未就绪时
        // 兜底临时目录，正常时序下不会发生）。
        .register_asynchronous_uri_scheme_protocol("pixiv-img", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                let cache_dir = app
                    .try_state::<AppState>()
                    .map(|state| state.paths.data_dir.join("cache").join("img"))
                    .unwrap_or_else(|| std::env::temp_dir().join("pixiv-tool-img-cache"));
                let response = image_proxy::handle_image_request(request, &cache_dir).await;
                responder.respond(response);
            });
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
            setup_app_menu(app);
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
            commands::auth_cmds::auth_accounts_list,
            commands::auth_cmds::auth_account_switch,
            // browse
            commands::browse_cmds::browse_open,
            commands::browse_cmds::browse_set_bounds,
            commands::browse_cmds::browse_hide,
            commands::browse_cmds::browse_deactivate,
            commands::browse_cmds::browse_set_theme,
            commands::browse_cmds::browse_show,
            commands::browse_cmds::browse_navigate,
            commands::browse_cmds::browse_go_back,
            commands::browse_cmds::browse_sync_login,
            commands::browse_cmds::browse_inject_login,
            // 浏览数据
            commands::browse_api_cmds::browse_home_feed,
            commands::browse_api_cmds::browse_channel,
            commands::browse_api_cmds::browse_discover,
            commands::browse_api_cmds::browse_follow_latest,
            commands::browse_api_cmds::browse_search,
            commands::browse_api_cmds::browse_ranking,
            commands::browse_api_cmds::browse_work_detail,
            commands::browse_api_cmds::browse_related,
            commands::browse_api_cmds::browse_user_profile,
            commands::browse_api_cmds::browse_user_works,
            commands::browse_api_cmds::browse_novel_series,
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
            // app
            commands::app_cmds::app_exit,
        ])
        .build(tauri::generate_context!())
        .expect("Pixiv Tool 构建失败")
        .run(|app_handle, event| {
            // Cmd+Q / 菜单 Quit 走 ExitRequested（不触发窗口 CloseRequested）；
            // code=None 才拦（app_exit 的 exit(0) 是 code=Some，放行避免死循环）
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
                let _ = app_handle.emit("app://confirm-exit", ());
            }
        });
}

/// 主窗口关闭（红叉 / Cmd+W）确认：prevent_close 后发事件给前端，
/// 由前端应用内确认框（原生 dialog）统一处理（与 Cmd+Q 路径一致）。
/// （Tauri 2 没有 v1 的 on_close_requested 便捷方法，走 on_window_event。）
fn register_close_confirmation(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let app_handle = app.handle().clone();
    window.on_window_event(move |event| {
        let tauri::WindowEvent::CloseRequested { api, .. } = event else {
            return;
        };
        api.prevent_close();
        let _ = app_handle.emit("app://confirm-exit", ());
    });
}

/// 应用菜单栏：自定义 Quit 项接管 Cmd+Q——macOS 系统 terminate 不经过
/// tauri 事件循环（tao 无 applicationShouldTerminate，ExitRequested 拦不到），
/// 把快捷键派发到自己的菜单项是唯一拦截点。附带 Edit 菜单保证 webview
/// 文本编辑快捷键（粘贴 Session 等）在有菜单栏后仍可用。
fn setup_app_menu(app: &tauri::App) {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
    let language = app
        .try_state::<AppState>()
        .and_then(|state| state.settings.lock().ok().map(|s| s.language.clone()))
        .unwrap_or_else(|| "zh-CN".to_string());
    let zh = language != "en-US";
    let quit = MenuItemBuilder::with_id(
        "app-quit",
        if zh {
            "退出 Pixiv Tool"
        } else {
            "Quit Pixiv Tool"
        },
    )
    .accelerator("CmdOrCtrl+Q")
    .build(app)
    .expect("菜单项构建不会失败");
    let edit_title = if zh { "编辑" } else { "Edit" };
    let result = (|| -> Result<(), tauri::Error> {
        let app_submenu = SubmenuBuilder::new(app, "Pixiv Tool").item(&quit).build()?;
        let edit_submenu = SubmenuBuilder::new(app, edit_title)
            .undo()
            .redo()
            .separator()
            .cut()
            .copy()
            .paste()
            .select_all()
            .build()?;
        let menubar = MenuBuilder::new(app)
            .item(&app_submenu)
            .item(&edit_submenu)
            .build()?;
        app.set_menu(menubar)?;
        Ok(())
    })();
    if let Err(e) = result {
        log::warn!("应用菜单构建失败（Cmd+Q 拦截不可用）: {e}");
    }
    app.on_menu_event(move |app_handle, event| {
        if event.id() == "app-quit" {
            let _ = app_handle.emit("app://confirm-exit", ());
        }
    });
}
