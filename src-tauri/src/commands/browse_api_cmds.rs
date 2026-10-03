//! 浏览命令层（browse-ui-v1，IPC 契约 v2，13 个命令；bookmark-ui-v1 契约
//! v3.1 追加 4 个收藏命令，watchlist-ui-v1 追加 1 个追更列表命令，
//! series-episode-ui 追加 1 个系列分集命令，共 19 个）。
//!
//! 形状约定与 history_cmds 一致：`#[tauri::command]` 薄壳 +
//! `*_impl(&AppState, ...)` 可离线调用，业务失败统一 `Err(中文文案)`。
//!
//! `*_impl` 的调用顺序（离线冒烟 tests/pixiv_api/offline_guard.rs 对齐此顺序）：
//! 1. 参数粗校验（空 word / page<1 / id 数字域 / offset<0 / 空 comment_id /
//!    kind-mode-date 白名单）
//!    → 可读中文 Err。放在登录守卫之前，未登录也能先暴露参数错误；
//! 2. [`build_browse_api`]：读登录态 → PHPSESSID 为空即
//!    [`NOT_LOGGED_IN`]（前端 browse IPC 层以「登录」关键字识别并弹登录窗，
//!    此文案逐字保留，勿改）→ PixivClient::new → PixivApi::new；
//! 3. 调 B1（pixiv/browse_api.rs）对应方法——api 层白名单校验为权威，
//!    命令层白名单只是登录前的粗校验，文案保持一致；
//! 4. `PixivError` → `err.to_string()`（thiserror 中文 Display，与 crawler 一致）。
//!
//! 日志纪律：本层不打印 cookie/token 值。

use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::pixiv::api::PixivApi;
use crate::pixiv::client::PixivClient;
use crate::state::AppState;

/// 未登录错误文案（前端 browse IPC 层以「登录」关键字识别并弹登录窗）。
pub const NOT_LOGGED_IN: &str = "未登录或登录态已失效，请先登录";

/// 构造浏览 API 客户端：登录守卫（PHPSESSID 非空）+ 指纹伪装客户端。
pub fn build_browse_api(state: &AppState) -> Result<PixivApi, String> {
    let Some(cookies) = state.cookies.load()? else {
        return Err(NOT_LOGGED_IN.to_string());
    };
    if cookies.get("PHPSESSID").is_none_or(|sid| sid.is_empty()) {
        return Err(NOT_LOGGED_IN.to_string());
    }
    let client =
        PixivClient::new(&cookies).map_err(|err| format!("创建 HTTP 客户端失败: {err}"))?;
    Ok(PixivApi::new(Arc::new(client)))
}

// ----------------------------------------------------------------------
// 参数粗校验（纯函数；api 层白名单为权威，此处保证未登录也先给参数错误）
// ----------------------------------------------------------------------

/// 页码：必须从 1 开始（api 层另有 max(1) 兜底，此处显式拒绝）。
fn validate_page(page: i64) -> Result<(), String> {
    if page < 1 {
        return Err("页码必须从 1 开始".to_string());
    }
    Ok(())
}

/// pixiv id 恒为正整数。非数字 id 在 Tauri IPC 反序列化层即被拒（形参 i64
/// 对应契约 id:number，到不了命令层），此处拦数字域越界（0/负数）。
fn validate_id(id: i64, what: &str) -> Result<(), String> {
    if id <= 0 {
        return Err(format!("{what} ID 必须为正整数"));
    }
    Ok(())
}

/// 频道 kind（B1 兼容 pixiv 端命名的 "illustration"）。
fn validate_channel_kind(kind: &str) -> Result<(), String> {
    if !matches!(kind, "illust" | "illustration" | "manga" | "novel") {
        return Err(format!("不支持的频道类型: {kind}"));
    }
    Ok(())
}

/// 追更列表 kind（官方 /following/watchlist 只有漫画、小说两个子 tab）。
fn validate_watchlist_kind(kind: &str) -> Result<(), String> {
    if !matches!(kind, "manga" | "novel") {
        return Err(format!("不支持的追更类型: {kind}"));
    }
    Ok(())
}

/// 关注动态 kind/mode 白名单。
fn validate_feed(kind: &str, mode: &str) -> Result<(), String> {
    if !matches!(kind, "illust" | "novel") {
        return Err(format!("不支持的类型: {kind}（仅 illust|novel）"));
    }
    if !matches!(mode, "all" | "safe" | "r18") {
        return Err(format!("不支持的过滤模式: {mode}"));
    }
    Ok(())
}

/// 搜索 kind/word/mode 粗校验（order/s_mode/type 白名单交由 api 层校验）。
fn validate_search(kind: &str, word: &str, mode: Option<&str>) -> Result<(), String> {
    if word.trim().is_empty() {
        return Err("搜索词不能为空".to_string());
    }
    if !matches!(kind, "illust" | "manga" | "ugoira" | "novel") {
        return Err(format!("不支持的搜索类型: {kind}"));
    }
    if let Some(mode) = mode {
        if !matches!(mode, "all" | "safe" | "r18") {
            return Err(format!("不支持的过滤模式: {mode}"));
        }
    }
    Ok(())
}

/// 计数批量 kind/ids 粗校验（illust/manga/ugoira 同走 /ajax/illust/{id} 详情）。
fn validate_work_counts(kind: &str, ids: &[i64]) -> Result<(), String> {
    if !matches!(kind, "illust" | "manga" | "ugoira" | "novel") {
        return Err(format!("不支持的搜索类型: {kind}"));
    }
    if ids.is_empty() {
        return Err("作品 ID 列表不能为空".to_string());
    }
    if ids.len() > 60 {
        return Err(format!("单次最多查询 60 个作品（收到 {}）", ids.len()));
    }
    if ids.iter().any(|id| *id <= 0) {
        return Err("作品 ID 必须为正整数".to_string());
    }
    Ok(())
}

/// 排行榜 mode 白名单（与 api 层 validate_ranking 同表；Premium 限定榜不在表内）。
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

/// 排行榜 kind/mode/date 粗校验。
fn validate_ranking(kind: &str, mode: &str, date: Option<&str>) -> Result<(), String> {
    let modes: &[&str] = match kind {
        "illust" | "manga" | "ugoira" => &RANKING_MODES_ILLUST,
        "novel" => &RANKING_MODES_NOVEL,
        other => return Err(format!("不支持的排行榜类型: {other}")),
    };
    if !modes.contains(&mode) {
        return Err(format!("不支持的排行榜模式: {mode}"));
    }
    if let Some(d) = date {
        if d.len() != 8 || !d.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("date 格式应为 yyyymmdd: {d}"));
        }
    }
    Ok(())
}

/// 详情/相关推荐 kind 白名单（ugoira 复用 illust 详情端点）。
fn validate_work_kind(kind: &str) -> Result<(), String> {
    if !matches!(kind, "illust" | "manga" | "ugoira" | "novel") {
        return Err(format!("不支持的作品类型: {kind}"));
    }
    Ok(())
}

/// 作者作品 kind 白名单（B1 仅支持 illust|manga|novel）。
fn validate_user_works_kind(kind: &str) -> Result<(), String> {
    if !matches!(kind, "illust" | "manga" | "novel") {
        return Err(format!("不支持的作品类型: {kind}"));
    }
    Ok(())
}

/// 评论 kind 白名单（manga 走 illusts 评论端点）。
fn validate_comment_kind(kind: &str) -> Result<(), String> {
    if !matches!(kind, "illust" | "manga" | "novel") {
        return Err(format!("不支持的作品类型: {kind}"));
    }
    Ok(())
}

/// 相关推荐 limit 区间（api 层同区间 clamp）。
fn validate_limit(limit: Option<i64>) -> Result<(), String> {
    if limit.is_some_and(|l| !(1..=30).contains(&l)) {
        return Err("limit 必须在 1~30 之间".to_string());
    }
    Ok(())
}

// ----------------------------------------------------------------------
// 收藏（bookmark，契约 v3.1）参数粗校验
// ----------------------------------------------------------------------

/// 收藏 kind 白名单（契约只分 illust|novel；漫画/动图的收藏走 illust 端点，
/// 由前端传 "illust"）。
fn validate_bookmark_kind(kind: &str) -> Result<(), String> {
    if !matches!(kind, "illust" | "novel") {
        return Err(format!("不支持的收藏类型: {kind}"));
    }
    Ok(())
}

/// 收藏可见性 rest 白名单（show=公开 / hide=非公开；api 层同表）。
fn validate_bookmark_rest(rest: &str) -> Result<(), String> {
    if !matches!(rest, "show" | "hide") {
        return Err(format!("不支持的可见范围: {rest}"));
    }
    Ok(())
}

/// 收藏列表 limit 区间（api 层 clamp 1~100：实测服务端接受 10/48/100）。
fn validate_bookmark_limit(limit: Option<i64>) -> Result<(), String> {
    if limit.is_some_and(|l| !(1..=100).contains(&l)) {
        return Err("limit 必须在 1~100 之间".to_string());
    }
    Ok(())
}

/// 收藏可见性 restrict（0=公开 / 1=非公开，与 pixiv restrict 字段语义一致）。
fn validate_bookmark_restrict(restrict: i64) -> Result<(), String> {
    if !matches!(restrict, 0 | 1) {
        return Err(format!(
            "restrict 只能是 0（公开）或 1（非公开）: {restrict}"
        ));
    }
    Ok(())
}

// ----------------------------------------------------------------------
// 命令薄壳 + 实现（薄壳只做 State 解引用与 String → &str 透传）
// ----------------------------------------------------------------------

/// 首页推荐流（无翻页；前端「换一批」重复调用并按 id 去重）。
#[tauri::command]
pub async fn browse_home_feed(state: State<'_, AppState>) -> Result<Value, String> {
    browse_home_feed_impl(&state).await
}

pub async fn browse_home_feed_impl(state: &AppState) -> Result<Value, String> {
    build_browse_api(state)?
        .get_home_street()
        .await
        .map_err(|err| err.to_string())
}

/// 频道页快照（插画/漫画/小说：四板块 + 热门标签）。
#[tauri::command]
pub async fn browse_channel(
    state: State<'_, AppState>,
    kind: String,
    mode: Option<String>,
) -> Result<Value, String> {
    browse_channel_impl(&state, &kind, mode.as_deref()).await
}

pub async fn browse_channel_impl(
    state: &AppState,
    kind: &str,
    mode: Option<&str>,
) -> Result<Value, String> {
    validate_channel_kind(kind)?;
    if !matches!(mode, None | Some("all" | "r18")) {
        return Err(format!("不支持的频道模式: {}", mode.unwrap_or_default()));
    }
    build_browse_api(state)?
        .get_channel(kind, mode)
        .await
        .map_err(|err| err.to_string())
}

/// 追更列表（漫画/小说订阅系列；后端按 max_page 聚合，前端无需翻页）。
#[tauri::command]
pub async fn browse_watchlist(state: State<'_, AppState>, kind: String) -> Result<Value, String> {
    browse_watchlist_impl(&state, &kind).await
}

pub async fn browse_watchlist_impl(state: &AppState, kind: &str) -> Result<Value, String> {
    validate_watchlist_kind(kind)?;
    build_browse_api(state)?
        .get_watchlist(kind)
        .await
        .map_err(|err| err.to_string())
}

/// 发现（仅作品无小说；无翻页参数，前端重复调用按 id 去重追加）。
#[tauri::command]
pub async fn browse_discover(state: State<'_, AppState>) -> Result<Value, String> {
    browse_discover_impl(&state).await
}

pub async fn browse_discover_impl(state: &AppState) -> Result<Value, String> {
    build_browse_api(state)?
        .get_discover()
        .await
        .map_err(|err| err.to_string())
}

/// 关注动态（next_page = isLastPage ? null : p+1）。
#[tauri::command]
pub async fn browse_follow_latest(
    state: State<'_, AppState>,
    kind: String,
    mode: String,
    page: i64,
) -> Result<Value, String> {
    browse_follow_latest_impl(&state, &kind, &mode, page).await
}

pub async fn browse_follow_latest_impl(
    state: &AppState,
    kind: &str,
    mode: &str,
    page: i64,
) -> Result<Value, String> {
    validate_feed(kind, mode)?;
    validate_page(page)?;
    build_browse_api(state)?
        .get_follow_latest(kind, mode, page)
        .await
        .map_err(|err| err.to_string())
}

/// 搜索（含 total；order/s_mode/type 缺省由 api 层兜底）。
/// `type` 为 Rust 关键字：形参用 r#type，Tauri 宏 unraw 后与前端键名 "type" 对齐。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn browse_search(
    state: State<'_, AppState>,
    kind: String,
    word: String,
    order: Option<String>,
    mode: Option<String>,
    s_mode: Option<String>,
    r#type: Option<String>,
    page: i64,
) -> Result<Value, String> {
    browse_search_impl(
        &state,
        &kind,
        &word,
        order.as_deref(),
        mode.as_deref(),
        s_mode.as_deref(),
        r#type.as_deref(),
        page,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn browse_search_impl(
    state: &AppState,
    kind: &str,
    word: &str,
    order: Option<&str>,
    mode: Option<&str>,
    s_mode: Option<&str>,
    type_: Option<&str>,
    page: i64,
) -> Result<Value, String> {
    validate_search(kind, word, mode)?;
    validate_page(page)?;
    build_browse_api(state)?
        .get_search(kind, word, order, mode, s_mode, type_, page)
        .await
        .map_err(|err| err.to_string())
}

/// 作品三项计数批量（搜索页本页排序用）。ids 上限 60（= 一个接口页）；
/// 逐项失败由 api 层跳过（返回表缺该 id，前端按未知处理）。
#[tauri::command]
pub async fn browse_work_counts(
    state: State<'_, AppState>,
    kind: String,
    ids: Vec<i64>,
) -> Result<Value, String> {
    browse_work_counts_impl(&state, &kind, &ids).await
}

pub async fn browse_work_counts_impl(
    state: &AppState,
    kind: &str,
    ids: &[i64],
) -> Result<Value, String> {
    validate_work_counts(kind, ids)?;
    build_browse_api(state)?
        .get_work_counts(kind, ids)
        .await
        .map_err(|err| err.to_string())
}

/// 排行榜（p 每页 50；date 供历史榜单切换）。
#[tauri::command]
pub async fn browse_ranking(
    state: State<'_, AppState>,
    kind: String,
    mode: String,
    page: i64,
    date: Option<String>,
) -> Result<Value, String> {
    browse_ranking_impl(&state, &kind, &mode, page, date.as_deref()).await
}

pub async fn browse_ranking_impl(
    state: &AppState,
    kind: &str,
    mode: &str,
    page: i64,
    date: Option<&str>,
) -> Result<Value, String> {
    validate_ranking(kind, mode, date)?;
    validate_page(page)?;
    build_browse_api(state)?
        .get_ranking(kind, mode, page, date)
        .await
        .map_err(|err| err.to_string())
}

/// 作品详情（illust/manga/ugoira → BrowseIllustDetail；novel → BrowseNovelDetail）。
#[tauri::command]
pub async fn browse_work_detail(
    state: State<'_, AppState>,
    kind: String,
    id: i64,
) -> Result<Value, String> {
    browse_work_detail_impl(&state, &kind, id).await
}

pub async fn browse_work_detail_impl(
    state: &AppState,
    kind: &str,
    id: i64,
) -> Result<Value, String> {
    validate_work_kind(kind)?;
    validate_id(id, "作品")?;
    let api = build_browse_api(state)?;
    let detail = if kind == "novel" {
        api.get_work_detail_novel(id).await
    } else {
        api.get_work_detail_illust(id).await
    };
    detail.map_err(|err| err.to_string())
}

/// 相关推荐（一次性池，limit 默认 30，page 无效）。
#[tauri::command]
pub async fn browse_related(
    state: State<'_, AppState>,
    kind: String,
    id: i64,
    limit: Option<i64>,
) -> Result<Value, String> {
    browse_related_impl(&state, &kind, id, limit).await
}

pub async fn browse_related_impl(
    state: &AppState,
    kind: &str,
    id: i64,
    limit: Option<i64>,
) -> Result<Value, String> {
    validate_work_kind(kind)?;
    validate_id(id, "作品")?;
    validate_limit(limit)?;
    build_browse_api(state)?
        .get_related(kind, id, limit)
        .await
        .map_err(|err| err.to_string())
}

/// 作者信息。
#[tauri::command]
pub async fn browse_user_profile(state: State<'_, AppState>, id: i64) -> Result<Value, String> {
    browse_user_profile_impl(&state, id).await
}

#[tauri::command]
pub async fn browse_user_follow(
    state: State<'_, AppState>,
    id: i64,
    followed: bool,
) -> Result<Value, String> {
    browse_user_follow_impl(&state, id, followed).await
}

pub async fn browse_user_follow_impl(
    state: &AppState,
    id: i64,
    followed: bool,
) -> Result<Value, String> {
    validate_id(id, "用户")?;
    build_browse_api(state)?
        .set_user_follow(id, followed, 0)
        .await
        .map_err(|err| err.to_string())
}

pub async fn browse_user_profile_impl(state: &AppState, id: i64) -> Result<Value, String> {
    validate_id(id, "用户")?;
    build_browse_api(state)?
        .get_user_profile(id)
        .await
        .map_err(|err| err.to_string())
}

/// 作者作品（后端按 id 全集降序切片 60/批）。
#[tauri::command]
pub async fn browse_user_works(
    state: State<'_, AppState>,
    id: i64,
    kind: String,
    page: i64,
) -> Result<Value, String> {
    browse_user_works_impl(&state, id, &kind, page).await
}

pub async fn browse_user_works_impl(
    state: &AppState,
    id: i64,
    kind: &str,
    page: i64,
) -> Result<Value, String> {
    validate_id(id, "用户")?;
    validate_user_works_kind(kind)?;
    validate_page(page)?;
    build_browse_api(state)?
        .get_user_works(id, kind, page)
        .await
        .map_err(|err| err.to_string())
}

/// 小说系列目录（游标 last_order 分页，每批 30）。
#[tauri::command]
pub async fn browse_novel_series(
    state: State<'_, AppState>,
    id: i64,
    last_order: Option<i64>,
) -> Result<Value, String> {
    browse_novel_series_impl(&state, id, last_order).await
}

pub async fn browse_novel_series_impl(
    state: &AppState,
    id: i64,
    last_order: Option<i64>,
) -> Result<Value, String> {
    validate_id(id, "系列")?;
    if last_order.is_some_and(|o| o < 0) {
        return Err("last_order 不能为负数".to_string());
    }
    build_browse_api(state)?
        .get_novel_series(id, last_order)
        .await
        .map_err(|err| err.to_string())
}

/// 插画/漫画系列目录（页码制分页，每页恒 12 条，恒话数降序）。
#[tauri::command]
pub async fn browse_illust_series(
    state: State<'_, AppState>,
    id: i64,
    page: i64,
) -> Result<Value, String> {
    browse_illust_series_impl(&state, id, page).await
}

pub async fn browse_illust_series_impl(state: &AppState, id: i64, page: i64) -> Result<Value, String> {
    validate_id(id, "系列")?;
    validate_page(page)?;
    build_browse_api(state)?
        .get_illust_series(id, page)
        .await
        .map_err(|err| err.to_string())
}

/// 作品评论根列表（offset 游标分页，每页 10，无 total）。
#[tauri::command]
pub async fn browse_work_comments(
    state: State<'_, AppState>,
    kind: String,
    id: i64,
    offset: i64,
) -> Result<Value, String> {
    browse_work_comments_impl(&state, &kind, id, offset).await
}

pub async fn browse_work_comments_impl(
    state: &AppState,
    kind: &str,
    id: i64,
    offset: i64,
) -> Result<Value, String> {
    validate_comment_kind(kind)?;
    validate_id(id, "作品")?;
    if offset < 0 {
        return Err("offset 不能为负数".to_string());
    }
    build_browse_api(state)?
        .get_work_comments(kind, id, offset)
        .await
        .map_err(|err| err.to_string())
}

/// 评论回复列表（page 从 1 起，无 limit，同官方 web）。
#[tauri::command]
pub async fn browse_comment_replies(
    state: State<'_, AppState>,
    kind: String,
    comment_id: String,
    page: i64,
) -> Result<Value, String> {
    browse_comment_replies_impl(&state, &kind, &comment_id, page).await
}

pub async fn browse_comment_replies_impl(
    state: &AppState,
    kind: &str,
    comment_id: &str,
    page: i64,
) -> Result<Value, String> {
    validate_comment_kind(kind)?;
    if comment_id.trim().is_empty() {
        return Err("评论 ID 不能为空".to_string());
    }
    validate_page(page)?;
    build_browse_api(state)?
        .get_comment_replies(kind, comment_id, page)
        .await
        .map_err(|err| err.to_string())
}

// ----------------------------------------------------------------------
// 收藏命令（契约 v3.1；端点细节 docs/research/pixiv-browse-api.md §11）
// ----------------------------------------------------------------------

/// 收藏列表（插画/漫画混排或小说；tag 筛选 + 公开/私密筛选 + offset 分页，
/// limit 缺省官方每页条数 illust 48 / novel 30）。自 uid 由后端探测，前端无需传。
#[tauri::command]
pub async fn browse_bookmark_list(
    state: State<'_, AppState>,
    kind: String,
    rest: String,
    tag: Option<String>,
    offset: i64,
    limit: Option<i64>,
) -> Result<Value, String> {
    browse_bookmark_list_impl(&state, &kind, &rest, tag.as_deref(), offset, limit).await
}

pub async fn browse_bookmark_list_impl(
    state: &AppState,
    kind: &str,
    rest: &str,
    tag: Option<&str>,
    offset: i64,
    limit: Option<i64>,
) -> Result<Value, String> {
    validate_bookmark_kind(kind)?;
    validate_bookmark_rest(rest)?;
    if offset < 0 {
        return Err("offset 不能为负数".to_string());
    }
    validate_bookmark_limit(limit)?;
    build_browse_api(state)?
        .bookmark_list(kind, rest, tag, offset, limit)
        .await
        .map_err(|err| err.to_string())
}

/// 收藏标签（一次返回 public/private 两组 {name,count}；空名 = 未分类，
/// 前端 i18n）。
#[tauri::command]
pub async fn browse_bookmark_tags(
    state: State<'_, AppState>,
    kind: String,
) -> Result<Value, String> {
    browse_bookmark_tags_impl(&state, &kind).await
}

pub async fn browse_bookmark_tags_impl(state: &AppState, kind: &str) -> Result<Value, String> {
    validate_bookmark_kind(kind)?;
    build_browse_api(state)?
        .bookmark_tags(kind)
        .await
        .map_err(|err| err.to_string())
}

/// 添加收藏（restrict: 0=公开 / 1=非公开；tags 为收藏标签，可空）。
/// 返回 `{ bookmarkId }`，取消收藏时原样传回。
#[tauri::command]
pub async fn browse_bookmark_add(
    state: State<'_, AppState>,
    kind: String,
    id: i64,
    restrict: i64,
    tags: Option<Vec<String>>,
) -> Result<Value, String> {
    browse_bookmark_add_impl(&state, &kind, id, restrict, tags.as_deref().unwrap_or(&[])).await
}

pub async fn browse_bookmark_add_impl(
    state: &AppState,
    kind: &str,
    id: i64,
    restrict: i64,
    tags: &[String],
) -> Result<Value, String> {
    validate_bookmark_kind(kind)?;
    validate_id(id, "作品")?;
    validate_bookmark_restrict(restrict)?;
    build_browse_api(state)?
        .bookmark_add(kind, id, restrict, tags)
        .await
        .map_err(|err| err.to_string())
}

/// 取消收藏（bookmarkId 来自列表项 bookmarkData 或 add 返回值；小说走旧式
/// 表单端点，302 跳转即成功）。id 仅作契约参数保留（端点按 bookmarkId 定位）。
#[tauri::command]
pub async fn browse_bookmark_remove(
    state: State<'_, AppState>,
    kind: String,
    id: i64,
    bookmark_id: String,
) -> Result<Value, String> {
    browse_bookmark_remove_impl(&state, &kind, id, &bookmark_id).await
}

pub async fn browse_bookmark_remove_impl(
    state: &AppState,
    kind: &str,
    id: i64,
    bookmark_id: &str,
) -> Result<Value, String> {
    validate_bookmark_kind(kind)?;
    validate_id(id, "作品")?;
    if bookmark_id.trim().is_empty() {
        return Err("收藏 ID 不能为空".to_string());
    }
    build_browse_api(state)?
        .bookmark_remove(kind, id, bookmark_id)
        .await
        .map_err(|err| err.to_string())
}
