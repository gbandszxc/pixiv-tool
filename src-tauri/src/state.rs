//! 全局应用状态（setup 里 manage，命令层经 `tauri::State<AppState>` 取用）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::accounts::AccountManager;
use crate::agent::CacheGeneration;
use crate::cookies::CookieStore;
use crate::core::task_manager::TaskManager;
use crate::db::Db;
use crate::paths::AppPaths;
use crate::settings::Settings;
use crate::translation::service::TranslationService;

pub struct AppState {
    pub paths: AppPaths,
    pub settings: Arc<Mutex<Settings>>,
    pub db: Db,
    pub cookies: CookieStore,
    /// 多账号管理（索引 + 每账号凭据条目）；`cookies`（default）恒为
    /// 当前激活账号的镜像。
    pub accounts: AccountManager,
    pub tasks: Arc<TaskManager>,
    pub translation_locks: TranslationLocks,
    /// 整篇翻译 Job 服务（v2）：状态真相源、事件出口与缓存世代。
    pub translation_service: TranslationService,
    /// 翻译缓存清理世代：每次清理递增，迟到回调用旧世代写入会被拒绝。
    pub translation_generation: CacheGeneration,
}

impl AppState {
    /// 在 setup 里构造（settings 加载 / db 打开已由 lib.rs 完成）。
    ///
    /// 事件出口默认为空实现；`lib.rs` 在 setup 里用
    /// [`Self::with_translation_sink`] 注入 Tauri 事件出口。
    pub fn new(paths: AppPaths, settings: Settings, db: Db) -> Self {
        let tasks = Arc::new(TaskManager::new(db.clone(), paths.data_dir.clone()));
        let settings = Arc::new(Mutex::new(settings));
        let translation_generation = CacheGeneration::new(0);
        let translation_service = TranslationService::new(
            paths.clone(),
            settings.clone(),
            translation_generation.clone(),
        );
        Self {
            accounts: AccountManager::new(&paths.config_dir),
            paths,
            settings,
            db,
            cookies: CookieStore::new(),
            tasks,
            translation_locks: TranslationLocks::default(),
            translation_service,
            translation_generation,
        }
    }

    /// 用真实事件出口重建翻译服务（setup 里 `AppHandle` 可用后调用）。
    pub fn with_translation_sink(mut self, sink: Arc<dyn crate::agent::EventSink>) -> Self {
        self.translation_service = TranslationService::with_sink(
            self.paths.clone(),
            self.settings.clone(),
            self.translation_generation.clone(),
            sink,
        );
        self
    }

    /// 读取设置快照（短锁立即释放，避免跨 await 持锁）。
    pub fn settings_snapshot(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// 小说翻译并发控制（ADR 0031）。
///
/// 两级语义：不同小说各自独立、可同时翻译；同一小说共享设定集与页译文文件，
/// 仍按小说串行。翻译缓存的整体清理需要「一个翻译都没在跑」，否则清掉在途任务
/// 正要写入的目录——由 `maintenance` 读写门表达。
#[derive(Default)]
pub struct TranslationLocks {
    /// 小说 ID → 该小说的串行槽位。占用为 0 的槽位在下一次取用时被回收。
    novels: Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
    /// 翻译任务持读锁（可并存多本小说），清理翻译缓存取写锁。
    maintenance: tokio::sync::RwLock<()>,
}

impl TranslationLocks {
    /// 占用某本小说的翻译槽位；guard 存活期间该小说不会再起第二个任务。
    pub async fn acquire(&self, novel_id: i64) -> TranslationTaskGuard<'_> {
        let gate = self.maintenance.read().await;
        let slot = {
            let mut novels = self.novels.lock().unwrap_or_else(|e| e.into_inner());
            novels.retain(|_, slot| Arc::strong_count(slot) > 1);
            novels
                .entry(novel_id)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let slot = slot.lock_owned().await;
        TranslationTaskGuard { _gate: gate, _slot: slot }
    }

    /// 翻译缓存清理门：有翻译在途时立即返回 `Err`，不阻塞命令。
    pub fn try_block_maintenance(
        &self,
    ) -> Result<tokio::sync::RwLockWriteGuard<'_, ()>, tokio::sync::TryLockError> {
        self.maintenance.try_write()
    }
}

/// 单个翻译任务的占用凭据，drop 即释放小说槽位与清理门。
pub struct TranslationTaskGuard<'a> {
    _gate: tokio::sync::RwLockReadGuard<'a, ()>,
    _slot: tokio::sync::OwnedMutexGuard<()>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 不同小说并行：A 未释放时 B 仍能立即拿到槽位。
    #[tokio::test]
    async fn different_novels_translate_concurrently() {
        let locks = TranslationLocks::default();
        let _a = locks.acquire(1).await;
        let second = tokio::time::timeout(Duration::from_millis(200), locks.acquire(2)).await;
        assert!(second.is_ok(), "不同小说不应互相阻塞");
    }

    /// 同一小说串行：前一页未释放时后一页必须等待。
    #[tokio::test]
    async fn same_novel_translation_is_serialized() {
        let locks = TranslationLocks::default();
        let guard = locks.acquire(7).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(150), locks.acquire(7))
                .await
                .is_err(),
            "同一小说的第二个任务必须等待"
        );
        drop(guard);
        // 释放后能拿到（回收槽位不影响后续获取）。
        assert!(
            tokio::time::timeout(Duration::from_millis(500), locks.acquire(7))
                .await
                .is_ok()
        );
    }

    /// 清理门：有翻译在途时取不到写锁，全部结束后可清理。
    #[tokio::test]
    async fn maintenance_is_blocked_while_translating() {
        let locks = TranslationLocks::default();
        let guard = locks.acquire(1).await;
        assert!(locks.try_block_maintenance().is_err(), "翻译中拒绝清理缓存");
        drop(guard);
        assert!(locks.try_block_maintenance().is_ok(), "任务结束后可清理");
    }
}
