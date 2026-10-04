//! 任务状态机：pending → running ⇄ paused → done/failed/canceled。
//!
//! 语义对齐 Python `core/task.py` + `api/tasks.py` 的创建/重试编排：
//! - create_task 校验通过后 INSERT pending、注册 TaskControls、tokio::spawn
//!   后台跑 crawler，立即返回 task_id；
//! - retry_failed 逐 id 串行复用 crawler 的 items 管线（共用新 task_id，
//!   计数累计——修正 Python 版每个 id 独立 run 导致 mark_done 互相覆写计数
//!   的问题）；
//! - 程序退出不清理（进程即生命周期），但任务终态后移除运行句柄防泄漏。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use crate::core::crawler::{
    EventSink, IllustCrawlArgs, NovelCrawlArgs, run_illust_items, run_novel_items,
};
use crate::core::sources::{IllustSource, NovelSource};
use crate::db::{Db, TaskInsert, TaskRow, now_iso};
use crate::pixiv::api::PixivApi;
use crate::pixiv::client::PixivClient;
use crate::settings::Settings;

/// 单个任务的运行控制句柄（爬虫循环与 TaskManager 共享同一份）。
#[derive(Clone)]
pub struct TaskControls {
    /// 暂停闸门：true=暂停。爬虫在每项开始前等它变回 false。
    pub pause: Arc<tokio::sync::watch::Sender<bool>>,
    /// 取消标志：true=取消。取消时同时解除暂停，允许循环退出。
    pub cancel: Arc<AtomicBool>,
}

impl TaskControls {
    pub fn new() -> Self {
        let (sender, _receiver) = tokio::sync::watch::channel(false);
        Self {
            pause: Arc::new(sender),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_paused(&self, paused: bool) {
        // send_replace 而非 send：watch 的 send 在无接收者时是 no-op，
        // 会在爬虫尚未 subscribe 的窗口期丢失暂停请求。
        self.pause.send_replace(paused);
    }

    pub fn is_paused(&self) -> bool {
        *self.pause.borrow()
    }

    /// 设置取消标志并解除暂停（当前正在下载的项会下完再退出）。
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.set_paused(false);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

impl Default for TaskControls {
    fn default() -> Self {
        Self::new()
    }
}

/// 任务管理器：创建、暂停、继续、取消、重试失败项。
/// 进度/终态落库走 Db；列表/详情查询也走 Db（这里只做薄封装）。
pub struct TaskManager {
    db: Db,
    /// 相对 output_dir 的锚定根。
    data_dir: PathBuf,
    /// task_id → 运行控制句柄（任务到终态后由后台任务移除，防泄漏）。
    /// Arc 包一层是为了让 spawn 的后台任务能移除自己（Rust 无弱引用借用的
    /// 简单写法）。
    tasks: Arc<Mutex<HashMap<String, TaskControls>>>,
}

impl TaskManager {
    pub fn new(db: Db, data_dir: PathBuf) -> Self {
        Self {
            db,
            data_dir,
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn register(&self, task_id: &str, controls: TaskControls) {
        if let Ok(mut map) = self.tasks.lock() {
            map.insert(task_id.to_string(), controls);
        }
    }

    fn controls_of(&self, task_id: &str) -> Result<TaskControls, String> {
        self.tasks
            .lock()
            .map_err(|_| "任务表锁中毒".to_string())?
            .get(task_id)
            .cloned()
            .ok_or_else(|| "任务不存在".to_string())
    }

    /// 创建任务并**立即返回 task_id**，爬取在 spawn 的后台 tokio task 内执行。
    ///
    /// 校验（不通过返回 Err，不落库）：
    /// - category ∈ {novel, illustration}，否则 Err("未知任务分类: {c}")
    /// - source_type 合法（novel: single/series/user；illustration: single/user），
    ///   否则 Err("未知来源类型: {t}")
    /// - cookies 含非空 PHPSESSID，否则 Err("未登录，请先登录")
    ///
    /// 流程：INSERT tasks(pending) → 记录 TaskControls → spawn 后台任务
    /// （构造 PixivClient/PixivApi，按 category 调 run_novel_crawler /
    /// run_illust_crawler，sink 推 task:// 事件；结束移除句柄）。
    #[allow(clippy::too_many_arguments)]
    pub fn create_task(
        &self,
        source_type: &str,
        source_id: &str,
        formats: Vec<String>,
        category: &str,
        cookies: std::collections::HashMap<String, String>,
        settings: Settings,
        sink: EventSink,
    ) -> Result<String, String> {
        if !matches!(category, "novel" | "illustration") {
            return Err(format!("未知任务分类: {category}"));
        }
        let allowed = if category == "novel" {
            matches!(source_type, "single" | "series" | "user")
        } else {
            matches!(source_type, "single" | "user")
        };
        if !allowed {
            return Err(format!("未知来源类型: {source_type}"));
        }
        if !cookies
            .get("PHPSESSID")
            .is_some_and(|v| !v.trim().is_empty())
        {
            return Err("未登录，请先登录".into());
        }
        let source_num: i64 = source_id
            .trim()
            .parse()
            .map_err(|_| format!("来源 ID 必须是数字: {source_id}"))?;
        if source_num <= 0 {
            return Err("来源 ID 必须是正整数".into());
        }

        let task_id = uuid::Uuid::new_v4().to_string();
        let now = now_iso();
        self.db.insert_task(&TaskInsert {
            task_id: task_id.clone(),
            source_type: source_type.to_string(),
            source_id: source_id.to_string(),
            category: category.to_string(),
            status: "pending".into(),
            created_at: now.clone(),
            updated_at: now,
        })?;
        let controls = TaskControls::new();
        self.register(&task_id, controls.clone());

        let db = self.db.clone();
        let data_dir = self.data_dir.clone();
        let registry = self.tasks.clone();
        let category = category.to_string();
        let source_type = source_type.to_string();
        let bg_task_id = task_id.clone();

        tokio::spawn(async move {
            let task_id = bg_task_id;
            let api = match build_api(&cookies) {
                Ok(api) => api,
                Err(err) => {
                    fail_fast(
                        &db,
                        &sink,
                        &task_id,
                        &format!("创建 HTTP 客户端失败: {err}"),
                    );
                    remove_controls(&registry, &task_id);
                    return;
                }
            };
            if category == "novel" {
                let source = match source_type.as_str() {
                    "series" => NovelSource::Series(source_num),
                    "user" => NovelSource::User(source_num),
                    _ => NovelSource::Single(source_num),
                };
                run_novel_items(
                    NovelCrawlArgs {
                        db,
                        api,
                        source,
                        task_id: task_id.clone(),
                        formats,
                        output_dir: settings.output_dir,
                        data_dir,
                        max_wait_seconds: settings.max_wait_seconds,
                        controls,
                        sink,
                    },
                    None,
                )
                .await;
            } else {
                let user_id = (source_type == "user").then_some(source_num);
                let source = if user_id.is_some() {
                    IllustSource::User(source_num)
                } else {
                    IllustSource::Single(source_num)
                };
                run_illust_items(
                    IllustCrawlArgs {
                        db,
                        api,
                        source,
                        task_id: task_id.clone(),
                        formats,
                        output_dir: settings.output_dir,
                        data_dir,
                        user_id,
                        max_wait_seconds: settings.max_wait_seconds,
                        controls,
                        sink,
                    },
                    None,
                )
                .await;
            }
            remove_controls(&registry, &task_id);
        });
        Ok(task_id)
    }

    /// 暂停：闸门置 true + 状态写 paused。
    pub fn pause(&self, task_id: &str) -> Result<(), String> {
        let controls = self.controls_of(task_id)?;
        controls.set_paused(true);
        self.db
            .update_task_fields(task_id, &[("status", json!("paused"))])
    }

    /// 继续：闸门置 false + 状态写 running。
    pub fn resume(&self, task_id: &str) -> Result<(), String> {
        let controls = self.controls_of(task_id)?;
        controls.set_paused(false);
        self.db
            .update_task_fields(task_id, &[("status", json!("running"))])
    }

    /// 取消：cancel 标志 + 解除暂停 + 状态写 canceled（终态由爬虫侧确认，
    /// 与 Python 版一致：正在下载的当前项会下完）。
    pub fn cancel(&self, task_id: &str) -> Result<(), String> {
        let controls = self.controls_of(task_id)?;
        controls.cancel();
        self.db
            .update_task_fields(task_id, &[("status", json!("canceled"))])
    }

    /// 重试失败项：读原任务 failed_ids，逗号拼接为新任务的 source_id，
    /// 逐 id 以 Single 源**串行**抓取（共用新 task_id 的计数）。
    /// 原任务不存在 → Err("任务不存在")；没有失败项 → Err("没有失败项")。
    pub fn retry_failed(
        &self,
        task_id: &str,
        cookies: std::collections::HashMap<String, String>,
        settings: Settings,
        sink: EventSink,
    ) -> Result<String, String> {
        let task = self
            .db
            .get_task(task_id)?
            .ok_or_else(|| "任务不存在".to_string())?;
        let failed_ids: Vec<i64> = serde_json::from_str(&task.failed_ids).unwrap_or_default();
        if failed_ids.is_empty() {
            return Err("没有失败项".into());
        }
        if !cookies
            .get("PHPSESSID")
            .is_some_and(|v| !v.trim().is_empty())
        {
            return Err("未登录，请先登录".into());
        }

        let new_task_id = uuid::Uuid::new_v4().to_string();
        let now = now_iso();
        self.db.insert_task(&TaskInsert {
            task_id: new_task_id.clone(),
            // 与 Python 版一致：source_type 保留原任务标签，source_id 为逗号拼接
            source_type: task.source_type.clone(),
            source_id: failed_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(","),
            category: task.category.clone(),
            status: "pending".into(),
            created_at: now.clone(),
            updated_at: now,
        })?;
        let controls = TaskControls::new();
        self.register(&new_task_id, controls.clone());

        let db = self.db.clone();
        let data_dir = self.data_dir.clone();
        let registry = self.tasks.clone();
        let category = task.category;
        let output_dir = settings.output_dir.clone();
        let formats = settings.output_formats.clone();
        let max_wait_seconds = settings.max_wait_seconds;
        let retry_task_id = new_task_id.clone();

        tokio::spawn(async move {
            let api = match build_api(&cookies) {
                Ok(api) => api,
                Err(err) => {
                    fail_fast(
                        &db,
                        &sink,
                        &retry_task_id,
                        &format!("创建 HTTP 客户端失败: {err}"),
                    );
                    remove_controls(&registry, &retry_task_id);
                    return;
                }
            };
            if category == "novel" {
                // 逐 id 串行、共用一个 task_id（计数累计），order 固定 None
                let items = failed_ids.into_iter().map(|id| (id, None)).collect();
                run_novel_items(
                    NovelCrawlArgs {
                        db,
                        api,
                        // preset_items 生效时 source 不参与解析
                        source: NovelSource::Single(0),
                        task_id: retry_task_id.clone(),
                        formats,
                        output_dir,
                        data_dir,
                        max_wait_seconds,
                        controls,
                        sink,
                    },
                    Some(items),
                )
                .await;
            } else {
                // 插画重试：逐 id 单作品语义、目录 {output}/pic/、增量计数
                run_illust_items(
                    IllustCrawlArgs {
                        db,
                        api,
                        source: IllustSource::Single(0),
                        task_id: retry_task_id.clone(),
                        formats: Vec::new(),
                        output_dir,
                        data_dir,
                        user_id: None,
                        max_wait_seconds,
                        controls,
                        sink,
                    },
                    Some(failed_ids),
                )
                .await;
            }
            remove_controls(&registry, &retry_task_id);
        });
        Ok(new_task_id)
    }

    /// 任务列表（ORDER BY created_at DESC），可按 category 过滤。
    pub fn list_tasks(&self, category: Option<&str>) -> Result<Vec<TaskRow>, String> {
        self.db.list_tasks(category)
    }

    /// 任务详情。
    pub fn get_task(&self, task_id: &str) -> Result<Option<TaskRow>, String> {
        self.db.get_task(task_id)
    }
}

/// 构造共享的 PixivApi（wreq Chrome147 指纹）。
fn build_api(cookies: &std::collections::HashMap<String, String>) -> anyhow::Result<Arc<PixivApi>> {
    let client = PixivClient::new(cookies)?;
    Ok(Arc::new(PixivApi::new(Arc::new(client))))
}

/// spawn 前置失败（HTTP 客户端构造失败）：直接标记 failed 并发终态事件。
fn fail_fast(db: &Db, sink: &EventSink, task_id: &str, error: &str) {
    log::error!("任务 {task_id} 启动失败: {error}");
    let _ = db.update_task_fields(
        task_id,
        &[("status", json!("failed")), ("error", json!(error))],
    );
    sink(
        "task://done",
        &json!({
            "task_id": task_id,
            "status": "failed",
            "done": 0,
            "total": 0,
            "skipped": 0,
            "failed": 0,
        }),
    );
}

/// 后台任务结束时移除运行句柄（防泄漏）。
fn remove_controls(registry: &Arc<Mutex<HashMap<String, TaskControls>>>, task_id: &str) {
    if let Ok(mut map) = registry.lock() {
        map.remove(task_id);
    }
}

// ----------------------------------------------------------------------
// 单元测试（离线：只测校验分支与错误路径，成功路径会 spawn 真实爬虫不发测）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_manager(tag: &str) -> (TaskManager, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "pixiv-tool-task-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("app.db")).unwrap();
        (TaskManager::new(db, dir.clone()), dir)
    }

    fn logged_in_cookies() -> HashMap<String, String> {
        let mut cookies = HashMap::new();
        cookies.insert("PHPSESSID".to_string(), "12345_abc".to_string());
        cookies
    }

    fn noop_sink() -> EventSink {
        Arc::new(|_name: &str, _payload: &serde_json::Value| {})
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn create_task_rejects_unknown_category() {
        let (tm, dir) = temp_manager("category");
        let err = tm
            .create_task(
                "single",
                "1",
                vec![],
                "manga",
                logged_in_cookies(),
                Settings::default(),
                noop_sink(),
            )
            .unwrap_err();
        assert_eq!(err, "未知任务分类: manga");
        assert!(tm.list_tasks(None).unwrap().is_empty(), "校验失败不落库");
        cleanup(&dir);
    }

    #[test]
    fn create_task_rejects_unknown_source_type() {
        let (tm, dir) = temp_manager("source");
        // novel 不接受 single 之外的非法值
        assert_eq!(
            tm.create_task(
                "box",
                "1",
                vec![],
                "novel",
                logged_in_cookies(),
                Settings::default(),
                noop_sink()
            )
            .unwrap_err(),
            "未知来源类型: box"
        );
        // illustration 没有 series 来源
        assert_eq!(
            tm.create_task(
                "series",
                "1",
                vec![],
                "illustration",
                logged_in_cookies(),
                Settings::default(),
                noop_sink()
            )
            .unwrap_err(),
            "未知来源类型: series"
        );
        assert!(tm.list_tasks(None).unwrap().is_empty());
        cleanup(&dir);
    }

    #[test]
    fn create_task_rejects_missing_or_blank_phpsessid() {
        let (tm, dir) = temp_manager("cookies");
        assert_eq!(
            tm.create_task(
                "single",
                "1",
                vec![],
                "novel",
                HashMap::new(),
                Settings::default(),
                noop_sink()
            )
            .unwrap_err(),
            "未登录，请先登录"
        );
        let mut blank = HashMap::new();
        blank.insert("PHPSESSID".to_string(), "   ".to_string());
        assert_eq!(
            tm.create_task(
                "single",
                "1",
                vec![],
                "novel",
                blank,
                Settings::default(),
                noop_sink()
            )
            .unwrap_err(),
            "未登录，请先登录"
        );
        assert!(tm.list_tasks(None).unwrap().is_empty());
        cleanup(&dir);
    }

    #[test]
    fn create_task_rejects_non_numeric_source_id() {
        let (tm, dir) = temp_manager("badid");
        assert_eq!(
            tm.create_task(
                "single",
                "abc",
                vec![],
                "novel",
                logged_in_cookies(),
                Settings::default(),
                noop_sink()
            )
            .unwrap_err(),
            "来源 ID 必须是数字: abc"
        );
        assert!(tm.list_tasks(None).unwrap().is_empty());
        cleanup(&dir);
    }

    #[test]
    fn create_task_rejects_empty_illustration_source_and_nonpositive_id() {
        let (tm, dir) = temp_manager("source-boundary");
        assert_eq!(
            tm.create_task(
                "",
                "1",
                vec![],
                "illustration",
                logged_in_cookies(),
                Settings::default(),
                noop_sink()
            )
            .unwrap_err(),
            "未知来源类型: "
        );
        for id in ["0", "-1"] {
            assert_eq!(
                tm.create_task(
                    "single",
                    id,
                    vec![],
                    "novel",
                    logged_in_cookies(),
                    Settings::default(),
                    noop_sink()
                )
                .unwrap_err(),
                "来源 ID 必须是正整数"
            );
        }
        assert!(tm.list_tasks(None).unwrap().is_empty());
        cleanup(&dir);
    }

    #[test]
    fn pause_resume_cancel_on_missing_task() {
        let (tm, dir) = temp_manager("missing");
        for (op, result) in [
            ("pause", tm.pause("nope")),
            ("resume", tm.resume("nope")),
            ("cancel", tm.cancel("nope")),
        ] {
            assert_eq!(result.unwrap_err(), "任务不存在", "{op} 未知任务应报错");
        }
        cleanup(&dir);
    }

    #[test]
    fn retry_failed_missing_task_and_empty_failed() {
        let (tm, dir) = temp_manager("retry");
        assert_eq!(
            tm.retry_failed(
                "nope",
                logged_in_cookies(),
                Settings::default(),
                noop_sink()
            )
            .unwrap_err(),
            "任务不存在"
        );

        // 插一条 failed_ids 为空的任务
        let now = now_iso();
        tm.db
            .insert_task(&TaskInsert {
                task_id: "t1".into(),
                source_type: "single".into(),
                source_id: "1".into(),
                created_at: now.clone(),
                updated_at: now,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            tm.retry_failed("t1", logged_in_cookies(), Settings::default(), noop_sink())
                .unwrap_err(),
            "没有失败项"
        );
        // 坏 JSON 同样按空处理
        tm.db
            .update_task_fields("t1", &[("failed_ids", json!("not-json"))])
            .unwrap();
        assert_eq!(
            tm.retry_failed("t1", logged_in_cookies(), Settings::default(), noop_sink())
                .unwrap_err(),
            "没有失败项"
        );
        cleanup(&dir);
    }

    #[test]
    fn task_controls_pause_and_cancel_flags() {
        let controls = TaskControls::new();
        assert!(!controls.is_paused());
        controls.set_paused(true);
        assert!(controls.is_paused());
        // 取消同时解除暂停
        assert!(!controls.is_cancelled());
        controls.cancel();
        assert!(controls.is_cancelled());
        assert!(!controls.is_paused());
    }
}
