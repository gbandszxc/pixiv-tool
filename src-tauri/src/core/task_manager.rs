//! 任务状态机：pending → running ⇄ paused → done/failed/canceled。
//!
//! **桩（phase C 实现）**：除 TaskControls（控制句柄）外本文件只定稿公共契约，
//! 函数体待填充。
#![allow(unused)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::db::{Db, TaskRow};
use crate::settings::Settings;

use super::crawler::EventSink;

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
        let _ = self.pause.send(paused);
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
    tasks: Mutex<HashMap<String, TaskControls>>,
}

impl TaskManager {
    pub fn new(db: Db, data_dir: PathBuf) -> Self {
        Self {
            db,
            data_dir,
            tasks: Mutex::new(HashMap::new()),
        }
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
        todo!("phase C")
    }

    /// 暂停：闸门置 true + 状态写 paused。
    pub fn pause(&self, task_id: &str) -> Result<(), String> {
        todo!("phase C")
    }

    /// 继续：闸门置 false + 状态写 running。
    pub fn resume(&self, task_id: &str) -> Result<(), String> {
        todo!("phase C")
    }

    /// 取消：cancel 标志 + 解除暂停 + 状态写 canceled（终态由爬虫侧确认，
    /// 与 Python 版一致：正在下载的当前项会下完）。
    pub fn cancel(&self, task_id: &str) -> Result<(), String> {
        todo!("phase C")
    }

    /// 重试失败项：读原任务 failed_ids，逗号拼接为新任务的 source_id，
    /// 逐 id 以 Single 源**串行**抓取（每项独立 run，共享新 task_id 的计数）。
    /// 原任务不存在 → Err("任务不存在")；没有失败项 → Err("没有失败项")。
    pub fn retry_failed(
        &self,
        task_id: &str,
        cookies: std::collections::HashMap<String, String>,
        settings: Settings,
        sink: EventSink,
    ) -> Result<String, String> {
        todo!("phase C")
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
