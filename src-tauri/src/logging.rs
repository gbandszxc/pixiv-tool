//! 日志 —— tauri-plugin-log。
//!
//! 日志文件 `<data_dir>/logs/app.log`，INFO 级别，格式带本地时间戳；
//! dev（debug 编译）额外输出到 stdout。
//!
//! 日志插件必须在 `tauri::Builder` 上注册（先于 setup 执行），因此本模块提供
//! `build_log_plugin(&AppPaths, dev)` 在 `run()` 里构造；`init()` 在 setup 里
//! 调用，记录启动信息。

use tauri::Manager;
use tauri::Wry;
use tauri::plugin::TauriPlugin;
use tauri_plugin_log::{Target, TargetKind};

use crate::paths::AppPaths;

/// 日志文件基名（插件自动追加 `.log`）。
pub const LOG_FILE_NAME: &str = "app";
/// 单文件上限 8MB，超出后轮转为 app_old.log。
pub const MAX_LOG_FILE_SIZE: u128 = 8 * 1024 * 1024;

/// 构建日志插件。在 `tauri::Builder::default()` 之后立刻 `.plugin(...)` 挂载。
pub fn build_log_plugin(paths: &AppPaths, dev: bool) -> TauriPlugin<Wry> {
    let file_target = Target::new(TargetKind::Folder {
        path: paths.logs_dir.clone(),
        file_name: Some(LOG_FILE_NAME.to_string()),
    });
    let targets: Vec<Target> = if dev {
        vec![Target::new(TargetKind::Stdout), file_target]
    } else {
        vec![file_target]
    };
    tauri_plugin_log::Builder::new()
        .level(log::LevelFilter::Info)
        .max_file_size(MAX_LOG_FILE_SIZE)
        .targets(targets)
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] [{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                record.level(),
                record.target(),
                message
            ))
        })
        .build()
}

/// 在 setup 里调用：记录启动信息（Python 侧本来就没接线日志，这里从简）。
pub fn init(app: &tauri::AppHandle) {
    let mode = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    log::info!("Pixiv Tool 启动（{mode}）");
    log::info!(
        "窗口: {:?}",
        app.webview_windows().keys().collect::<Vec<_>>()
    );
}
