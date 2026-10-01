//! 浏览模式 ajax 接口层（browse-ui-v1，IPC 契约 v2）。
//!
//! 为契约 v2 的 11 个 browse 命令提供 `PixivApi` 方法（跨文件固有实现块），
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

/// 频道页（/ajax/top/illust|manga|novel）组装结果。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseChannel {
    pub follow: BrowseList,
    pub recommend: BrowseList,
    pub ranking: BrowseList,
    pub new_post: BrowseList,
    pub trending_tags: Vec<TrendingTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking_date: Option<String>,
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

/// 插画/漫画/动图详情（顶层带 detail_kind: "illust"）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseIllustDetail {
    pub detail_kind: &'static str,
    pub item: BrowseWorkItem,
    /// 单页作品也返回 1 项。
    pub pages: Vec<BrowseIllustPage>,
    pub ugoira: Option<BrowseUgoira>,
    pub series: Option<BrowseSeriesRef>,
}

/// 小说详情（顶层带 detail_kind: "novel"）。
#[derive(Debug, Clone, Serialize)]
pub struct BrowseNovelDetail {
    pub detail_kind: &'static str,
    pub item: BrowseWorkItem,
    /// 全文，保留 [newpage]/[chapter:]/[rb:]/[pixivimage:] 原始标记，前端切分。
    pub content: String,
    pub series: Option<BrowseSeriesRef>,
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
                .or_else(|| urls.as_object()?.values().find_map(Value::as_str).map(String::from))
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
            let id = as_i64_loose(idv)?;
            let mut item = parse_work_thumb(index.get(&id)?, fallback_kind)?;
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
    let trending_tags = body
        .pointer("/page/trendingTags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let name = str_field(t, "tag")?;
                    Some(TrendingTag {
                        name,
                        translated_name: str_field(t, "translatedName"),
                        count: t.get("illustCount").and_then(as_i64_loose),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let ranking_date = body
        .pointer("/page/ranking/date")
        .and_then(Value::as_str)
        .map(String::from);
    BrowseChannel {
        follow: list_from_items(follow),
        recommend: list_from_items(recommend),
        ranking: list_from_items(ranking),
        new_post: list_from_items(new_post),
        trending_tags,
        ranking_date,
    }
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
        date: str_field(body, "date"),
        prev_date: str_field(body, "prev_date"),
        next_date: str_field(body, "next_date"),
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
        date: str_field(body, "date"),
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

/// 搜索词清理：去掉会破坏 URL 结构的 `?`/`#` 与控制字符（其余字符交给
/// wreq 的 IntoUri/url 规范化做百分号编码，不手写 encode）。
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
    Ok(format!("/ajax/search/{seg}/{word}?{}", params.join("&")))
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
        assert_eq!(item.cover.as_deref(), Some("https://i.pximg.net/c/540x540/img-master/mid.jpg"));
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
                "ranking": {"items": [1, 3], "date": "20260929"},
                "newPost": [4],
                "trendingTags": [
                    {"tag": "オリジナル", "translatedName": "原创", "illustCount": 9999},
                    {"tag": ""}
                ]
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
        assert_eq!(
            ch.ranking.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(
            ch.new_post.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            vec![4]
        );
        assert_eq!(ch.ranking_date.as_deref(), Some("20260929"));
        assert_eq!(ch.trending_tags.len(), 1, "空 tag 跳过");
        assert_eq!(ch.trending_tags[0].name, "オリジナル");
        assert_eq!(ch.trending_tags[0].translated_name.as_deref(), Some("原创"));
        assert_eq!(ch.trending_tags[0].count, Some(9999));
    }

    #[test]
    fn parse_channel_missing_sections_is_empty_not_error() {
        let ch = parse_channel(&json!({"thumbnails": {"novel": []}}), "novel");
        assert!(ch.follow.items.is_empty());
        assert!(ch.recommend.items.is_empty());
        assert!(ch.ranking.items.is_empty());
        assert!(ch.new_post.items.is_empty());
        assert!(ch.trending_tags.is_empty());
        assert!(ch.ranking_date.is_none());
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
    fn parse_ranking_novel_shape_and_last_page() {
        let entry = json!({
            "rank": 1, "id": "27466576", "title": "小说", "create_date": "2026-09-01",
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

    // ---- 详情 ----

    #[test]
    fn parse_illust_detail_fields() {
        let body = json!({
            "illustId": "131592804",
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
        assert_eq!(item.id, 131592804);
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
            "id": "27466576",
            "title": "相対性理論",
            "content": "第一章[newpage]第二章",
            "description": "<p>简介</p>",
            "coverUrl": "https://i.pximg.net/novel-cover.jpg",
            "userId": "28640", "userName": "作者",
            "pageCount": 2,
            "tags": [{"tag": "SF"}],
            "seriesNavData": {"seriesType": "novel", "seriesId": 1093870, "title": "系列", "orderNumber": 2,
                "next": {"id": "28625793", "title": "下一话"}},
            "characterCount": 12345,
            "readingTime": 25,
            "bookmarkCount": 66,
            "xRestrict": 0,
            "createDate": "2026-07-21T00:00:00+09:00"
        });
        let (item, series) = parse_novel_detail(&body);
        assert_eq!(item.id, 27466576);
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
        assert_eq!(series.next_id, Some(28625793), "seriesNavData.next.id");
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
        // word 原样进路径（编码交给 wreq IntoUri/url 规范化），query 为白名单 ASCII
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
            "/ajax/search/artworks/東方?order=date&mode=r18&s_mode=s_tag&type=illust&ai_type=0&p=2&lang=zh"
        );
        let path = search_path("novel", "オリジナル", None, None, None, Some("manga"), 1).unwrap();
        assert_eq!(
            path, "/ajax/search/novels/オリジナル?order=date_d&mode=all&p=1&lang=zh",
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
}
