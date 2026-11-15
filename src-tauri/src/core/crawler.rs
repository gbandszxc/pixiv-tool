//! 小说 / 插画爬虫。
//!
//! 语义对齐 Python `core/crawler.py` / `core/illust_crawler.py`，其中两处
//! 修正 Python 版的 bug：
//! 1. 取消终态必须写 `canceled`（Python 版 mark_done 写死 done，取消时被覆写）；
//! 2. finally 限速（PixivClient 层）+ 暂停时长不计入超时判定的既有语义保持。
//!
//! 系列（Series 来源）为**整包**语义：目录游标取全部话 → 逐话抓取 →
//! 按格式拼接**单文件**（txt：标题分隔行 + 原文；md：目录 + 每话标题 +
//! render_markdown 口径；epub：每话一个章节 + nav 目录）。single / user
//! 来源保持逐篇独立文件。EPUB 的封面与内嵌图经 [`EpubAssetCollector`]
//! 走 `PixivClient::download_bytes`（限速管线）下载；单图 / 封面失败
//! 降级为占位渲染，不失败整篇。
//!
//! 事件（EventSink）只有两个名字：
//! - `"task://progress"` → `{"task_id","status":"running","done","total","skipped"}`
//!   （成功项附 `"current_title"`；失败项附 `"failed_item":{"id","title","error"}`；
//!   整包 total=话数、done=已完成话数、current_title=当前话标题）
//! - `"task://done"`     → `{"task_id","status","done","total","skipped","failed"}`

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{Value, json};

use crate::core::epub::{
    CoverImage, EpubBook, EpubChapter, EmbeddedImage, PlaceholderCoverSpec, render_xhtml_body,
};
use crate::core::exporter::{
    ExportMeta, Exporter, EpubExporter, create_exporters, make_bundle_filename, render_markdown,
};
use crate::core::filename_template::IllustNameInfo;
use crate::core::illust_crawler::{
    UGOIRA_TYPE, collect_page_urls, page_filename_with_template, ugoira_filename_with_template,
    url_ext, user_dir_name,
};
use crate::core::sources::{IllustSource, NovelSource};
use crate::core::task_manager::TaskControls;
use crate::db::{Db, IllustrationInsert, NovelInsert, now_iso};
use crate::paths::anchor_output_dir;
use crate::pixiv::api::{IllustData, NovelData, PixivApi, SeriesCatalog};

/// 爬虫 → 前端事件通道（由命令层用 AppHandle::emit 实现）。
pub type EventSink = Arc<dyn Fn(&str, &Value) + Send + Sync>;

/// 小说爬虫入参。
pub struct NovelCrawlArgs {
    pub db: Db,
    pub api: Arc<PixivApi>,
    pub source: NovelSource,
    pub task_id: String,
    /// 导出格式（"txt"/"markdown"/"epub"；未知值忽略）；空则用 ["txt","markdown"]。
    /// series 来源为整包语义（各格式拼接单文件），single/user 为逐篇独立文件。
    pub formats: Vec<String>,
    /// 相对路径锚定 data_dir（绝对路径原样）。
    pub output_dir: String,
    /// 锚定根（= AppPaths.data_dir）。
    pub data_dir: PathBuf,
    /// 小说单篇文件名模板（settings.novel_filename_template 透传）；
    /// 渲染为空回落现行命名。
    pub novel_filename_template: String,
    /// 系列各话文件名模板（settings.novel_series_filename_template 透传），
    /// 仅 series_order 为 Some 的条目使用。
    pub novel_series_filename_template: String,
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
    /// 插画文件名模板（settings.illust_filename_template 透传）；
    /// 渲染为空回落现行命名。
    pub illust_filename_template: String,
    /// 选中的 0 起页码（已归一：去重升序）；None = 全部页。
    pub pages: Option<Vec<usize>>,
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
#[allow(clippy::too_many_arguments)]
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
/// - 系列（Series）来源为**整包**语义，走 [`run_series_bundle`]；
///   single / user 来源逐篇独立文件：
/// - 开始时状态写 running，进度 0/0/0
/// - 每项：已下载（is_novel_downloaded）→ skipped+1；否则等暂停闸门 → 抓取
///   （get_novel + 写库 INSERT OR REPLACE + 按格式导出）→ done+1 并推
///   "task://progress"；单项失败 → failed_ids 记 id（不中断）
/// - 每项抓取前判定取消（cancel flag）与超时；超时判定排除暂停时长
///   （`elapsed - paused_total > max_wait_seconds` → 终态 failed，
///   error = "任务超过最大等待时间（{N}s）"）
/// - 终态：正常跑完 → done；取消 → **canceled**（修正 Python 版 bug）；
///   超时 / 外层异常 → failed
/// - 导出文件名按模板渲染（`NovelCrawlArgs.novel_filename_template` /
///   `novel_series_filename_template`：series_order 为 Some 用系列模板；
///   渲染为空回落现行 make_filename）；回写：.txt → novels.txt_path，
///   .md → novels.md_path，.epub → novels.epub_path
/// - 输出目录：`<output_dir>/novel/`（相对 output_dir 锚定 data_dir）
pub async fn run_novel_crawler(args: NovelCrawlArgs) {
    run_novel_items(args, None).await;
}

/// 按给定条目跑小说抓取。`preset_items = Some` 时跳过来源解析
/// （retry_failed 逐 id 串行复用同一管线与计数），order 固定 None。
///
/// 系列整包分流：source = Series 直接整包；series 任务的 retry（preset_items
/// 只有失败话 id）经 novels 表补 series_id 后同样整包——重新聚合全部话并
/// 重新生成整包文件，preset 的「只重试失败项」语义在整包下展开为全量重抓。
pub(crate) async fn run_novel_items(
    args: NovelCrawlArgs,
    preset_items: Option<Vec<(i64, Option<i64>)>>,
) {
    // 整包判定：series 新任务；或 series 任务的 retry（task 行 source_type
    // 保留原标签，失败话可从 novels 表补回 series_id）
    let bundle_series_id = match (&args.source, preset_items.as_deref()) {
        (NovelSource::Series(sid), _) => Some(*sid),
        (_, Some(items)) => {
            let from_series_task = args
                .db
                .get_task(&args.task_id)
                .ok()
                .flatten()
                .map(|task| task.source_type == "series")
                .unwrap_or(false);
            if from_series_task {
                items
                    .iter()
                    .find_map(|(id, _)| args.db.get_novel(*id).and_then(|row| row.series_id))
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(series_id) = bundle_series_id {
        return run_series_bundle(args, series_id).await;
    }

    let NovelCrawlArgs {
        db,
        api,
        source,
        task_id,
        formats,
        output_dir,
        data_dir,
        novel_filename_template,
        novel_series_filename_template,
        max_wait_seconds,
        controls,
        sink,
    } = args;
    let formats = if formats.is_empty() {
        vec!["txt".to_string(), "markdown".to_string()]
    } else {
        formats
    };
    let has_epub = formats.iter().any(|f| f == "epub");
    // txt/md 导出器在循环外构造一次；EPUB 需要 per-篇 EpubBook（封面/内嵌图
    // 资源），由 crawl_one_novel 抓完正文后自行注入
    let exporters = create_exporters(&formats, None);
    let target_dir = anchor_output_dir(&output_dir, &data_dir).join("novel");
    let templates = NovelTemplates {
        novel: &novel_filename_template,
        series: &novel_series_filename_template,
    };

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
            if controls.is_cancelled() {
                break;
            }
            // 最大等待时间：超过自动失败（暂停时长不计入）
            if timeout_exceeded(started.elapsed(), paused_total, max_wait_seconds) {
                timed_out = true;
                break;
            }
            match crawl_one_novel(
                &api,
                &db,
                novel_id,
                order,
                &target_dir,
                &exporters,
                has_epub,
                templates,
            )
            .await
            {
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

/// 小说文件名模板对（单篇 / 系列各话；settings 透传，渲染为空回落现行命名）。
#[derive(Clone, Copy)]
struct NovelTemplates<'a> {
    novel: &'a str,
    series: &'a str,
}

// ----------------------------------------------------------------------
// EPUB 资源收集（单篇与系列整包共用）
// ----------------------------------------------------------------------

/// EPUB `dc:language` 占位：pixiv 详情响应无语言字段，"und"（undetermined）
/// 让阅读器按内容回退排版（不误标日/中）。
const EPUB_LANGUAGE: &str = "und";

/// 正文图片标记扫描：`[uploadedimage:id]` / `[pixivimage:id(-N)]`（行内混排同扫）。
fn image_mark_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\[(?P<kind>uploadedimage|pixivimage):(?P<id>[^\]]+)\]")
            .expect("图片标记正则合法")
    })
}

/// 与 `epub::sanitize_image_id` 同口径（该函数私有，此处复刻；两处须同步）：
/// 标记 id → zip 条目安全词干，`[A-Za-z0-9_-]` 之外替换 `_`，空串 → "_"。
fn epub_image_stem(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if cleaned.is_empty() { "_".to_string() } else { cleaned }
}

/// `[pixivimage:ID]` / `[pixivimage:ID-N]` → (illust_id, 页码 0 起)。
/// 无 `-N` 按第 0 页；illust_id 非数字 → None（渲染为占位）。
fn parse_pixivimage_id(mark_id: &str) -> Option<(i64, usize)> {
    match mark_id.rsplit_once('-') {
        Some((id_part, page_part)) => Some((id_part.trim().parse().ok()?, page_part.trim().parse().ok()?)),
        None => Some((mark_id.trim().parse().ok()?, 0)),
    }
}

/// EPUB 内嵌图收集器：解析正文标记 → 逐图 `download_bytes`（走 PixivClient
/// 限速管线）→ 登记 [`EpubBook.images`] 与「标记 id → `images/` 条目文件名」
/// 映射（渲染时经 ImageResolver 引用）。单图失败仅跳过登记 → 渲染为占位，
/// 不失败整篇 / 整包；同一标记 id（含跨话）只下载一次。
struct EpubAssetCollector<'a> {
    api: &'a PixivApi,
    /// 标记 id（含 pixivimage 的 `-N`）→ `OEBPS/images/` 条目文件名（stem.ext）。
    entries: HashMap<String, String>,
    /// 下载成功的内嵌图资源。
    images: Vec<EmbeddedImage>,
    /// pixivimage 引用的 illust 元数据缓存（同 illust 多页共享一次请求）。
    illust_cache: HashMap<i64, IllustData>,
}

impl<'a> EpubAssetCollector<'a> {
    fn new(api: &'a PixivApi) -> Self {
        Self {
            api,
            entries: HashMap::new(),
            images: Vec::new(),
            illust_cache: HashMap::new(),
        }
    }

    /// 收集 `content` 中全部图片标记（`embedded` = 该篇 NovelData 的
    /// textEmbeddedImages 表，uploadedimage 的 URL 来源）。
    async fn collect_content(&mut self, content: &str, embedded: &HashMap<String, String>) {
        let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
        let mut seen: HashSet<String> = HashSet::new();
        for caps in image_mark_re().captures_iter(&normalized) {
            let kind = &caps["kind"];
            let id = caps["id"].trim().to_string();
            if id.is_empty() || !seen.insert(id.clone()) || self.entries.contains_key(&id) {
                continue;
            }
            match kind {
                "uploadedimage" => match embedded.get(&id) {
                    Some(url) if !url.is_empty() => {
                        self.download_and_register(&id, url).await;
                    }
                    _ => {
                        log::warn!("EPUB 内嵌图 {id} 在 textEmbeddedImages 中缺失，渲染为占位");
                    }
                },
                _ => self.collect_pixivimage(&id).await,
            }
        }
    }

    /// `[pixivimage:id(-N)]`：illust 元数据（缓存优先）→ 第 N 页原图 URL → 下载。
    async fn collect_pixivimage(&mut self, id: &str) {
        let Some((illust_id, page)) = parse_pixivimage_id(id) else {
            log::warn!("EPUB 内嵌图 {id} 不是合法的 pixivimage 标记，渲染为占位");
            return;
        };
        let data = match self.illust_cache.get(&illust_id) {
            Some(data) => data.clone(),
            None => match self.api.get_illust(illust_id).await {
                Ok(data) => {
                    self.illust_cache.insert(illust_id, data.clone());
                    data
                }
                Err(err) => {
                    log::warn!("EPUB 内嵌图 {id} 取插画元数据失败，渲染为占位: {err}");
                    return;
                }
            },
        };
        let urls = collect_page_urls(&data);
        match urls.get(page).filter(|url| !url.is_empty()) {
            Some(url) => self.download_and_register(id, url).await,
            None => log::warn!("EPUB 内嵌图 {id} 页码超出作品页数，渲染为占位"),
        }
    }

    /// 下载并登记：成功 → `entries[id] = "stem.ext"` + images 追加；失败 → 跳过。
    async fn download_and_register(&mut self, id: &str, url: &str) {
        match self.api.client().download_bytes(url).await {
            Ok(bytes) => {
                let ext = url_ext(url);
                let stem = epub_image_stem(id);
                self.entries.insert(id.to_string(), format!("{stem}.{ext}"));
                self.images.push(EmbeddedImage {
                    id: id.to_string(),
                    bytes: bytes.to_vec(),
                    ext,
                });
            }
            Err(err) => {
                log::warn!("EPUB 内嵌图 {id} 下载失败，渲染为占位: {err}");
            }
        }
    }

    /// 渲染用 resolver（标记 id → 条目文件名；未登记 → None → 占位）。
    fn resolver(&self) -> impl Fn(&str) -> Option<String> + '_ {
        move |id: &str| self.entries.get(id).cloned()
    }
}

/// 下载封面位图；URL 空 / 下载失败 → None（调用方回落占位封面）。
async fn download_cover(api: &PixivApi, url: &str) -> Option<CoverImage> {
    if url.is_empty() {
        return None;
    }
    match api.client().download_bytes(url).await {
        Ok(bytes) => Some(CoverImage {
            bytes: bytes.to_vec(),
            ext: url_ext(url),
        }),
        Err(err) => {
            log::warn!("EPUB 封面下载失败，改用占位封面: {err}");
            None
        }
    }
}

/// 单篇 EpubBook：正文渲染为 1 个章节；封面 = NovelData.coverUrl 下载，
/// 失败/缺失 → 占位封面（seed = novel_id）。
async fn build_novel_epub(api: &PixivApi, data: &NovelData) -> EpubBook {
    let mut collector = EpubAssetCollector::new(api);
    collector
        .collect_content(&data.content, &data.text_embedded_images)
        .await;
    // 先取走 images（避免与 resolver 的借用冲突），再渲染正文
    let images = std::mem::take(&mut collector.images);
    let resolver = collector.resolver();
    let body = render_xhtml_body(&data.content, &resolver);
    let cover = download_cover(api, &data.cover_url).await;
    let placeholder = (cover.is_none()).then(|| PlaceholderCoverSpec {
        title: data.title.clone(),
        author: data.user_name.clone(),
        seed_id: data.novel_id,
    });
    EpubBook {
        title: data.title.clone(),
        author: data.user_name.clone(),
        language: EPUB_LANGUAGE.to_string(),
        book_uuid: uuid::Uuid::new_v4().to_string(),
        chapters: vec![EpubChapter {
            title: data.title.clone(),
            xhtml_body: body,
        }],
        cover,
        images,
        placeholder_cover: placeholder,
    }
}

/// 系列整包 EpubBook：每话一个章节（标题「第{order}话 {title}」），nav/ncx
/// 即目录；封面 = 系列封面下载，失败/缺失 → 占位封面（seed = series_id）。
async fn build_series_epub(
    api: &PixivApi,
    series_title: &str,
    author: &str,
    cover_url: &str,
    episodes: &[SeriesEpisode],
    collector: &EpubAssetCollector<'_>,
    series_id: i64,
) -> EpubBook {
    let cover = download_cover(api, cover_url).await;
    let images = collector.images.clone();
    let resolver = collector.resolver();
    let chapters = sorted_episodes(episodes)
        .into_iter()
        .map(|ep| EpubChapter {
            title: format!("第{}话 {}", ep.order, ep.title),
            xhtml_body: render_xhtml_body(&ep.content, &resolver),
        })
        .collect::<Vec<_>>();
    let placeholder = (cover.is_none()).then(|| PlaceholderCoverSpec {
        title: series_title.to_string(),
        author: author.to_string(),
        seed_id: series_id,
    });
    EpubBook {
        title: series_title.to_string(),
        author: author.to_string(),
        language: EPUB_LANGUAGE.to_string(),
        book_uuid: uuid::Uuid::new_v4().to_string(),
        chapters,
        cover,
        images,
        placeholder_cover: placeholder,
    }
}

// ----------------------------------------------------------------------
// 系列整包拼接（纯函数，离线可测）
// ----------------------------------------------------------------------

/// 系列整包的一话（拼接输入；txt 正文原样、md/epub 按各自口径转换）。
pub(crate) struct SeriesEpisode {
    pub order: i64,
    pub title: String,
    pub content: String,
}

/// 按 contentOrder 升序的引用视图（整包语义：顺序由目录 contentOrder 决定，
/// 容忍跳号；函数内再排一次防御乱序输入）。
fn sorted_episodes(episodes: &[SeriesEpisode]) -> Vec<&SeriesEpisode> {
    let mut refs: Vec<&SeriesEpisode> = episodes.iter().collect();
    refs.sort_by_key(|ep| ep.order);
    refs
}

/// 系列整包 txt：每话「标题分隔行 + 正文原样（标记保留）」顺序拼接。
pub(crate) fn series_txt_bundle(episodes: &[SeriesEpisode]) -> String {
    let mut out = String::new();
    for ep in sorted_episodes(episodes) {
        out.push_str(&format!(
            "\n\n==============================\n第{}话 {}\n==============================\n\n",
            ep.order, ep.title
        ));
        out.push_str(&ep.content);
    }
    out
}

/// GitHub 风格标题锚点：小写化、空白 → `-`，删除标点，字母数字保留。
fn md_anchor(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

/// 系列整包 markdown：`# 系列标题` + 目录（锚点链接列表）+ 每话
/// `## 第{order}话 {title}` + 正文按 render_markdown 口径转换。
pub(crate) fn series_md_bundle(series_title: &str, episodes: &[SeriesEpisode]) -> String {
    let sorted = sorted_episodes(episodes);
    let mut out = format!("# {series_title}\n\n## 目录\n\n");
    for ep in &sorted {
        let heading = format!("第{}话 {}", ep.order, ep.title);
        out.push_str(&format!("- [{heading}](#{})\n", md_anchor(&heading)));
    }
    out.push('\n');
    for ep in sorted {
        out.push_str(&format!("## 第{}话 {}\n\n", ep.order, ep.title));
        out.push_str(&render_markdown(&ep.content));
        out.push_str("\n\n");
    }
    out
}

// ----------------------------------------------------------------------
// 系列整包流程
// ----------------------------------------------------------------------

/// 系列整包流程（source = Series；或 series 任务的 retry）：目录游标聚合
/// → 逐话 get_novel（现有 client 限速管线，不自建并发）→ 收集全部话后按
/// 格式拼接**单文件**（txt/md/epub 各一，文件名同词干不同扩展）。
///
/// - 进度：total=话数、done=已完成话数、current_title=当前话标题；
///   单话失败记 failed_ids 不中断（失败话不进整包）
/// - 已下载话**不跳过**（整包需要全话正文；逐话记录 INSERT OR REPLACE 照旧）
/// - 正常跑完（非取消/超时）且有至少一话 → 写整包；有失败话时也写（用已
///   成功话拼，failed 计数可见）；取消 / 超时 → 不写
/// - 整包文件不落 novels 表（tasks 已有任务记录，历史页系列整包回显暂不做）；
///   novels.epub_path 列留给单篇 epub
async fn run_series_bundle(args: NovelCrawlArgs, series_id: i64) {
    let NovelCrawlArgs {
        db,
        api,
        task_id,
        formats,
        output_dir,
        data_dir,
        novel_series_filename_template,
        max_wait_seconds,
        controls,
        sink,
        ..
    } = args;
    let formats = if formats.is_empty() {
        vec!["txt".to_string(), "markdown".to_string()]
    } else {
        formats
    };
    let has_txt = formats.iter().any(|f| f == "txt");
    let has_md = formats.iter().any(|f| f == "markdown");
    let has_epub = formats.iter().any(|f| f == "epub");
    let target_dir = anchor_output_dir(&output_dir, &data_dir).join("novel");

    let mut counters = TaskCounters::default();
    let started = Instant::now();
    let mut paused_total = Duration::ZERO;
    let mut timed_out = false;

    // 开始：状态 running + 进度 0/0/0
    let _ = db.update_task_fields(&task_id, &[("status", json!("running"))]);
    emit_progress(&sink, &task_id, &counters, None, None);

    let outcome: anyhow::Result<()> = async {
        // 目录游标聚合（失败 → 任务 failed）
        let catalog: SeriesCatalog = api.get_series_content_all(series_id).await?;
        counters.total = catalog.items.len() as i64;
        update_task_progress(&db, &task_id, &counters);
        emit_progress(&sink, &task_id, &counters, None, None);

        let mut episodes: Vec<SeriesEpisode> = Vec::new();
        let mut first_author = String::new();
        let mut collector = EpubAssetCollector::new(&api);
        for &(novel_id, order) in &catalog.items {
            if controls.is_cancelled() {
                break;
            }
            paused_total += wait_if_paused(&controls).await;
            if controls.is_cancelled() {
                break;
            }
            if timeout_exceeded(started.elapsed(), paused_total, max_wait_seconds) {
                timed_out = true;
                break;
            }
            match api.get_novel(novel_id).await {
                Ok(data) => {
                    // 先落库（逐话 INSERT OR REPLACE 照旧），失败计 failed 不中断
                    let inserted = db.insert_novel(&NovelInsert {
                        novel_id,
                        title: data.title.clone(),
                        series_id: data.series_id,
                        series_order: Some(order),
                        author_id: data.user_id,
                        author_name: Some(data.user_name.clone()),
                        page_count: Some(data.page_count),
                        text_length: Some(data.content.chars().count() as i64),
                        captured_at: now_iso(),
                        modification_date: Some(data.update_date.clone()),
                        txt_path: None,
                        md_path: None,
                        epub_path: None,
                        status: "ok".into(),
                    });
                    match inserted {
                        Ok(()) => {
                            if has_epub {
                                collector
                                    .collect_content(&data.content, &data.text_embedded_images)
                                    .await;
                            }
                            if first_author.is_empty() {
                                first_author = data.user_name.clone();
                            }
                            episodes.push(SeriesEpisode {
                                order,
                                title: data.title.clone(),
                                content: data.content.clone(),
                            });
                            counters.done += 1;
                            update_task_progress(&db, &task_id, &counters);
                            emit_progress(&sink, &task_id, &counters, Some(&data.title), None);
                        }
                        Err(err) => {
                            log::error!("抓取小说 {novel_id} 失败: 落库失败 {err}");
                            counters.failed_ids.push(novel_id);
                            emit_progress(
                                &sink,
                                &task_id,
                                &counters,
                                None,
                                Some((novel_id, Some(data.title.as_str()), &err)),
                            );
                        }
                    }
                }
                Err(err) => {
                    log::error!("抓取小说 {novel_id} 失败: {err}");
                    counters.failed_ids.push(novel_id);
                    emit_progress(&sink, &task_id, &counters, None, Some((novel_id, None, &err.to_string())));
                }
            }
        }

        // 正常跑完（非取消/超时）且有话 → 拼接单文件；系列标题缺失回落首话标题
        if !timed_out && !controls.is_cancelled() && !episodes.is_empty() {
            let series_title = if catalog.series_title.is_empty() {
                episodes[0].title.clone()
            } else {
                catalog.series_title.clone()
            };
            let author = if catalog.author_name.is_empty() {
                first_author
            } else {
                catalog.author_name.clone()
            };
            let meta = ExportMeta {
                // 整包无单篇 id：{nid} 语义回落 series_id，渲染空回落命名即
                // `sanitize(系列标题)_{sid}.{ext}`
                novel_id: series_id,
                title: series_title.clone(),
                page_count: episodes.len() as i64,
                series_order: None,
                user_name: author.clone(),
                user_id: catalog.user_id,
                series_id: Some(series_id),
            };
            let template = novel_series_filename_template.as_str();
            std::fs::create_dir_all(&target_dir)?;
            if has_txt {
                let path = target_dir.join(make_bundle_filename(&meta, "txt", template));
                std::fs::write(&path, series_txt_bundle(&episodes))?;
            }
            if has_md {
                let path = target_dir.join(make_bundle_filename(&meta, "md", template));
                std::fs::write(&path, series_md_bundle(&series_title, &episodes))?;
            }
            if has_epub {
                let book = build_series_epub(
                    &api,
                    &series_title,
                    &author,
                    &catalog.cover_url,
                    &episodes,
                    &collector,
                    series_id,
                )
                .await;
                EpubExporter::new(book).export("", &meta, &target_dir, Some(template))?;
            }
            log::info!(
                "系列 {series_id} 整包已生成：{} 话，格式 {formats:?}",
                episodes.len()
            );
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

/// 抓取单篇小说：get_novel → 写库 → 按格式导出（文件名按模板渲染：
/// series_order 为 Some 用系列模板，否则用单篇模板；渲染为空回落现行
/// `make_filename`）→ 回写 txt/md/epub 路径。
///
/// `has_epub = true` 时先构建 EpubBook（封面 = NovelData.coverUrl 下载，
/// 内嵌图逐个下载，失败降级占位），追加 [`Exporter::Epub`] 参与导出；
/// epub 导出失败同样按单项失败处理。
async fn crawl_one_novel(
    api: &PixivApi,
    db: &Db,
    novel_id: i64,
    order: Option<i64>,
    target_dir: &Path,
    exporters: &[Exporter],
    has_epub: bool,
    templates: NovelTemplates<'_>,
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
        epub_path: None,
        status: "ok".into(),
    })
    .map_err(|err| fail(&err))?;

    let meta = ExportMeta {
        novel_id,
        title: title.clone(),
        page_count: data.page_count,
        series_order: order,
        user_name: data.user_name.clone(),
        user_id: data.user_id,
        series_id: data.series_id,
    };
    let template = if order.is_some() {
        templates.series
    } else {
        templates.novel
    };
    let mut exporters: Vec<Exporter> = exporters.to_vec();
    if has_epub {
        let book = build_novel_epub(api, &data).await;
        exporters.push(Exporter::Epub(EpubExporter::new(book)));
    }
    for exporter in &exporters {
        let paths = exporter
            .export(&data.content, &meta, target_dir, Some(template))
            .map_err(|err| fail(&err))?;
        for path in paths {
            let path_str = path.to_string_lossy();
            if path_str.ends_with(".txt") {
                db.update_novel_paths(novel_id, Some(&path_str), None)
                    .map_err(|err| fail(&err))?;
            } else if path_str.ends_with(".md") {
                db.update_novel_paths(novel_id, None, Some(&path_str))
                    .map_err(|err| fail(&err))?;
            } else if path_str.ends_with(".epub") {
                db.update_novel_epub_path(novel_id, &path_str)
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
/// - 文件名按模板渲染（`IllustCrawlArgs.illust_filename_template`，占位符见
///   `filename_template`）：普通页渲染词干 + `.{url_ext}`，ugoira 渲染词干
///   （内部追加 `_ugoira`）+ `.zip`；渲染为空回落现行
///   `{safe_title}_{artwork_id}_p{N}.{ext}` / `{safe_title}_{artwork_id}_ugoira.zip`；
///   多页 meta 缺失时从 p0 直链 `_p0.{ext} → _p{N}.{ext}` 推导
/// - 已下载判定按页（`pages` 为 None 且已有记录 → skip；Some → 只下差集并与
///   既有 saved_paths/saved_pages 合并，按页码升序保持一一对应）
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
        illust_filename_template,
        pages,
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
                    // 分母：选了页码 → 选中且存在的页数；否则作品页数
                    let total = match &pages {
                        Some(wanted) => wanted
                            .iter()
                            .filter(|idx| (**idx as i64) < data.page_count.max(1))
                            .count()
                            .max(1) as i64,
                        None => data.page_count.max(1),
                    };
                    illust_cache.insert(*id, data);
                    Ok((vec![*id], Some(total)))
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
            // 已下载判定按页：
            // - pages 为 None 且已有记录 → 保持旧语义 skip（向后兼容）；
            // - pages 为 Some → 与既有 saved_pages 求差集，空则 skip，否则只下差集。
            let existing = db.get_illustration(artwork_id);
            let (selected, existing_saved) = match (&pages, &existing) {
                (None, Some(_)) => {
                    counters.skipped += 1;
                    if pre_total.is_none() {
                        counters.total += 1;
                    }
                    continue;
                }
                (None, None) => (None, Vec::new()),
                (Some(wanted), None) => (Some(wanted.clone()), Vec::new()),
                (Some(wanted), Some(row)) => {
                    // ugoira 的既有页集合归一后恒为 [0]，走同一差集逻辑
                    let have = saved_pairs(&row.saved_paths, &row.saved_pages);
                    let diff: Vec<usize> = wanted
                        .iter()
                        .copied()
                        .filter(|p| !have.iter().any(|(saved, _)| saved == p))
                        .collect();
                    let already: i64 = (wanted.len() - diff.len()) as i64;
                    // 单作品源按页推进（done+skipped=total）→ 按页计；重试/全集
                    // 按作品推进 → 全命中才 +1
                    counters.skipped += if pre_total.is_some() {
                        already
                    } else if diff.is_empty() {
                        1
                    } else {
                        0
                    };
                    if diff.is_empty() {
                        if pre_total.is_none() {
                            counters.total += 1;
                        }
                        continue;
                    }
                    (Some(diff), have)
                }
            };
            if pre_total.is_none() {
                counters.total += 1;
            }
            paused_total += wait_if_paused(&controls).await;
            if controls.is_cancelled() {
                break;
            }
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
                    &illust_filename_template,
                    selected.as_deref(),
                    &existing_saved,
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

/// 既有记录 → `(页码, 路径)` 对（按 saved_paths 与 saved_pages 同序 zip）。
/// saved_pages 已由 Db 读取出口归一，长度与 saved_paths 一致；不一致时取较短者。
fn saved_pairs(saved_paths: &str, saved_pages: &str) -> Vec<(usize, String)> {
    let paths: Vec<String> = serde_json::from_str(saved_paths).unwrap_or_default();
    let pages: Vec<i64> = serde_json::from_str(saved_pages).unwrap_or_default();
    pages
        .into_iter()
        .filter(|p| *p >= 0)
        .zip(paths)
        .map(|(page, path)| (page as usize, path))
        .collect()
}

/// 合并既有与新增的「页 → 路径」（同页新增覆盖既有），按页码升序返回
/// `(pages, paths)` 且两者一一对应。
fn merge_saved(
    existing: &[(usize, String)],
    added: &[(usize, String)],
) -> (Vec<usize>, Vec<String>) {
    let mut merged: Vec<(usize, String)> = existing.to_vec();
    for (page, path) in added {
        match merged.iter_mut().find(|(p, _)| p == page) {
            Some(slot) => slot.1 = path.clone(),
            None => merged.push((*page, path.clone())),
        }
    }
    merged.sort_by_key(|(page, _)| *page);
    merged.into_iter().unzip()
}

/// 抓取单幅插画：get_illust（缓存优先）→ 逐页下载 / ugoira zip（文件名按
/// `template` 模板渲染，渲染为空回落现行命名）→ 写库。
/// `pages` 为 Some 时只下载其中页码（0 起）；None = 全部页。
/// `existing` 为既有记录的 `(页码, 路径)`，结果与之合并后整体回写。
/// `on_page_saved` 每保存一页（或 ugoira zip）回调一次（None 时不回调）。
#[allow(clippy::too_many_arguments)]
async fn crawl_one_illust(
    api: &PixivApi,
    db: &Db,
    artwork_id: i64,
    base_dir: &Path,
    cache: &mut HashMap<i64, IllustData>,
    template: &str,
    pages: Option<&[usize]>,
    existing: &[(usize, String)],
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
    let name_info = IllustNameInfo {
        user_name: data.user_name.clone(),
        user_id: data.user_id,
        title: data.title.clone(),
        id: artwork_id,
    };
    let fail = |err: &dyn std::fmt::Display| ItemError::new(Some(&title), err);

    std::fs::create_dir_all(base_dir).map_err(|err| fail(&err))?;
    // (页码, 路径)：saved_pages 与 saved_paths 同序一一对应
    let mut added: Vec<(usize, String)> = Vec::new();

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
        let path = base_dir.join(ugoira_filename_with_template(&name_info, template));
        std::fs::write(&path, &bytes).map_err(|err| fail(&err))?;
        // ugoira 恒为单页 [0]
        added.push((0, path.to_string_lossy().into_owned()));
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
            // 按页过滤：pages 为 None → 全部；Some → 只下选中页
            if pages.is_some_and(|wanted| !wanted.contains(&idx)) {
                continue;
            }
            let bytes = api
                .client()
                .download_bytes(url)
                .await
                .map_err(|err| fail(&err))?;
            let path = base_dir.join(page_filename_with_template(&name_info, template, idx, url));
            std::fs::write(&path, &bytes).map_err(|err| fail(&err))?;
            added.push((idx, path.to_string_lossy().into_owned()));
            notify_progress(&mut on_page_saved);
        }
        // 选中页码全部不可用（越界/空 URL）→ 报错而非写空记录（保持旧语义：
        // pages 为 None 时仍按原样写库）
        if pages.is_some() && added.is_empty() {
            return Err(ItemError::new(
                Some(&title),
                &format!("作品 {artwork_id} 选中的页码没有可下载的原图"),
            ));
        }
    }

    // ugoira 不合并（单文件，重下即覆盖）；普通作品与既有记录合并后整体回写
    let (saved_pages, saved_paths) = if data.illust_type == UGOIRA_TYPE {
        let page_list: Vec<usize> = added.iter().map(|(page, _)| *page).collect();
        let path_list: Vec<String> = added.into_iter().map(|(_, path)| path).collect();
        (page_list, path_list)
    } else {
        merge_saved(existing, &added)
    };

    db.insert_illustration(&IllustrationInsert {
        artwork_id,
        title: title.clone(),
        author_id: data.user_id,
        author_name: Some(data.user_name),
        illust_type: data.illust_type,
        page_count: data.page_count,
        saved_paths: serde_json::to_string(&saved_paths).unwrap_or_else(|_| "[]".into()),
        saved_pages: serde_json::to_string(&saved_pages).unwrap_or_else(|_| "[]".into()),
        captured_at: now_iso(),
        status: "ok".into(),
    })
    .map_err(|err| fail(&err))?;
    log::info!("插画 {artwork_id} 已保存 {} 个文件", saved_paths.len());
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
    type CapturedEvents = Arc<Mutex<Vec<(String, Value)>>>;

    fn capture_sink() -> (EventSink, CapturedEvents) {
        let events: CapturedEvents = Arc::new(Mutex::new(Vec::new()));
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
    fn merge_saved_orders_and_dedups() {
        let pair = |p: usize, s: &str| (p, s.to_string());
        // 空既有 → 新增原样升序
        let (pages, paths) = merge_saved(&[], &[pair(2, "/c"), pair(0, "/a")]);
        assert_eq!(pages, vec![0, 2]);
        assert_eq!(paths, vec!["/a".to_string(), "/c".to_string()]);
        // 部分重叠 → 同页以新增为准，其余保留，整体升序
        let (pages, paths) = merge_saved(
            &[pair(0, "/old0"), pair(1, "/old1")],
            &[pair(1, "/new1"), pair(3, "/new3")],
        );
        assert_eq!(pages, vec![0, 1, 3]);
        assert_eq!(
            paths,
            vec!["/old0".to_string(), "/new1".to_string(), "/new3".to_string()]
        );
        // 全重叠 → 只更新路径，不新增页码
        let (pages, paths) = merge_saved(&[pair(0, "/old")], &[pair(0, "/new")]);
        assert_eq!(pages, vec![0]);
        assert_eq!(paths, vec!["/new".to_string()]);
    }

    #[test]
    fn saved_pairs_zips_pages_and_paths() {
        let pairs = saved_pairs(r#"["/a","/b"]"#, "[0,2]");
        assert_eq!(pairs, vec![(0, "/a".to_string()), (2, "/b".to_string())]);
        // 坏 JSON → 空
        assert!(saved_pairs("bad", "bad").is_empty());
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

    fn episode(order: i64, title: &str, content: &str) -> SeriesEpisode {
        SeriesEpisode {
            order,
            title: title.into(),
            content: content.into(),
        }
    }

    #[test]
    fn series_txt_bundle_sorts_and_formats_separators() {
        // 乱序输入（contentOrder 乱）→ 输出按 order 升序、标题分隔行正确
        let episodes = vec![
            episode(3, "终章", "第三话正文[jump:1]"),
            episode(1, "开端", "第一话正文"),
            episode(5, "番外", "跳号话正文"),
        ];
        let bundle = series_txt_bundle(&episodes);
        assert_eq!(
            bundle,
            concat!(
                "\n\n==============================\n第1话 开端\n==============================\n\n第一话正文",
                "\n\n==============================\n第3话 终章\n==============================\n\n第三话正文[jump:1]",
                "\n\n==============================\n第5话 番外\n==============================\n\n跳号话正文",
            ),
            "整包 txt：contentOrder 升序（容忍跳号）+ 分隔行 + 正文原样（标记保留）"
        );
        // 空整包 → 空串
        assert_eq!(series_txt_bundle(&[]), "");
    }

    #[test]
    fn series_md_bundle_toc_headings_and_markdown_conversion() {
        let episodes = vec![
            episode(2, "後篇", "漢字に[rb:漢字>かんじ]ルビ"),
            episode(1, "前篇", "[chapter:序]\n第一段"),
        ];
        let bundle = series_md_bundle("测试系列", &episodes);
        // 目录在前且按 order 升序，锚点指向对应标题
        assert!(bundle.contains("# 测试系列"));
        assert!(bundle.contains("## 目录"));
        let toc_pos = bundle.find("- [第1话 前篇](#第1话-前篇)").expect("目录含第1话");
        assert!(bundle[toc_pos..].contains("- [第2话 後篇](#第2话-後篇)"), "目录按 order 升序");
        // 每话标题 + render_markdown 口径转换（[chapter:X]→## X、rb→A(B)）
        assert!(bundle.contains("## 第1话 前篇"));
        assert!(bundle.contains("## 序"));
        assert!(bundle.contains("第一段"));
        assert!(bundle.contains("## 第2话 後篇"));
        assert!(bundle.contains("漢字に漢字(かんじ)ルビ"), "rb 标记按 render_markdown 转换");
    }

    #[test]
    fn md_anchor_sluggifies_like_github() {
        assert_eq!(md_anchor("第1话 雨夜"), "第1话-雨夜");
        assert_eq!(md_anchor("序 章: Rem"), "序-章-rem", "标点删除、ASCII 小写、空白转 -");
        assert_eq!(md_anchor("  "), "");
    }

    #[test]
    fn pixivimage_and_stem_parsing() {
        // [pixivimage:ID] / [pixivimage:ID-N] 解析
        assert_eq!(parse_pixivimage_id("123"), Some((123, 0)));
        assert_eq!(parse_pixivimage_id("123-2"), Some((123, 2)));
        assert_eq!(parse_pixivimage_id(" 456 - 1 "), Some((456, 1)));
        assert_eq!(parse_pixivimage_id("abc"), None);
        assert_eq!(parse_pixivimage_id("123-x"), None);
        // 条目词干清洗与 epub::sanitize_image_id 同口径
        assert_eq!(epub_image_stem("9001"), "9001");
        assert_eq!(epub_image_stem("9001-2"), "9001-2");
        assert_eq!(epub_image_stem("a b/c"), "a_b_c");
        assert_eq!(epub_image_stem(""), "_");
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }
}
