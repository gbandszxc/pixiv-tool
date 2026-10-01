//! 浏览模式 ajax 接口层（browse-ui-v1，IPC 契约 v2，13 个命令）。
//!
//! 为契约 v2 的 13 个 browse 命令 + 契约 v3.1 的 4 个收藏（bookmark）命令 +
//! 追更列表（browse_watchlist，§12）+ 系列分集（browse_illust_series，§13）
//! 提供 `PixivApi` 方法（跨文件固有实现块），
//! 全部返回 `serde_json::Value`（结构已按契约组装，命令层原样透传前端）。
//!
//! 端点与响应形状真相源：`docs/research/pixiv-browse-api.md`（2026-10-01 实测）。
//! 两类响应形状统一 parse：
//! - 「id 列表 + thumbnails.illust|novel / users 索引表」（top/discovery/follow_latest）；
//! - 平铺结构（ranking.php contents、novel ranking 的 display_a.rank_a）；
//! - 详情接口扁平字段（/ajax/illust/{id}、/ajax/novel/{id}）。
//!
//! 解析风格对齐 api.rs：`parse_*` 纯函数（&Value → 结构体）+ 字段级容错
//! （缺字段兜底空值，id 缺失跳过整条；索引表缺项跳过不报错）。
//! 所有解析逻辑离线可测；测试样例贴合实测结构，不发真实网络。
//!
//! 已失效端点提醒（勿用）：`/ajax/ranking/illust`（404，走 ranking.php）、
//! `/ajax/user/{id}/profile/illusts|novels`（400，走 illusts|novels?ids[]=）。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use super::api::{PixivApi, as_i64_loose, as_str_or};
use super::client::{PixivClient, PixivError};
use super::csrf;

// ----------------------------------------------------------------------
// 契约结构体（手写 Serialize；可选字段序列化时省略，线格式与契约一致）
// ----------------------------------------------------------------------

/// 作品卡片/详情条目（BrowseWorkItem 及详情扩展字段的并集）。
///
/// 列表解析只填列表字段；详情解析额外填 description/width/... 等扩展字段
/// （None 序列化时省略，wire 格式与契约的 `BrowseWorkItem & {...}` 交叉类型一致）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct BrowseWorkItem {
    pub id: i64,
    /// "illust" | "manga" | "ugoira" | "novel"
    pub kind: String,
    pub title: String,
    pub author_id: i64,
    pub author_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_img: Option<String>,
    /// pximg URL 原样返回，前端经 pixiv-img 代理显示。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    pub page_count: i64,
    /// 0 无 | 1 R-18 | 2 R-18G
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_restrict: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_date: Option<String>,
    /// novel：字数（textCount/characterCount）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_length: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_title: Option<String>,
    /// ranking 专用。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<i64>,
    // ---- 以下为详情扩展字段 ----
    /// illust：illustComment（HTML）；novel：description（HTML）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub like_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark_count: Option<i64>,
    /// bookmarkData 非空即已收藏。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmarked: Option<bool>,
    /// novel：阅读时长（分钟）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reading_time: Option<i64>,
    // ---- 以下为收藏列表项扩展（契约 v3.1，wire 名为 camelCase）----
    /// 收藏列表项：当前查看者的 bookmarkData.id（小说实测出现过数字 id，
    /// 统一 String 化）；取消收藏直接用它。非收藏列表无此键 → None。
    #[serde(rename = "bookmarkId", skip_serializing_if = "Option::is_none")]
    pub bookmark_id: Option<String>,
    /// 收藏可见性：0 公开 | 1 非公开（bookmarkData.private）。
    #[serde(rename = "bookmarkRestrict", skip_serializing_if = "Option::is_none")]
    pub bookmark_restrict: Option<i64>,
}

/// 通用列表返回。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseList {
    pub items: Vec<BrowseWorkItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_last_page: Option<bool>,
}

/// 频道页热门标签。
#[derive(Debug, Clone, Serialize)]
pub struct TrendingTag {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
}

/// 频道页「按标签推荐」板块（#tag 的推荐作品；实测仅插画频道返回，其余为空数组）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseChannelSection {
    pub tag: String,
    pub items: Vec<BrowseWorkItem>,
}

/// 频道页（/ajax/top/illust|manga|novel）组装结果。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseChannel {
    pub follow: BrowseList,
    pub recommend: BrowseList,
    pub ranking: BrowseList,
    pub new_post: BrowseList,
    pub tag_sections: Vec<BrowseChannelSection>,
    pub trending_tags: Vec<TrendingTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking_date: Option<String>,
}

/// 追更列表条目（用户订阅的系列；漫画来自 illustSeries + thumbnails 二次映射，
/// 小说来自 novelSeries 自带字段，见 docs/research/pixiv-browse-api.md §12）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseWatchlistItem {
    /// 系列 id
    pub id: i64,
    /// "manga" | "novel"（与请求 kind 一致）
    pub kind: String,
    pub title: String,
    pub user_id: i64,
    /// 缺失时为空串（前端兜底显示）
    pub user_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_avatar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// 最新话的 R-18 标记（漫画取自最新话缩略项，小说为系列本体字段）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_restrict: Option<i64>,
    /// 已发布话数
    pub total: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_date: Option<String>,
    /// 最新话作品 id（latestIllustId / latestNovelId，「读最新话」直达）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_work_id: Option<i64>,
}

/// 追更列表（/ajax/watch_list/manga|novel；后端按 max_page 聚合至多 20 页）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseWatchlist {
    pub kind: String,
    /// 订阅系列总数（page.total，pixiv 侧为字符串数字）
    pub total: i64,
    pub max_page: i64,
    pub items: Vec<BrowseWatchlistItem>,
}

/// 排行榜返回（契约形状：{ items, date, prev_date, next_date, next_page }）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseRanking {
    pub items: Vec<BrowseWorkItem>,
    /// 榜单日期 yyyymmdd（pixiv 榜单有约 2 天延迟，以响应为准）。
    pub date: Option<String>,
    pub prev_date: Option<String>,
    pub next_date: Option<String>,
    pub next_page: Option<i64>,
}

/// 插画详情页对象（pages 每页 URL 档位）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseIllustPage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small: Option<String>,
    /// pages 接口的 regular 档（最长边 1200）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medium: Option<String>,
    pub original: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i64>,
}

/// ugoira 动图元数据（合成预览 zip + 帧序列）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseUgoira {
    pub src: String,
    pub frames: Vec<BrowseUgoiraFrame>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowseUgoiraFrame {
    pub file: String,
    pub delay: i64,
}

/// 所属系列（illust 详情 / novel 详情通用；novel 额外有 next_id）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseSeriesRef {
    pub id: i64,
    pub title: String,
    pub order: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_id: Option<i64>,
}

/// 详情体 bookmarkData 的收藏态（契约 v3.1：bookmarkState）。
/// 未收藏（null/缺失）时整个字段省略，不做「空对象」表达。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseBookmarkState {
    #[serde(rename = "bookmarkId")]
    pub bookmark_id: String,
    /// 0 公开 | 1 非公开（bookmarkData.private）。
    pub restrict: i64,
}

/// 插画/漫画/动图详情（顶层带 detail_kind: "illust"）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseIllustDetail {
    pub detail_kind: &'static str,
    pub item: BrowseWorkItem,
    /// 单页作品也返回 1 项。
    pub pages: Vec<BrowseIllustPage>,
    pub ugoira: Option<BrowseUgoira>,
    pub series: Option<BrowseSeriesRef>,
    /// 当前查看者的收藏态（bookmarkData 三态；未收藏省略）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark_state: Option<BrowseBookmarkState>,
}

/// 小说详情（顶层带 detail_kind: "novel"）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseNovelDetail {
    pub detail_kind: &'static str,
    pub item: BrowseWorkItem,
    /// 全文，保留 [newpage]/[chapter:]/[rb:]/[pixivimage:] 原始标记，前端切分。
    pub content: String,
    pub series: Option<BrowseSeriesRef>,
    /// 当前查看者的收藏态（bookmarkData 三态；未收藏省略）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark_state: Option<BrowseBookmarkState>,
}

/// 作者信息（/ajax/user/{id}?full=1）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseUserProfile {
    pub id: i64,
    pub name: String,
    pub pixiv_id: String,
    pub profile_img: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub following_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mypixiv_count: Option<i64>,
}

/// 小说系列内容条目。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseSeriesContent {
    pub id: i64,
    pub title: String,
    pub series_order: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_length: Option<i64>,
    /// uploadTimestamp 原样字符串化（epoch 数字或字符串，前端格式化）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_restrict: Option<i64>,
}

/// 单条评论（roots 根评论 / replies 回复通用）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct BrowseComment {
    /// pixiv 评论 id（字符串原样；前端作列表 key 与回复查询参数）。
    pub id: String,
    pub user_id: i64,
    pub user_name: String,
    /// 头像 URL（pixiv 返回无协议前缀，后端补全 https://；缺失/空省略）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_img: Option<String>,
    /// 纯文本正文；表情（stamp）评论为空（前端纯文本渲染）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// 表情贴图 URL（stampId 生成；无表情省略）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamp_url: Option<String>,
    /// commentDate 原样（"2026-10-01 08:15"）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// 是否有回复（bool/"true" 字符串两形态容错）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_replies: Option<bool>,
    /// 被回复者用户名（仅 replies 条目，replyToUserName）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_user_name: Option<String>,
}

/// 评论列表（roots / replies 通用；next 游标，省略=到底）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseComments {
    pub comments: Vec<BrowseComment>,
    /// roots: hasNext ? offset+len : null；replies: hasNext ? page+1 : null。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<i64>,
    /// 评论区被作者关闭（roots 端点恒 400 的映射，仅 roots 出现）；此时
    /// comments 恒为空。省略 = 正常评论区。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

/// 小说系列详情 + 一批内容（游标分页）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseSeriesDetail {
    pub id: i64,
    pub title: String,
    pub user_id: i64,
    pub user_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    pub total: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_concluded: Option<bool>,
    pub contents: Vec<BrowseSeriesContent>,
    /// 游标：下一批传回；null=到底。
    pub next_last_order: Option<i64>,
}

/// 插画/漫画系列分集条目（契约：contents[]；作品字段来自 thumbnails.illust
/// 同 id 条目，话数来自 page.series[].order，见 docs/research/pixiv-browse-api.md §13）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseIllustSeriesEntry {
    pub id: i64,
    /// 恒 "illust"：illustType 0/1/2 均入此 kind，进详情页后再分
    pub kind: String,
    pub title: String,
    /// pixiv urls.360x360 优先，回退顶层 url（官方卡片档）；两者皆缺时空串
    pub cover: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_count: Option<i64>,
    /// 1=R-18（条目级口径，系列头无 xRestrict；全局过滤 filterByR18 用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_restrict: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_date: Option<String>,
    /// 话数 1..total（page.series[].order；UI #N 徽标同源）
    pub series_order: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_type: Option<i64>,
}

/// 插画/漫画系列目录（/ajax/series/{id}，页码制每页恒 12 条，恒话数降序）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseIllustSeriesDetail {
    pub id: i64,
    pub title: String,
    pub user_id: i64,
    pub user_name: String,
    /// users[] 按 userId 映射，imageBig 优先回退 image
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_avatar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// illustSeries[0].url；空 = 未设自定义封面（isSetCover=false 恒 null），
    /// 前端回退 contents[0].cover
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// page.total（系列总话数）
    pub total: i64,
    /// pixiv 无漫画/插画系列完结标记（illustSeries[0] 无 isConcluded），恒 false
    pub is_concluded: bool,
    /// 当前登录用户是否已追更（illustSeries[0].isWatched，回退 page.isWatched）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_watched: Option<bool>,
    /// illustSeries[0].updateDate（ISO 含时区）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_date: Option<String>,
    /// 本页分集，series_order 降序（pixiv page.series[] 与 thumbnails.illust
    /// 同序一一对应，实现按 workId → 缩略项 id 映射，正常数据下与按位一致）
    pub contents: Vec<BrowseIllustSeriesEntry>,
    /// 当前页码回显（从 1 起）
    pub page: i64,
    /// ceil(total/12)
    pub total_pages: i64,
    /// page < total_pages ? page+1 : null（pixiv 超页返回空数组不报错，等价到底）
    pub next_page: Option<i64>,
}

/// 收藏列表（契约 v3.1：{ items, total, next }）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseBookmarkList {
    /// 插画/漫画/动图混排（官方每页 48）或小说（每页 30）。
    pub items: Vec<BrowseWorkItem>,
    /// 收藏总数（实测公开/私密各自返回自己的 total；缺失容错为 null）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
    /// offset 游标：下一页传回；null=到底（沿用 browse 命令惯例）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<i64>,
}

/// 单个收藏标签（契约 {name, count}；空名 = 「未分类」聚合标签，前端 i18n）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseBookmarkTag {
    pub name: String,
    pub count: i64,
}

/// 收藏标签分组（契约 { public, private }，一次请求返回两组）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseBookmarkTags {
    pub public: Vec<BrowseBookmarkTag>,
    pub private: Vec<BrowseBookmarkTag>,
}

// ----------------------------------------------------------------------
// 主站 csrf token（street POST 需要 x-csrf-token）进程级 TTL 缓存
// ----------------------------------------------------------------------

/// token 缓存：进程级单条 + 30 分钟 TTL。token 值绝不写入日志。
static WEB_CSRF_CACHE: OnceLock<Mutex<Option<(String, Instant)>>> = OnceLock::new();
const WEB_CSRF_TTL: Duration = Duration::from_secs(30 * 60);

/// 取主站会话 token：缓存命中直接用；否则 GET 主站首页解析 `__NEXT_DATA__`。
async fn web_csrf_token(client: &PixivClient) -> Result<String, PixivError> {
    if let Some(token) = cached_web_csrf() {
        log::debug!("street csrf token 缓存命中");
        return Ok(token);
    }
    let token = csrf::fetch_web_csrf_token(client).await?;
    let cache = WEB_CSRF_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((token.clone(), Instant::now()));
    }
    Ok(token)
}

fn cached_web_csrf() -> Option<String> {
    let cache = WEB_CSRF_CACHE.get()?;
    let guard = cache.lock().ok()?;
    let (token, at) = guard.as_ref()?;
    (at.elapsed() < WEB_CSRF_TTL).then(|| token.clone())
}

/// token 失效自愈：street 认证/业务失败后清缓存，下次调用重新抓取。
fn invalidate_web_csrf() {
    if let Some(cache) = WEB_CSRF_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            *guard = None;
        }
    }
}

/// 自 uid 缓存：进程级单条 + 30 分钟 TTL（与 token 缓存同策略）。
/// 复用 csrf.rs 的 /ajax/user/self 探测（parse_self_response），避免每次
/// 收藏列表/标签请求都多打一发 self 探测。
static SELF_UID_CACHE: OnceLock<Mutex<Option<(i64, Instant)>>> = OnceLock::new();
const SELF_UID_TTL: Duration = Duration::from_secs(30 * 60);

/// 取登录用户 uid：缓存命中直接用；否则 GET /ajax/user/self 解析 userData.id。
/// 复用 [`csrf::parse_self_response`] 的登录态分类：登录失效 → `Auth`，
/// 响应异常 → `Client`（保留中文文案）。
async fn self_user_id(client: &PixivClient) -> Result<i64, PixivError> {
    if let Some(uid) = cached_self_uid().map(|(uid, _)| uid) {
        log::debug!("自 uid 缓存命中");
        return Ok(uid);
    }
    // get_json 成功即 HTTP 200（非 200 已按 Auth/NotFound/... 分类）；
    // /ajax/user/self 是扁平结构，extract_ajax_body 无 body 键时原样返回。
    let body = client.get_json(csrf::SELF_PATH).await?;
    let probe = csrf::parse_self_response(200, Some(body)).map_err(|err| match err {
        csrf::ProbeError::Invalid(_) => PixivError::Auth,
        csrf::ProbeError::Csrf(msg) => PixivError::Client(msg),
    })?;
    let user = probe.user.ok_or(PixivError::Auth)?;
    let uid: i64 = user
        .user_id
        .parse()
        .map_err(|_| PixivError::Client("自 uid 解析失败".into()))?;
    let cache = SELF_UID_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((uid, Instant::now()));
    }
    Ok(uid)
}

fn cached_self_uid() -> Option<(i64, Instant)> {
    let cache = SELF_UID_CACHE.get()?;
    let guard = cache.lock().ok()?;
    let (uid, at) = guard.as_ref()?;
    (at.elapsed() < SELF_UID_TTL).then_some((*uid, *at))
}

/// 自 uid 失效自愈：收藏接口 404/400（uid 失效极罕见）或登录切换后可清缓存。
#[allow(dead_code)]
fn invalidate_self_uid() {
    if let Some(cache) = SELF_UID_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            *guard = None;
        }
    }
}

// ----------------------------------------------------------------------
// 通用解析助手（纯函数）
// ----------------------------------------------------------------------

/// 契约结构 → Value（手写结构的 Serialize 不会失败）。
fn to_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("Browse 契约结构序列化不会失败")
}

/// 非空字符串字段。
fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// 标题：缺失/空时回退 id 字符串（pixiv 偶有空标题）。
fn title_field(v: &Value, id: i64) -> String {
    str_field(v, "title").unwrap_or_else(|| id.to_string())
}

/// 数字/字符串 epoch 字段 → 原样字符串（前端格式化）。
fn timestamp_field(v: &Value, key: &str) -> Option<String> {
    match v.get(key) {
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

/// tags 数组：兼容字符串数组（索引表/搜索/排行）与对象数组两种实测形状，
/// 对象形字段名兼容 `name`（street 缩略图 `{name, translatedName}`）与
/// `tag`（/ajax/illust/{id} 的 `{tag, locked, ...}`）。
fn parse_tags(value: Option<&Value>) -> Option<Vec<String>> {
    let arr = value?.as_array()?;
    let tags: Vec<String> = arr
        .iter()
        .filter_map(|t| match t {
            Value::String(s) => Some(s.clone()),
            other => other
                .get("name")
                .or_else(|| other.get("tag"))
                .and_then(Value::as_str)
                .map(String::from),
        })
        .filter(|s| !s.is_empty())
        .collect();
    (!tags.is_empty()).then_some(tags)
}

/// 作品类型：illustType 数字优先；否则读 `type` 字符串（street 卡片）；
/// 都缺则用调用方给定的 fallback（列表上下文的频道类型）。
fn kind_of(v: &Value, fallback: &str) -> String {
    match v.get("illustType").and_then(as_i64_loose) {
        Some(1) => return "manga".into(),
        Some(2) => return "ugoira".into(),
        Some(0) => return "illust".into(),
        _ => {}
    }
    match v.get("type").and_then(Value::as_str) {
        Some(t @ ("illust" | "manga" | "ugoira" | "novel")) => t.to_string(),
        _ => fallback.to_string(),
    }
}

/// 单个缩略作品对象（索引表条目 / 搜索 data 项 / street 缩略项）→ BrowseWorkItem。
/// 缺 id → None（整条跳过）；其余字段容错兜底。
fn parse_work_thumb(v: &Value, fallback_kind: &str) -> Option<BrowseWorkItem> {
    let id = v.get("id").and_then(as_i64_loose)?;
    let kind = kind_of(v, fallback_kind);
    let is_novel = kind == "novel";
    // 封面兜底链：索引表/搜索项用顶层 `url`；详情 urls 用 `square`/`medium`；
    // street 卡片（2026-10-01 实测）无 url/urls，封面在 pages[0].urls，
    // 键名为尺寸（"540x540" / "360x360" / "1200x1200_standard"）。
    let cover = str_field(v, "url")
        .or_else(|| {
            v.pointer("/urls/square")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .or_else(|| {
            v.pointer("/urls/medium")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .or_else(|| {
            let urls = v.pointer("/pages/0/urls")?;
            ["540x540", "360x360", "1200x1200_standard"]
                .iter()
                .find_map(|k| urls.get(k).and_then(Value::as_str))
                .map(String::from)
                .or_else(|| {
                    urls.as_object()?
                        .values()
                        .find_map(Value::as_str)
                        .map(String::from)
                })
        });
    let text_length = if is_novel {
        v.get("textCount")
            .and_then(as_i64_loose)
            .or_else(|| v.get("characterCount").and_then(as_i64_loose))
            .or_else(|| v.get("wordCount").and_then(as_i64_loose))
    } else {
        None
    };
    let create_date = str_field(v, "createDate").or_else(|| str_field(v, "updateDate"));
    // 收藏列表项自带当前查看者的收藏态（bookmarkData）；其他列表无此键 → None。
    let bookmark = parse_bookmark_data(v.get("bookmarkData"));
    let (bookmark_id, bookmark_restrict) = match bookmark {
        Some(state) => (Some(state.bookmark_id), Some(state.restrict)),
        None => (None, None),
    };
    Some(BrowseWorkItem {
        id,
        rank: v.get("rank").and_then(as_i64_loose),
        kind,
        title: title_field(v, id),
        author_id: v.get("userId").and_then(as_i64_loose).unwrap_or(0),
        author_name: str_field(v, "userName").unwrap_or_default(),
        profile_img: str_field(v, "profileImageUrl"),
        cover,
        page_count: v.get("pageCount").and_then(as_i64_loose).unwrap_or(1),
        x_restrict: v.get("xRestrict").and_then(as_i64_loose),
        tags: parse_tags(v.get("tags")),
        create_date,
        text_length,
        series_id: v
            .get("seriesId")
            .and_then(as_i64_loose)
            .filter(|sid| *sid != 0),
        series_title: str_field(v, "seriesTitle"),
        bookmark_id,
        bookmark_restrict,
        ..BrowseWorkItem::default()
    })
}

/// `thumbnails.illust|novel` 索引表：id → 条目引用（缺 id 的条目跳过）。
fn parse_index<'a>(body: &'a Value, key: &str) -> HashMap<i64, &'a Value> {
    let mut map = HashMap::new();
    if let Some(arr) = body
        .pointer(&format!("/thumbnails/{key}"))
        .and_then(Value::as_array)
    {
        for item in arr {
            if let Some(id) = item.get("id").and_then(as_i64_loose) {
                map.insert(id, item);
            }
        }
    }
    map
}

/// `users` 索引表：userId → 用户对象引用。
fn users_index(body: &Value) -> HashMap<i64, &Value> {
    body.get("users")
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| k.parse::<i64>().ok().map(|id| (id, v)))
                .collect()
        })
        .unwrap_or_default()
}

/// watch_list 的 `users` 为**数组**形状（`[{userId, name, image, imageBig}, ...]`，
/// 与频道页的对象形状不同）：userId → 用户对象引用。
fn users_index_array(body: &Value) -> HashMap<i64, &Value> {
    body.get("users")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|u| u.get("userId").and_then(as_i64_loose).map(|id| (id, u)))
                .collect()
        })
        .unwrap_or_default()
}

/// 列表项 id 提取：兼容标量 id 与 `{id, rank}` 对象两种形态。
/// 频道页 `page.ranking.items` 实测是 **100 个 `{"id":"...","rank":"1"}` 对象**
/// （2026-10-01，三频道一致），而 `page.follow` / `recommend.ids` / `newPost` /
/// `recommendByTag[].ids` 是标量数组；两种都要能解析，否则榜单板块会整块为空。
fn value_id_and_rank(v: &Value) -> Option<(i64, Option<i64>)> {
    match v {
        Value::Object(map) => {
            let id = map.get("id").and_then(as_i64_loose)?;
            Some((id, map.get("rank").and_then(as_i64_loose)))
        }
        scalar => Some((as_i64_loose(scalar)?, None)),
    }
}

/// 按 id 列表顺序映射索引表 → items（pixiv 偶发索引缺项：跳过不报错）。
/// 作者名/头像缺失时回退 users 索引表（imageBig 优先）。
fn items_from_ids(
    ids: &[Value],
    index: &HashMap<i64, &Value>,
    users: &HashMap<i64, &Value>,
    fallback_kind: &str,
) -> Vec<BrowseWorkItem> {
    ids.iter()
        .filter_map(|idv| {
            let (id, rank) = value_id_and_rank(idv)?;
            let mut item = parse_work_thumb(index.get(&id)?, fallback_kind)?;
            item.rank = rank.or(item.rank);
            if let Some(user) = users.get(&item.author_id) {
                if item.author_name.is_empty() {
                    item.author_name = str_field(user, "name").unwrap_or_default();
                }
                if item.profile_img.is_none() {
                    item.profile_img =
                        str_field(user, "imageBig").or_else(|| str_field(user, "image"));
                }
            }
            Some(item)
        })
        .collect()
}

/// 无分页语义的列表包装。
fn list_from_items(items: Vec<BrowseWorkItem>) -> BrowseList {
    BrowseList {
        items,
        total: None,
        next_page: None,
        is_last_page: None,
    }
}

// ----------------------------------------------------------------------
// 各端点 parse（纯函数）
// ----------------------------------------------------------------------

/// 榜单日期归一为 `yyyymmdd`。pixiv 同一语义字段实测三种形态（2026-10-01）：
/// `20260930`（ranking.php、illust/manga 频道）、`2026-09-30`（novel 频道）、
/// `2026年9月30日`（/ajax/ranking/novel，月/日无前导零）。识别不了的原样返回
/// （不丢数据；前端对非 yyyymmdd 一律原样展示）。
fn normalize_ymd(raw: &str) -> String {
    if raw.len() == 8 && raw.bytes().all(|b| b.is_ascii_digit()) {
        return raw.to_string();
    }
    let parts: Vec<&str> = raw
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .collect();
    let [y, m, d] = parts.as_slice() else {
        return raw.to_string();
    };
    let (Ok(y), Ok(m), Ok(d)) = (y.parse::<u32>(), m.parse::<u32>(), d.parse::<u32>()) else {
        return raw.to_string();
    };
    if !(1000..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return raw.to_string();
    }
    format!("{y:04}{m:02}{d:02}")
}

/// POST /ajax/street/v2/main body → BrowseList。
/// contents[].kind 分派解析：illust/manga 卡组取 thumbnails 全部项逐个输出；
/// novel 卡组走 novel 解析；collection 等其它类型 V1 跳过。
fn parse_street(body: &Value) -> BrowseList {
    let items = body
        .get("contents")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|card| {
                    let kind = card.get("kind").and_then(Value::as_str)?;
                    let thumbs = card.get("thumbnails").and_then(Value::as_array)?;
                    match kind {
                        "illust" | "manga" | "novel" => Some(
                            thumbs
                                .iter()
                                .filter_map(|t| parse_work_thumb(t, kind))
                                .collect::<Vec<_>>(),
                        ),
                        _ => None,
                    }
                })
                .flatten()
                .collect()
        })
        .unwrap_or_default();
    list_from_items(items)
}

/// GET /ajax/top/illust|manga|novel body → BrowseChannel。
/// page.* 板块 id → thumbnails 索引表映射（novel 频道查 thumbnails.novel）；
/// 缺板块输出空列表不报错。
fn parse_channel(body: &Value, kind: &str) -> BrowseChannel {
    let index_key = if kind == "novel" { "novel" } else { "illust" };
    let index = parse_index(body, index_key);
    let users = users_index(body);
    let ids_at = |pointer: &str| -> &[Value] {
        body.pointer(pointer)
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    };
    let follow = items_from_ids(ids_at("/page/follow"), &index, &users, index_key);
    let recommend = items_from_ids(ids_at("/page/recommend/ids"), &index, &users, index_key);
    let ranking = items_from_ids(ids_at("/page/ranking/items"), &index, &users, index_key);
    let new_post = items_from_ids(ids_at("/page/newPost"), &index, &users, index_key);
    // #标签推荐板块（实测仅 illust 频道有 recommendByTag）：tag 缺失或映射不到任何作品的板块跳过
    let tag_sections = body
        .pointer("/page/recommendByTag")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|section| {
                    let tag = str_field(section, "tag")?;
                    let ids = section
                        .get("ids")
                        .and_then(Value::as_array)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    let items = items_from_ids(ids, &index, &users, index_key);
                    (!items.is_empty()).then_some(BrowseChannelSection { tag, items })
                })
                .collect()
        })
        .unwrap_or_default();
    // 热门标签：实测条目键为 `{tag, ids, trendingRate}`（2026-10-01，仅 illust
    // 频道有该板块）——**没有** `translatedName` / `illustCount`。中文译名从
    // 同响应的 `tagTranslation[tag].zh`（回退 zh_tw）取；`count` 无对应源字段，
    // 恒为 None（前端当前不消费该字段，保留仅为契约稳定）。
    let tag_translation = body.get("tagTranslation");
    let trending_tags = body
        .pointer("/page/trendingTags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let name = str_field(t, "tag")?;
                    let translated_name = tag_translation
                        .and_then(|m| m.get(&name))
                        .and_then(|entry| {
                            str_field(entry, "zh").or_else(|| str_field(entry, "zh_tw"))
                        });
                    Some(TrendingTag {
                        name,
                        translated_name,
                        count: t.get("illustCount").and_then(as_i64_loose),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // 榜单日期实测三频道两种形态（illust/manga `20260930`、novel `2026-09-30`）→ 归一
    let ranking_date = body
        .pointer("/page/ranking/date")
        .and_then(Value::as_str)
        .map(normalize_ymd);
    BrowseChannel {
        follow: list_from_items(follow),
        recommend: list_from_items(recommend),
        ranking: list_from_items(ranking),
        new_post: list_from_items(new_post),
        tag_sections,
        trending_tags,
        ranking_date,
    }
}

/// 追更列表聚合页数上限（官方单页约 30 条；20 页已覆盖常规订阅规模）。
const WATCHLIST_MAX_PAGES: i64 = 20;

/// GET /ajax/watch_list/manga|novel body → (items, total, max_page)。
/// 顺序按 page.watchedSeriesIds（官方追更列表序）；表里没有或映射不到的 id 跳过，
/// watchedSeriesIds 缺失时回退系列数组原序。漫画：illustSeries + latestIllustId →
/// thumbnails.illust 二次映射封面/R-18，作者回退 users（数组形状）；小说：novelSeries
/// 自带 cover.urls / xRestrict / 作者（users 仅兜底）。缺板块输出空列表不报错。
fn parse_watchlist(body: &Value, kind: &str) -> (Vec<BrowseWatchlistItem>, i64, i64) {
    let total = body
        .pointer("/page/total")
        .and_then(as_i64_loose)
        .unwrap_or(0);
    let max_page = body
        .pointer("/page/maxPage")
        .and_then(as_i64_loose)
        .unwrap_or(1)
        .max(1);
    let order: Vec<i64> = body
        .pointer("/page/watchedSeriesIds")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(as_i64_loose).collect())
        .unwrap_or_default();
    let users = users_index_array(body);
    let is_novel = kind == "novel";
    let thumbs = if is_novel {
        HashMap::new()
    } else {
        parse_index(body, "illust")
    };
    let series: &[Value] = body
        .get(if is_novel { "novelSeries" } else { "illustSeries" })
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let by_id: HashMap<i64, &Value> = series
        .iter()
        .filter_map(|s| s.get("id").and_then(as_i64_loose).map(|id| (id, s)))
        .collect();
    let ids: Vec<i64> = if order.is_empty() {
        series
            .iter()
            .filter_map(|s| s.get("id").and_then(as_i64_loose))
            .collect()
    } else {
        order
    };
    let items = ids
        .iter()
        .filter_map(|id| {
            let s = by_id.get(id)?;
            let user_id = s.get("userId").and_then(as_i64_loose).unwrap_or(0);
            let (mut user_name, mut user_avatar) =
                (str_field(s, "userName"), str_field(s, "profileImageUrl"));
            if let Some(u) = users.get(&user_id) {
                if user_name.is_none() {
                    user_name = str_field(u, "name");
                }
                if user_avatar.is_none() {
                    user_avatar = str_field(u, "imageBig").or_else(|| str_field(u, "image"));
                }
            }
            let latest_work_id = s
                .get(if is_novel { "latestNovelId" } else { "latestIllustId" })
                .and_then(as_i64_loose);
            // 封面/R-18：小说直接取系列本体；漫画按最新话 id 映射缩略项
            let (cover, x_restrict) = if is_novel {
                (
                    s.pointer("/cover/urls/240mw")
                        .and_then(Value::as_str)
                        .map(String::from),
                    s.get("xRestrict").and_then(as_i64_loose),
                )
            } else {
                match latest_work_id.and_then(|wid| thumbs.get(&wid)) {
                    Some(t) => (
                        t.pointer("/urls/240mw")
                            .and_then(Value::as_str)
                            .map(String::from)
                            .or_else(|| str_field(t, "url")),
                        t.get("xRestrict").and_then(as_i64_loose),
                    ),
                    None => (None, None),
                }
            };
            Some(BrowseWatchlistItem {
                id: *id,
                kind: kind.to_string(),
                title: title_field(s, *id),
                user_id,
                user_name: user_name.unwrap_or_default(),
                user_avatar,
                cover,
                x_restrict,
                total: s
                    .get("total")
                    .and_then(as_i64_loose)
                    .or_else(|| s.get("publishedContentCount").and_then(as_i64_loose))
                    .unwrap_or(0),
                update_date: str_field(s, "updateDate"),
                latest_work_id,
            })
        })
        .collect();
    (items, total, max_page)
}

/// GET /ajax/discovery/artworks body → BrowseList。
/// recommendedIllusts[].illustId 顺序映射 thumbnails.illust（仅作品无小说）。
fn parse_discover(body: &Value) -> BrowseList {
    let index = parse_index(body, "illust");
    let users = users_index(body);
    let ids: Vec<Value> = body
        .get("recommendedIllusts")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|r| r.get("illustId").and_then(as_i64_loose))
                .map(Value::from)
                .collect()
        })
        .unwrap_or_default();
    list_from_items(items_from_ids(&ids, &index, &users, "illust"))
}

/// GET /ajax/follow_latest/illust|novel body → BrowseList。
/// page.ids 顺序映射；isLastPage → is_last_page 与 next_page（契约语义）。
fn parse_follow_latest(body: &Value, kind: &str, page: i64) -> BrowseList {
    let index_key = if kind == "novel" { "novel" } else { "illust" };
    let index = parse_index(body, index_key);
    let users = users_index(body);
    let ids = body
        .pointer("/page/ids")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let items = items_from_ids(ids, &index, &users, index_key);
    let next_page = match body.pointer("/page/isLastPage").and_then(Value::as_bool) {
        Some(true) => None,
        Some(false) => Some(page + 1),
        // isLastPage 缺失：按每页 60 条估算是否还有下一页
        None => (items.len() >= 60).then(|| page + 1),
    };
    BrowseList {
        is_last_page: Some(next_page.is_none()),
        items,
        total: None,
        next_page,
    }
}

/// GET /ajax/search/artworks|novels body → BrowseList。
/// illustManga / novel 容器里的 data 数组已是完整作品对象，逐项 parse。
fn parse_search(body: &Value, kind: &str, page: i64) -> BrowseList {
    let is_novel = kind == "novel";
    let container = if is_novel {
        body.get("novel")
    } else {
        body.get("illustManga")
    };
    let fallback_kind = if is_novel { "novel" } else { "illust" };
    let data = container
        .and_then(|c| c.get("data"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let items: Vec<BrowseWorkItem> = data
        .iter()
        .filter_map(|d| parse_work_thumb(d, fallback_kind))
        .collect();
    let total = container
        .and_then(|c| c.get("total"))
        .and_then(as_i64_loose);
    let (next_page, is_last_page) = match container
        .and_then(|c| c.get("lastPage"))
        .and_then(as_i64_loose)
    {
        Some(last_page) => (
            (page < last_page).then(|| page + 1),
            Some(page >= last_page),
        ),
        None => (None, None),
    };
    BrowseList {
        items,
        total,
        next_page,
        is_last_page,
    }
}

/// ranking.php（illust/manga/ugoira）平铺 contents → BrowseRanking。
/// next 字段：数字 → next_page；false → 无下一页；键缺失 → 每页 50 条估算。
fn parse_ranking_illust(body: &Value, page: i64) -> BrowseRanking {
    let items: Vec<BrowseWorkItem> = body
        .get("contents")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let id = c.get("illust_id").and_then(as_i64_loose)?;
                    let kind = match c.get("illust_type").and_then(as_i64_loose) {
                        Some(1) => "manga",
                        Some(2) => "ugoira",
                        _ => "illust",
                    };
                    Some(BrowseWorkItem {
                        id,
                        rank: c.get("rank").and_then(as_i64_loose),
                        kind: kind.to_string(),
                        title: title_field(c, id),
                        author_id: c.get("user_id").and_then(as_i64_loose).unwrap_or(0),
                        author_name: str_field(c, "user_name").unwrap_or_default(),
                        profile_img: str_field(c, "profile_img"),
                        cover: str_field(c, "url"),
                        page_count: c
                            .get("illust_page_count")
                            .and_then(as_i64_loose)
                            .unwrap_or(1),
                        // ranking.php 无顶层 x_restrict 字段，R-18 标记在
                        // illust_content_type.sexual（0 一般 | 1 R-18 | 2 R-18G）；
                        // 不采用语义不明的 is_masked 作判据。ugoira 榜存在
                        // illust_content_type 为空数组的条目（sexual 无从取得），
                        // 此时与两者都缺一样留 None，由前端按 fail-closed 处理：
                        // 关闭 R-18 时只在「全部」档可见。
                        x_restrict: c
                            .pointer("/illust_content_type/sexual")
                            .and_then(as_i64_loose)
                            .or_else(|| c.get("x_restrict").and_then(as_i64_loose)),
                        tags: parse_tags(c.get("tags")),
                        create_date: str_field(c, "date"),
                        ..BrowseWorkItem::default()
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let next_page = match body.get("next") {
        // next 为 false/null/0 → 明确无下一页
        Some(v) if !v.is_null() => as_i64_loose(v).filter(|n| *n > 0),
        // 键缺失 → 每页 50 条估算
        _ => (items.len() >= 50).then(|| page + 1),
    };
    BrowseRanking {
        items,
        date: str_field(body, "date").map(|d| normalize_ymd(&d)),
        prev_date: str_field(body, "prev_date").map(|d| normalize_ymd(&d)),
        next_date: str_field(body, "next_date").map(|d| normalize_ymd(&d)),
        next_page,
    }
}

/// /ajax/ranking/novel 的 display_a.rank_a 平铺结构 → BrowseRanking。
/// 无 next 字段：按每页 50 条（rank_a 实际条数）判断是否还有下一页。
fn parse_ranking_novel(body: &Value, page: i64) -> BrowseRanking {
    let items: Vec<BrowseWorkItem> = body
        .pointer("/display_a/rank_a")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let id = c.get("id").and_then(as_i64_loose)?;
                    Some(BrowseWorkItem {
                        id,
                        rank: c.get("rank").and_then(as_i64_loose),
                        kind: "novel".to_string(),
                        title: title_field(c, id),
                        author_id: c.get("user_id").and_then(as_i64_loose).unwrap_or(0),
                        author_name: str_field(c, "user_name").unwrap_or_default(),
                        profile_img: str_field(c, "profile_img"),
                        cover: str_field(c, "url"),
                        x_restrict: c.get("x_restrict").and_then(as_i64_loose),
                        tags: parse_tags(c.get("tag_a")),
                        create_date: str_field(c, "create_date"),
                        text_length: c.get("character_count").and_then(as_i64_loose),
                        series_id: c
                            .get("series_id")
                            .and_then(as_i64_loose)
                            .filter(|sid| *sid != 0),
                        series_title: str_field(c, "series_title"),
                        bookmark_count: c.get("bookmark_count").and_then(as_i64_loose),
                        ..BrowseWorkItem::default()
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    BrowseRanking {
        next_page: (items.len() >= 50).then(|| page + 1),
        items,
        // 实测为日文展示串（如 `2026年9月30日`）→ 归一成 yyyymmdd，与插画榜同口径
        date: str_field(body, "date").map(|d| normalize_ymd(&d)),
        prev_date: None,
        next_date: None,
    }
}

/// /ajax/illust/{id} body → (item, series)。
fn parse_illust_detail(body: &Value) -> (BrowseWorkItem, Option<BrowseSeriesRef>) {
    let id = body.get("illustId").and_then(as_i64_loose).unwrap_or(0);
    let kind = match body.get("illustType").and_then(as_i64_loose) {
        Some(1) => "manga",
        Some(2) => "ugoira",
        _ => "illust",
    };
    let item = BrowseWorkItem {
        id,
        kind: kind.to_string(),
        title: title_field(body, id),
        author_id: body.get("userId").and_then(as_i64_loose).unwrap_or(0),
        author_name: str_field(body, "userName").unwrap_or_default(),
        // /ajax/illust/{id} 响应无作者头像字段；cover 取 urls.regular（契约指定）
        cover: body
            .pointer("/urls/regular")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from),
        page_count: body.get("pageCount").and_then(as_i64_loose).unwrap_or(1),
        x_restrict: body.get("xRestrict").and_then(as_i64_loose),
        tags: parse_tags(body.get("tags")),
        create_date: str_field(body, "createDate"),
        description: str_field(body, "illustComment"),
        width: body.get("width").and_then(as_i64_loose),
        height: body.get("height").and_then(as_i64_loose),
        view_count: body.get("viewCount").and_then(as_i64_loose),
        like_count: body.get("likeCount").and_then(as_i64_loose),
        bookmark_count: body.get("bookmarkCount").and_then(as_i64_loose),
        // bookmarkData 非空（非 null）即当前用户已收藏
        bookmarked: body.get("bookmarkData").map(|v| !v.is_null()),
        series_id: body
            .pointer("/seriesNavData/seriesId")
            .and_then(as_i64_loose),
        series_title: body
            .pointer("/seriesNavData/title")
            .and_then(Value::as_str)
            .map(String::from),
        ..BrowseWorkItem::default()
    };
    let series = body
        .get("seriesNavData")
        .filter(|v| v.is_object())
        .map(|s| BrowseSeriesRef {
            id: s.get("seriesId").and_then(as_i64_loose).unwrap_or(0),
            title: str_field(s, "title").unwrap_or_default(),
            order: s.get("orderNumber").and_then(as_i64_loose).unwrap_or(1),
            next_id: None,
        });
    (item, series)
}

/// /ajax/illust/{id}/pages 数组 → 每页 URL 档位。
/// original 缺失时回退 regular/small；三档皆空跳过该页。
fn parse_illust_pages(body: &Value) -> Vec<BrowseIllustPage> {
    body.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    let url_at = |seg: &str| {
                        p.pointer(&format!("/urls/{seg}"))
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                    };
                    let original = url_at("original")
                        .or_else(|| url_at("regular"))
                        .or_else(|| url_at("small"))?;
                    Some(BrowseIllustPage {
                        small: url_at("small"),
                        medium: url_at("regular"),
                        original,
                        width: p.get("width").and_then(as_i64_loose),
                        height: p.get("height").and_then(as_i64_loose),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// /ajax/illust/{id}/ugoira_meta body（src + frames 平铺）→ BrowseUgoira。
/// src 缺失 → None（V1 动图降级为封面帧展示）。
fn parse_ugoira(body: &Value) -> Option<BrowseUgoira> {
    let src = str_field(body, "src")?;
    let frames = body
        .get("frames")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    Some(BrowseUgoiraFrame {
                        file: str_field(f, "file")?,
                        delay: f.get("delay").and_then(as_i64_loose)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(BrowseUgoira { src, frames })
}

/// /ajax/novel/{id} body → (item, series)。
fn parse_novel_detail(body: &Value) -> (BrowseWorkItem, Option<BrowseSeriesRef>) {
    let id = body.get("id").and_then(as_i64_loose).unwrap_or(0);
    let item = BrowseWorkItem {
        id,
        kind: "novel".to_string(),
        title: title_field(body, id),
        author_id: body.get("userId").and_then(as_i64_loose).unwrap_or(0),
        author_name: str_field(body, "userName").unwrap_or_default(),
        cover: str_field(body, "coverUrl"),
        page_count: body.get("pageCount").and_then(as_i64_loose).unwrap_or(1),
        x_restrict: body.get("xRestrict").and_then(as_i64_loose),
        tags: parse_tags(body.get("tags")),
        create_date: str_field(body, "createDate"),
        description: str_field(body, "description"),
        text_length: body.get("characterCount").and_then(as_i64_loose),
        bookmark_count: body.get("bookmarkCount").and_then(as_i64_loose),
        reading_time: body.get("readingTime").and_then(as_i64_loose),
        series_id: body
            .pointer("/seriesNavData/seriesId")
            .and_then(as_i64_loose),
        series_title: body
            .pointer("/seriesNavData/title")
            .and_then(Value::as_str)
            .map(String::from),
        ..BrowseWorkItem::default()
    };
    let series = body
        .get("seriesNavData")
        .filter(|v| v.is_object())
        .map(|s| BrowseSeriesRef {
            id: s.get("seriesId").and_then(as_i64_loose).unwrap_or(0),
            title: str_field(s, "title").unwrap_or_default(),
            order: s.get("orderNumber").and_then(as_i64_loose).unwrap_or(1),
            next_id: s.pointer("/next/id").and_then(as_i64_loose),
        });
    (item, series)
}

/// /ajax/illust|novel/{id}/recommend/init body → items。
/// 推荐数组在 `illusts`（illust 端点）或 `novels`（novel 端点），条目为完整缩略对象。
fn parse_related(body: &Value, fallback_kind: &str) -> Vec<BrowseWorkItem> {
    body.get("illusts")
        .or_else(|| body.get("novels"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| parse_work_thumb(v, fallback_kind))
                .collect()
        })
        .unwrap_or_default()
}

/// /ajax/user/{id}?full=1 body → BrowseUserProfile。
/// `pixiv_id`：实测（2026-10-01，full=1/full=0/自己/他人四种组合）响应**没有**
/// `account` 键，恒为空串；页面因此隐藏 @handle 行（见 BrowseAuthorView）。
/// 登录用户自己的 pixivId 由 `/ajax/user/self` 提供（csrf.rs，账号索引已存）。
fn parse_user_profile(id: i64, body: &Value) -> BrowseUserProfile {
    BrowseUserProfile {
        id: body.get("userId").and_then(as_i64_loose).unwrap_or(id),
        name: str_field(body, "name").unwrap_or_default(),
        pixiv_id: str_field(body, "account").unwrap_or_default(),
        profile_img: str_field(body, "imageBig")
            .or_else(|| str_field(body, "image"))
            .unwrap_or_default(),
        comment_html: str_field(body, "commentHtml"),
        background: str_field(body, "background"),
        following_count: body.get("following").and_then(as_i64_loose),
        mypixiv_count: body.get("mypixivCount").and_then(as_i64_loose),
    }
}

/// /ajax/user/{id}/illusts|novels?ids[]= 响应 → items。
/// 响应兼容对象形（{id: 条目}）与数组形两种。
fn parse_user_works_batch(body: &Value, fallback_kind: &str) -> Vec<BrowseWorkItem> {
    let entries: Vec<&Value> = match body {
        Value::Array(arr) => arr.iter().collect(),
        Value::Object(map) => map.values().collect(),
        _ => Vec::new(),
    };
    entries
        .into_iter()
        .filter_map(|v| parse_work_thumb(v, fallback_kind))
        .collect()
}

/// /ajax/novel/series_content/{id} body.page.seriesContents → 内容条目。
fn parse_series_contents(content: &Value) -> Vec<BrowseSeriesContent> {
    content
        .pointer("/page/seriesContents")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .enumerate()
                .filter_map(|(idx, c)| {
                    let cid = c.get("id").and_then(as_i64_loose)?;
                    Some(BrowseSeriesContent {
                        id: cid,
                        title: str_field(c, "title").unwrap_or_else(|| cid.to_string()),
                        series_order: c
                            .pointer("/series/contentOrder")
                            .and_then(as_i64_loose)
                            .unwrap_or((idx + 1) as i64),
                        text_length: c.get("textLength").and_then(as_i64_loose),
                        update_date: timestamp_field(c, "uploadTimestamp"),
                        x_restrict: c.get("xRestrict").and_then(as_i64_loose),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 系列元数据 + 内容列表 → BrowseSeriesDetail。
/// 游标语义：末条 contentOrder 为下一批 last_order；返回空或已覆盖 total → null。
fn parse_novel_series_detail(
    id: i64,
    meta: &Value,
    content: &Value,
    limit: i64,
) -> BrowseSeriesDetail {
    let total = meta.get("total").and_then(as_i64_loose).unwrap_or(0);
    let contents = parse_series_contents(content);
    let next_last_order = match contents.last() {
        None => None,
        Some(last) => {
            if total > 0 && last.series_order >= total {
                None
            } else if (contents.len() as i64) < limit {
                // 不足一批：已到底
                None
            } else {
                Some(last.series_order)
            }
        }
    };
    BrowseSeriesDetail {
        id: meta.get("id").and_then(as_i64_loose).unwrap_or(id),
        title: str_field(meta, "title").unwrap_or_default(),
        user_id: meta.get("userId").and_then(as_i64_loose).unwrap_or(0),
        user_name: str_field(meta, "userName").unwrap_or_default(),
        caption: str_field(meta, "caption"),
        cover: str_field(meta, "cover"),
        total,
        is_concluded: meta.get("isConcluded").and_then(Value::as_bool),
        contents,
        next_last_order,
    }
}

// ----------------------------------------------------------------------
// 插画/漫画系列分集（browse_illust_series，§13）parse（纯函数）
// ----------------------------------------------------------------------

/// 系列分集每页条数（pixiv /ajax/series 端点恒定值，§13.3）。
pub const ILLUST_SERIES_PAGE_SIZE: i64 = 12;

/// GET /ajax/series/{id} body → BrowseIllustSeriesDetail（§13.2）。
/// `page.series[]{workId, order}` 提供本页分集顺序与话数，`thumbnails.illust`
/// 同 id 条目提供作品字段（实测同序一一对应；按 workId 映射在缺项时逐条跳过，
/// 正常数据下与按位映射一致）。系列元数据取 `illustSeries[0]`；作者名/头像
/// 按 users[]（数组形状，同 §12）userId 映射。缺板块 / 空 series / 缺 id
/// 条目 → 空目录或逐条跳过，不报错；`page` 回显入参并推出 next_page。
fn parse_illust_series(body: &Value, id: i64, page: i64) -> BrowseIllustSeriesDetail {
    let empty = Value::Null;
    let meta = body.pointer("/illustSeries/0").unwrap_or(&empty);
    let user_id = meta.get("userId").and_then(as_i64_loose).unwrap_or(0);
    let mut user_name = str_field(meta, "userName");
    let mut user_avatar = str_field(meta, "profileImageUrl");
    // 作者名/头像兜底 users 索引表（/ajax/series 的 users 是数组形状，§13.2）
    if let Some(u) = users_index_array(body).get(&user_id) {
        if user_name.is_none() {
            user_name = str_field(u, "name");
        }
        if user_avatar.is_none() {
            user_avatar = str_field(u, "imageBig").or_else(|| str_field(u, "image"));
        }
    }
    let total = body
        .pointer("/page/total")
        .and_then(as_i64_loose)
        .or_else(|| meta.get("total").and_then(as_i64_loose))
        .unwrap_or(0);
    let total_pages = (total + ILLUST_SERIES_PAGE_SIZE - 1) / ILLUST_SERIES_PAGE_SIZE;
    let index = parse_index(body, "illust");
    let contents = body
        .pointer("/page/series")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    let wid = s.get("workId").and_then(as_i64_loose)?;
                    let thumb = index.get(&wid)?;
                    Some(BrowseIllustSeriesEntry {
                        id: wid,
                        kind: "illust".to_string(),
                        title: title_field(thumb, wid),
                        // 封面兜底链：官方卡片档 urls.360x360 优先，回退顶层 url
                        cover: thumb
                            .pointer("/urls/360x360")
                            .and_then(Value::as_str)
                            .or_else(|| thumb.get("url").and_then(Value::as_str))
                            .unwrap_or_default()
                            .to_string(),
                        page_count: thumb.get("pageCount").and_then(as_i64_loose),
                        x_restrict: thumb.get("xRestrict").and_then(as_i64_loose),
                        update_date: str_field(thumb, "updateDate"),
                        // pixiv 恒返回 order（1..total 降序）；0 仅异常数据兜底
                        series_order: s.get("order").and_then(as_i64_loose).unwrap_or(0),
                        ai_type: thumb.get("aiType").and_then(as_i64_loose),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    BrowseIllustSeriesDetail {
        id: meta.get("id").and_then(as_i64_loose).unwrap_or(id),
        title: str_field(meta, "title").unwrap_or_default(),
        user_id,
        user_name: user_name.unwrap_or_default(),
        user_avatar,
        caption: str_field(meta, "caption"),
        cover: str_field(meta, "url"),
        total,
        // illustSeries[0] 无 isConcluded 字段（§13.2 实测），契约锁定恒 false
        is_concluded: false,
        is_watched: meta
            .get("isWatched")
            .and_then(as_bool_loose)
            .or_else(|| body.pointer("/page/isWatched").and_then(as_bool_loose)),
        update_date: str_field(meta, "updateDate"),
        contents,
        page,
        total_pages,
        next_page: (page < total_pages).then(|| page + 1),
    }
}

// ----------------------------------------------------------------------
// 评论（roots / replies）parse（纯函数）
// ----------------------------------------------------------------------

/// 宽松 bool：pixiv 布尔字段在 bool 与 "true"/"false" 字符串间漂移
/// （实测 hasReplies 两形态都有）。其余值视为缺失。
fn as_bool_loose(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// 单条评论对象（roots / replies 条目同构）→ BrowseComment。
/// id 缺失 → 空串，由 parse_comment_list 整条跳过；其余字段容错兜底：
/// img 无协议前缀补 https://（空值跳过）；stampId 非空且可解析时拼贴图 URL。
fn parse_comment(v: &Value) -> BrowseComment {
    let id = match v.get("id") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let profile_img = str_field(v, "img").map(|img| {
        if img.starts_with("http://") || img.starts_with("https://") {
            img
        } else {
            format!("https://{img}")
        }
    });
    let stamp_url = v
        .get("stampId")
        .and_then(as_i64_loose)
        .map(|sid| format!("https://s.pximg.net/common/images/stamp/generated-stamps/{sid}_s.jpg"));
    BrowseComment {
        id,
        user_id: v.get("userId").and_then(as_i64_loose).unwrap_or(0),
        user_name: str_field(v, "userName").unwrap_or_default(),
        profile_img,
        content: str_field(v, "comment"),
        stamp_url,
        date: str_field(v, "commentDate"),
        has_replies: v.get("hasReplies").and_then(as_bool_loose),
        reply_to_user_name: str_field(v, "replyToUserName"),
    }
}

/// body.comments 数组逐条 parse（缺 id 的条目跳过；缺失/形状异常 → 空列表）。
fn parse_comment_list(body: &Value) -> Vec<BrowseComment> {
    body.get("comments")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(parse_comment)
                .filter(|c| !c.id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// 翻页信标 hasNext（缺失或不可识别 → false，视为到底）。
fn has_next_page(body: &Value) -> bool {
    body.get("hasNext").and_then(as_bool_loose) == Some(true)
}

/// roots 响应 → BrowseComments。next = hasNext ? offset+len : null（无 total）。
fn parse_comments_roots(body: &Value, offset: i64) -> BrowseComments {
    let comments = parse_comment_list(body);
    let next = has_next_page(body).then(|| offset + comments.len() as i64);
    BrowseComments {
        comments,
        next,
        disabled: None,
    }
}

/// replies 响应 → BrowseComments。next = hasNext ? page+1 : null。
fn parse_comments_replies(body: &Value, page: i64) -> BrowseComments {
    let comments = parse_comment_list(body);
    let next = has_next_page(body).then(|| page + 1);
    BrowseComments {
        comments,
        next,
        disabled: None,
    }
}

// ----------------------------------------------------------------------
// 收藏（bookmark）parse（纯函数；端点实测 docs/research/pixiv-browse-api.md §11）
// ----------------------------------------------------------------------

/// bookmarkData 三态解析（实测 §11.7）：
/// - 未收藏：null / 键缺失 / id 缺失或空 → None；
/// - 已收藏：`{id, private}` → 状态。id 数字/字符串两形态统一 String 化
///   （小说实测出现过数字 id）；private 兼容 bool 与 "true"/"false" 字符串。
fn parse_bookmark_data(v: Option<&Value>) -> Option<BrowseBookmarkState> {
    let data = v?;
    let id = match data.get("id") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return None,
    };
    let private = data.get("private").and_then(as_bool_loose).unwrap_or(false);
    Some(BrowseBookmarkState {
        bookmark_id: id,
        restrict: i64::from(private),
    })
}

/// /ajax/user/{uid}/(illusts|novels)/bookmarks body → BrowseBookmarkList。
/// 分页语义（实测 §11.1）：next = offset + works.len()；终止条件
/// works.len() < limit 或 offset + works.len() >= total（total 缺失时只按
/// 满页判断；works[] 缺 id 的条目跳过但按原条数推进游标，与服务端分页一致）。
fn parse_bookmark_list(
    body: &Value,
    fallback_kind: &str,
    offset: i64,
    limit: i64,
) -> BrowseBookmarkList {
    let works = body.get("works").and_then(Value::as_array);
    let items: Vec<BrowseWorkItem> = works
        .map(|arr| {
            arr.iter()
                .filter_map(|w| parse_work_thumb(w, fallback_kind))
                .collect()
        })
        .unwrap_or_default();
    let total = body.get("total").and_then(as_i64_loose);
    let next = match works {
        Some(arr) if (arr.len() as i64) >= limit => {
            let candidate = offset + arr.len() as i64;
            match total {
                Some(t) => (candidate < t).then_some(candidate),
                None => Some(candidate),
            }
        }
        _ => None,
    };
    BrowseBookmarkList {
        items,
        total,
        next,
    }
}

/// 收藏标签数组 `[{tag, cnt}]` → Vec（空 tag 名原样保留：「未分类」聚合标签
/// 的本地化展示由前端 i18n 负责；cnt 缺失容错为 0）。
fn parse_bookmark_tag_group(v: Option<&Value>) -> Vec<BrowseBookmarkTag> {
    v.and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let name = t.get("tag").and_then(Value::as_str)?;
                    Some(BrowseBookmarkTag {
                        name: name.to_string(),
                        count: t.get("cnt").and_then(as_i64_loose).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// /ajax/user/{uid}/(illusts|novels)/bookmark/tags body → BrowseBookmarkTags
/// （实测 §11.2：一次返回 public/private 两组；缺键容错为空组）。
fn parse_bookmark_tags(body: &Value) -> BrowseBookmarkTags {
    BrowseBookmarkTags {
        public: parse_bookmark_tag_group(body.get("public")),
        private: parse_bookmark_tag_group(body.get("private")),
    }
}

/// add 响应 → bookmarkId（实测 §11.4，两端点响应形状不同）：
/// - 插画：body 是对象，取 `last_bookmark_id`；
/// - 小说：body 直接是 bookmarkId 字符串。
///
/// 两种形状统一兼容（字符串 body / 对象 body 互为兜底），空 id 视为失败。
fn parse_bookmark_add_id(body: &Value) -> Result<String, PixivError> {
    let id = match body {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        v => v
            .get("last_bookmark_id")
            .map(|id| match id {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .filter(|s| !s.is_empty()),
    };
    id.ok_or_else(|| PixivError::Client("收藏响应缺少 bookmark id".into()))
}

// ----------------------------------------------------------------------
// 参数校验与 URL 组装（纯函数，离线可测）
// ----------------------------------------------------------------------

const SEARCH_ORDERS: [&str; 4] = ["date_d", "date", "date_asc", "popular_d"];
const SEARCH_MODES: [&str; 3] = ["all", "safe", "r18"];
const SEARCH_S_MODES: [&str; 3] = ["s_tag_full", "s_tag", "s_tc"];
const SEARCH_TYPES: [&str; 3] = ["illust", "manga", "ugoira"];
/// 排行榜 mode 合法值（实测；Premium 限定榜不在表内）。
const RANKING_MODES_ILLUST: [&str; 6] = [
    "daily",
    "weekly",
    "monthly",
    "rookie",
    "daily_r18",
    "weekly_r18",
];
const RANKING_MODES_NOVEL: [&str; 6] =
    ["daily", "weekly", "monthly", "male", "female", "daily_r18"];

/// 搜索词清理：去掉会破坏 URL 结构的 `?`/`#` 与控制字符、去首尾空白。
/// 其余字符（含非 ASCII）由 [`percent_encode`] 编码后进路径段——**不能**直接拼接：
/// wreq 不会对路径里的非 ASCII 做编码，实测直拼日文标签 → HTTP 400（2026-10-01）。
fn sanitize_search_word(word: &str) -> Result<String, PixivError> {
    let cleaned: String = word
        .chars()
        .filter(|c| !matches!(c, '?' | '#') && !c.is_control())
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return Err(PixivError::Client("搜索词不能为空".into()));
    }
    if cleaned.chars().count() > 200 {
        return Err(PixivError::Client("搜索词过长".into()));
    }
    Ok(cleaned.to_string())
}

/// 搜索请求路径（word 在路径段，query 为白名单 ASCII 参数）。
fn search_path(
    kind: &str,
    word: &str,
    order: Option<&str>,
    mode: Option<&str>,
    s_mode: Option<&str>,
    type_: Option<&str>,
    page: i64,
) -> Result<String, PixivError> {
    let word = sanitize_search_word(word)?;
    let order = order.unwrap_or("date_d");
    if !SEARCH_ORDERS.contains(&order) {
        return Err(PixivError::Client(format!("不支持的排序方式: {order}")));
    }
    let mode = mode.unwrap_or("all");
    if !SEARCH_MODES.contains(&mode) {
        return Err(PixivError::Client(format!("不支持的过滤模式: {mode}")));
    }
    let mut params = vec![format!("order={order}"), format!("mode={mode}")];
    if let Some(s_mode) = s_mode {
        if !SEARCH_S_MODES.contains(&s_mode) {
            return Err(PixivError::Client(format!("不支持的检索方式: {s_mode}")));
        }
        params.push(format!("s_mode={s_mode}"));
    }
    let (seg, type_effective) = if kind == "novel" {
        ("novels", None)
    } else if matches!(kind, "illust" | "manga" | "ugoira") {
        ("artworks", Some(type_.unwrap_or(kind)))
    } else {
        return Err(PixivError::Client(format!("不支持的搜索类型: {kind}")));
    };
    if let Some(t) = type_effective {
        if !SEARCH_TYPES.contains(&t) {
            return Err(PixivError::Client(format!("不支持的作品类型: {t}")));
        }
        params.push(format!("type={t}"));
    }
    // 实测默认带 ai_type=0（排除 AI 生成）
    if seg == "artworks" {
        params.push("ai_type=0".to_string());
    }
    params.push(format!("p={}", page.max(1)));
    params.push("lang=zh".to_string());
    // word 在路径段，必须 percent-encode（非 ASCII 直拼实测 400，见 sanitize 注释）
    Ok(format!(
        "/ajax/search/{seg}/{}?{}",
        percent_encode(&word),
        params.join("&")
    ))
}

/// 排行榜 kind/mode 合法性校验。
fn validate_ranking(kind: &str, mode: &str) -> Result<(), PixivError> {
    let modes: &[&str] = match kind {
        "illust" | "manga" | "ugoira" => &RANKING_MODES_ILLUST,
        "novel" => &RANKING_MODES_NOVEL,
        other => return Err(PixivError::Client(format!("不支持的排行榜类型: {other}"))),
    };
    if !modes.contains(&mode) {
        return Err(PixivError::Client(format!("不支持的排行榜类型: {mode}")));
    }
    Ok(())
}

/// date 参数校验（yyyymmdd）。
fn validate_ranking_date(date: Option<&str>) -> Result<(), PixivError> {
    match date {
        None => Ok(()),
        Some(d) if d.len() == 8 && d.chars().all(|c| c.is_ascii_digit()) => Ok(()),
        Some(d) => Err(PixivError::Client(format!("date 格式应为 yyyymmdd: {d}"))),
    }
}

/// percent-encode：RFC 3986 unreserved（字母数字 `-_.~`）之外的 UTF-8 字节
/// 编码为大写 `%XX`。用于搜索词的路径段、收藏接口的 query tag 值与 form 体
/// 字段值（这些位置可含非 ASCII 与 `&`/`/` 等保留字符，直接拼接会破坏
/// URL/form 结构，或让 pixiv 返回 400）。
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// 收藏列表请求路径（§11.1 实测参数全集；order 只支持 desc、mode 只支持
/// all，写死；tag 缺省传空串与官方请求一致）。
fn bookmark_list_path(
    uid: i64,
    kind: &str,
    rest: &str,
    tag: Option<&str>,
    offset: i64,
    limit: i64,
) -> String {
    let seg = if kind == "novel" { "novels" } else { "illusts" };
    let tag_enc = percent_encode(tag.unwrap_or(""));
    format!(
        "/ajax/user/{uid}/{seg}/bookmarks?tag={tag_enc}&offset={offset}&limit={limit}&rest={rest}&order=desc&mode=all&lang=zh"
    )
}

/// 取消收藏（插画）form 体：`bookmark_id=`（实测 §11.5，与 add 的 JSON 体不对称）。
fn illust_delete_form(bookmark_id: &str) -> String {
    format!("bookmark_id={}", percent_encode(bookmark_id))
}

/// 取消收藏（小说）form 体（旧式表单端点，实测 §11.6）：
/// `tt`=csrf token（放表单字段而非请求头）+ 固定分页参数 +
/// `book_id%5B%5D`={bookmarkId}（`[]` 按实测做百分号编码）+ `del=1`。
fn novel_delete_form(csrf_token: &str, bookmark_id: &str) -> String {
    format!(
        "tt={}&p=1&untagged=0&rest=show&book_id%5B%5D={}&del=1",
        percent_encode(csrf_token),
        percent_encode(bookmark_id)
    )
}

// ----------------------------------------------------------------------
// PixivApi 方法（browse 命令层逐个对应，全部返回 Value）
// ----------------------------------------------------------------------

impl PixivApi {
    /// POST /ajax/street/v2/main（首页混合推荐流；需 x-csrf-token，进程内 TTL 缓存）。
    /// 无翻页：前端「换一批」重复调用并去重。token 失效（Auth/Client 错误）
    /// 自动清缓存自愈。
    pub async fn get_home_street(&self) -> Result<Value, PixivError> {
        let client = self.client();
        let token = web_csrf_token(client).await?;
        let payload = json!({"k": null, "vhi": null, "vhm": null, "vhn": null, "vhc": null});
        match client
            .post_json("/ajax/street/v2/main?lang=zh", &token, &payload)
            .await
        {
            Ok(body) => Ok(to_value(&parse_street(&body))),
            Err(err) => {
                if matches!(err, PixivError::Auth | PixivError::Client(_)) {
                    invalidate_web_csrf();
                }
                Err(err)
            }
        }
    }

    /// GET /ajax/top/illust|manga|novel → BrowseChannel（一次性快照）。
    /// kind 兼容前端命名的 "illustration"。
    pub async fn get_channel(&self, kind: &str) -> Result<Value, PixivError> {
        let seg = match kind {
            "illust" | "illustration" => "illust",
            "manga" => "manga",
            "novel" => "novel",
            other => return Err(PixivError::Client(format!("不支持的频道类型: {other}"))),
        };
        let canonical = if seg == "illust" { "illust" } else { kind };
        let body = self
            .client()
            .get_json(&format!("/ajax/top/{seg}?lang=zh"))
            .await?;
        Ok(to_value(&parse_channel(&body, canonical)))
    }

    /// GET /ajax/watch_list/manga|novel → BrowseWatchlist（追更列表，§12）。
    /// 后端按 page.maxPage 聚合后续页（上限 20 页防异常大订阅；空页提前收尾），
    /// 前端无需翻页。
    pub async fn get_watchlist(&self, kind: &str) -> Result<Value, PixivError> {
        let seg = match kind {
            "manga" => "manga",
            "novel" => "novel",
            other => return Err(PixivError::Client(format!("不支持的追更类型: {other}"))),
        };
        let body = self
            .client()
            .get_json(&format!("/ajax/watch_list/{seg}?p=1&lang=zh"))
            .await?;
        let (mut items, total, max_page) = parse_watchlist(&body, seg);
        let last_page = max_page.min(WATCHLIST_MAX_PAGES);
        let mut page = 1;
        while page < last_page {
            page += 1;
            let next = self
                .client()
                .get_json(&format!("/ajax/watch_list/{seg}?p={page}&lang=zh"))
                .await?;
            let (more, _, _) = parse_watchlist(&next, seg);
            let before = items.len();
            items.extend(more);
            if items.len() == before {
                break;
            }
        }
        Ok(to_value(&BrowseWatchlist {
            kind: seg.to_string(),
            total,
            max_page,
            items,
        }))
    }

    /// GET /ajax/discovery/artworks?mode=all&limit=60 → BrowseList。
    /// 无翻页参数；前端重复调用按 id 去重追加。
    pub async fn get_discover(&self) -> Result<Value, PixivError> {
        let body = self
            .client()
            .get_json("/ajax/discovery/artworks?mode=all&limit=60&lang=zh")
            .await?;
        Ok(to_value(&parse_discover(&body)))
    }

    /// GET /ajax/follow_latest/illust|novel?p=&mode= → BrowseList。
    /// next_page = isLastPage ? null : p+1。
    pub async fn get_follow_latest(
        &self,
        kind: &str,
        mode: &str,
        page: i64,
    ) -> Result<Value, PixivError> {
        if !matches!(kind, "illust" | "novel") {
            return Err(PixivError::Client(format!(
                "不支持的类型: {kind}（仅 illust|novel）"
            )));
        }
        if !matches!(mode, "all" | "safe" | "r18") {
            return Err(PixivError::Client(format!("不支持的过滤模式: {mode}")));
        }
        let page = page.max(1);
        let body = self
            .client()
            .get_json(&format!(
                "/ajax/follow_latest/{kind}?p={page}&mode={mode}&lang=zh"
            ))
            .await?;
        Ok(to_value(&parse_follow_latest(&body, kind, page)))
    }

    /// GET /ajax/search/artworks|novels/{word} → BrowseList（含 total）。
    #[allow(clippy::too_many_arguments)]
    pub async fn get_search(
        &self,
        kind: &str,
        word: &str,
        order: Option<&str>,
        mode: Option<&str>,
        s_mode: Option<&str>,
        type_: Option<&str>,
        page: i64,
    ) -> Result<Value, PixivError> {
        let page = page.max(1);
        let path = search_path(kind, word, order, mode, s_mode, type_, page)?;
        let body = self.client().get_json(&path).await?;
        Ok(to_value(&parse_search(&body, kind, page)))
    }

    /// 排行榜：illust/manga/ugoira → /ranking.php；novel → /ajax/ranking/novel。
    /// 返回 { items, date, prev_date, next_date, next_page }；date 为 yyyymmdd。
    pub async fn get_ranking(
        &self,
        kind: &str,
        mode: &str,
        page: i64,
        date: Option<&str>,
    ) -> Result<Value, PixivError> {
        validate_ranking(kind, mode)?;
        validate_ranking_date(date)?;
        let page = page.max(1);
        let date_param = date.map(|d| format!("&date={d}")).unwrap_or_default();
        let body = if kind == "novel" {
            self.client()
                .get_json(&format!(
                    "/ajax/ranking/novel?mode={mode}&p={page}&format=json&lang=zh{date_param}"
                ))
                .await?
        } else {
            self.client()
                .get_json(&format!(
                    "/ranking.php?format=json&mode={mode}&content={kind}&p={page}&lang=zh{date_param}"
                ))
                .await?
        };
        let ranking = if kind == "novel" {
            parse_ranking_novel(&body, page)
        } else {
            parse_ranking_illust(&body, page)
        };
        Ok(to_value(&ranking))
    }

    /// 插画/漫画/动图详情：/ajax/illust/{id} + /pages +（动图）/ugoira_meta。
    /// ugoira 元数据获取失败降级为 None（V1 显示封面帧，见计划 assumption 2）。
    pub async fn get_work_detail_illust(&self, id: i64) -> Result<Value, PixivError> {
        let client = self.client();
        let main = client.get_json(&format!("/ajax/illust/{id}")).await?;
        let (item, series) = parse_illust_detail(&main);
        let pages_body = client.get_json(&format!("/ajax/illust/{id}/pages")).await?;
        let pages = parse_illust_pages(&pages_body);
        let ugoira = if item.kind == "ugoira" {
            match client
                .get_json(&format!("/ajax/illust/{id}/ugoira_meta"))
                .await
            {
                Ok(meta) => parse_ugoira(&meta),
                Err(err) => {
                    log::warn!("ugoira 元数据获取失败（作品 {id}），降级为封面帧: {err}");
                    None
                }
            }
        } else {
            None
        };
        Ok(to_value(&BrowseIllustDetail {
            detail_kind: "illust",
            item,
            pages,
            ugoira,
            series,
            // 详情体 bookmarkData 三态：null/缺失 → 字段省略
            bookmark_state: parse_bookmark_data(main.get("bookmarkData")),
        }))
    }

    /// 小说详情：/ajax/novel/{id}（content 原文保留 [newpage] 等标记）。
    pub async fn get_work_detail_novel(&self, id: i64) -> Result<Value, PixivError> {
        let body = self.client().get_json(&format!("/ajax/novel/{id}")).await?;
        let (item, series) = parse_novel_detail(&body);
        let content = body
            .get("content")
            .map(|v| as_str_or(v, ""))
            .unwrap_or_default();
        Ok(to_value(&BrowseNovelDetail {
            detail_kind: "novel",
            item,
            content,
            series,
            // 详情体 bookmarkData 三态：null/缺失 → 字段省略
            bookmark_state: parse_bookmark_data(body.get("bookmarkData")),
        }))
    }

    /// 相关推荐：/ajax/illust|novel/{id}/recommend/init（一次性池，page 无效，
    /// next_page=null）。manga/ugoira 走 illust 端点。
    pub async fn get_related(
        &self,
        kind: &str,
        id: i64,
        limit: Option<i64>,
    ) -> Result<Value, PixivError> {
        let (seg, fallback) = match kind {
            "novel" => ("novel", "novel"),
            "illust" | "manga" | "ugoira" => ("illust", "illust"),
            other => return Err(PixivError::Client(format!("不支持的类型: {other}"))),
        };
        let limit = limit.unwrap_or(30).clamp(1, 30);
        let body = self
            .client()
            .get_json(&format!(
                "/ajax/{seg}/{id}/recommend/init?limit={limit}&lang=zh"
            ))
            .await?;
        Ok(to_value(&list_from_items(parse_related(&body, fallback))))
    }

    /// 作者信息：/ajax/user/{id}?full=1 → BrowseUserProfile。
    pub async fn get_user_profile(&self, id: i64) -> Result<Value, PixivError> {
        let body = self
            .client()
            .get_json(&format!("/ajax/user/{id}?full=1&lang=zh"))
            .await?;
        Ok(to_value(&parse_user_profile(id, &body)))
    }

    /// 作者作品：profile/all 取 id 全集 → id 降序 → 60/批切片 → illusts|novels?ids[]=。
    /// next_page = 批索引+1（还有剩余 id 时）。
    pub async fn get_user_works(
        &self,
        id: i64,
        kind: &str,
        page: i64,
    ) -> Result<Value, PixivError> {
        const BATCH: usize = 60;
        let (all_key, seg) = match kind {
            "illust" => ("illusts", "illusts"),
            "manga" => ("manga", "illusts"),
            "novel" => ("novels", "novels"),
            other => return Err(PixivError::Client(format!("不支持的作品类型: {other}"))),
        };
        let page = page.max(1);
        let all = self
            .client()
            .get_json(&format!(
                "/ajax/user/{id}/profile/all?sensitiveFilterMode=userSetting&lang=zh"
            ))
            .await?;
        let profile = super::api::parse_profile_all(&all);
        let mut ids = match all_key {
            "illusts" => profile.illusts,
            "manga" => profile.manga,
            _ => profile.novels,
        };
        ids.sort_unstable_by(|a, b| b.cmp(a)); // id 降序（新作品在前）
        let total = ids.len() as i64;
        let start = ((page - 1) as usize) * BATCH;
        let batch: Vec<i64> = ids.into_iter().skip(start).take(BATCH).collect();
        if batch.is_empty() {
            return Ok(to_value(&BrowseList {
                items: Vec::new(),
                total: Some(total),
                next_page: None,
                is_last_page: Some(true),
            }));
        }
        // ids 均为数字，固定 ASCII 键名 ids[] 直拼；重复 ids[] 参数
        let query = batch
            .iter()
            .map(|wid| format!("ids[]={wid}"))
            .collect::<Vec<_>>()
            .join("&");
        let body = self
            .client()
            .get_json(&format!("/ajax/user/{id}/{seg}?{query}&lang=zh"))
            .await?;
        let fallback = if kind == "novel" { "novel" } else { "illust" };
        let items = parse_user_works_batch(&body, fallback);
        let has_more = start + BATCH < total as usize;
        Ok(to_value(&BrowseList {
            items,
            total: Some(total),
            next_page: has_more.then(|| page + 1),
            is_last_page: Some(!has_more),
        }))
    }

    /// 小说系列：/ajax/novel/series/{id} 元数据 + series_content 游标分页。
    /// last_order 缺省 0（首批）；next_last_order = 末条 contentOrder，到底为 null。
    pub async fn get_novel_series(
        &self,
        id: i64,
        last_order: Option<i64>,
    ) -> Result<Value, PixivError> {
        const LIMIT: i64 = 30;
        let last_order = last_order.unwrap_or(0).max(0);
        let meta = self
            .client()
            .get_json(&format!("/ajax/novel/series/{id}?lang=zh"))
            .await?;
        let content = self
            .client()
            .get_json(&format!(
                "/ajax/novel/series_content/{id}?limit={LIMIT}&last_order={last_order}&order_by=asc&lang=zh"
            ))
            .await?;
        Ok(to_value(&parse_novel_series_detail(
            id, &meta, &content, LIMIT,
        )))
    }

    /// 插画/漫画系列分集：/ajax/series/{id}?p={page}&lang=zh（§13）。
    /// 页码制：每页恒 12 条、恒话数降序，无游标/排序参数；
    /// 超页返回空 series 不报错（next_page=null，等价到底）。
    pub async fn get_illust_series(&self, id: i64, page: i64) -> Result<Value, PixivError> {
        let page = page.max(1);
        let body = self
            .client()
            .get_json(&format!("/ajax/series/{id}?p={page}&lang=zh"))
            .await?;
        Ok(to_value(&parse_illust_series(&body, id, page)))
    }

    /// 作品评论根列表：/ajax/illusts|novels/comments/roots（manga 走 illusts 端点）。
    /// offset 游标分页（limit=10，同官方 web）；next = hasNext ? offset+len : null
    /// （无 total，前端以 next 为 0 时请求首页）。
    pub async fn get_work_comments(
        &self,
        kind: &str,
        id: i64,
        offset: i64,
    ) -> Result<Value, PixivError> {
        let (seg, id_key) = match kind {
            "illust" | "manga" => ("illusts", "illust_id"),
            "novel" => ("novels", "novel_id"),
            other => return Err(PixivError::Client(format!("不支持的作品类型: {other}"))),
        };
        let offset = offset.max(0);
        let body = match self
            .client()
            .get_json(&format!(
                "/ajax/{seg}/comments/roots?{id_key}={id}&offset={offset}&limit=10&lang=zh"
            ))
            .await
        {
            Ok(body) => body,
            // 作者关闭评论区的作品该端点恒 400，body 仅泛化「不正确的请求。」、
            // 无专属标志（2026-10-02 实测，docs/research §评论）；详情页能打开说明
            // 作品存在，此处 400 即「关闭」语义 → 返回 disabled 空信封而非报错。
            Err(err) if err.is_bad_request() => {
                return Ok(to_value(&BrowseComments {
                    comments: Vec::new(),
                    next: None,
                    disabled: Some(true),
                }));
            }
            Err(err) => return Err(err),
        };
        Ok(to_value(&parse_comments_roots(&body, offset)))
    }

    /// 评论回复列表：/ajax/illusts|novels/comments/replies（无 limit，同官方 web）。
    /// page 从 1 起；next = hasNext ? page+1 : null。
    pub async fn get_comment_replies(
        &self,
        kind: &str,
        comment_id: &str,
        page: i64,
    ) -> Result<Value, PixivError> {
        let seg = match kind {
            "illust" | "manga" => "illusts",
            "novel" => "novels",
            other => return Err(PixivError::Client(format!("不支持的作品类型: {other}"))),
        };
        let page = page.max(1);
        let body = self
            .client()
            .get_json(&format!(
                "/ajax/{seg}/comments/replies?comment_id={comment_id}&page={page}&lang=zh"
            ))
            .await?;
        Ok(to_value(&parse_comments_replies(&body, page)))
    }
}

// ----------------------------------------------------------------------
// 收藏（bookmark）方法（契约 v3.1；端点实测 docs/research §11）
// ----------------------------------------------------------------------

/// 收藏列表每页默认条数：插画/漫画官方 48、小说官方 30（§11.1）。
const BOOKMARK_PAGE_ILLUST: i64 = 48;
const BOOKMARK_PAGE_NOVEL: i64 = 30;
/// 自定义 limit 上限（实测 10/48/100 服务端均接受，取 100 封顶防误用）。
const BOOKMARK_LIMIT_MAX: i64 = 100;

impl PixivApi {
    /// 收藏列表：GET /ajax/user/{uid}/(illusts|novels)/bookmarks（§11.1）。
    /// 自 uid 走 /ajax/user/self 探测缓存；rest: show=公开 / hide=非公开；
    /// order 写死 desc（asc 静默空列表）、mode 写死 all（其余值报错）；
    /// 项内带 bookmarkData 收藏态（bookmarkId/bookmarkRestrict）。
    pub async fn bookmark_list(
        &self,
        kind: &str,
        rest: &str,
        tag: Option<&str>,
        offset: i64,
        limit: Option<i64>,
    ) -> Result<Value, PixivError> {
        if !matches!(kind, "illust" | "novel") {
            return Err(PixivError::Client(format!("不支持的收藏类型: {kind}")));
        }
        if !matches!(rest, "show" | "hide") {
            return Err(PixivError::Client(format!("不支持的可见范围: {rest}")));
        }
        let offset = offset.max(0);
        let default_limit = if kind == "novel" {
            BOOKMARK_PAGE_NOVEL
        } else {
            BOOKMARK_PAGE_ILLUST
        };
        let limit = limit.unwrap_or(default_limit).clamp(1, BOOKMARK_LIMIT_MAX);
        let client = self.client();
        let uid = self_user_id(client).await?;
        let body = client
            .get_json(&bookmark_list_path(uid, kind, rest, tag, offset, limit))
            .await?;
        let fallback = if kind == "novel" { "novel" } else { "illust" };
        Ok(to_value(&parse_bookmark_list(
            &body, fallback, offset, limit,
        )))
    }

    /// 收藏标签（一次返回 public/private 两组）：
    /// GET /ajax/user/{uid}/(illusts|novels)/bookmark/tags（§11.2，
    /// 注意路径是 `bookmark/tags` 单数子路径，无 rest 参数）。
    pub async fn bookmark_tags(&self, kind: &str) -> Result<Value, PixivError> {
        let seg = match kind {
            "illust" => "illusts",
            "novel" => "novels",
            other => return Err(PixivError::Client(format!("不支持的收藏类型: {other}"))),
        };
        let client = self.client();
        let uid = self_user_id(client).await?;
        let body = client
            .get_json(&format!("/ajax/user/{uid}/{seg}/bookmark/tags?lang=zh"))
            .await?;
        Ok(to_value(&parse_bookmark_tags(&body)))
    }

    /// 添加收藏（全局 JSON 端点，§11.4）：
    /// POST /ajax/(illusts|novels)/bookmarks/add，体为
    /// `{illust_id|novel_id, restrict, comment, tags}`（id 实测为字符串）。
    /// restrict: 0=公开 / 1=非公开。返回 `{ bookmarkId }`
    /// （插画取 body.last_bookmark_id，小说 body 即 id 字符串）。
    /// token 失效自愈策略与 street 一致（Auth/Client 失败后清缓存）。
    pub async fn bookmark_add(
        &self,
        kind: &str,
        id: i64,
        restrict: i64,
        tags: &[String],
    ) -> Result<Value, PixivError> {
        if !matches!(kind, "illust" | "novel") {
            return Err(PixivError::Client(format!("不支持的收藏类型: {kind}")));
        }
        if !matches!(restrict, 0 | 1) {
            return Err(PixivError::Client(format!(
                "restrict 只能是 0（公开）或 1（非公开）: {restrict}"
            )));
        }
        let client = self.client();
        let token = web_csrf_token(client).await?;
        let (path, payload) = if kind == "illust" {
            (
                "/ajax/illusts/bookmarks/add",
                json!({
                    "illust_id": id.to_string(),
                    "restrict": restrict,
                    "comment": "",
                    "tags": tags,
                }),
            )
        } else {
            (
                "/ajax/novels/bookmarks/add",
                json!({
                    "novel_id": id.to_string(),
                    "restrict": restrict,
                    "comment": "",
                    "tags": tags,
                }),
            )
        };
        match client.post_json(path, &token, &payload).await {
            Ok(body) => {
                let bookmark_id = parse_bookmark_add_id(&body)?;
                Ok(json!({ "bookmarkId": bookmark_id }))
            }
            Err(err) => {
                if matches!(err, PixivError::Auth | PixivError::Client(_)) {
                    invalidate_web_csrf();
                }
                Err(err)
            }
        }
    }

    /// 取消收藏（§11.5/§11.6）。`id` 仅作契约参数保留（删除按 bookmark_id
    /// 定位，端点不接受作品 id）：
    /// - 插画：POST /ajax/illusts/bookmarks/delete，form 体 `bookmark_id=`
    ///   （与 add 的 JSON 体不对称），响应 `{"error":false,"body":[]}`；
    /// - 小说：POST /novel/bookmark_setting.php 旧式表单（token 放 `tt`
    ///   表单字段而非请求头），成功以 302 跳转表示（post_form 对 2xx/3xx
    ///   放行，最终页是 HTML 无需解析）。
    pub async fn bookmark_remove(
        &self,
        kind: &str,
        id: i64,
        bookmark_id: &str,
    ) -> Result<Value, PixivError> {
        let _ = id;
        if !matches!(kind, "illust" | "novel") {
            return Err(PixivError::Client(format!("不支持的收藏类型: {kind}")));
        }
        if bookmark_id.trim().is_empty() {
            return Err(PixivError::Client("收藏 ID 不能为空".into()));
        }
        let client = self.client();
        let token = web_csrf_token(client).await?;
        if kind == "illust" {
            let text = client
                .post_form(
                    "/ajax/illusts/bookmarks/delete",
                    Some(&token),
                    &illust_delete_form(bookmark_id),
                )
                .await?;
            // 响应错误信标（error 真值 → API error）在重试管线之外解析
            let value: Value = serde_json::from_str(&text)
                .map_err(|e| PixivError::Client(format!("Pixiv 响应不是有效 JSON: {e}")))?;
            super::client::extract_ajax_body(value).map(|_| Value::Null)
        } else {
            client
                .post_form(
                    "/novel/bookmark_setting.php",
                    None,
                    &novel_delete_form(&token, bookmark_id),
                )
                .await?;
            Ok(Value::Null)
        }
    }
}

// ----------------------------------------------------------------------
// 单元测试（内嵌样例 JSON，全部离线）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    // ---- parse_work_thumb ----

    #[test]
    fn parse_work_thumb_illust_full() {
        let v = json!({
            "id": "123",
            "illustType": 1,
            "title": "作品名",
            "userId": "456",
            "userName": "画师",
            "profileImageUrl": "https://i.pximg.net/user.png",
            "url": "https://i.pximg.net/c/540x540/x.jpg",
            "pageCount": 3,
            "xRestrict": 1,
            "tags": ["風景", "夕日"],
            "createDate": "2026-09-28T15:06:28+09:00"
        });
        let item = parse_work_thumb(&v, "illust").unwrap();
        assert_eq!(item.id, 123);
        assert_eq!(item.kind, "manga", "illustType=1 → manga");
        assert_eq!(item.title, "作品名");
        assert_eq!(item.author_id, 456);
        assert_eq!(item.author_name, "画师");
        assert_eq!(
            item.profile_img.as_deref(),
            Some("https://i.pximg.net/user.png")
        );
        assert_eq!(
            item.cover.as_deref(),
            Some("https://i.pximg.net/c/540x540/x.jpg")
        );
        assert_eq!(item.page_count, 3);
        assert_eq!(item.x_restrict, Some(1));
        assert_eq!(
            item.tags,
            Some(vec!["風景".to_string(), "夕日".to_string()])
        );
        assert!(item.create_date.is_some());
        assert!(item.text_length.is_none(), "非 novel 不读字数");
    }

    #[test]
    fn parse_work_thumb_novel_shape() {
        // street/搜索 novel 项：type 字符串 + 对象形 tags + textCount + series
        let v = json!({
            "id": 789,
            "type": "novel",
            "title": "小说",
            "userId": "1",
            "userName": "作者",
            "url": "https://i.pximg.net/novel-cover.jpg",
            "tags": [{"name": "ファンタジー", "translatedName": "奇幻"}],
            "textCount": 4200,
            "seriesId": 1093870,
            "seriesTitle": "系列名"
        });
        let item = parse_work_thumb(&v, "illust").unwrap();
        assert_eq!(
            item.kind, "novel",
            "type 字符串优先级低于 illustType，缺省时生效"
        );
        assert_eq!(item.text_length, Some(4200));
        assert_eq!(
            item.tags,
            Some(vec!["ファンタジー".to_string()]),
            "对象形 tags 取 name"
        );
        assert_eq!(item.series_id, Some(1093870));
        assert_eq!(item.series_title.as_deref(), Some("系列名"));
        assert_eq!(item.page_count, 1, "缺 pageCount 兜底 1");
    }

    #[test]
    fn parse_work_thumb_tolerates_missing() {
        // 缺 id → 整条跳过
        assert!(parse_work_thumb(&json!({"title": "x"}), "illust").is_none());
        // 最小条目：其余字段兜底
        let item = parse_work_thumb(&json!({"id": "5", "title": ""}), "novel").unwrap();
        assert_eq!(item.kind, "novel", "无 illustType/type 时用 fallback");
        assert_eq!(item.title, "5", "空标题回退 id 字符串");
        assert_eq!(item.author_id, 0);
        assert!(item.cover.is_none());
        assert!(item.tags.is_none());
    }

    #[test]
    fn parse_work_thumb_street_pages_urls() {
        // 2026-10-01 street 实测：插画/漫画缩略无 url/urls，封面在 pages[0].urls，
        // 键名为尺寸字符串；优先 540x540。
        let item = parse_work_thumb(
            &json!({
                "id": 149279618_i64, "type": "manga", "title": "街卡漫画",
                "userId": "10", "userName": "甲",
                "pages": [{"width": 1200, "height": 800, "urls": {
                    "1200x1200_standard": "https://i.pximg.net/c/1200/img-master/big.jpg",
                    "540x540": "https://i.pximg.net/c/540x540/img-master/mid.jpg",
                    "360x360": "https://i.pximg.net/c/360x360/img-master/small.jpg"
                }}]
            }),
            "illust",
        )
        .unwrap();
        assert_eq!(
            item.cover.as_deref(),
            Some("https://i.pximg.net/c/540x540/img-master/mid.jpg")
        );
        // 只有 1200 档时也能取到
        let item2 = parse_work_thumb(
            &json!({"id": 1, "pages": [{"urls": {"1200x1200_standard": "https://i.pximg.net/big.jpg"}}]}),
            "illust",
        )
        .unwrap();
        assert!(item2.cover.is_some());
    }

    // ---- 索引表 + id 映射 ----

    #[test]
    fn items_from_ids_maps_order_and_skips_missing() {
        let body = json!({
            "thumbnails": {
                "illust": [
                    {"id": "1", "illustType": 0, "title": "A", "userId": "10", "userName": "甲"},
                    {"id": "2", "illustType": 0, "title": "B", "userId": "10"},
                    {"id": "3", "illustType": 0, "title": "C", "userId": "11"}
                ]
            },
            "users": {
                "10": {"userId": "10", "name": "甲", "imageBig": "https://i.pximg.net/u10_big.png"},
                "11": {"userId": "11", "name": "", "image": "https://i.pximg.net/u11.png"}
            }
        });
        let index = parse_index(&body, "illust");
        let users = users_index(&body);
        let ids = vec![json!("2"), json!("999"), json!("1"), json!("abc")];
        let items = items_from_ids(&ids, &index, &users, "illust");
        // 缺索引项（999）与非法 id（abc）跳过，顺序保持 ids 顺序
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![2, 1]);
        assert_eq!(items[1].author_name, "甲");
        assert_eq!(
            items[1].profile_img.as_deref(),
            Some("https://i.pximg.net/u10_big.png"),
            "users 表 imageBig 兜底头像"
        );
    }

    // ---- street ----

    #[test]
    fn parse_street_flattens_kinds() {
        let body = json!({
            "contents": [
                {"kind": "illust", "thumbnails": [
                    {"id": "1", "type": "illust", "title": "A"},
                    {"id": "2", "type": "manga", "title": "B"}
                ]},
                {"kind": "novel", "thumbnails": [{"id": "3", "type": "novel", "title": "C", "textCount": 100}]},
                {"kind": "collection", "thumbnails": [{"id": "4"}]},
                {"kind": "illust", "pickup": {"type": "comment"}, "thumbnails": []}
            ]
        });
        let list = parse_street(&body);
        // illust/manga 卡组逐项输出；novel 走 novel 解析；collection 与空卡组跳过
        assert_eq!(
            list.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(list.items[2].kind, "novel");
        assert_eq!(list.items[2].text_length, Some(100));
    }

    #[test]
    fn parse_street_missing_contents_is_empty() {
        assert!(parse_street(&json!({})).items.is_empty());
        assert!(parse_street(&json!({"contents": []})).items.is_empty());
    }

    // ---- channel ----

    #[test]
    fn parse_channel_assembles_sections() {
        let body = json!({
            "page": {
                "follow": [1],
                "recommend": {"ids": [2]},
                // 实测（2026-10-01）：ranking.items 是 {id, rank} 对象数组
                "ranking": {"items": [{"id": "1", "rank": "1"}, {"id": "3", "rank": "2"}], "date": "20260929"},
                "newPost": [4],
                "recommendByTag": [
                    // 实测形状：字符串 id + details（推荐跟踪信息，解析忽略）
                    {"tag": "オリジナル", "ids": ["2", "1"], "details": {"2": {"methods": ["by_tag"]}}},
                    {"tag": "", "ids": ["1"]},
                    {"tag": "孤儿板块", "ids": ["999999"]}
                ],
                // 实测形状：{tag, ids, trendingRate}，无 translatedName/illustCount
                "trendingTags": [
                    {"tag": "オリジナル", "ids": [1, 2], "trendingRate": -4},
                    {"tag": "未收录译名", "ids": [3], "trendingRate": 1},
                    {"tag": ""}
                ]
            },
            // 中文译名源：tagTranslation[tag].zh（回退 zh_tw）
            "tagTranslation": {
                "オリジナル": {"en": "original", "romaji": "orijinaru", "zh": "原创", "zh_tw": "原創"}
            },
            "thumbnails": {"illust": [
                {"id": "1", "illustType": 0, "title": "A", "userId": "9"},
                {"id": "2", "illustType": 0, "title": "B", "userId": "9"},
                {"id": "3", "illustType": 0, "title": "C", "userId": "9"},
                {"id": "4", "illustType": 0, "title": "D", "userId": "9"}
            ]},
            "users": {"9": {"userId": "9", "name": "画师九"}}
        });
        let ch = parse_channel(&body, "illust");
        assert_eq!(
            ch.follow.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(
            ch.recommend.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![2]
        );
        // ranking：对象条目按 id 映射，rank 一并带回
        assert_eq!(
            ch.ranking.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(
            ch.ranking
                .items
                .iter()
                .map(|i| i.rank)
                .collect::<Vec<_>>(),
            vec![Some(1), Some(2)],
            "ranking 条目的 rank 应保留"
        );
        assert_eq!(
            ch.new_post.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![4]
        );
        // #标签板块：空 tag 与映射不到作品的板块跳过，其余按 ids 顺序映射
        assert_eq!(ch.tag_sections.len(), 1);
        assert_eq!(ch.tag_sections[0].tag, "オリジナル");
        assert_eq!(
            ch.tag_sections[0]
                .items
                .iter()
                .map(|i| i.id)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(ch.ranking_date.as_deref(), Some("20260929"));
        assert_eq!(ch.trending_tags.len(), 2, "空 tag 跳过");
        assert_eq!(ch.trending_tags[0].name, "オリジナル");
        assert_eq!(
            ch.trending_tags[0].translated_name.as_deref(),
            Some("原创"),
            "译名取自 tagTranslation.zh"
        );
        assert_eq!(
            ch.trending_tags[1].translated_name, None,
            "tagTranslation 未收录 → 无译名"
        );
        assert_eq!(ch.trending_tags[0].count, None, "响应无 illustCount 源字段");
    }

    /// 频道页日期形态：novel 频道实测 `2026-09-30`，归一到 yyyymmdd。
    #[test]
    fn parse_channel_normalizes_novel_ranking_date() {
        let ch = parse_channel(
            &json!({"page": {"ranking": {"items": [], "date": "2026-09-30"}}}),
            "novel",
        );
        assert_eq!(ch.ranking_date.as_deref(), Some("20260930"));
    }

    #[test]
    fn parse_channel_missing_sections_is_empty_not_error() {
        let ch = parse_channel(&json!({"thumbnails": {"novel": []}}), "novel");
        assert!(ch.follow.items.is_empty());
        assert!(ch.recommend.items.is_empty());
        assert!(ch.ranking.items.is_empty());
        assert!(ch.new_post.items.is_empty());
        assert!(ch.tag_sections.is_empty(), "无 recommendByTag 输出空数组不报错");
        assert!(ch.trending_tags.is_empty());
        assert!(ch.ranking_date.is_none());
    }

    // ---- watchlist（§12 追更列表）----

    #[test]
    fn parse_watchlist_manga_maps_thumbs_and_order() {
        let body = json!({
            "page": {
                "total": "2",
                "maxPage": 1,
                // 官方列表序；含一个不在系列表的孤儿 id（应跳过）
                "watchedSeriesIds": ["344074", "283930", "999999"]
            },
            "thumbnails": {"illust": [
                {"id": "149896311", "illustType": 1, "title": "最新话A", "userId": "1101",
                 "url": "https://i.pximg.net/c/250x250_80_a2/custom-thumb/a.jpg",
                 "urls": {"240mw": "https://i.pximg.net/c/240x480/img-master/a.jpg"},
                 "xRestrict": 1},
                {"id": "149798080", "illustType": 1, "title": "最新话B", "userId": "2202",
                 "url": "https://i.pximg.net/c/250x250_80_a2/custom-thumb/b.jpg",
                 "xRestrict": 0}
            ]},
            "illustSeries": [
                {"id": "283930", "userId": "2202", "title": "系列B", "total": 20,
                 "latestIllustId": "149798080", "updateDate": "2026-09-18T12:17:48+09:00"},
                {"id": "344074", "userId": "1101", "title": "系列A", "total": 14,
                 "latestIllustId": "149896311", "updateDate": "2026-09-20T20:57:00+09:00"}
            ],
            // watch_list 的 users 是数组形状（区别于频道页的对象形状）
            "users": [
                {"userId": "1101", "name": "画师A", "image": "https://i.pximg.net/a50.png",
                 "imageBig": "https://i.pximg.net/a170.png"},
                {"userId": "2202", "name": "画师B"}
            ]
        });
        let (items, total, max_page) = parse_watchlist(&body, "manga");
        assert_eq!(total, 2, "page.total 字符串数字");
        assert_eq!(max_page, 1);
        assert_eq!(
            items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![344074, 283930],
            "按 watchedSeriesIds 排序，孤儿 id 跳过"
        );
        let first = &items[0];
        assert_eq!(first.kind, "manga");
        assert_eq!(first.title, "系列A");
        assert_eq!(first.user_id, 1101);
        assert_eq!(first.user_name, "画师A", "作者名来自 users 数组");
        assert_eq!(
            first.user_avatar.as_deref(),
            Some("https://i.pximg.net/a170.png"),
            "头像 imageBig 优先"
        );
        assert_eq!(
            first.cover.as_deref(),
            Some("https://i.pximg.net/c/240x480/img-master/a.jpg"),
            "封面取最新话的 urls.240mw"
        );
        assert_eq!(first.x_restrict, Some(1), "R-18 取最新话标记");
        assert_eq!(first.total, 14);
        assert_eq!(first.latest_work_id, Some(149896311));
        // 第二项：缩略项无 urls.240mw → 回退顶层 url 方图
        assert_eq!(
            items[1].cover.as_deref(),
            Some("https://i.pximg.net/c/250x250_80_a2/custom-thumb/b.jpg")
        );
        assert_eq!(items[1].x_restrict, Some(0));
    }

    #[test]
    fn parse_watchlist_novel_uses_series_fields() {
        let body = json!({
            "page": {"total": "1", "maxPage": 1, "watchedSeriesIds": ["10559822"]},
            "thumbnails": {"illust": [], "novel": []},
            "illustSeries": [],
            "novelSeries": [
                {"id": "10559822", "userId": "4501", "userName": "Daiakko",
                 "profileImageUrl": "https://i.pximg.net/p170.jpg",
                 "xRestrict": 1, "title": "系列N", "total": 4,
                 "latestNovelId": "20201502",
                 "updateDate": "2023-09-19T12:17:05+09:00",
                 "cover": {"urls": {
                     "240mw": "https://i.pximg.net/c/240x480_80/novel-cover-master/n.jpg",
                     "original": "https://i.pximg.net/novel-cover-original/n.png"
                 }}}
            ],
            "users": []
        });
        let (items, total, max_page) = parse_watchlist(&body, "novel");
        assert_eq!(total, 1);
        assert_eq!(max_page, 1);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.kind, "novel");
        assert_eq!(item.title, "系列N");
        assert_eq!(item.user_name, "Daiakko", "小说条目自带作者名");
        assert_eq!(
            item.user_avatar.as_deref(),
            Some("https://i.pximg.net/p170.jpg")
        );
        assert_eq!(
            item.cover.as_deref(),
            Some("https://i.pximg.net/c/240x480_80/novel-cover-master/n.jpg"),
            "封面取 cover.urls.240mw"
        );
        assert_eq!(item.x_restrict, Some(1), "R-18 为系列本体字段");
        assert_eq!(item.total, 4);
        assert_eq!(item.latest_work_id, Some(20201502), "latestNovelId");
    }

    #[test]
    fn parse_watchlist_missing_or_empty_is_not_error() {
        // 无 page / 无系列数组：空列表、total 0、maxPage 兜底 1
        let (items, total, max_page) = parse_watchlist(&json!({}), "manga");
        assert!(items.is_empty());
        assert_eq!(total, 0);
        assert_eq!(max_page, 1);
        // watchedSeriesIds 缺失 → 回退系列数组原序
        let body = json!({
            "illustSeries": [
                {"id": "2", "userId": "1", "title": "B", "total": 2, "latestIllustId": "22"},
                {"id": "1", "userId": "1", "title": "A", "total": 1, "latestIllustId": "11"}
            ]
        });
        let (items, _, _) = parse_watchlist(&body, "manga");
        assert_eq!(
            items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![2, 1],
            "无 watchedSeriesIds 时保持数组原序"
        );
        assert_eq!(items[0].user_name, "", "users 缺失时作者名兜底空串");
        assert!(items[0].cover.is_none(), "无 thumbnails 映射时封面缺省");
    }

    // ---- discover ----

    #[test]
    fn parse_discover_maps_ids_in_order() {
        let body = json!({
            "recommendedIllusts": [
                {"illustId": "7", "recommendScore": 1},
                {"illustId": 8},
                {"illustId": "bad"},
                {"recommendScore": 2}
            ],
            "thumbnails": {"illust": [
                {"id": "8", "illustType": 0, "title": "B"},
                {"id": "7", "illustType": 0, "title": "A"}
            ]}
        });
        let list = parse_discover(&body);
        assert_eq!(
            list.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![7, 8],
            "按推荐顺序输出，非法/缺项跳过"
        );
    }

    // ---- follow_latest ----

    #[test]
    fn parse_follow_latest_pagination_semantics() {
        let thumbs: Vec<_> = (0..60)
            .map(|i| json!({"id": i.to_string(), "title": format!("t{i}")}))
            .collect();
        let mk = |is_last: Value| {
            json!({
                "page": {"ids": (0..60).map(|i| json!(i.to_string())).collect::<Vec<_>>(), "isLastPage": is_last},
                "thumbnails": {"novel": thumbs}
            })
        };
        // isLastPage=true → 无下一页
        let last = parse_follow_latest(&mk(json!(true)), "novel", 3);
        assert_eq!(last.next_page, None);
        assert_eq!(last.is_last_page, Some(true));
        // isLastPage=false → p+1
        let more = parse_follow_latest(&mk(json!(false)), "novel", 3);
        assert_eq!(more.next_page, Some(4));
        assert_eq!(more.is_last_page, Some(false));
        // 缺 isLastPage → 按每页 60 条估算
        let mut body = mk(json!(false));
        body["page"].as_object_mut().unwrap().remove("isLastPage");
        let guessed = parse_follow_latest(&body, "novel", 1);
        assert_eq!(guessed.next_page, Some(2));
    }

    #[test]
    fn parse_follow_latest_novel_uses_novel_index() {
        let body = json!({
            "page": {"ids": [5], "isLastPage": true},
            "thumbnails": {
                "illust": [{"id": "5", "illustType": 0, "title": "插画5"}],
                "novel": [{"id": "5", "type": "novel", "title": "小说5"}]
            }
        });
        let list = parse_follow_latest(&body, "novel", 1);
        assert_eq!(
            list.items[0].title, "小说5",
            "novel 频道查 thumbnails.novel"
        );
    }

    // ---- search ----

    #[test]
    fn parse_search_artworks_and_novels() {
        let artworks = json!({
            "illustManga": {
                "data": [
                    {"id": "1", "illustType": 0, "title": "A", "userId": "2", "userName": "u"},
                    {"id": "3", "illustType": 2, "title": "G"}
                ],
                "total": 2345, "lastPage": 3
            }
        });
        let list = parse_search(&artworks, "illust", 2);
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.items[1].kind, "ugoira");
        assert_eq!(list.total, Some(2345));
        assert_eq!(list.next_page, Some(3), "p=2 < lastPage=3");
        assert_eq!(list.is_last_page, Some(false));

        let novels = json!({
            "novel": {
                "data": [{"id": "9", "type": "novel", "title": "N", "seriesId": 1, "seriesTitle": "S"}],
                "total": 30, "lastPage": 1
            }
        });
        let list = parse_search(&novels, "novel", 1);
        assert_eq!(list.total, Some(30));
        assert_eq!(list.next_page, None, "p=1 >= lastPage=1");
        assert_eq!(list.is_last_page, Some(true));
        assert_eq!(list.items[0].series_id, Some(1));
    }

    #[test]
    fn parse_search_missing_container_is_empty() {
        let list = parse_search(&json!({}), "illust", 1);
        assert!(list.items.is_empty());
        assert!(list.total.is_none());
        assert!(list.next_page.is_none());
    }

    // ---- ranking ----

    #[test]
    fn parse_ranking_illust_next_semantics() {
        let content = json!({
            "rank": 1, "illust_id": "100", "title": "T", "date": "2026-09-28",
            "tags": ["tag1"], "url": "https://i.pximg.net/t.jpg",
            "illust_type": 0, "illust_page_count": 1,
            "user_id": "7", "user_name": "u", "profile_img": "https://i.pximg.net/p.jpg"
        });
        let items = vec![content; 50];
        // next 为数字 → next_page
        let body = json!({"contents": items, "rank_total": 500, "next": 2,
            "date": "20260929", "prev_date": "20260928", "next_date": "20260930"});
        let r = parse_ranking_illust(&body, 1);
        assert_eq!(r.next_page, Some(2));
        assert_eq!(r.date.as_deref(), Some("20260929"));
        assert_eq!(r.prev_date.as_deref(), Some("20260928"));
        assert_eq!(r.next_date.as_deref(), Some("20260930"));
        assert_eq!(r.items[0].rank, Some(1));
        assert_eq!(r.items[0].author_name, "u");
        assert_eq!(r.items[0].tags, Some(vec!["tag1".to_string()]));
        // next=false → 无下一页
        let r = parse_ranking_illust(&json!({"contents": items, "next": false}), 2);
        assert_eq!(r.next_page, None);
        // 缺 next 键 → 50 条估算有下一页
        let r = parse_ranking_illust(&json!({"contents": items}), 1);
        assert_eq!(r.next_page, Some(2));
    }

    #[test]
    fn parse_ranking_illust_reads_r18_flag() {
        fn entry(extra: Value) -> Value {
            let mut c = json!({
                "rank": 1, "illust_id": "100", "title": "T", "date": "2026-09-28",
                "tags": ["tag1"], "url": "https://i.pximg.net/t.jpg",
                "illust_type": 0, "illust_page_count": 1,
                "user_id": "7", "user_name": "u", "profile_img": "https://i.pximg.net/p.jpg"
            });
            if let Some(extra_obj) = extra.as_object() {
                c.as_object_mut().unwrap().extend(extra_obj.clone());
            }
            c
        }
        fn first_x_restrict(content: Value) -> Option<i64> {
            parse_ranking_illust(&json!({"contents": [content]}), 1).items[0].x_restrict
        }

        // illust_content_type.sexual：1 R-18 / 2 R-18G / 0 一般向
        assert_eq!(
            first_x_restrict(entry(json!({"illust_content_type": {"sexual": 1}}))),
            Some(1)
        );
        assert_eq!(
            first_x_restrict(entry(json!({"illust_content_type": {"sexual": 2}}))),
            Some(2)
        );
        assert_eq!(
            first_x_restrict(entry(json!({"illust_content_type": {"sexual": 0}}))),
            Some(0)
        );
        // sexual 优先于顶层 x_restrict
        assert_eq!(
            first_x_restrict(entry(
                json!({"illust_content_type": {"sexual": 2}, "x_restrict": 0})
            )),
            Some(2)
        );
        // 字段缺失 → 顶层 x_restrict 兜底；两者都缺 → None
        assert_eq!(first_x_restrict(entry(json!({"x_restrict": 1}))), Some(1));
        assert_eq!(first_x_restrict(entry(json!({}))), None);
        // ugoira 榜实测形态：illust_content_type 为空数组 → sexual 无从取得 → None
        assert_eq!(
            first_x_restrict(entry(json!({"illust_content_type": []}))),
            None
        );
        // is_masked 不参与判定（语义不明，不据此标 R-18）
        assert_eq!(
            first_x_restrict(entry(
                json!({"illust_content_type": {"sexual": 0}, "is_masked": true})
            )),
            Some(0)
        );
        assert_eq!(first_x_restrict(entry(json!({"is_masked": true}))), None);
    }

    #[test]
    fn parse_ranking_novel_shape_and_last_page() {
        let entry = json!({
            "rank": 1, "id": "9000012", "title": "小说", "create_date": "2026-09-01",
            "user_id": "5", "user_name": "作者", "profile_img": "https://i.pximg.net/p.jpg",
            "x_restrict": 1, "tag_a": ["ファンタジー"], "url": "https://i.pximg.net/c.jpg",
            "series_id": 0, "character_count": 3200, "bookmark_count": 88
        });
        // 满页 50 → 有下一页
        let body = json!({"display_a": {"rank_a": vec![entry.clone(); 50]}, "date": "20260929"});
        let r = parse_ranking_novel(&body, 1);
        assert_eq!(r.items.len(), 50);
        assert_eq!(r.next_page, Some(2));
        assert_eq!(r.date.as_deref(), Some("20260929"));
        assert!(
            r.prev_date.is_none() && r.next_date.is_none(),
            "novel 榜无 prev/next date"
        );
        let first = &r.items[0];
        assert_eq!(first.kind, "novel");
        assert_eq!(first.rank, Some(1));
        assert_eq!(first.x_restrict, Some(1));
        assert_eq!(first.text_length, Some(3200));
        assert_eq!(first.bookmark_count, Some(88));
        assert_eq!(first.series_id, None, "series_id=0 视为无系列");
        // 不足 50 → 最后一页
        let body = json!({"display_a": {"rank_a": [entry]}, "date": "20260929"});
        let r = parse_ranking_novel(&body, 7);
        assert_eq!(r.next_page, None);
    }

    #[test]
    fn parse_ranking_empty_or_missing_contents() {
        let r = parse_ranking_illust(&json!({}), 1);
        assert!(r.items.is_empty());
        assert_eq!(r.next_page, None);
        let r = parse_ranking_novel(&json!({"display_a": {}}), 1);
        assert!(r.items.is_empty());
    }

    /// 榜单日期归一：小说榜实测日文展示串、插画榜已是 yyyymmdd（prev/next 同规则）。
    #[test]
    fn parse_ranking_dates_normalized_to_yyyymmdd() {
        let r = parse_ranking_novel(
            &json!({"display_a": {"rank_a": []}, "date": "2026年9月30日"}),
            1,
        );
        assert_eq!(r.date.as_deref(), Some("20260930"));
        let r = parse_ranking_illust(
            &json!({"contents": [], "date": "20260930", "prev_date": "2026-09-29"}),
            1,
        );
        assert_eq!(r.date.as_deref(), Some("20260930"));
        assert_eq!(r.prev_date.as_deref(), Some("20260929"));
    }

    // ---- 详情 ----

    #[test]
    fn parse_illust_detail_fields() {
        let body = json!({
            "illustId": "9000021",
            "illustTitle": "標題",
            "illustComment": "说明 <strong>html</strong>",
            "illustType": 2,
            "pageCount": 4,
            "createDate": "2026-09-28T15:06:28+09:00",
            "userId": "28640", "userName": "作者",
            "urls": {"regular": "https://i.pximg.net/img-master/p0_master1200.jpg"},
            "width": 4093, "height": 2894,
            "viewCount": 111, "likeCount": 22, "bookmarkCount": 33,
            "bookmarkData": {"id": "abc"},
            "xRestrict": 0,
            "tags": [{"tag": "風景"}, {"tag": "空"}],
            "seriesNavData": {"seriesId": 55, "title": "系列", "orderNumber": 3}
        });
        let (item, series) = parse_illust_detail(&body);
        assert_eq!(item.id, 9000021);
        assert_eq!(item.kind, "ugoira");
        assert_eq!(
            item.cover.as_deref(),
            Some("https://i.pximg.net/img-master/p0_master1200.jpg")
        );
        assert_eq!(item.bookmarked, Some(true), "bookmarkData 非空即已收藏");
        assert_eq!(item.tags, Some(vec!["風景".to_string(), "空".to_string()]));
        assert_eq!(item.bookmark_count, Some(33));
        let series = series.unwrap();
        assert_eq!(
            (series.id, series.title.as_str(), series.order),
            (55, "系列", 3)
        );
        assert!(series.next_id.is_none(), "illust 系列无 next_id");
        // 匿名访问：bookmarkData 为 null → 未收藏
        let (item, _) =
            parse_illust_detail(&json!({"illustId": "1", "bookmarkData": null, "illustType": 0}));
        assert_eq!(item.bookmarked, Some(false));
    }

    #[test]
    fn parse_illust_pages_urls_and_fallback() {
        let body = json!([
            {"urls": {"small": "s.jpg", "regular": "r.jpg", "original": "o.jpg"}, "width": 800, "height": 600},
            {"urls": {"regular": "r2.jpg"}, "width": 1, "height": 1},
            {"urls": {}},
            {}
        ]);
        let pages = parse_illust_pages(&body);
        assert_eq!(pages.len(), 2, "三档 URL 皆空的页跳过");
        assert_eq!(pages[0].original, "o.jpg");
        assert_eq!(pages[0].medium.as_deref(), Some("r.jpg"));
        assert_eq!(pages[0].small.as_deref(), Some("s.jpg"));
        assert_eq!(pages[1].original, "r2.jpg", "original 缺失回退 regular");
        assert!(pages[1].small.is_none());
        // 响应不是数组 → 空
        assert!(parse_illust_pages(&json!({})).is_empty());
    }

    #[test]
    fn parse_ugoira_frames_and_missing_src() {
        let body = json!({
            "src": "https://i.pximg.net/img-zip-ugoira/x_ugoira600x600.zip",
            "originalSrc": "https://i.pximg.net/x_ugoira1920x1080.zip",
            "mime_type": "image/jpeg",
            "frames": [{"file": "000000.jpg", "delay": 65}, {"file": "000001.jpg", "delay": 10}, {"file": "bad"}]
        });
        let u = parse_ugoira(&body).unwrap();
        assert_eq!(
            u.src,
            "https://i.pximg.net/img-zip-ugoira/x_ugoira600x600.zip"
        );
        assert_eq!(u.frames.len(), 2, "缺 file/delay 的帧跳过");
        assert_eq!(
            (u.frames[0].file.as_str(), u.frames[0].delay),
            ("000000.jpg", 65)
        );
        assert!(
            parse_ugoira(&json!({"frames": []})).is_none(),
            "缺 src → None（降级封面帧）"
        );
    }

    #[test]
    fn parse_novel_detail_fields() {
        let body = json!({
            "id": "9000012",
            "title": "相対性理論",
            "content": "第一章[newpage]第二章",
            "description": "<p>简介</p>",
            "coverUrl": "https://i.pximg.net/novel-cover.jpg",
            "userId": "28640", "userName": "作者",
            "pageCount": 2,
            "tags": [{"tag": "SF"}],
            "seriesNavData": {"seriesType": "novel", "seriesId": 1093870, "title": "系列", "orderNumber": 2,
                "next": {"id": "9000013", "title": "下一话"}},
            "characterCount": 12345,
            "readingTime": 25,
            "bookmarkCount": 66,
            "xRestrict": 0,
            "createDate": "2026-07-21T00:00:00+09:00"
        });
        let (item, series) = parse_novel_detail(&body);
        assert_eq!(item.id, 9000012);
        assert_eq!(item.kind, "novel");
        assert_eq!(
            item.cover.as_deref(),
            Some("https://i.pximg.net/novel-cover.jpg")
        );
        assert_eq!(item.text_length, Some(12345));
        assert_eq!(item.reading_time, Some(25));
        assert_eq!(item.bookmark_count, Some(66));
        let series = series.unwrap();
        assert_eq!(series.id, 1093870);
        assert_eq!(series.order, 2);
        assert_eq!(series.next_id, Some(9000013), "seriesNavData.next.id");
        // 无系列
        let (_, series) = parse_novel_detail(&json!({"id": "1", "content": "c"}));
        assert!(series.is_none());
    }

    // ---- related ----

    #[test]
    fn parse_related_accepts_illusts_and_novels_keys() {
        let illusts = json!({"illusts": [
            {"id": "1", "illustType": 0, "title": "A"},
            {"title": "缺 id 跳过"}
        ], "nextIds": ["2", "3"]});
        let items = parse_related(&illusts, "illust");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, 1);
        let novels = json!({"novels": [{"id": "9", "type": "novel", "title": "N"}]});
        let items = parse_related(&novels, "novel");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, "novel");
        assert!(parse_related(&json!({}), "illust").is_empty());
    }

    // ---- user ----

    #[test]
    fn parse_user_profile_fields() {
        let body = json!({
            "userId": "28640", "name": "作者", "account": "account_id",
            "image": "https://i.pximg.net/u_50.jpg", "imageBig": "https://i.pximg.net/u_170.jpg",
            "commentHtml": "<p>简介</p>", "background": "https://i.pximg.net/bg.jpg",
            "following": 42, "mypixivCount": 7
        });
        let p = parse_user_profile(28640, &body);
        assert_eq!(p.id, 28640);
        assert_eq!(p.name, "作者");
        assert_eq!(p.pixiv_id, "account_id", "account → pixiv_id");
        assert_eq!(
            p.profile_img, "https://i.pximg.net/u_170.jpg",
            "imageBig 优先"
        );
        assert_eq!(p.comment_html.as_deref(), Some("<p>简介</p>"));
        assert_eq!(p.following_count, Some(42));
        assert_eq!(p.mypixiv_count, Some(7));
        // 缺 imageBig 回退 image；缺字段兜底
        let p = parse_user_profile(
            1,
            &json!({"userId": "1", "image": "https://i.pximg.net/u_50.jpg"}),
        );
        assert_eq!(p.profile_img, "https://i.pximg.net/u_50.jpg");
        assert_eq!(p.name, "");
        assert!(p.background.is_none());
    }

    #[test]
    fn parse_user_works_batch_object_and_array_shapes() {
        let obj = json!({
            "1": {"id": "1", "illustType": 0, "title": "A"},
            "2": {"id": "2", "illustType": 1, "title": "B"}
        });
        let items = parse_user_works_batch(&obj, "illust");
        assert_eq!(items.len(), 2);
        let arr = json!([
            {"id": "9", "type": "novel", "title": "N", "textCount": 500},
            {"title": "缺 id 跳过"}
        ]);
        let items = parse_user_works_batch(&arr, "novel");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text_length, Some(500));
        assert!(parse_user_works_batch(&json!("junk"), "illust").is_empty());
    }

    // ---- novel series ----

    #[test]
    fn parse_novel_series_detail_cursor_semantics() {
        let meta = json!({
            "id": 1093870, "title": "系列", "userId": "28640", "userName": "作者",
            "caption": "简介", "cover": "https://i.pximg.net/sc.jpg",
            "total": 45, "isConcluded": false
        });
        let content = |orders: &[i64]| {
            json!({"page": {"seriesContents": orders.iter().map(|o| json!({
            "id": o.to_string(), "title": format!("第{o}话"), "series": {"contentOrder": o},
            "textLength": 1000 + o, "uploadTimestamp": 1759000000i64, "xRestrict": 0
        })).collect::<Vec<_>>()}})
        };
        // 满 30 条 → 游标 = 末条 contentOrder
        let d =
            parse_novel_series_detail(1093870, &meta, &content(&(1..=30).collect::<Vec<_>>()), 30);
        assert_eq!(d.total, 45);
        assert_eq!(d.next_last_order, Some(30));
        assert_eq!(d.contents[0].series_order, 1);
        assert_eq!(d.contents[0].update_date.as_deref(), Some("1759000000"));
        assert_eq!(d.contents[0].text_length, Some(1001));
        assert!(d.is_concluded == Some(false));
        // 已覆盖 total → 到底
        let d =
            parse_novel_series_detail(1093870, &meta, &content(&(31..=45).collect::<Vec<_>>()), 30);
        assert_eq!(d.next_last_order, None, "末条 order=45 >= total=45");
        // 空列表 → 到底
        let d = parse_novel_series_detail(1093870, &meta, &content(&[]), 30);
        assert_eq!(d.next_last_order, None);
        // total 未知：不足一批 → 到底
        let meta_no_total = json!({"id": 1093870, "title": "S"});
        let d = parse_novel_series_detail(1093870, &meta_no_total, &content(&[1, 2]), 30);
        assert_eq!(d.next_last_order, None);
        // 缺 contentOrder 用 1-based 遍历序兜底；缺 id 跳过
        let raw = json!({"page": {"seriesContents": [
            {"id": "1", "title": "a"},
            {"title": "缺 id"},
            {"id": "2", "series": {"contentOrder": 9}}
        ]}});
        let contents = parse_series_contents(&raw);
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[0].series_order, 1, "缺 contentOrder 用遍历序兜底");
        assert_eq!(contents[1].series_order, 9);
    }

    // ---- illust series（§13 系列分集）----

    #[test]
    fn parse_illust_series_maps_series_and_contents() {
        // 形状照 §13.2 实测：page.series[]{workId, order} + thumbnails.illust
        // 同序映射 + illustSeries[0] 元数据 + users[] 数组（同 §12）。
        let body = json!({
            "page": {
                "series": [
                    {"workId": "149896311", "order": 14},
                    {"workId": "149798080", "order": 13}
                ],
                "total": 14,
                "seriesId": "344074",
                "isSetCover": false,
                "otherSeriesId": "341166",
                "recentUpdatedWorkIds": [],
                "isWatched": true,
                "isNotifying": false
            },
            "thumbnails": {"illust": [
                {"id": "149896311", "title": "第14话", "illustType": 1,
                 "pageCount": 30, "xRestrict": 1,
                 "seriesId": "344074", "seriesTitle": "测试系列",
                 "url": "https://i.pximg.net/c/250x250_80_a2/custom-thumb/a.jpg",
                 "urls": {
                     "250x250": "https://i.pximg.net/c/250x250_80_a2/custom-thumb/a.jpg",
                     "360x360": "https://i.pximg.net/c/360x360/custom-thumb/a.jpg",
                     "540x540": "https://i.pximg.net/c/540x540/custom-thumb/a.jpg"
                 },
                 "tags": [{"name": "R-18"}], "aiType": 1, "sl": 2,
                 "userId": "1101", "userName": "画师A",
                 "createDate": "2026-09-20T20:57:00+09:00",
                 "updateDate": "2026-09-20T20:57:00+09:00",
                 "bookmarkData": null, "isUnlisted": false, "isMasked": false},
                {"id": "149798080", "title": "第13话", "illustType": 1,
                 "pageCount": 12, "xRestrict": 0,
                 "seriesId": "344074", "seriesTitle": "测试系列",
                 "url": "https://i.pximg.net/c/250x250_80_a2/custom-thumb/b.jpg",
                 "aiType": 2, "sl": 2,
                 "userId": "1101", "userName": "画师A",
                 "updateDate": "2026-09-01T12:00:00+09:00"}
            ]},
            "illustSeries": [
                {"id": "344074", "userId": "1101", "title": "测试系列",
                 "description": "", "caption": "系列简介", "total": 14,
                 "firstIllustId": "149798080", "latestIllustId": "149896311",
                 "createDate": "2026-08-01T10:00:00+09:00",
                 "updateDate": "2026-09-20T20:57:00+09:00",
                 "content_order": null, "url": null, "coverImageSl": null,
                 "watchCount": null, "isWatched": true, "isNotifying": false},
                {"id": "341166", "userId": "1101", "title": "其他系列", "total": 3}
            ],
            "users": [
                {"userId": "1101", "name": "画师A",
                 "image": "https://i.pximg.net/a50.jpg",
                 "imageBig": "https://i.pximg.net/a170.jpg",
                 "premium": false, "isFollowed": true}
            ]
        });
        // 系列 url 实测 isSetCover=false 时恒 null（cover 省略）；此处单独给
        // 一个带自定义封面的元数据验证映射路径（isSetCover=true 形状未采样，
        // 合理假设 url 为字符串）。
        let mut with_cover = body.clone();
        with_cover["illustSeries"][0]["url"] =
            json!("https://i.pximg.net/series-cover/master/x.jpg");

        let d = parse_illust_series(&body, 344074, 1);
        assert_eq!(d.id, 344074);
        assert_eq!(d.title, "测试系列");
        assert_eq!(d.user_id, 1101);
        assert_eq!(d.user_name, "画师A", "作者名来自 users[] 数组映射");
        assert_eq!(
            d.user_avatar.as_deref(),
            Some("https://i.pximg.net/a170.jpg"),
            "头像 imageBig 优先"
        );
        assert_eq!(d.caption.as_deref(), Some("系列简介"));
        assert!(d.cover.is_none(), "isSetCover=false 时 url=null → 省略");
        assert_eq!(d.total, 14, "total 取 page.total");
        assert!(!d.is_concluded, "pixiv 无完结标记，契约锁定恒 false");
        assert_eq!(d.is_watched, Some(true));
        assert_eq!(d.update_date.as_deref(), Some("2026-09-20T20:57:00+09:00"));
        // 分集：同序按位（workId 映射），order 降序保序
        assert_eq!(
            d.contents.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![149896311, 149798080]
        );
        let first = &d.contents[0];
        assert_eq!(first.kind, "illust", "illustType 0/1/2 均入此 kind");
        assert_eq!(first.title, "第14话");
        assert_eq!(
            first.cover,
            "https://i.pximg.net/c/360x360/custom-thumb/a.jpg",
            "urls.360x360 优先"
        );
        assert_eq!(first.page_count, Some(30));
        assert_eq!(first.x_restrict, Some(1));
        assert_eq!(
            first.update_date.as_deref(),
            Some("2026-09-20T20:57:00+09:00")
        );
        assert_eq!(first.series_order, 14, "order 原样（降序保序）");
        assert_eq!(first.ai_type, Some(1));
        // 第二条：无 urls 档 → 回退顶层 url（官方卡片档回退）
        assert_eq!(
            d.contents[1].cover,
            "https://i.pximg.net/c/250x250_80_a2/custom-thumb/b.jpg"
        );
        assert_eq!(d.contents[1].series_order, 13);
        // 翻页：page 回显 + ceil(14/12)=2 + next
        assert_eq!(d.page, 1);
        assert_eq!(d.total_pages, 2);
        assert_eq!(d.next_page, Some(2));

        // 自定义封面路径 + 封面空值对照
        let covered = parse_illust_series(&with_cover, 344074, 1);
        assert_eq!(
            covered.cover.as_deref(),
            Some("https://i.pximg.net/series-cover/master/x.jpg"),
            "cover = illustSeries[0].url"
        );
    }

    #[test]
    fn parse_illust_series_empty_and_overpage_semantics() {
        // 全缺：无 illustSeries / 无 page → 空目录不报错，id 回显入参
        let d = parse_illust_series(&json!({}), 42, 1);
        assert!(d.contents.is_empty());
        assert_eq!(d.id, 42);
        assert_eq!(d.title, "");
        assert_eq!(d.user_name, "");
        assert_eq!(d.total, 0);
        assert!(!d.is_concluded);
        assert_eq!(d.page, 1);
        assert_eq!(d.total_pages, 0, "total=0 → ceil(0/12)=0");
        assert_eq!(d.next_page, None);

        // 超页（§13.3）：空 series + total 照常 → 空目录，翻页按 total 判定
        let over = json!({
            "page": {"series": [], "total": 14, "seriesId": "344074"},
            "thumbnails": {"illust": []},
            "illustSeries": [
                {"id": "344074", "userId": "1101", "title": "测试系列",
                 "total": 14, "url": null, "isWatched": false}
            ],
            "users": []
        });
        let p1 = parse_illust_series(&over, 344074, 1);
        assert_eq!(p1.total_pages, 2);
        assert_eq!(p1.next_page, Some(2));
        let p2 = parse_illust_series(&over, 344074, 2);
        assert_eq!(p2.next_page, None, "page == total_pages → 到底");
        let p3 = parse_illust_series(&over, 344074, 3);
        assert!(p3.contents.is_empty());
        assert_eq!(p3.next_page, None, "超页静默空页，等价到底");
        assert_eq!(
            p3.is_watched,
            Some(false),
            "meta.isWatched 优先；本例 meta 有值"
        );

        // 页容量边界：total=12 → 1 页无 next；total=13 → 2 页有 next
        let mk = |total: i64| {
            json!({
                "page": {"series": [], "total": total},
                "illustSeries": [{"id": "7", "userId": "1", "title": "S", "total": total}]
            })
        };
        assert_eq!(parse_illust_series(&mk(12), 7, 1).next_page, None);
        assert_eq!(
            parse_illust_series(&mk(13), 7, 1).next_page,
            Some(2),
            "ceil(13/12)=2"
        );

        // 缺 id / 孤儿 workId 逐条跳过（有缩略项的才输出）
        let partial = json!({
            "page": {"series": [
                {"workId": "1", "order": 3},
                {"order": 2},
                {"workId": "999", "order": 1}
            ], "total": 3},
            "thumbnails": {"illust": [
                {"id": "1", "title": "第3话", "illustType": 0,
                 "url": "https://i.pximg.net/a.jpg"}
            ]},
            "illustSeries": [{"id": "7", "userId": "1", "title": "S", "total": 3}]
        });
        let d = parse_illust_series(&partial, 7, 1);
        assert_eq!(d.contents.len(), 1, "缺 workId 与孤儿 workId 跳过");
        assert_eq!(d.contents[0].id, 1);
        assert_eq!(d.contents[0].series_order, 3);
        assert_eq!(d.contents[0].kind, "illust");
    }

    // ---- comments ----

    #[test]
    fn parse_comment_maps_fields_and_completes_img_protocol() {
        // 实测形状：img 无协议前缀；id 字符串；commentDate 原样
        let v = json!({
            "id": "194911294",
            "userId": "28640",
            "userName": "画师",
            "img": "i.pximg.net/userprofile/x.jpg",
            "comment": "こんにちは",
            "commentDate": "2026-10-01 08:15",
            "hasReplies": true
        });
        let c = parse_comment(&v);
        assert_eq!(c.id, "194911294");
        assert_eq!(c.user_id, 28640);
        assert_eq!(c.user_name, "画师");
        assert_eq!(
            c.profile_img.as_deref(),
            Some("https://i.pximg.net/userprofile/x.jpg"),
            "无协议前缀补 https://"
        );
        assert_eq!(c.content.as_deref(), Some("こんにちは"));
        assert_eq!(c.date.as_deref(), Some("2026-10-01 08:15"));
        assert_eq!(c.has_replies, Some(true));
        assert!(c.stamp_url.is_none(), "无 stampId 不拼贴图 URL");
        assert!(c.reply_to_user_name.is_none(), "roots 条目无被回复者");
    }

    #[test]
    fn parse_comment_stamp_and_has_replies_forms() {
        // 表情评论：comment 空字符串 + stampId；hasReplies 字符串形态
        let c = parse_comment(&json!({
            "id": "195000001",
            "userId": "1",
            "userName": "甲",
            "img": "https://i.pximg.net/a.jpg",
            "comment": "",
            "stampId": "636",
            "hasReplies": "true",
            "replyToUserName": "乙"
        }));
        assert_eq!(c.content, None, "空正文省略（表情评论）");
        assert_eq!(
            c.stamp_url.as_deref(),
            Some("https://s.pximg.net/common/images/stamp/generated-stamps/636_s.jpg")
        );
        assert_eq!(c.has_replies, Some(true), "字符串 \"true\" 容错");
        assert_eq!(c.reply_to_user_name.as_deref(), Some("乙"));
        // hasReplies 两形态 + 缺失
        assert_eq!(
            parse_comment(&json!({"id": "1", "hasReplies": false})).has_replies,
            Some(false)
        );
        assert_eq!(
            parse_comment(&json!({"id": "1", "hasReplies": "false"})).has_replies,
            Some(false)
        );
        assert_eq!(parse_comment(&json!({"id": "1"})).has_replies, None);
        // stampId 空/非数字 → 不拼 URL；已带协议的 img 不重复补前缀
        let c = parse_comment(&json!({"id": "2", "stampId": "", "img": "i.pximg.net/b.jpg"}));
        assert!(c.stamp_url.is_none());
        assert_eq!(c.profile_img.as_deref(), Some("https://i.pximg.net/b.jpg"));
    }

    #[test]
    fn parse_comment_tolerates_missing_fields() {
        let c = parse_comment(&json!({"id": "9", "userId": "abc", "img": ""}));
        assert_eq!(c.user_id, 0, "非法 userId 兜底 0");
        assert_eq!(c.profile_img, None, "空 img 跳过");
        assert!(c.content.is_none());
        assert!(c.date.is_none());
        // 缺 id → 空串，parse_comment_list 整条跳过；数字 id 字符串化容错
        let body = json!({"comments": [{"userName": "无 id"}, {"id": 7}, {"id": "1"}]});
        let list = parse_comment_list(&body);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "7");
        assert_eq!(list[1].id, "1");
        // comments 缺失/形状异常 → 空列表
        assert!(parse_comment_list(&json!({})).is_empty());
        assert!(parse_comment_list(&json!({"comments": "junk"})).is_empty());
    }

    #[test]
    fn parse_comments_next_cursor_semantics() {
        let body =
            |has_next: Value| json!({"comments": [{"id": "1"}, {"id": "2"}], "hasNext": has_next});
        // roots：hasNext=true → offset+len
        let r = parse_comments_roots(&body(json!(true)), 10);
        assert_eq!(r.comments.len(), 2);
        assert_eq!(r.next, Some(12));
        // roots：hasNext=false → 到底
        let r = parse_comments_roots(&body(json!(false)), 0);
        assert_eq!(r.next, None);
        // replies：page 游标
        let r = parse_comments_replies(&body(json!("true")), 3);
        assert_eq!(r.next, Some(4));
        let r = parse_comments_replies(&body(json!(false)), 3);
        assert_eq!(r.next, None);
        // hasNext 缺失 → 视为到底
        let r = parse_comments_roots(&json!({"comments": [{"id": "1"}]}), 5);
        assert_eq!(r.next, None);
    }

    // ---- 参数校验与 URL 组装 ----

    #[test]
    fn sanitize_search_word_rules() {
        assert_eq!(
            sanitize_search_word("  東方 Project ").unwrap(),
            "東方 Project"
        );
        assert_eq!(
            sanitize_search_word("a?b#c").unwrap(),
            "abc",
            "去除破坏 URL 结构的字符"
        );
        assert_eq!(
            sanitize_search_word("a\r\nb").unwrap(),
            "ab",
            "去除控制字符"
        );
        assert!(sanitize_search_word("   ").is_err(), "清空后为空 → 报错");
        assert!(sanitize_search_word("?#").is_err());
        let long = "あ".repeat(201);
        assert!(sanitize_search_word(&long).is_err(), "超长 → 报错");
    }

    #[test]
    fn search_path_builds_artworks_and_novels() {
        // word 经 percent-encode 进路径段（非 ASCII 直拼实测 400），query 为白名单 ASCII
        let path = search_path(
            "illust",
            "東方",
            Some("date"),
            Some("r18"),
            Some("s_tag"),
            None,
            2,
        )
        .unwrap();
        assert_eq!(
            path,
            "/ajax/search/artworks/%E6%9D%B1%E6%96%B9?order=date&mode=r18&s_mode=s_tag&type=illust&ai_type=0&p=2&lang=zh"
        );
        let path = search_path("novel", "オリジナル", None, None, None, Some("manga"), 1).unwrap();
        assert_eq!(
            path,
            "/ajax/search/novels/%E3%82%AA%E3%83%AA%E3%82%B8%E3%83%8A%E3%83%AB?order=date_d&mode=all&p=1&lang=zh",
            "novel 不带 type/ai_type，type_ 被忽略"
        );
        let path = search_path("manga", "w", None, None, None, Some("ugoira"), 0).unwrap();
        assert!(path.contains("type=ugoira"), "显式 type 优先");
        assert!(path.contains("p=1"), "页码下限 1");
        // 白名单校验
        assert!(search_path("illust", "w", Some("bogus"), None, None, None, 1).is_err());
        assert!(search_path("illust", "w", None, Some("x"), None, None, 1).is_err());
        assert!(search_path("illust", "w", None, None, Some("y"), None, 1).is_err());
        assert!(search_path("illust", "w", None, None, None, Some("z"), 1).is_err());
        assert!(search_path("video", "w", None, None, None, None, 1).is_err());
        assert!(search_path("illust", "", None, None, None, None, 1).is_err());
    }

    /// 搜索词编码：ASCII 保留、非 ASCII 与保留字符编码、`?`/`#` 已在
    /// sanitize 阶段剥除；编码后不得再出现裸的非 ASCII 字节。
    #[test]
    fn search_path_percent_encodes_non_ascii_word() {
        let path = search_path("illust", "風景 100%", None, None, None, None, 1).unwrap();
        assert!(
            path.starts_with("/ajax/search/artworks/%E9%A2%A8%E6%99%AF%20100%25?"),
            "空格与 % 均应编码，实际: {path}"
        );
        assert!(path.is_ascii(), "路径必须全 ASCII（wreq 不会自动编码）");
    }

    #[test]
    fn normalize_ymd_forms() {
        assert_eq!(normalize_ymd("20260930"), "20260930");
        assert_eq!(normalize_ymd("2026-09-30"), "20260930", "novel 频道形态");
        assert_eq!(
            normalize_ymd("2026年9月30日"),
            "20260930",
            "小说榜形态（月/日无前导零）"
        );
        assert_eq!(normalize_ymd("2026-9-3"), "20260903");
        assert_eq!(normalize_ymd(""), "", "空串原样");
        assert_eq!(normalize_ymd("本周"), "本周", "识别不了的原样返回");
        assert_eq!(normalize_ymd("2026-13-01"), "2026-13-01", "非法月原样返回");
    }

    #[test]
    fn validate_ranking_modes_table() {
        for kind in ["illust", "manga", "ugoira"] {
            for mode in [
                "daily",
                "weekly",
                "monthly",
                "rookie",
                "daily_r18",
                "weekly_r18",
            ] {
                assert!(validate_ranking(kind, mode).is_ok(), "{kind}/{mode}");
            }
            // Premium 限定榜不在表内
            assert!(
                validate_ranking(kind, "male").is_err(),
                "{kind}/male 应拒绝"
            );
        }
        for mode in ["daily", "weekly", "monthly", "male", "female", "daily_r18"] {
            assert!(validate_ranking("novel", mode).is_ok(), "novel/{mode}");
        }
        assert!(validate_ranking("novel", "rookie").is_err());
        assert!(validate_ranking("video", "daily").is_err());
        assert!(validate_ranking_date(None).is_ok());
        assert!(validate_ranking_date(Some("20260929")).is_ok());
        assert!(validate_ranking_date(Some("2026-09-29")).is_err());
        assert!(validate_ranking_date(Some("2026093")).is_err());
    }

    #[test]
    fn web_csrf_cache_roundtrip() {
        // 缓存空 → 未命中；写入 → 命中；失效 → 未命中
        invalidate_web_csrf();
        assert!(cached_web_csrf().is_none());
        let cache = WEB_CSRF_CACHE.get_or_init(|| Mutex::new(None));
        *cache.lock().unwrap() = Some(("tok-for-test".to_string(), Instant::now()));
        assert_eq!(cached_web_csrf().as_deref(), Some("tok-for-test"));
        invalidate_web_csrf();
        assert!(cached_web_csrf().is_none());
    }

    #[test]
    fn kind_of_priority_order() {
        // illustType 数字优先于 type 字符串
        let v = json!({"illustType": 1, "type": "illust"});
        assert_eq!(kind_of(&v, "novel"), "manga");
        // 仅 type 字符串
        assert_eq!(kind_of(&json!({"type": "ugoira"}), "illust"), "ugoira");
        // 都缺 → fallback
        assert_eq!(kind_of(&json!({}), "illust"), "illust");
        // 非法 type 字符串 → fallback
        assert_eq!(kind_of(&json!({"type": "collection"}), "illust"), "illust");
    }

    #[test]
    fn parse_tags_mixed_shapes() {
        assert_eq!(
            parse_tags(Some(&json!(["a", "b"]))).unwrap(),
            vec!["a", "b"]
        );
        assert_eq!(
            parse_tags(Some(&json!([{"name": "x"}, {"name": ""}, {"other": 1}]))).unwrap(),
            vec!["x"],
            "对象形取 name，空名跳过"
        );
        assert!(parse_tags(Some(&json!([]))).is_none(), "空数组视为无标签");
        assert!(parse_tags(None).is_none());
        assert!(parse_tags(Some(&json!("not-array"))).is_none());
    }

    // ---- bookmark ----

    #[test]
    fn parse_bookmark_data_three_states() {
        // 未收藏：null / 键缺失 / id 缺失或空 → None
        assert!(parse_bookmark_data(Some(&Value::Null)).is_none());
        assert!(parse_bookmark_data(None).is_none());
        assert!(parse_bookmark_data(Some(&json!({}))).is_none());
        assert!(parse_bookmark_data(Some(&json!({"id": ""}))).is_none());
        // 已收藏（公开）：{id, private:false}（§11.7 实测形状）
        let s =
            parse_bookmark_data(Some(&json!({"id": "31000000001", "private": false}))).unwrap();
        assert_eq!(s.bookmark_id, "31000000001");
        assert_eq!(s.restrict, 0);
        // 已收藏（非公开）
        let s =
            parse_bookmark_data(Some(&json!({"id": "31000000002", "private": true}))).unwrap();
        assert_eq!(s.restrict, 1);
        // 小说实测数字 id → 统一 String 化
        let s =
            parse_bookmark_data(Some(&json!({"id": 3100000001_i64, "private": true}))).unwrap();
        assert_eq!(s.bookmark_id, "3100000001");
        // private 字符串形态容错
        let s = parse_bookmark_data(Some(&json!({"id": "1", "private": "true"}))).unwrap();
        assert_eq!(s.restrict, 1);
        let s = parse_bookmark_data(Some(&json!({"id": "1", "private": "false"}))).unwrap();
        assert_eq!(s.restrict, 0);
    }

    #[test]
    fn parse_bookmark_list_total_and_next_semantics() {
        let work = |id: i64, bid: Value, private: bool| {
            json!({
                "id": id.to_string(), "illustType": 0,
                "title": format!("t{id}"), "userId": "10", "userName": "甲",
                "bookmarkData": {"id": bid, "private": private}
            })
        };
        // 列表项映射：bookmarkData → bookmarkId/bookmarkRestrict；数字 id String 化
        let body = json!({
            "works": [work(1, json!("100"), false), work(2, json!(200), true)],
            "total": 545
        });
        let list = parse_bookmark_list(&body, "illust", 0, 48);
        assert_eq!(list.total, Some(545));
        assert_eq!(list.items.len(), 2);
        assert_eq!(list.items[0].bookmark_id.as_deref(), Some("100"));
        assert_eq!(list.items[0].bookmark_restrict, Some(0));
        assert_eq!(
            list.items[1].bookmark_id.as_deref(),
            Some("200"),
            "小说/数字 id 统一字符串化"
        );
        assert_eq!(list.items[1].bookmark_restrict, Some(1));
        // 未满页（works.len()=2 < limit=48）→ 到底
        assert_eq!(list.next, None);

        // 满页 48：next = offset + len
        let works: Vec<_> = (0..48).map(|i| work(i, json!(1000 + i), false)).collect();
        let full = json!({"works": works, "total": 100});
        let list = parse_bookmark_list(&full, "illust", 0, 48);
        assert_eq!(list.next, Some(48));
        // offset 推进后越过 total → 到底
        let list = parse_bookmark_list(&full, "illust", 96, 48);
        assert_eq!(list.next, None, "96+48 >= total=100");
        // total 缺失：满页 → 仅按 len 推进
        let no_total = json!({"works": (0..48).map(|i| work(i, json!(i), false)).collect::<Vec<_>>()});
        let list = parse_bookmark_list(&no_total, "illust", 48, 48);
        assert_eq!(list.next, Some(96));

        // 空 works / 缺 works / bookmarkData=null（他人列表语义）容错
        let list = parse_bookmark_list(&json!({"works": [], "total": 0}), "novel", 0, 30);
        assert!(list.items.is_empty());
        assert_eq!(list.total, Some(0));
        assert_eq!(list.next, None);
        let list = parse_bookmark_list(&json!({}), "novel", 0, 30);
        assert!(list.items.is_empty());
        assert_eq!(list.total, None);
        let body = json!({"works": [{"id": "1", "bookmarkData": null}], "total": 1});
        let list = parse_bookmark_list(&body, "illust", 0, 48);
        assert_eq!(list.items.len(), 1);
        assert!(list.items[0].bookmark_id.is_none());
    }

    #[test]
    fn parse_bookmark_tags_groups_and_empty_name_kept() {
        // 实测形状（§11.2）：{public:[{tag,cnt}], private:[...]}，一次两组
        let body = json!({
            "public": [{"tag": "未分類", "cnt": 545}, {"tag": "風景", "cnt": 23}],
            "private": [{"tag": "R-18", "cnt": 24}],
            "tooManyBookmark": false,
            "tooManyBookmarkTags": false
        });
        let tags = parse_bookmark_tags(&body);
        assert_eq!(tags.public.len(), 2);
        assert_eq!(tags.public[0].name, "未分類");
        assert_eq!(tags.public[0].count, 545);
        assert_eq!(tags.public[1].name, "風景");
        assert_eq!(tags.private.len(), 1);
        assert_eq!(tags.private[0].count, 24);
        // 空 tag 名原样保留（未分类的本地化由前端 i18n）
        let tags = parse_bookmark_tags(&json!({"public": [{"tag": "", "cnt": 3}], "private": []}));
        assert_eq!(tags.public[0].name, "");
        assert_eq!(tags.public[0].count, 3);
        assert!(tags.private.is_empty());
        // 缺键 → 空组；cnt 缺失 → 0；缺 tag 的条目跳过
        let tags = parse_bookmark_tags(&json!({}));
        assert!(tags.public.is_empty() && tags.private.is_empty());
        let tags = parse_bookmark_tags(&json!({"public": [{"cnt": 9}, {"tag": "x"}]}));
        assert_eq!(tags.public.len(), 1, "缺 tag 名的条目跳过");
        assert_eq!(tags.public[0].count, 0);
    }

    #[test]
    fn parse_bookmark_add_id_both_response_shapes() {
        // 插画（§11.4）：body 是对象，取 last_bookmark_id
        assert_eq!(
            parse_bookmark_add_id(
                &json!({"last_bookmark_id": "31000000001", "stacc_status_id": null})
            )
            .unwrap(),
            "31000000001"
        );
        // 小说：body 直接是 bookmarkId 字符串
        assert_eq!(
            parse_bookmark_add_id(&json!("3100000001")).unwrap(),
            "3100000001"
        );
        // 容错：last_bookmark_id 数字形态
        assert_eq!(
            parse_bookmark_add_id(&json!({"last_bookmark_id": 123})).unwrap(),
            "123"
        );
        // 空 / 缺失 → Err
        assert!(parse_bookmark_add_id(&json!({})).is_err());
        assert!(parse_bookmark_add_id(&json!("")).is_err());
        assert!(parse_bookmark_add_id(&json!(null)).is_err());
    }

    #[test]
    fn bookmark_request_encoding_and_forms() {
        // 列表路径：官方参数全集（order=desc / mode=all 写死），tag 缺省空串
        assert_eq!(
            bookmark_list_path(9000099, "novel", "show", None, 0, 30),
            "/ajax/user/9000099/novels/bookmarks?tag=&offset=0&limit=30&rest=show&order=desc&mode=all&lang=zh"
        );
        // tag 值 percent-encode（日文 + 空格），illust → illusts
        assert_eq!(
            bookmark_list_path(1, "illust", "hide", Some("東方 Project"), 48, 48),
            "/ajax/user/1/illusts/bookmarks?tag=%E6%9D%B1%E6%96%B9%20Project&offset=48&limit=48&rest=hide&order=desc&mode=all&lang=zh"
        );
        // 插画删除 form：bookmark_id=（与 add 不对称）
        assert_eq!(illust_delete_form("31000000001"), "bookmark_id=31000000001");
        // 小说删除 form：旧式表单（tt 字段 + book_id%5B%5D + del=1）
        assert_eq!(
            novel_delete_form("abc123def", "3100000001"),
            "tt=abc123def&p=1&untagged=0&rest=show&book_id%5B%5D=3100000001&del=1"
        );
        // percent_encode 边界
        assert_eq!(percent_encode("a b&c=1"), "a%20b%26c%3D1");
        assert_eq!(percent_encode("AZaz09-_.~"), "AZaz09-_.~", "unreserved 原样");
    }

    #[test]
    fn bookmark_wire_field_names_camel_case() {
        // 契约 v3.1：扩展字段 wire 名为 camelCase（bookmarkId/bookmarkRestrict）
        let item = BrowseWorkItem {
            bookmark_id: Some("2".into()),
            bookmark_restrict: Some(1),
            ..BrowseWorkItem::default()
        };
        let v = serde_json::to_value(&item).unwrap();
        assert_eq!(v.get("bookmarkId"), Some(&json!("2")));
        assert_eq!(v.get("bookmarkRestrict"), Some(&json!(1)));
        // 详情 bookmarkState：未收藏省略字段，已收藏 camelCase
        let detail = BrowseIllustDetail {
            detail_kind: "illust",
            item: BrowseWorkItem::default(),
            pages: vec![],
            ugoira: None,
            series: None,
            bookmark_state: None,
        };
        let v = serde_json::to_value(&detail).unwrap();
        assert!(v.get("bookmarkState").is_none(), "未收藏省略 bookmarkState");
        let state = BrowseBookmarkState {
            bookmark_id: "3".into(),
            restrict: 1,
        };
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            json!({"bookmarkId": "3", "restrict": 1})
        );
    }

    #[test]
    fn parse_novel_detail_bookmark_state_numeric_id() {
        // 小说详情：bookmarkData.id 数字形态 → bookmarkState 字符串化
        // （注：parse_novel_detail 的 item 历史上不填 bookmarked——novel 详情
        // 契约无该字段；收藏态统一走顶层 bookmarkState）
        let body = json!({
            "id": "9000012", "title": "t", "content": "c",
            "bookmarkData": {"id": 3100000001_i64, "private": true}
        });
        let (item, _) = parse_novel_detail(&body);
        assert_eq!(item.bookmarked, None, "novel 详情 item 不含 bookmarked（现状）");
        let state = parse_bookmark_data(body.get("bookmarkData")).unwrap();
        assert_eq!(state.bookmark_id, "3100000001");
        assert_eq!(state.restrict, 1);
    }

    #[test]
    fn self_uid_cache_roundtrip() {
        // 缓存空 → 未命中；写入 → 命中；失效 → 未命中（与 token 缓存同策略）
        invalidate_self_uid();
        assert!(cached_self_uid().is_none());
        let cache = SELF_UID_CACHE.get_or_init(|| Mutex::new(None));
        *cache.lock().unwrap() = Some((9000099, Instant::now()));
        assert_eq!(cached_self_uid().map(|(uid, _)| uid), Some(9000099));
        invalidate_self_uid();
        assert!(cached_self_uid().is_none());
    }

    #[test]
    fn comments_closed_envelope_and_bad_request_gate() {
        // 关闭评论区信封：disabled=true + 空列表 + 无 next；正常信封省略 disabled
        let closed = serde_json::to_value(&BrowseComments {
            comments: Vec::new(),
            next: None,
            disabled: Some(true),
        })
        .unwrap();
        assert_eq!(closed, json!({"comments": [], "disabled": true}));
        let normal = serde_json::to_value(&parse_comments_roots(&json!({ "comments": [] }), 0))
            .unwrap();
        assert_eq!(normal, json!({ "comments": [] }), "正常信封不应出现 disabled");
        // 400 判定只认 classify_status 的 "HTTP 400" 文案；API 层 error body
        // （"API error: …"）与其他状态码不触发关闭映射
        assert!(PixivError::Client("HTTP 400".into()).is_bad_request());
        assert!(!PixivError::Client("HTTP 418".into()).is_bad_request());
        assert!(!PixivError::Client("API error: 不正确的请求。".into()).is_bad_request());
        assert!(!PixivError::NotFound.is_bad_request());
    }
}
