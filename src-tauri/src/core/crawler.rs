//! 小说 / 插画爬虫。
//!
//! **桩（phase C 实现）**：本文件只定稿公共契约与事件语义，函数体待填充。
#![allow(unused)]

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::core::sources::{IllustSource, NovelSource};
use crate::core::task_manager::TaskControls;
use crate::db::Db;
use crate::pixiv::api::PixivApi;

/// 爬虫 → 前端事件通道（由命令层用 AppHandle::emit 实现）。
///
/// 事件名只有两个：
/// - `"task://progress"` → `{"task_id","done","total","skipped"}`（单项失败时
///   可附 `"failed_item":{"id","error"}`，phase C 定稿具体载荷）
/// - `"task://done"`     → `{"task_id","done","total","skipped","failed"}`
pub type EventSink = Arc<dyn Fn(&str, &Value) + Send + Sync>;

/// 小说爬虫入参。
pub struct NovelCrawlArgs {
    pub db: Db,
    pub api: Arc<PixivApi>,
    pub source: NovelSource,
    pub task_id: String,
    /// 导出格式（"txt"/"markdown"）；空则用 ["txt","markdown"]。
    pub formats: Vec<String>,
    /// 相对路径锚定 data_dir（绝对路径原样）。
    pub output_dir: String,
    /// 锚定根（= AppPaths.data_dir）。
    pub data_dir: PathBuf,
    pub max_wait_seconds: i64,
    pub controls: TaskControls,
    pub sink: EventSink,
}

/// 插画爬虫入参（formats 为预留字段，插画不导出 txt/md）。
pub struct IllustCrawlArgs {
    pub db: Db,
    pub api: Arc<PixivApi>,
    pub source: IllustSource,
    pub task_id: String,
    pub formats: Vec<String>,
    /// 相对路径锚定 data_dir（绝对路径原样）。
    pub output_dir: String,
    /// 锚定根（= AppPaths.data_dir）。
    pub data_dir: PathBuf,
    /// 用户全集目录布局时为 Some(uid)；单作品为 None。
    pub user_id: Option<i64>,
    pub max_wait_seconds: i64,
    pub controls: TaskControls,
    pub sink: EventSink,
}

/// 执行小说抓取（语义与 Python `crawler.Crawler.run` 一致）：
///
/// - 开始时状态写 running，进度 0/0
/// - 每项：已下载（is_novel_downloaded）→ skipped+1；否则等暂停闸门 → 抓取
///   （get_novel + 写库 INSERT OR REPLACE + 按格式导出）→ done+1 并推
///   "task://progress"；单项失败 → failed_ids 记 id（不中断）
/// - 每项抓取前判定取消（cancel flag）与超时；超时判定排除暂停时长
///   （`elapsed - paused_total > max_wait_seconds` → 终态 failed，
///   error = "任务超过最大等待时间（{N}s）"）
/// - 终态：正常跑完 → done；取消 → **canceled**（修正 Python 版 bug：
///   Python 版先 mark_done(status 由变量决定但 mark_done 写死 done，取消时
///   终态被 done 覆写；Rust 版取消必须写 canceled）；超时 → failed
/// - 导出路径回写：.txt → novels.txt_path，.md → novels.md_path
///   （update_novel_paths，每导出一个文件调一次）
/// - 输出目录：`<output_dir>/novel/`（相对 output_dir 锚定 data_dir）
pub async fn run_novel_crawler(args: NovelCrawlArgs) {
    todo!("phase C")
}

/// 执行插画抓取（语义与 Python `illust_crawler.IllustCrawler.run` 一致）：
///
/// - 开始先 resolve_total 预置分母（失败退回增量计数）；user 源目录布局
///   `pic/users/{sanitize(作者名)}_{uid}/`（拿不到作者名退回 `user_{uid}`）
/// - 单作品：每保存一页 done+1（1 页作品即 1/1）；用户全集：每完成一个作品 done+1
/// - ugoira（illust_type=2）下载原图 zip（originalSrc 优先，zip_urls.original 兜底）
/// - 文件名：`{safe_title}_{artwork_id}_p{N}.{ext}`（ext 从 URL 取）/
///   `{safe_title}_{artwork_id}_ugoira.zip`；多页 meta 缺失时从 p0 直链
///   `_p0.{ext} → _p{N}.{ext}` 推导
/// - 已下载（is_illust_downloaded）→ skipped+1；saved_paths JSON 数组回写库
/// - 超时/取消/终态语义与 run_novel_crawler 相同（取消写 canceled，含修正）
/// - 输出目录：单作品 `<output_dir>/pic/`
pub async fn run_illust_crawler(args: IllustCrawlArgs) {
    todo!("phase C")
}
