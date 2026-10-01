//! 只读端点在线实测（真实登录态 + 真实 pixiv）。
//!
//! 全部用例 `#[ignore]`，只由 `./dev.ps1 test-live`
//! （`cargo test --locked --test pixiv_api -- --ignored --test-threads=1`）触发；
//! 串行 + 共用 `common::live_api()` 的单客户端（并发 2 / 间隔 400ms / 重试 3 /
//! 429 暂停 60s），不会刷接口。
//!
//! 样例 id 一律在用例内动态取样（日榜 / 搜索首条），不硬编码可能失效的旧 id；
//! 断言验证真实形状与语义（字段名、类型、分页游标、封面落点），不只断言 `is_ok()`。
//! 断言消息不打印 cookie / csrf token 值。
//!
//! 已知真实偏差（2026-10-01 晚实测，结论见最终报告；用例保持失败以标记问题）：
//! - `search_path` 把非 ASCII 搜索词原样拼进 URL → pixiv 返回 HTTP 400
//!   （手工 percent-encode 后 200）；
//! - `/ajax/top/*` 的 `page.ranking.items[]` 是 `{id, rank}` 对象，`parse_channel`
//!   按标量 id 解析 → 频道页 ranking 板块恒空（id 实际都在 thumbnails 索引表内）；
//! - `/ajax/top/illust` 的 `page.trendingTags[]` 只有 `{ids, tag, trendingRate}`，
//!   没有 docs §2 记载的 `illustCount` → `TrendingTag.count` 恒为 None；
//! - `/ajax/ranking/novel` 的 `date` 是日文展示格式（"2026年9月30日"），与
//!   ranking.php 的 yyyymmdd 不同；`/ajax/top/novel` 的 `ranking_date` 为
//!   "2026-09-30"（illust/manga 为 "20260930"）；
//! - `/ajax/user/{id}?full=1` 响应没有 `account` 键 → `pixiv_id` 恒为空。

use serde_json::Value;

use crate::common;

/// 榜单首条 (作品 id, 作者 id)；榜单为空直接失败（端点可用性已由 HTTP 200 保证）。
async fn ranking_first(kind: &str) -> (i64, i64) {
    let body = common::live_api()
        .get_ranking(kind, "daily", 1, None)
        .await
        .unwrap_or_else(|e| panic!("{kind} 日榜请求失败: {e}"));
    let items = common::assert_list_envelope(&body, &format!("{kind} 日榜"));
    let first = items
        .first()
        .unwrap_or_else(|| panic!("{kind} 日榜 items 不应为空"));
    (common::id_of(first), common::author_of(first))
}

/// 找一个带系列的小说样本 (novel_id, series_id)：先扫小说日榜前 50 条，
/// 再兜底扫小说搜索结果；两处都没有则失败（系列端点无从取样）。
async fn novel_with_series() -> (i64, i64) {
    let api = common::live_api();
    let ranking = api
        .get_ranking("novel", "daily", 1, None)
        .await
        .expect("小说日榜请求失败");
    for item in common::assert_list_envelope(&ranking, "小说日榜") {
        if let Some(series_id) = common::as_i64_loose(&item["series_id"]) {
            return (common::id_of(item), series_id);
        }
    }
    let search = api
        .get_search("novel", "original", None, None, None, None, 1)
        .await
        .expect("小说搜索请求失败（系列样本兜底）");
    for item in common::assert_list_envelope(&search, "小说搜索") {
        if let Some(series_id) = common::as_i64_loose(&item["series_id"]) {
            return (common::id_of(item), series_id);
        }
    }
    panic!("小说日榜前 50 条与搜索首屏均无带系列的作品，无法取样系列端点");
}

// ----------------------------------------------------------------------
// 1. 登录态 / csrf
// ----------------------------------------------------------------------

/// GET /ajax/user/self：userData.id > 0，csrf token 非空。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_self_and_csrf() {
    let (user_data, token) = common::live_api()
        .get_user_self()
        .await
        .expect("GET /ajax/user/self 应成功（登录态有效）");
    let uid = common::as_i64_loose(&user_data["id"]).unwrap_or(0);
    assert!(uid > 0, "userData.id 应为正整数，实际解析值 {uid}");
    assert!(!token.is_empty(), "csrf token 不应为空（值不打印）");
    assert!(
        user_data
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty()),
        "userData.name 不应为空"
    );
    assert_eq!(
        common::live_uid().await,
        uid,
        "live_uid 应与 self 的 userData.id 一致"
    );
}

// ----------------------------------------------------------------------
// 2. 首页 street（POST + csrf token）
// ----------------------------------------------------------------------

/// POST /ajax/street/v2/main：items 非空，首条 id/title/cover 合理。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_home_street() {
    let body = common::live_api()
        .get_home_street()
        .await
        .expect("POST /ajax/street/v2/main 应成功（含 __NEXT_DATA__ csrf token 获取）");
    let items = common::assert_list_envelope(&body, "street");
    assert!(!items.is_empty(), "street 推荐流 items 不应为空");
    let first = &items[0];
    common::assert_work_item(first, "street.items[0]", None);
    let cover = first["cover"]
        .as_str()
        .expect("street 首条应有 cover（pages[0].urls / url 兜底链）");
    common::assert_pixiv_url(cover, "street.items[0].cover");
    assert!(
        items.iter().all(|i| common::id_of(i) > 0),
        "street 所有条目 id 应为正整数"
    );
    let covered = items
        .iter()
        .filter(|i| i["cover"].as_str().is_some_and(|u| !u.is_empty()))
        .count();
    assert!(
        covered * 2 >= items.len(),
        "street 至少一半条目应有封面，实际 {covered}/{}",
        items.len()
    );
}

// ----------------------------------------------------------------------
// 3. 频道页
// ----------------------------------------------------------------------

/// /ajax/top/{illust,manga,novel}：四大板块可解析（含 ranking 的 `{id,rank}`
/// 对象条目）+ 热门标签形态 + 中英译名 + 榜单日期归一。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_channel_illust_manga_novel() {
    for kind in ["illust", "manga", "novel"] {
        let channel = common::live_api()
            .get_channel(kind)
            .await
            .unwrap_or_else(|e| panic!("/ajax/top/{kind} 失败: {e}"));
        let mut section_items = 0usize;
        for section in ["follow", "recommend", "ranking", "new_post"] {
            let list = channel
                .get(section)
                .unwrap_or_else(|| panic!("{kind} 频道缺少 {section} 板块"));
            let items = common::assert_list_envelope(list, &format!("{kind}.{section}"));
            section_items += items.len();
            for (idx, item) in items.iter().take(3).enumerate() {
                common::assert_work_item(item, &format!("{kind}.{section}.items[{idx}]"), None);
            }
        }
        assert!(section_items > 0, "{kind} 频道四大板块合计不应为空");

        // ranking 板块：实测原始 page.ranking.items 是 100 个 {"id","rank"} 对象，
        // 解析必须兼容对象形态（2026-10-01 修复前整块为空）；rank 一并带回。
        let ranking = common::assert_list_envelope(&channel["ranking"], &format!("{kind}.ranking"));
        assert!(
            !ranking.is_empty(),
            "{kind} 频道 ranking 板块不应为空（对象条目 {{id,rank}} 应能解析）"
        );
        assert_eq!(
            ranking[0]["rank"].as_i64(),
            Some(1),
            "{kind} 频道 ranking 首条应带回 rank=1"
        );

        let tags = channel["trending_tags"]
            .as_array()
            .unwrap_or_else(|| panic!("{kind} 频道 trending_tags 应为数组"));
        // 实测：只有 illust 频道返回 page.trendingTags；条目键 {tag, ids, trendingRate}
        // 无 translatedName —— 译名改由同响应 tagTranslation[tag].zh 映射。
        if kind == "illust" {
            assert!(!tags.is_empty(), "illust 频道热门标签不应为空");
            assert!(
                tags.iter()
                    .all(|t| t["name"].as_str().is_some_and(|s| !s.is_empty())),
                "illust 频道热门标签 name 应均非空"
            );
            let translated = tags
                .iter()
                .filter(|t| t["translated_name"].as_str().is_some_and(|s| !s.is_empty()))
                .count();
            assert!(
                translated > 0,
                "热门标签应有中文译名（取自 tagTranslation），实际 0/{}",
                tags.len()
            );
            assert!(
                tags[0].get("count").is_none(),
                "响应无 illustCount 源字段，count 恒缺席（契约如此）"
            );
        } else {
            assert!(tags.is_empty(), "{kind} 频道实测无热门标签板块");
        }

        // ranking_date：三频道不同形态（illust/manga `20260930`、novel `2026-09-30`）
        // 均已归一为 yyyymmdd，前端才能统一格式化成 YYYY-MM-DD。
        if channel["ranking_date"].is_string() {
            common::assert_yyyymmdd(&channel["ranking_date"], &format!("{kind}.ranking_date"));
        }
    }
}

// ----------------------------------------------------------------------
// 4. 追更列表
// ----------------------------------------------------------------------

/// /ajax/watch_list/{manga,novel}：max_page>=1、total 为字符串数字、items 允许为空。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_watchlist_manga_novel() {
    for kind in ["manga", "novel"] {
        let watch = common::live_api()
            .get_watchlist(kind)
            .await
            .unwrap_or_else(|e| panic!("/ajax/watch_list/{kind} 失败: {e}"));
        assert_eq!(
            watch["kind"].as_str(),
            Some(kind),
            "watchlist.kind 应回显请求 kind"
        );
        let total = watch["total"]
            .as_i64()
            .unwrap_or_else(|| panic!("{kind} 追更 total 应为整数（pixiv 侧字符串数字）"));
        assert!(total >= 0, "{kind} 追更 total 不应为负，实际 {total}");
        let max_page = watch["max_page"]
            .as_i64()
            .unwrap_or_else(|| panic!("{kind} 追更 max_page 应为整数"));
        assert!(max_page >= 1, "{kind} 追更 max_page 至少为 1");
        let items = common::assert_list_envelope(&watch, &format!("watchlist.{kind}"));
        // 后端按 max_page 聚合：只要账号有订阅（total>0），聚合结果就不应为空
        if total > 0 {
            assert!(
                !items.is_empty(),
                "{kind} 追更 total={total} 但聚合 items 为空（聚合逻辑异常）"
            );
        }
        if let Some(first) = items.first() {
            assert!(common::id_of(first) > 0, "{kind} 追更系列 id 应为正整数");
            assert_eq!(first["kind"].as_str(), Some(kind), "{kind} 追更条目 kind");
            assert!(
                first["title"].as_str().is_some_and(|s| !s.is_empty()),
                "{kind} 追更系列标题不应为空"
            );
            assert!(
                common::as_i64_loose(&first["user_id"]).is_some_and(|v| v > 0),
                "{kind} 追更系列作者 id 应为正"
            );
            assert!(
                first["total"].as_i64().is_some_and(|v| v >= 1),
                "{kind} 追更系列已发布话数应 >= 1，实际: {}",
                first["total"]
            );
            if let Some(cover) = first["cover"].as_str() {
                common::assert_pixiv_url(cover, &format!("watchlist.{kind}.items[0].cover"));
            }
        }
    }
}

// ----------------------------------------------------------------------
// 5. 发现页
// ----------------------------------------------------------------------

/// /ajax/discovery/artworks：items 非空。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_discover() {
    let body = common::live_api()
        .get_discover()
        .await
        .expect("/ajax/discovery/artworks 应成功");
    let items = common::assert_list_envelope(&body, "discover");
    assert!(!items.is_empty(), "discover items 不应为空");
    assert!(
        items.iter().all(|i| common::id_of(i) > 0),
        "discover 所有条目 id 应为正整数"
    );
    common::assert_work_item(&items[0], "discover.items[0]", None);
    let cover = items[0]["cover"]
        .as_str()
        .expect("discover 首条应有封面");
    common::assert_pixiv_url(cover, "discover.items[0].cover");
}

// ----------------------------------------------------------------------
// 6. 关注动态
// ----------------------------------------------------------------------

/// /ajax/follow_latest/{illust,novel}：分页语义（is_last_page 与 next_page）。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_follow_latest_illust_novel() {
    for kind in ["illust", "novel"] {
        let list = common::live_api()
            .get_follow_latest(kind, "all", 1)
            .await
            .unwrap_or_else(|e| panic!("/ajax/follow_latest/{kind} 失败: {e}"));
        let is_last = list["is_last_page"]
            .as_bool()
            .expect("is_last_page 应为 bool（由 page.isLastPage 解析）");
        let next = list["next_page"].as_i64();
        assert_eq!(
            is_last,
            next.is_none(),
            "{kind}: is_last_page={is_last} 与 next_page={next:?} 语义应互斥"
        );
        if let Some(page) = next {
            assert_eq!(page, 2, "第 1 页的 next_page 应为 2");
        }
        let items = common::assert_list_envelope(&list, &format!("follow_latest.{kind}"));
        if let Some(first) = items.first() {
            common::assert_work_item(first, &format!("follow_latest.{kind}.items[0]"), None);
            if kind == "novel" {
                assert_eq!(
                    first["kind"].as_str(),
                    Some("novel"),
                    "小说动态应查 thumbnails.novel"
                );
            }
        } else {
            eprintln!("注意：{kind} 关注动态为空（该账号可能未关注任何用户）");
        }
    }
}

// ----------------------------------------------------------------------
// 7. 搜索
// ----------------------------------------------------------------------

/// /ajax/search/{artworks,novels}/{word}：total>=1、lastPage>=1、items 非空。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_search_artworks_novels() {
    let api = common::live_api();

    // ① 插画（ASCII 词，路径全 ASCII，可正常往返）
    let art = api
        .get_search("illust", "original", None, None, None, None, 1)
        .await
        .expect("/ajax/search/artworks 应成功（ASCII 搜索词）");
    let total = art["total"]
        .as_i64()
        .unwrap_or_else(|| panic!("搜索 total 应为整数，实际: {}", art["total"]));
    assert!(total >= 1, "搜索 total 应 >= 1，实际 {total}");
    let is_last = art["is_last_page"]
        .as_bool()
        .expect("is_last_page 应存在（lastPage 解析）");
    assert_eq!(
        is_last,
        art["next_page"].is_null(),
        "is_last_page 与 next_page 语义应互斥"
    );
    if !is_last {
        assert_eq!(art["next_page"].as_i64(), Some(2), "p=1 的 next_page 应为 2");
    }
    let art_items = common::assert_list_envelope(&art, "search artworks");
    assert!(!art_items.is_empty(), "搜索插画 items 不应为空");
    assert!(
        art_items.iter().all(|i| common::id_of(i) > 0),
        "搜索插画所有条目 id 应为正整数"
    );
    common::assert_work_item(&art_items[0], "search artworks items[0]", None);
    let art_cover = art_items[0]["cover"]
        .as_str()
        .expect("搜索插画首条应有封面（data 项顶层 url）");
    common::assert_pixiv_url(art_cover, "search artworks items[0].cover");

    // 原始响应字段名核对（契约把 lastPage 投影成 is_last_page）
    let raw_art = api
        .client()
        .get_json("/ajax/search/artworks/original?order=date_d&mode=all&type=illust&ai_type=0&p=1&lang=zh")
        .await
        .expect("原始搜索插画响应应成功");
    let raw_total = raw_art
        .pointer("/illustManga/total")
        .and_then(common::as_i64_loose)
        .expect("illustManga.total 应为整数");
    let raw_last_page = raw_art
        .pointer("/illustManga/lastPage")
        .and_then(common::as_i64_loose)
        .expect("illustManga.lastPage 应为整数");
    assert!(raw_last_page >= 1, "illustManga.lastPage 应 >= 1");
    // 字段映射校验（illustManga.total → 契约 total）。两次请求之间 pixiv 语料会继续
    // 新增投稿（实测同一天两次调用差 1：20841521 vs 20841522），故不能做等值断言。
    common::assert_same_total(raw_total, total, "illustManga.total");
    assert_eq!(
        raw_art
            .pointer("/illustManga/data")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(art_items.len()),
        "原始 data 条数应与契约 items 一致"
    );

    // ② 小说（ASCII 词）
    let novel = api
        .get_search("novel", "original", None, None, None, None, 1)
        .await
        .expect("/ajax/search/novels 应成功（ASCII 搜索词）");
    let novel_total = novel["total"]
        .as_i64()
        .unwrap_or_else(|| panic!("小说搜索 total 应为整数，实际: {}", novel["total"]));
    assert!(novel_total >= 1, "小说搜索 total 应 >= 1，实际 {novel_total}");
    let novel_items = common::assert_list_envelope(&novel, "search novels");
    assert!(!novel_items.is_empty(), "搜索小说 items 不应为空");
    assert!(
        novel_items
            .iter()
            .all(|i| i["kind"].as_str() == Some("novel")),
        "搜索小说条目 kind 应全为 novel"
    );
    common::assert_work_item(&novel_items[0], "search novels items[0]", Some("novel"));

    let raw_novel = api
        .client()
        .get_json("/ajax/search/novels/original?order=date_d&mode=all&p=1&lang=zh")
        .await
        .expect("原始搜索小说响应应成功");
    let raw_novel_last = raw_novel
        .pointer("/novel/lastPage")
        .and_then(common::as_i64_loose)
        .expect("novel.lastPage 应为整数");
    assert!(raw_novel_last >= 1, "novel.lastPage 应 >= 1");
    // 同上：两次请求之间语料会增长，用容差比较（novel.total → 契约 total 的映射校验）
    common::assert_same_total(
        raw_novel
            .pointer("/novel/total")
            .and_then(common::as_i64_loose)
            .expect("novel.total 应为整数"),
        novel_total,
        "novel.total",
    );

    // ③ 非 ASCII 词（日文标签，真实使用场景）
    // docs §5 明确 `{word}` 应为 URL 编码后的关键词：直拼原文实测 HTTP 400，
    // percent-encode（%E3%82%AA…）后 200。search_path 已改为 percent_encode
    // （2026-10-01 修复），此处即该修复的在线回归。
    let jp = api
        .get_search("illust", "オリジナル", None, None, None, None, 1)
        .await
        .expect("日文搜索词应可用（search_path 对 word 做 percent-encode）");
    assert!(
        jp["items"].as_array().is_some_and(|a| !a.is_empty()),
        "日文搜索 items 不应为空"
    );
    assert!(
        jp["total"].as_i64().is_some_and(|t| t >= 1),
        "日文搜索 total 应 >= 1"
    );
}

// ----------------------------------------------------------------------
// 8. 排行榜
// ----------------------------------------------------------------------

/// ranking.php?format=json 与 /ajax/ranking/novel：items 非空、首条 rank==1、date 8 位。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_ranking_illust_and_novel() {
    let api = common::live_api();

    // ranking.php（插画）
    let illust = api
        .get_ranking("illust", "daily", 1, None)
        .await
        .expect("ranking.php?format=json&content=illust 应成功");
    let items = common::assert_list_envelope(&illust, "illust 日榜");
    assert!(!items.is_empty(), "插画日榜 items 不应为空");
    assert_eq!(
        items[0]["rank"].as_i64(),
        Some(1),
        "插画日榜首条 rank 应为 1"
    );
    assert_eq!(
        items[0]["kind"].as_str(),
        Some("illust"),
        "content=illust 榜条目 kind 应为 illust"
    );
    common::assert_work_item(&items[0], "ranking illust items[0]", Some("illust"));
    common::assert_yyyymmdd(&illust["date"], "ranking illust.date");
    // prev_date/next_date 为 Option：最新一期榜单 next_date 实测为 null，
    // 仅在非 null 时校验 yyyymmdd 格式。
    for (field, value) in [
        ("prev_date", &illust["prev_date"]),
        ("next_date", &illust["next_date"]),
    ] {
        if !value.is_null() {
            common::assert_yyyymmdd(value, &format!("ranking illust.{field}"));
        }
    }
    assert_eq!(
        illust["next_page"].as_i64(),
        Some(2),
        "日榜第 1 页 next_page 应为 2（响应 next 字段）"
    );

    // /ajax/ranking/novel（小说）
    let novel = api
        .get_ranking("novel", "daily", 1, None)
        .await
        .expect("/ajax/ranking/novel 应成功");
    let items = common::assert_list_envelope(&novel, "novel 日榜");
    assert!(!items.is_empty(), "小说日榜 items 不应为空");
    assert_eq!(
        items[0]["rank"].as_i64(),
        Some(1),
        "小说日榜首条 rank 应为 1"
    );
    common::assert_work_item(&items[0], "ranking novel items[0]", Some("novel"));
    // 实测 /ajax/ranking/novel 的 date 为日文展示串（`2026年9月30日`），
    // 与 ranking.php 的 yyyymmdd 不同形态；后端已统一归一（normalize_ymd），
    // 前端因此能按 YYYYMMDD 走同一套格式化，而不是原样吐出日文串。
    common::assert_yyyymmdd(&novel["date"], "ranking novel.date");
    assert!(
        items[0]["text_length"].as_i64().is_some_and(|v| v > 0),
        "小说榜条目 character_count 应 > 0，实际: {}",
        items[0]["text_length"]
    );
    assert!(
        items[0]["bookmark_count"].as_i64().is_some_and(|v| v >= 0),
        "小说榜条目 bookmark_count 应为非负数"
    );
}

// ----------------------------------------------------------------------
// 9. 详情（插画 / 漫画 / 动图）
// ----------------------------------------------------------------------

/// /ajax/illust/{id} + /pages + ugoira 的 /ugoira_meta：三种作品类型逐一验证。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_illust_detail_pages_ugoira() {
    let api = common::live_api();

    // ① 普通插画
    let (illust_id, author_id) = ranking_first("illust").await;
    let detail = api
        .get_work_detail_illust(illust_id)
        .await
        .unwrap_or_else(|e| panic!("插画详情失败（illust {illust_id}）: {e}"));
    assert_eq!(detail["detail_kind"].as_str(), Some("illust"));
    let item = &detail["item"];
    assert_eq!(
        item["id"].as_i64(),
        Some(illust_id),
        "详情 item.id 应回显请求 id"
    );
    common::assert_work_item(item, "illust detail.item", Some("illust"));
    assert_eq!(
        item["author_id"].as_i64(),
        Some(author_id),
        "详情作者应与榜单一致"
    );
    let pages = detail["pages"]
        .as_array()
        .filter(|a| !a.is_empty())
        .expect("插画详情 pages 不应为空（单页作品也返回 1 项）");
    assert_eq!(
        pages.len() as i64,
        item["page_count"].as_i64().unwrap_or(0),
        "pages 条数应等于 pageCount"
    );
    for (idx, page) in pages.iter().enumerate() {
        let original = page["original"]
            .as_str()
            .unwrap_or_else(|| panic!("pages[{idx}].original 应为字符串"));
        common::assert_pixiv_url(original, &format!("pages[{idx}].original"));
        assert!(
            page["width"].as_i64().is_some_and(|v| v > 0)
                && page["height"].as_i64().is_some_and(|v| v > 0),
            "pages[{idx}] 宽高应为正数"
        );
        if let Some(medium) = page["medium"].as_str() {
            common::assert_pixiv_url(medium, &format!("pages[{idx}].medium"));
        }
    }
    assert!(
        detail["ugoira"].is_null(),
        "非动图作品的 ugoira 元数据应为 null"
    );
    if let Some(state) = detail.get("bookmarkState").filter(|v| !v.is_null()) {
        assert!(
            state["bookmarkId"].as_str().is_some_and(|s| !s.is_empty()),
            "已收藏时 bookmarkId 应非空"
        );
        assert!(
            matches!(state["restrict"].as_i64(), Some(0..=1)),
            "bookmarkState.restrict 应为 0 或 1，实际: {}",
            state["restrict"]
        );
    }

    // ② 漫画（content=manga 榜首条）
    let (manga_id, _) = ranking_first("manga").await;
    let detail = api
        .get_work_detail_illust(manga_id)
        .await
        .unwrap_or_else(|e| panic!("漫画详情失败（illust {manga_id}）: {e}"));
    common::assert_work_item(&detail["item"], "manga detail.item", Some("manga"));
    assert!(
        detail["pages"].as_array().is_some_and(|a| !a.is_empty()),
        "漫画详情 pages 不应为空"
    );
    assert!(detail["ugoira"].is_null(), "漫画不应有 ugoira 元数据");

    // ③ 动图（content=ugoira 榜首条）
    let (ugoira_id, _) = ranking_first("ugoira").await;
    let detail = api
        .get_work_detail_illust(ugoira_id)
        .await
        .unwrap_or_else(|e| panic!("动图详情失败（illust {ugoira_id}）: {e}"));
    common::assert_work_item(&detail["item"], "ugoira detail.item", Some("ugoira"));
    let ugoira = detail["ugoira"]
        .as_object()
        .unwrap_or_else(|| panic!("动图作品应有 ugoira 元数据（ugoira_meta 解析）"));
    let src = ugoira["src"].as_str().expect("ugoira.src 应为字符串");
    assert!(
        src.starts_with("https://i.pximg.net/img-zip-ugoira/") && src.ends_with(".zip"),
        "ugoira.src 应为 img-zip-ugoira zip 直链，实际: {src}"
    );
    let frames = ugoira["frames"]
        .as_array()
        .expect("ugoira.frames 应为数组");
    assert!(!frames.is_empty(), "ugoira 帧序列不应为空");
    assert!(
        frames.iter().all(|f| f["file"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
            && f["delay"].as_i64().is_some_and(|d| d >= 0)),
        "ugoira 帧应含 file 与非负 delay"
    );
}

// ----------------------------------------------------------------------
// 10. 小说详情
// ----------------------------------------------------------------------

/// /ajax/novel/{id}：content 非空、pageCount>=1、[newpage] 数 = pageCount-1。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_novel_detail() {
    let (novel_id, author_id) = ranking_first("novel").await;
    let detail = common::live_api()
        .get_work_detail_novel(novel_id)
        .await
        .unwrap_or_else(|e| panic!("小说详情失败（novel {novel_id}）: {e}"));
    assert_eq!(detail["detail_kind"].as_str(), Some("novel"));
    let item = &detail["item"];
    assert_eq!(
        item["id"].as_i64(),
        Some(novel_id),
        "详情 item.id 应回显请求 id"
    );
    common::assert_work_item(item, "novel detail.item", Some("novel"));
    assert_eq!(
        item["author_id"].as_i64(),
        Some(author_id),
        "详情作者应与榜单一致"
    );

    let content = detail["content"]
        .as_str()
        .expect("小说详情 content 应为字符串");
    assert!(!content.is_empty(), "小说正文不应为空");
    let page_count = item["page_count"].as_i64().unwrap_or(0);
    assert!(page_count >= 1, "pageCount 应 >= 1，实际 {page_count}");
    // docs/research/pixiv-browse-api.md §7.2：多页以 [newpage] 标记，数量 = pageCount - 1
    let newpage = content.matches("[newpage]").count() as i64;
    assert_eq!(
        newpage + 1,
        page_count,
        "[newpage] 数 + 1 应等于 pageCount（§7.2）"
    );
    assert!(
        item["text_length"].as_i64().is_some_and(|v| v > 0),
        "characterCount（text_length）应 > 0，实际: {}",
        item["text_length"]
    );
    assert!(
        item["bookmark_count"].as_i64().is_some_and(|v| v >= 0),
        "bookmarkCount 应为非负数"
    );
    if let Some(series) = detail["series"].as_object() {
        assert!(
            series["id"].as_i64().is_some_and(|v| v > 0),
            "系列 id 应为正整数"
        );
        assert!(
            series["title"].as_str().is_some_and(|s| !s.is_empty()),
            "系列标题不应为空"
        );
        assert!(
            series["order"].as_i64().is_some_and(|v| v >= 1),
            "话序应 >= 1"
        );
    }
}

// ----------------------------------------------------------------------
// 11. 相关推荐
// ----------------------------------------------------------------------

/// /{illust,novel}/{id}/recommend/init：items 非空、limit 生效。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_related_illust_novel() {
    let api = common::live_api();

    let (illust_id, _) = ranking_first("illust").await;
    let related = api
        .get_related("illust", illust_id, Some(10))
        .await
        .unwrap_or_else(|e| panic!("插画相关推荐失败（illust {illust_id}）: {e}"));
    let items = common::assert_list_envelope(&related, "illust 相关推荐");
    assert!(!items.is_empty(), "插画相关推荐不应为空");
    assert!(
        items.len() <= 10,
        "limit=10 时推荐条目不应超过 10，实际 {}",
        items.len()
    );
    common::assert_work_item(&items[0], "related illust items[0]", None);

    let (novel_id, _) = ranking_first("novel").await;
    let related = api
        .get_related("novel", novel_id, Some(10))
        .await
        .unwrap_or_else(|e| panic!("小说相关推荐失败（novel {novel_id}）: {e}"));
    let items = common::assert_list_envelope(&related, "novel 相关推荐");
    assert!(!items.is_empty(), "小说相关推荐不应为空");
    assert!(
        items.iter().all(|i| i["kind"].as_str() == Some("novel")),
        "小说相关推荐条目 kind 应全为 novel"
    );
    common::assert_work_item(&items[0], "related novel items[0]", Some("novel"));
}

// ----------------------------------------------------------------------
// 12. 作者
// ----------------------------------------------------------------------

/// /ajax/user/{id}?full=1：name 非空、头像/关注数齐备。
/// `pixiv_id` 恒为空：实测（2026-10-01，他人/自己 × full=1/0 四种组合）响应
/// **没有** `account` 键——不是解析 bug，契约已按「无该字段」记录在
/// docs/PIXIV-API.md；前端在为空时隐藏 @handle 行。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_user_profile() {
    let api = common::live_api();
    let (_, author_id) = ranking_first("illust").await;
    let profile = api
        .get_user_profile(author_id)
        .await
        .unwrap_or_else(|e| panic!("作者信息失败（user {author_id}）: {e}"));
    assert_eq!(
        profile["id"].as_i64(),
        Some(author_id),
        "user.id 应回显请求 id"
    );
    assert!(
        profile["name"].as_str().is_some_and(|s| !s.is_empty()),
        "作者 name 不应为空"
    );
    let img = profile["profile_img"].as_str().unwrap_or("");
    common::assert_pixiv_url(img, "user.profile_img");
    assert!(
        profile["following_count"].as_i64().is_some(),
        "full=1 应返回 following（following_count）"
    );
    // 原始响应确实没有 account 键（防止有人误以为「改个字段名就能取到」）
    let raw = api
        .client()
        .get_json(&format!("/ajax/user/{author_id}?full=1&lang=zh"))
        .await
        .expect("原始用户响应应成功");
    assert!(
        !common::keys(&raw).iter().any(|k| *k == "account"),
        "pixiv 已新增 account 键 → 应回到契约里补 pixiv_id 映射，实测键: {:?}",
        common::keys(&raw)
    );
    assert_eq!(
        profile["pixiv_id"].as_str(),
        Some(""),
        "无 account 源字段时 pixiv_id 为空串（契约如此）"
    );
}

/// /ajax/user/{id}/profile/all + 60/批 ids[]：total>0、items 非空且都属于该作者。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_user_works_illust_novel() {
    let api = common::live_api();

    let (_, illust_author) = ranking_first("illust").await;
    let works = api
        .get_user_works(illust_author, "illust", 1)
        .await
        .unwrap_or_else(|e| panic!("作者插画作品失败（user {illust_author}）: {e}"));
    let total = works["total"].as_i64().unwrap_or(0);
    assert!(total > 0, "插画作者 total 应 > 0，实际 {total}");
    let items = common::assert_list_envelope(&works, "user works illust");
    assert!(!items.is_empty(), "作者插画首批不应为空");
    assert!(
        items.len() as i64 <= 60,
        "单批不应超过 60，实际 {}",
        items.len()
    );
    assert!(
        items
            .iter()
            .all(|i| i["author_id"].as_i64() == Some(illust_author)),
        "批量接口条目应都属于该作者"
    );
    let is_last = works["is_last_page"]
        .as_bool()
        .expect("is_last_page 应为 bool");
    assert_eq!(
        is_last,
        works["next_page"].is_null(),
        "is_last_page 与 next_page 语义应互斥"
    );
    if total > 60 {
        assert!(!is_last, "total={total}>60 时首批不应是最后一页");
        assert_eq!(works["next_page"].as_i64(), Some(2));
    } else {
        assert!(is_last, "total={total}<=60 时首批应为最后一页");
    }

    let (_, novel_author) = ranking_first("novel").await;
    let works = api
        .get_user_works(novel_author, "novel", 1)
        .await
        .unwrap_or_else(|e| panic!("作者小说作品失败（user {novel_author}）: {e}"));
    let total = works["total"].as_i64().unwrap_or(0);
    assert!(total > 0, "小说作者 total 应 > 0，实际 {total}");
    let items = common::assert_list_envelope(&works, "user works novel");
    assert!(!items.is_empty(), "作者小说首批不应为空");
    assert!(
        items.iter().all(|i| i["kind"].as_str() == Some("novel")
            && i["author_id"].as_i64() == Some(novel_author)),
        "作者小说条目应 kind=novel 且属于该作者"
    );
}

// ----------------------------------------------------------------------
// 13. 小说系列
// ----------------------------------------------------------------------

/// /ajax/novel/series/{id} + series_content：目录条数 >= 1、order_by=asc、游标语义。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_novel_series_detail_and_content() {
    let (_, series_id) = novel_with_series().await;
    let detail = common::live_api()
        .get_novel_series(series_id, None)
        .await
        .unwrap_or_else(|e| panic!("小说系列失败（series {series_id}）: {e}"));
    assert_eq!(detail["id"].as_i64(), Some(series_id));
    assert!(
        detail["title"].as_str().is_some_and(|s| !s.is_empty()),
        "系列标题不应为空"
    );
    assert!(
        detail["user_id"].as_i64().is_some_and(|v| v > 0),
        "系列作者 id 应为正整数"
    );
    assert!(
        !detail["user_name"].as_str().unwrap_or("").is_empty(),
        "系列作者名不应为空"
    );
    let total = detail["total"].as_i64().unwrap_or(0);
    assert!(total >= 1, "系列总章节数应 >= 1，实际 {total}");

    let contents = detail["contents"]
        .as_array()
        .expect("系列 contents 应为数组");
    assert!(!contents.is_empty(), "系列目录不应为空");
    assert_eq!(
        contents[0]["series_order"].as_i64(),
        Some(1),
        "order_by=asc 首批首条 contentOrder 应为 1"
    );
    assert!(
        contents.windows(2).all(|w| {
            w[0]["series_order"].as_i64().unwrap_or(0) < w[1]["series_order"].as_i64().unwrap_or(0)
        }),
        "目录应按 contentOrder 升序"
    );
    assert!(
        contents.iter().all(|c| common::as_i64_loose(&c["id"]).is_some_and(|v| v > 0)
            && c["title"].as_str().is_some_and(|s| !s.is_empty())
            && c["series_order"].as_i64().is_some_and(|v| v >= 1)),
        "目录条目应含正整数 id、非空 title、>=1 的 series_order"
    );

    let last_order = contents.last().and_then(|c| c["series_order"].as_i64());
    match detail["next_last_order"].as_i64() {
        Some(next) => {
            assert_eq!(
                Some(next),
                last_order,
                "next_last_order 应等于末条 contentOrder"
            );
            assert!(
                total > contents.len() as i64,
                "还有下一页时 total({total}) 应大于本批条数({})",
                contents.len()
            );
            assert_eq!(
                contents.len(),
                30,
                "未取完 total 时应为满页 30 条（limit=30）"
            );
        }
        None => {
            assert!(
                total <= contents.len() as i64 || contents.len() < 30,
                "next_last_order=null 应已到底（覆盖 total 或不足一批）"
            );
        }
    }
    if let Some(cover) = detail["cover"].as_str() {
        common::assert_pixiv_url(cover, "novel series.cover");
    }
}

// ----------------------------------------------------------------------
// 14. 评论
// ----------------------------------------------------------------------

/// /ajax/{illusts,novels}/comments/roots（+ replies 视数据）：字段与游标语义。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_comments_roots_and_replies() {
    let api = common::live_api();

    // 插画：日榜前 10 件里找有根评论的作品
    let ranking = api
        .get_ranking("illust", "daily", 1, None)
        .await
        .expect("插画日榜请求失败（评论取样）");
    let candidates: Vec<i64> = common::assert_list_envelope(&ranking, "插画日榜")
        .iter()
        .take(10)
        .map(common::id_of)
        .collect();
    let mut thread: Option<(i64, Value)> = None;
    for id in &candidates {
        let body = api
            .get_work_comments("illust", *id, 0)
            .await
            .unwrap_or_else(|e| panic!("/ajax/illusts/comments/roots 失败（illust {id}）: {e}"));
        let comments = body["comments"]
            .as_array()
            .unwrap_or_else(|| panic!("illust {id} 评论响应缺少 comments 数组"));
        assert!(
            body["next"].is_null()
                || body["next"]
                    .as_i64()
                    .is_some_and(|n| n >= comments.len() as i64),
            "illust {id} 评论 next 游标语义异常: {:?}",
            body["next"]
        );
        if !comments.is_empty() {
            thread = Some((*id, body));
            break;
        }
    }
    let (work_id, body) = thread.expect("日榜前 10 件插画均无根评论，无法取样评论内容");
    let comments = body["comments"].as_array().unwrap();
    assert!(
        comments.iter().all(|c| {
            c["id"].as_str().is_some_and(|s| !s.is_empty())
                && common::as_i64_loose(&c["user_id"]).is_some_and(|v| v > 0)
                && c["user_name"].as_str().is_some_and(|s| !s.is_empty())
                && c["date"].as_str().is_some_and(|s| !s.is_empty())
                && (c["content"].as_str().is_some_and(|s| !s.is_empty())
                    || c["stamp_url"].as_str().is_some_and(|s| !s.is_empty()))
        }),
        "插画 {work_id} 根评论字段不完整（id/user_id/user_name/date/正文或表情）"
    );
    if let Some(next) = body["next"].as_i64() {
        assert_eq!(
            next,
            comments.len() as i64,
            "offset=0 且 hasNext 时 next 应等于本页条数"
        );
    }
    if let Some(root) = comments
        .iter()
        .find(|c| c["has_replies"].as_bool() == Some(true))
    {
        let comment_id = root["id"].as_str().expect("根评论 id 应为字符串");
        let replies_body = api
            .get_comment_replies("illust", comment_id, 1)
            .await
            .unwrap_or_else(|e| {
                panic!("/ajax/illusts/comments/replies 失败（comment {comment_id}）: {e}")
            });
        let replies = replies_body["comments"]
            .as_array()
            .expect("replies 响应缺少 comments 数组");
        assert!(
            !replies.is_empty(),
            "hasReplies=true 的根评论（{comment_id}）replies 不应为空"
        );
        assert!(
            replies
                .iter()
                .all(|c| c["id"].as_str().is_some_and(|s| !s.is_empty())
                    && c["user_name"].as_str().is_some_and(|s| !s.is_empty())),
            "回复条目应含 id 与 user_name"
        );
        assert!(
            replies
                .iter()
                .any(|c| c["reply_to_user_name"].as_str().is_some_and(|s| !s.is_empty())),
            "replies 条目应带 replyToUserName"
        );
        if let Some(next) = replies_body["next"].as_i64() {
            assert_eq!(next, 2, "replies page=1 且 hasNext 时 next 应为 2");
        }
    } else {
        eprintln!("注意：插画 {work_id} 的根评论均无回复（hasReplies 无 true），跳过 replies 内容校验");
    }

    // 小说：日榜前 5 件里找有根评论的作品
    let ranking = api
        .get_ranking("novel", "daily", 1, None)
        .await
        .expect("小说日榜请求失败（评论取样）");
    let candidates: Vec<i64> = common::assert_list_envelope(&ranking, "小说日榜")
        .iter()
        .take(5)
        .map(common::id_of)
        .collect();
    let mut novel_thread: Option<Value> = None;
    for id in &candidates {
        let body = api
            .get_work_comments("novel", *id, 0)
            .await
            .unwrap_or_else(|e| panic!("/ajax/novels/comments/roots 失败（novel {id}）: {e}"));
        let comments = body["comments"]
            .as_array()
            .unwrap_or_else(|| panic!("novel {id} 评论响应缺少 comments 数组"));
        if !comments.is_empty() {
            novel_thread = Some(body);
            break;
        }
    }
    if let Some(body) = novel_thread {
        let comments = body["comments"].as_array().unwrap();
        assert!(
            comments.iter().all(|c| c["id"].as_str().is_some_and(|s| !s.is_empty())
                && c["user_name"].as_str().is_some_and(|s| !s.is_empty())),
            "小说根评论应含 id 与 user_name"
        );
    } else {
        eprintln!("注意：小说日榜前 5 件均无根评论（数据依赖），仅验证了端点形状");
    }
}

// ----------------------------------------------------------------------
// 15. 收藏（只读）
// ----------------------------------------------------------------------

/// /ajax/{illusts,novels}/bookmarks + bookmark/tags：用自己的登录 uid。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_bookmark_list_and_tags() {
    let api = common::live_api();
    let uid = common::live_uid().await;

    for kind in ["illust", "novel"] {
        let list = api
            .bookmark_list(kind, "show", None, 0, None)
            .await
            .unwrap_or_else(|e| panic!("收藏列表失败（{kind}，uid {uid}）: {e}"));
        let items = common::assert_list_envelope(&list, &format!("bookmark list {kind}"));
        let total = list["total"]
            .as_i64()
            .unwrap_or_else(|| panic!("{kind} 收藏列表 total 应存在（§11.1）"));
        assert!(total >= 0, "{kind} 收藏 total 不应为负");
        assert!(
            total >= items.len() as i64,
            "{kind} 收藏 total({total}) 应不小于本页条数({})",
            items.len()
        );
        if !items.is_empty() {
            assert!(
                items.iter().all(|i| i["bookmarkId"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())),
                "{kind} 收藏列表项应带 bookmarkId（当前查看者收藏态）"
            );
            assert!(
                items
                    .iter()
                    .all(|i| i["bookmarkRestrict"].as_i64() == Some(0)),
                "{kind} rest=show 应只返回公开收藏（bookmarkRestrict=0）"
            );
            common::assert_work_item(&items[0], &format!("bookmark list {kind}.items[0]"), None);
        }
        // next 游标：未满页必须 null；满页时为 offset+len（§11.1）
        let limit = if kind == "novel" { 30 } else { 48 };
        match list["next"].as_i64() {
            Some(next) => assert_eq!(
                next,
                items.len() as i64,
                "{kind} offset=0 满页时 next 应等于本页条数"
            ),
            None => assert!(
                (items.len() as i64) < limit,
                "{kind} 未满页（{}/{limit}）时 next 应为 null",
                items.len()
            ),
        }
        // 非公开列表（rest=hide）形状可用
        let hidden = api
            .bookmark_list(kind, "hide", None, 0, Some(10))
            .await
            .unwrap_or_else(|e| panic!("非公开收藏列表失败（{kind}）: {e}"));
        assert!(
            hidden["items"].as_array().is_some(),
            "{kind} rest=hide 响应应含 items 数组"
        );
        if let Some(hidden_items) = hidden["items"].as_array().filter(|a| !a.is_empty()) {
            assert!(
                hidden_items
                    .iter()
                    .all(|i| i["bookmarkRestrict"].as_i64() == Some(1)),
                "{kind} rest=hide 应只返回非公开收藏（bookmarkRestrict=1）"
            );
        }
        // 标签：一次返回 public/private 两组
        let tags = api
            .bookmark_tags(kind)
            .await
            .unwrap_or_else(|e| panic!("收藏标签失败（{kind}）: {e}"));
        for group in ["public", "private"] {
            let arr = tags[group]
                .as_array()
                .unwrap_or_else(|| panic!("{kind} 收藏标签缺少 {group} 组"));
            assert!(
                arr.iter().all(|t| t["name"].as_str().is_some()
                    && common::as_i64_loose(&t["count"]).is_some_and(|c| c >= 0)),
                "{kind} {group} 标签应含 name 与非负 count"
            );
        }
        // offset 超界：空 items、total 照常返回（§11.1 实测 1000 超界安全）。
        // 本机实测 offset=1e6 → HTTP 400，而 100000 仍 200，故探测值取
        // total+1000 且不超过 100000（1e6 被服务端拒绝，见最终报告）。
        let probe_offset = total + 1000;
        if probe_offset <= 100_000 {
            let overflow = api
                .bookmark_list(kind, "show", None, probe_offset, Some(10))
                .await
                .unwrap_or_else(|e| {
                    panic!("{kind} offset 超界请求应成功（offset={probe_offset}）: {e}")
                });
            assert!(
                overflow["items"].as_array().is_some_and(|a| a.is_empty()),
                "{kind} offset={probe_offset} 超界应返回空 items"
            );
            assert!(
                overflow["total"].as_i64().is_some(),
                "{kind} offset 超界仍应返回 total"
            );
        } else {
            eprintln!("注意：{kind} 收藏 total={total} 过大，跳过 offset 超界探测");
        }
    }
}

// ----------------------------------------------------------------------
// 16. 图片下载（Referer 防盗链）
// ----------------------------------------------------------------------

/// `download_bytes` 拉一张封面：非空且首字节为 JPEG/PNG/GIF 魔数。
#[tokio::test]
#[ignore = "需要真实登录态与网络：./dev.ps1 test-live"]
async fn live_image_download_with_referer() {
    let api = common::live_api();
    let (illust_id, _) = ranking_first("illust").await;
    let detail = api
        .get_work_detail_illust(illust_id)
        .await
        .unwrap_or_else(|e| panic!("插画详情失败（illust {illust_id}）: {e}"));
    let pages = detail["pages"]
        .as_array()
        .filter(|a| !a.is_empty())
        .expect("插画详情 pages 不应为空");
    let page = &pages[0];
    // 取最小档位，避免下载原图
    let url = page["small"]
        .as_str()
        .or_else(|| page["medium"].as_str())
        .unwrap_or_else(|| {
            page["original"]
                .as_str()
                .expect("original 应为字符串")
        });
    common::assert_pixiv_url(url, "下载样本 URL");
    let bytes = api
        .client()
        .download_bytes(url)
        .await
        .unwrap_or_else(|e| panic!("download_bytes 失败（Referer 伪装可能失效）: {e}"));
    assert!(bytes.len() > 100, "图片字节数过小: {}", bytes.len());
    let magic_ok = bytes.starts_with(&[0xFF, 0xD8])
        || bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47])
        || bytes.starts_with(b"GIF8");
    assert!(
        magic_ok,
        "首字节不是 JPEG/PNG/GIF 魔数: {:02X?}",
        &bytes[..bytes.len().min(4)]
    );
}
