//! 日志 —— tauri-plugin-log。
//!
//! 日志文件 `<data_dir>/logs/app.log`，默认 INFO 级别，格式带本地时间戳；
//! dev（debug 编译）额外输出到 stdout。
//!
//! **详细诊断**：`agent` target 的门限固定允许 DEBUG，其余依赖保持 INFO，
//! 运行时用 [`set_diagnostic`] 在 `log::set_max_level` 的 Info / Debug 之间切换。
//! 这样默认不产生 DEBUG 记录，开启后也只放行本项目 `agent` target 的工具 / 请求 /
//! 子任务起止与长度，不会把依赖库噪声写进日志。
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
/// 单文件上限 8MB，插件默认 KeepOne 策略，超出后重建当前日志。
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
        // `agent` target 单独放宽：默认被 `log::set_max_level(Info)` 挡住，
        // 只有会话级诊断开关会把它抬到 Debug。
        .level_for(AGENT_LOG_TARGET, log::LevelFilter::Debug)
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

/// 详细诊断日志所属的 target（见 `agent::events`）。
pub const AGENT_LOG_TARGET: &str = "agent";

/// 会话级详细诊断开关：`true` 放行 `agent` target 的 DEBUG 记录，`false` 恢复 INFO。
///
/// 只切日志门限，不改变业务行为；调用方（翻译服务）负责保存会话状态。
/// `RUST_LOG` 已显式注入时不覆盖，尊重开发者的环境设置。
pub fn set_diagnostic(enabled: bool) -> bool {
    if std::env::var_os("RUST_LOG").is_some() {
        log::info!("检测到 RUST_LOG，跳过诊断门限切换");
        return false;
    }
    log::set_max_level(if enabled {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    });
    log::info!("日志门限切换为 {}", if enabled { "debug" } else { "info" });
    true
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
