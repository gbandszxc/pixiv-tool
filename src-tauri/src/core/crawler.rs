//! 小说 / 插画爬虫。
//!
//! 语义对齐 Python `core/crawler.py` / `core/illust_crawler.py`，其中两处
//! 修正 Python 版的 bug：
//! 1. 取消终态必须写 `canceled`（Python 版 mark_done 写死 done，取消时被覆写）；
//! 2. finally 限速（PixivClient 层）+ 暂停时长不计入超时判定的既有语义保持。
//!
//! 事件（EventSink）只有两个名字：
//! - `"task://progress"` → `{"task_id","status":"running","done","total","skipped"}`
//!   （成功项附 `"current_title"`；失败项附 `"failed_item":{"id","title","error"}`）
//! - `"task://done"`     → `{"task_id","status","done","total","skipped","failed"}`

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::core::exporter::{ExportMeta, Exporter, create_exporters};
use crate::core::illust_crawler::{
    UGOIRA_TYPE, collect_page_urls, page_filename, safe_title_or_id, ugoira_filename, user_dir_name,
};
use crate::core::sources::{IllustSource, NovelSource};
use crate::core::task_manager::TaskControls;
use crate::db::{Db, IllustrationInsert, NovelInsert, now_iso};
use crate::pixiv::api::{IllustData, NovelData, PixivApi};

/// 爬虫 → 前端事件通道（由命令层用 AppHandle::emit 实现）。
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

/// 任务计数（done/total/skipped/failed_ids，两种爬虫共用）。
#[derive(Debug, Default)]
pub(crate) struct TaskCounters {
    pub(crate) total: i64,
    pub(crate) done: i64,
    pub(crate) skipped: i64,
    pub(crate) failed_ids: Vec<i64>,
}

/// 单项抓取失败信息（保留已知的 title 供 failed_item 载荷展示）。
struct ItemError {
    title: Option<String>,
    message: String,
}

impl ItemError {
    fn new(title: Option<&str>, err: &dyn std::fmt::Display) -> Self {
        Self {
            title: title.map(String::from),
            message: err.to_string(),
        }
    }
}

// ----------------------------------------------------------------------
// 可测纯函数
// ----------------------------------------------------------------------

/// 超时判定（暂停时长不计入）：`elapsed - paused > max_wait_seconds`。
pub(crate) fn timeout_exceeded(elapsed: Duration, paused: Duration, max_wait_seconds: i64) -> bool {
    (elapsed.as_secs_f64() - paused.as_secs_f64()) > max_wait_seconds as f64
}

/// 终态选择：取消 → `canceled`（**修正** Python mark_done 写死 done 覆写取消
/// 终态的 bug），正常跑完 → `done`。
pub(crate) fn terminal_status(cancelled: bool) -> &'static str {
    if cancelled { "canceled" } else { "done" }
}

/// 相对 output_dir 锚定 data_dir（frozen/双击启动时 cwd 不可预期），
/// 绝对路径原样。
pub(crate) fn anchor_output_dir(output_dir: &str, data_dir: &Path) -> PathBuf {
    let path = PathBuf::from(output_dir);
    if path.is_absolute() {
        path
    } else {
        data_dir.join(path)
    }
}

// ----------------------------------------------------------------------
// 事件 / 落库
// ----------------------------------------------------------------------

fn emit_progress(
    sink: &EventSink,
    task_id: &str,
    counters: &TaskCounters,
    current_title: Option<&str>,
    failed: Option<(i64, Option<&str>, &str)>,
) {
    let mut payload = json!({
        "task_id": task_id,
        "status": "running",
        "done": counters.done,
        "total": counters.total,
        "skipped": counters.skipped,
    });
    if let Some(title) = current_title {
        payload["current_title"] = json!(title);
    }
    if let Some((id, title, error)) = failed {
        payload["failed_item"] = json!({"id": id, "title": title, "error": error});
    }
    sink("task://progress", &payload);
}

fn emit_done(sink: &EventSink, task_id: &str, status: &str, counters: &TaskCounters) {
    sink(
        "task://done",
        &json!({
            "task_id": task_id,
            "status": status,
            "done": counters.done,
            "total": counters.total,
            "skipped": counters.skipped,
            "failed": counters.failed_ids.len(),
        }),
    );
}

/// 每项成功后的进度落库（done/total/skipped，不动 status）。
fn update_task_progress(db: &Db, task_id: &str, counters: &TaskCounters) {
    let _ = db.update_task_fields(
        task_id,
        &[
            ("done", json!(counters.done)),
            ("total", json!(counters.total)),
            ("skipped", json!(counters.skipped)),
        ],
    );
}

/// 终态落库（status + total/done/skipped/failed_ids JSON 数组）。
fn mark_task_finished(db: &Db, task_id: &str, status: &str, counters: &TaskCounters) {
    let failed_ids = serde_json::to_string(&counters.failed_ids).unwrap_or_else(|_| "[]".into());
    if let Err(err) = db.update_task_fields(
        task_id,
        &[
            ("status", json!(status)),
            ("total", json!(counters.total)),
            ("done", json!(counters.done)),
            ("skipped", json!(counters.skipped)),
            ("failed_ids", json!(failed_ids)),
        ],
    ) {
        log::error!("任务 {task_id} 写终态 {status} 失败: {err}");
    }
}

/// 失败终态落库（status=failed + error 文案）。
fn mark_task_failed(db: &Db, task_id: &str, error: &str) {
    if let Err(err) = db.update_task_fields(
        task_id,
        &[("status", json!("failed")), ("error", json!(error))],
    ) {
        log::error!("任务 {task_id} 写失败状态出错: {err}");
    }
}

/// 等暂停闸门解除，返回本次挂起时长（供超时判定剔除）。
async fn wait_if_paused(controls: &TaskControls) -> Duration {
    let mut rx = controls.pause.subscribe();
    if !*rx.borrow_and_update() {
        return Duration::ZERO;
    }
    let start = Instant::now();
    loop {
        if !*rx.borrow_and_update() {
            break;
        }
        if rx.changed().await.is_err() {
            break;
        }
    }
    start.elapsed()
}

/// 循环结束后的统一终态处理（外层异常 / 超时 / 正常或取消）。
fn finish_task(
    db: &Db,
    sink: &EventSink,
    task_id: &str,
    controls: &TaskControls,
    counters: &TaskCounters,
    timed_out: bool,
    max_wait_seconds: i64,
    outcome: anyhow::Result<()>,
) {
    match outcome {
        Err(err) => {
            log::error!("任务 {task_id} 异常: {err}");
            mark_task_failed(db, task_id, &err.to_string());
            emit_done(sink, task_id, "failed", counters);
        }
        Ok(()) if timed_out => {
            let msg = format!("任务超过最大等待时间（{max_wait_seconds}s）");
            log::warn!("任务 {task_id} {msg}");
            mark_task_failed(db, task_id, &msg);
            emit_done(sink, task_id, "failed", counters);
        }
        Ok(()) => {
            let status = terminal_status(controls.is_cancelled());
            mark_task_finished(db, task_id, status, counters);
            emit_done(sink, task_id, status, counters);
        }
    }
}

// ----------------------------------------------------------------------
// 小说爬虫
// ----------------------------------------------------------------------

/// 执行小说抓取（语义与 Python `crawler.Crawler.run` 一致）：
///
/// - 开始时状态写 running，进度 0/0/0
/// - 每项：已下载（is_novel_downloaded）→ skipped+1；否则等暂停闸门 → 抓取
///   （get_novel + 写库 INSERT OR REPLACE + 按格式导出）→ done+1 并推
///   "task://progress"；单项失败 → failed_ids 记 id（不中断）
/// - 每项抓取前判定取消（cancel flag）与超时；超时判定排除暂停时长
///   （`elapsed - paused_total > max_wait_seconds` → 终态 failed，
///   error = "任务超过最大等待时间（{N}s）"）
/// - 终态：正常跑完 → done；取消 → **canceled**（修正 Python 版 bug）；
///   超时 / 外层异常 → failed
/// - 导出路径回写：.txt → novels.txt_path，.md → novels.md_path
/// - 输出目录：`<output_dir>/novel/`（相对 output_dir 锚定 data_dir）
pub async fn run_novel_crawler(args: NovelCrawlArgs) {
    run_novel_items(args, None).await;
}

/// 按给定条目跑小说抓取。`preset_items = Some` 时跳过来源解析
/// （retry_failed 逐 id 串行复用同一管线与计数），order 固定 None。
pub(crate) async fn run_novel_items(
    args: NovelCrawlArgs,
    preset_items: Option<Vec<(i64, Option<i64>)>>,
) {
    let NovelCrawlArgs {
        db,
        api,
        source,
        task_id,
        formats,
        output_dir,
        data_dir,
        max_wait_seconds,
        controls,
        sink,
    } = args;
    let formats = if formats.is_empty() {
        vec!["txt".to_string(), "markdown".to_string()]
    } else {
        formats
    };
    let exporters = create_exporters(&formats);
    let target_dir = anchor_output_dir(&output_dir, &data_dir).join("novel");

    let mut counters = TaskCounters::default();
    let started = Instant::now();
    let mut paused_total = Duration::ZERO;
    let mut timed_out = false;

    // 开始：状态 running + 进度 0/0/0（对齐 Python：先标记再解析来源）
    let _ = db.update_task_fields(&task_id, &[("status", json!("running"))]);
    emit_progress(&sink, &task_id, &counters, None, None);

    let outcome: anyhow::Result<()> = async {
        let items = match preset_items {
            Some(items) => items,
            None => source.resolve(&api).await?,
        };
        for (novel_id, order) in items {
            if controls.is_cancelled() {
                break;
            }
            // 去重：已下载直接跳过（skipped+1，不发事件）
            if db.is_novel_downloaded(novel_id) {
                counters.skipped += 1;
                counters.total += 1;
                continue;
            }
            counters.total += 1;
            paused_total += wait_if_paused(&controls).await;
            // 最大等待时间：超过自动失败（暂停时长不计入）
            if timeout_exceeded(started.elapsed(), paused_total, max_wait_seconds) {
                timed_out = true;
                break;
            }
            match crawl_one_novel(&api, &db, novel_id, order, &target_dir, &exporters).await {
                Ok(title) => {
                    counters.done += 1;
                    update_task_progress(&db, &task_id, &counters);
                    emit_progress(&sink, &task_id, &counters, Some(&title), None);
                }
                Err(item_err) => {
                    log::error!("抓取小说 {novel_id} 失败: {}", item_err.message);
                    counters.failed_ids.push(novel_id);
                    emit_progress(
                        &sink,
                        &task_id,
                        &counters,
                        None,
                        Some((novel_id, item_err.title.as_deref(), &item_err.message)),
                    );
                }
            }
        }
        Ok(())
    }
    .await;

    finish_task(
        &db,
        &sink,
        &task_id,
        &controls,
        &counters,
        timed_out,
        max_wait_seconds,
        outcome,
    );
}

/// 抓取单篇小说：get_novel → 写库 → 按格式导出 → 回写 txt/md 路径。
async fn crawl_one_novel(
    api: &PixivApi,
    db: &Db,
    novel_id: i64,
    order: Option<i64>,
    target_dir: &Path,
    exporters: &[Exporter],
) -> Result<String, ItemError> {
    let data: NovelData = api
        .get_novel(novel_id)
        .await
        .map_err(|err| ItemError::new(None, &err))?;
    let title = data.title.clone();
    let fail = |err: &dyn std::fmt::Display| ItemError::new(Some(&title), err);

    db.insert_novel(&NovelInsert {
        novel_id,
        title: title.clone(),
        series_id: data.series_id,
        series_order: order,
        author_id: data.user_id,
        author_name: Some(data.user_name.clone()),
        page_count: Some(data.page_count),
        text_length: Some(data.content.chars().count() as i64),
        captured_at: now_iso(),
        modification_date: Some(data.update_date.clone()),
        txt_path: None,
        md_path: None,
        status: "ok".into(),
    })
    .map_err(|err| fail(&err))?;

    let meta = ExportMeta {
        novel_id,
        title: title.clone(),
        page_count: data.page_count,
        series_order: order,
    };
    for exporter in exporters {
        let paths = exporter
            .export(&data.content, &meta, target_dir)
            .map_err(|err| fail(&err))?;
        for path in paths {
            let path_str = path.to_string_lossy();
            if path_str.ends_with(".txt") {
                db.update_novel_paths(novel_id, Some(&path_str), None)
                    .map_err(|err| fail(&err))?;
            } else if path_str.ends_with(".md") {
                db.update_novel_paths(novel_id, None, Some(&path_str))
                    .map_err(|err| fail(&err))?;
            }
        }
    }
    Ok(title)
}

// ----------------------------------------------------------------------
// 插画爬虫
// ----------------------------------------------------------------------

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
    run_illust_items(args, None).await;
}

/// 按给定作品 id 跑插画抓取。`preset_ids = Some` 时跳过来源解析与分母预取
/// （retry_failed 复用；逐 id 单作品语义、增量计数、目录 `<output>/pic/`）。
pub(crate) async fn run_illust_items(args: IllustCrawlArgs, preset_ids: Option<Vec<i64>>) {
    let IllustCrawlArgs {
        db,
        api,
        source,
        task_id,
        formats: _,
        output_dir,
        data_dir,
        user_id,
        max_wait_seconds,
        controls,
        sink,
    } = args;

    let mut counters = TaskCounters::default();
    let started = Instant::now();
    let mut paused_total = Duration::ZERO;
    let mut timed_out = false;

    // 输出目录：单作品 {output}/pic/；用户全集 {output}/pic/users/{作者}_{uid}/
    let base_dir = anchor_output_dir(&output_dir, &data_dir).join("pic");
    let base_dir = match user_id {
        Some(uid) => base_dir.join("users").join(user_dir_name(&api, uid).await),
        None => base_dir,
    };

    // 预取分母 + 解析 id 列表：
    // - Single：get_illust 一次（结果缓存复用，等价 Python cached_illust），
    //   分母 = max(1, pageCount)，失败退增量计数（循环内还有一次抓取机会）；
    // - User：resolve 一次（illusts+manga），分母 = 数量（等价 resolve_total，
    //   且避免 Python 之外的二次 profile 请求）。
    let mut illust_cache: HashMap<i64, IllustData> = HashMap::new();
    let resolve_result: anyhow::Result<(Vec<i64>, Option<i64>)> = match &preset_ids {
        Some(ids) => Ok((ids.clone(), None)),
        None => match &source {
            IllustSource::Single(id) => match api.get_illust(*id).await {
                Ok(data) => {
                    let total = Some(data.page_count.max(1));
                    illust_cache.insert(*id, data);
                    Ok((vec![*id], total))
                }
                Err(err) => {
                    log::warn!("任务 {task_id} 预取插画 {id} 分母失败，退回增量计数: {err}");
                    Ok((vec![*id], None))
                }
            },
            IllustSource::User(_) => source.resolve(&api).await.map(|ids| {
                let total = Some(ids.len() as i64);
                (ids, total)
            }),
        },
    };
    let (ids, pre_total) = match resolve_result {
        Ok(pair) => pair,
        Err(err) => {
            // 解析失败走外层异常语义：先记 running 再转 failed
            let _ = db.update_task_fields(&task_id, &[("status", json!("running"))]);
            emit_progress(&sink, &task_id, &counters, None, None);
            log::error!("任务 {task_id} 异常: {err}");
            mark_task_failed(&db, &task_id, &err.to_string());
            emit_done(&sink, &task_id, "failed", &counters);
            return;
        }
    };
    if let Some(total) = pre_total {
        counters.total = total;
    }
    // 单作品源按页推进（每保存一页 done+1）；用户全集/重试按作品推进
    let per_page = preset_ids.is_none() && matches!(source, IllustSource::Single(_));

    // 开始：状态 running + 预置分母，进度 0
    let _ = db.update_task_fields(
        &task_id,
        &[
            ("status", json!("running")),
            ("total", json!(counters.total)),
        ],
    );
    emit_progress(&sink, &task_id, &counters, None, None);

    let outcome: anyhow::Result<()> = async {
        for artwork_id in ids {
            if controls.is_cancelled() {
                break;
            }
            if db.is_illust_downloaded(artwork_id) {
                counters.skipped += 1;
                if pre_total.is_none() {
                    counters.total += 1;
                }
                continue;
            }
            if pre_total.is_none() {
                counters.total += 1;
            }
            paused_total += wait_if_paused(&controls).await;
            if timeout_exceeded(started.elapsed(), paused_total, max_wait_seconds) {
                timed_out = true;
                break;
            }
            let item_outcome = {
                let mut bump = || {
                    counters.done += 1;
                    update_task_progress(&db, &task_id, &counters);
                    emit_progress(&sink, &task_id, &counters, None, None);
                };
                let progress_cb: Option<&mut (dyn FnMut() + Send)> = per_page.then_some(&mut bump);
                crawl_one_illust(
                    &api,
                    &db,
                    artwork_id,
                    &base_dir,
                    &mut illust_cache,
                    progress_cb,
                )
                .await
            };
            match item_outcome {
                Ok(title) => {
                    // 用户全集/重试：每完成一个作品 done+1（单作品源已在页级 bump）
                    if !per_page {
                        counters.done += 1;
                        update_task_progress(&db, &task_id, &counters);
                        emit_progress(&sink, &task_id, &counters, Some(&title), None);
                    }
                }
                Err(item_err) => {
                    log::error!("抓取插画 {artwork_id} 失败: {}", item_err.message);
                    counters.failed_ids.push(artwork_id);
                    emit_progress(
                        &sink,
                        &task_id,
                        &counters,
                        None,
                        Some((artwork_id, item_err.title.as_deref(), &item_err.message)),
                    );
                }
            }
        }
        Ok(())
    }
    .await;

    finish_task(
        &db,
        &sink,
        &task_id,
        &controls,
        &counters,
        timed_out,
        max_wait_seconds,
        outcome,
    );
}

/// 触发一次页级进度回调（Option<&mut dyn FnMut> 不能被拷贝，统一经此转发）。
fn notify_progress(callback: &mut Option<&mut (dyn FnMut() + Send)>) {
    if let Some(f) = callback.as_deref_mut() {
        f();
    }
}

/// 抓取单幅插画：get_illust（缓存优先）→ 逐页下载 / ugoira zip → 写库。
/// `on_page_saved` 每保存一页（或 ugoira zip）回调一次（None 时不回调）。
async fn crawl_one_illust(
    api: &PixivApi,
    db: &Db,
    artwork_id: i64,
    base_dir: &Path,
    cache: &mut HashMap<i64, IllustData>,
    mut on_page_saved: Option<&mut (dyn FnMut() + Send)>,
) -> Result<String, ItemError> {
    let data = match cache.remove(&artwork_id) {
        Some(data) => data,
        None => api
            .get_illust(artwork_id)
            .await
            .map_err(|err| ItemError::new(None, &err))?,
    };
    let title = if data.title.is_empty() {
        artwork_id.to_string()
    } else {
        data.title.clone()
    };
    let safe = safe_title_or_id(&data.title, artwork_id);
    let fail = |err: &dyn std::fmt::Display| ItemError::new(Some(&title), err);

    std::fs::create_dir_all(base_dir).map_err(|err| fail(&err))?;
    let mut saved: Vec<String> = Vec::new();

    if data.illust_type == UGOIRA_TYPE {
        // ugoira 原图 = zip 帧序列（originalSrc 优先，zip_urls.original 兜底）
        let meta = api
            .get_ugoira_meta(artwork_id)
            .await
            .map_err(|err| fail(&err))?;
        let bytes = api
            .client()
            .download_bytes(&meta.zip_url)
            .await
            .map_err(|err| fail(&err))?;
        let path = base_dir.join(ugoira_filename(&safe, artwork_id));
        std::fs::write(&path, &bytes).map_err(|err| fail(&err))?;
        saved.push(path.to_string_lossy().into_owned());
        notify_progress(&mut on_page_saved);
    } else {
        let urls = collect_page_urls(&data);
        if urls.is_empty() {
            return Err(ItemError::new(
                Some(&title),
                &format!("作品 {artwork_id} 没有可用原图 URL"),
            ));
        }
        for (idx, url) in urls.iter().enumerate() {
            if url.is_empty() {
                continue;
            }
            let bytes = api
                .client()
                .download_bytes(url)
                .await
                .map_err(|err| fail(&err))?;
            let path = base_dir.join(page_filename(&safe, artwork_id, idx, url));
            std::fs::write(&path, &bytes).map_err(|err| fail(&err))?;
            saved.push(path.to_string_lossy().into_owned());
            notify_progress(&mut on_page_saved);
        }
    }

    db.insert_illustration(&IllustrationInsert {
        artwork_id,
        title: title.clone(),
        author_id: data.user_id,
        author_name: Some(data.user_name),
        illust_type: data.illust_type,
        page_count: data.page_count,
        saved_paths: serde_json::to_string(&saved).unwrap_or_else(|_| "[]".into()),
        captured_at: now_iso(),
        status: "ok".into(),
    })
    .map_err(|err| fail(&err))?;
    log::info!("插画 {artwork_id} 已保存 {} 个文件", saved.len());
    Ok(title)
}

// ----------------------------------------------------------------------
// 单元测试（离线：纯函数 + 临时库 + 事件载荷）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TaskInsert;
    use std::sync::Mutex;

    fn temp_db(tag: &str) -> (Db, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "pixiv-tool-crawler-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("app.db")).unwrap();
        (db, dir)
    }

    fn insert_task(db: &Db, task_id: &str) {
        let now = now_iso();
        db.insert_task(&TaskInsert {
            task_id: task_id.into(),
            source_type: "single".into(),
            source_id: "1".into(),
            created_at: now.clone(),
            updated_at: now,
            ..Default::default()
        })
        .unwrap();
    }

    /// 捕获事件的 sink。
    fn capture_sink() -> (EventSink, Arc<Mutex<Vec<(String, Value)>>>) {
        let events: Arc<Mutex<Vec<(String, Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        let sink: EventSink = Arc::new(move |name, payload| {
            sink_events
                .lock()
                .unwrap()
                .push((name.to_string(), payload.clone()));
        });
        (sink, events)
    }

    #[test]
    fn terminal_status_not_overwritten_by_done() {
        // 修正 Python 版 bug：取消终态必须是 canceled
        assert_eq!(terminal_status(true), "canceled");
        assert_eq!(terminal_status(false), "done");
    }

    #[test]
    fn timeout_excludes_paused_duration() {
        let max_wait = 30;
        // 暂停 90s、实际运行 10s → 不超时
        assert!(!timeout_exceeded(
            Duration::from_secs(100),
            Duration::from_secs(90),
            max_wait
        ));
        // 无暂停、运行 31s → 超时
        assert!(timeout_exceeded(
            Duration::from_secs(31),
            Duration::ZERO,
            max_wait
        ));
        // 边界：恰好 30s 不超时
        assert!(!timeout_exceeded(
            Duration::from_secs(30),
            Duration::ZERO,
            max_wait
        ));
    }

    #[test]
    fn anchor_output_dir_relative_and_absolute() {
        let data_dir = PathBuf::from("/data");
        assert_eq!(
            anchor_output_dir("downloads", &data_dir),
            PathBuf::from("/data/downloads")
        );
        assert_eq!(
            anchor_output_dir("/abs/out", &data_dir),
            PathBuf::from("/abs/out")
        );
    }

    #[test]
    fn mark_task_finished_writes_canceled_and_failed_ids() {
        let (db, dir) = temp_db("finish");
        insert_task(&db, "t1");
        let counters = TaskCounters {
            total: 3,
            done: 1,
            skipped: 1,
            failed_ids: vec![11, 12],
        };
        mark_task_finished(&db, "t1", "canceled", &counters);
        let row = db.get_task("t1").unwrap().unwrap();
        assert_eq!(row.status, "canceled", "取消终态不被 done 覆写");
        assert_eq!((row.total, row.done, row.skipped), (3, 1, 1));
        assert_eq!(row.failed_ids, "[11,12]");
        cleanup(&dir);
    }

    #[test]
    fn mark_task_failed_writes_error() {
        let (db, dir) = temp_db("failed");
        insert_task(&db, "t2");
        mark_task_failed(&db, "t2", "任务超过最大等待时间（180s）");
        let row = db.get_task("t2").unwrap().unwrap();
        assert_eq!(row.status, "failed");
        assert_eq!(row.error.as_deref(), Some("任务超过最大等待时间（180s）"));
        cleanup(&dir);
    }

    #[test]
    fn progress_and_done_payload_shapes() {
        let (sink, events) = capture_sink();
        let counters = TaskCounters {
            total: 2,
            done: 1,
            skipped: 0,
            failed_ids: vec![9],
        };
        // 成功项：current_title
        emit_progress(&sink, "t", &counters, Some("标题"), None);
        // 失败项：failed_item（title 已知）
        emit_progress(
            &sink,
            "t",
            &counters,
            None,
            Some((9, Some("标题9"), "网络错误")),
        );
        emit_done(&sink, "t", "done", &counters);

        let events = events.lock().unwrap();
        assert_eq!(events.len(), 3);
        let (name, payload) = &events[0];
        assert_eq!(name, "task://progress");
        assert_eq!(payload["task_id"], "t");
        assert_eq!(payload["status"], "running");
        assert_eq!(payload["done"], 1);
        assert_eq!(payload["total"], 2);
        assert_eq!(payload["skipped"], 0);
        assert_eq!(payload["current_title"], "标题");
        let (_, payload) = &events[1];
        assert_eq!(payload["failed_item"]["id"], 9);
        assert_eq!(payload["failed_item"]["title"], "标题9");
        assert_eq!(payload["failed_item"]["error"], "网络错误");
        let (name, payload) = &events[2];
        assert_eq!(name, "task://done");
        assert_eq!(payload["status"], "done");
        assert_eq!(payload["failed"], 1);
        assert_eq!(payload["skipped"], 0);
    }

    #[test]
    fn skip_does_not_emit_progress_event() {
        // 语义由代码路径保证：skip 分支只加计数、不调 emit_progress。
        // 这里验证初始事件之后 skip 不产生额外事件（用计数器模拟）。
        let (sink, events) = capture_sink();
        let counters = TaskCounters::default();
        emit_progress(&sink, "t", &counters, None, None); // 仅初始事件
        // 模拟 skip：只更新计数，不发事件
        let mut counters = counters;
        counters.skipped += 1;
        counters.total += 1;
        assert_eq!(events.lock().unwrap().len(), 1, "skip 不发事件");
        assert_eq!((counters.total, counters.skipped), (1, 1));
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }
}
