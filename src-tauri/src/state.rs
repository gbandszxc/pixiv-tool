//! 全局应用状态（setup 里 manage，命令层经 `tauri::State<AppState>` 取用）。

use std::sync::{Arc, Mutex};

use crate::accounts::AccountManager;
use crate::cookies::CookieStore;
use crate::core::task_manager::TaskManager;
use crate::db::Db;
use crate::paths::AppPaths;
use crate::settings::Settings;

pub struct AppState {
    pub paths: AppPaths,
    pub settings: Arc<Mutex<Settings>>,
    pub db: Db,
    pub cookies: CookieStore,
    /// 多账号管理（索引 + 每账号凭据条目）；`cookies`（default）恒为
    /// 当前激活账号的镜像。
    pub accounts: AccountManager,
    pub tasks: Arc<TaskManager>,
    pub translation_lock: tokio::sync::Mutex<()>,
}

impl AppState {
    /// 在 setup 里构造（settings 加载 / db 打开已由 lib.rs 完成）。
    pub fn new(paths: AppPaths, settings: Settings, db: Db) -> Self {
        let tasks = Arc::new(TaskManager::new(db.clone(), paths.data_dir.clone()));
        Self {
            accounts: AccountManager::new(&paths.config_dir),
            paths,
            settings: Arc::new(Mutex::new(settings)),
            db,
            cookies: CookieStore::new(),
            tasks,
            translation_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// 读取设置快照（短锁立即释放，避免跨 await 持锁）。
    pub fn settings_snapshot(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }
}
