//! Pixiv Ajax API 的 typed 封装（serde struct + 从 JSON 解析）。
//!
//! 解析路径全部实测对齐 Python 版（2026-07-21 / 2026-08-02 chrome-devtools）：
//! - 小说 series 信息在 `seriesNavData`（不是顶层 seriesId/seriesTitle）；
//! - 系列列表走 `/ajax/novel/series_content/{id}`（`body.page.seriesContents[]`）；
//! - profile/all 的 novels/illusts/manga 是 `{id: null}` 对象，novelSeries 是数组；
//! - /ajax/user/self 响应是**扁平结构**（顶层 `userData` + `token`，没有
//!   error/body 信封），get_json 的 body 兜底会原样返回整个对象。
//!
//! "JSON → 结构体"抽成纯函数（parse_*）便于离线单测。缺字段容错策略：
//! 能兜底就兜底（空串 / 0 / None），不能兜底返回 `PixivError::Client`。

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use super::client::{PixivClient, PixivError};

/// 宽松 i64：pixiv 的 id 字段多数是字符串（"userId":"12345"），少数是数字。
pub(crate) fn as_i64_loose(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub(crate) fn as_str_or(value: &Value, default: &str) -> String {
    value.as_str().unwrap_or(default).to_string()
}

/// 单篇小说完整数据（/ajax/novel/{id}）。
#[derive(Debug, Clone, Serialize)]
pub struct NovelData {
    pub novel_id: i64,
    pub title: String,
    pub user_id: i64,
    pub user_name: String,
    pub page_count: i64,
    /// updateDate 原样字符串。
    pub update_date: String,
    /// 完整正文。
    pub content: String,
    /// series 信息在 seriesNavData（实测 2026-07-21），不是顶层 seriesId/seriesTitle。
    pub series_id: Option<i64>,
    pub series_title: Option<String>,
    /// 封面 URL（coverUrl；缺失为空串）。
    pub cover_url: String,
    /// 正文内嵌图索引：`[uploadedimage:id]` 的 id → 图片 URL
    /// （textEmbeddedImages，1200x1200 档优先，缺失按 original→480mw→240mw→128x128 回退）。
    pub text_embedded_images: HashMap<String, String>,
}

/// 用户主页全部作品 id（/ajax/user/{id}/profile/all）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProfileAll {
    /// 全部小说 id（body.novels 是 id→null 映射）。
    pub novels: Vec<i64>,
    /// (series_id, series_title)（body.novelSeries 是 [{id,title}] 数组）。
    pub novel_series: Vec<(i64, String)>,
    /// 插画 id（含 ugoira）。
    pub illusts: Vec<i64>,
    /// 漫画 id。
    pub manga: Vec<i64>,
}

/// 单幅插画元数据（/ajax/illust/{id}）。
#[derive(Debug, Clone, Serialize)]
pub struct IllustData {
    /// body.urls.original（p0 原图直链）。
    pub urls_original_p0: String,
    /// body.meta.pages[].image_urls.original（多页各页原图；匿名访问为空，
    /// 缺失时从 p0 直链按 _p0 → _pN 推导兜底）。可能含空串条目（调用方跳过）。
    pub meta_pages_original: Vec<String>,
    /// 0 插画 / 1 漫画 / 2 ugoira。
    pub illust_type: i64,
    pub page_count: i64,
    pub user_id: i64,
    pub user_name: String,
    pub title: String,
}

/// ugoira 动图元数据（/ajax/illust/{id}/ugoira_meta，需登录态）。
#[derive(Debug, Clone, Serialize)]
pub struct UgoiraMeta {
    /// 原始尺寸 zip 直链：body.originalSrc 优先，body.zip_urls.original 兜底。
    pub zip_url: String,
}

/// `{id: null}` 对象形映射 → id 列表（非法键跳过，对齐 Python try/int() 跳过）。
fn id_map_keys(value: &Value) -> Vec<i64> {
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    map.keys().filter_map(|k| k.parse().ok()).collect()
}

/// 图片 URL 档位偏好：1200x1200 足够正文宽度且明显小于 original；
/// 缺失时按 original → 480mw → 240mw → 128x128 回退（与 browse_api.rs
/// `parse_novel_embedded_images` 同口径）。
pub(crate) const IMAGE_URL_TIERS: [&str; 5] = ["1200x1200", "original", "480mw", "240mw", "128x128"];

/// 按 [`IMAGE_URL_TIERS`] 偏好从 `urls` 对象取第一个非空档位。
fn pick_url_tier(urls: &Value) -> Option<String> {
    IMAGE_URL_TIERS
        .iter()
        .find_map(|tier| urls.get(*tier).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// `/ajax/novel/{id}` 的 `textEmbeddedImages` → `图片 id → URL`（纯函数）。
/// 键即正文 `[uploadedimage:id]` 的 id；条目无 `urls` 或五档全空则跳过。
pub(crate) fn parse_novel_embedded_images(body: &Value) -> HashMap<String, String> {
    let Some(table) = body.get("textEmbeddedImages").and_then(Value::as_object) else {
        return HashMap::new();
    };
    table
        .iter()
        .filter_map(|(id, entry)| {
            let url = entry.get("urls").and_then(pick_url_tier)?;
            Some((id.clone(), url))
        })
        .collect()
}

/// `/ajax/novel/{id}` body → NovelData（纯函数，离线可测）。
pub(crate) fn parse_novel_data(novel_id: i64, body: &Value) -> NovelData {
    NovelData {
        novel_id,
        title: body
            .get("title")
            .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
            .map(|v| v.as_str().unwrap_or_default().to_string())
            .unwrap_or_else(|| novel_id.to_string()),
        user_id: body.get("userId").and_then(as_i64_loose).unwrap_or(0),
        user_name: body
            .get("userName")
            .map(|v| as_str_or(v, ""))
            .unwrap_or_default(),
        page_count: body.get("pageCount").and_then(as_i64_loose).unwrap_or(1),
        update_date: body
            .get("updateDate")
            .map(|v| as_str_or(v, ""))
            .unwrap_or_default(),
        content: body
            .get("content")
            .map(|v| as_str_or(v, ""))
            .unwrap_or_default(),
        series_id: body
            .pointer("/seriesNavData/seriesId")
            .and_then(as_i64_loose),
        series_title: body
            .pointer("/seriesNavData/title")
            .and_then(|v| v.as_str().map(String::from)),
        cover_url: body
            .get("coverUrl")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        text_embedded_images: parse_novel_embedded_images(body),
    }
}

/// `/ajax/novel/series_content/{id}` body → Vec<(novel_id, contentOrder)>（纯函数）。
///
/// `body.page.seriesContents[]`：`{"id":"9000012","series":{"contentOrder":1}}`。
/// id 字符串转 i64（非法跳过）；contentOrder 从 1 开始，缺失时用遍历顺序
/// （1-based）兜底——签名固定为 (i64, i64)，无法表达 None。
pub(crate) fn parse_series_content(body: &Value) -> Vec<(i64, i64)> {
    let contents = body
        .pointer("/page/seriesContents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    contents
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            let nid = item.get("id").and_then(as_i64_loose)?;
            let order = item
                .pointer("/series/contentOrder")
                .and_then(as_i64_loose)
                .unwrap_or((idx + 1) as i64);
            Some((nid, order))
        })
        .collect()
}

/// 小说系列目录整包聚合结果（`get_series_content_all` 产出）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct SeriesCatalog {
    /// 系列标题（meta.title；缺失为空串）。
    pub series_title: String,
    /// 作者名（meta.userName）。
    pub author_name: String,
    /// 作者 userId（meta.userId）。
    pub user_id: i64,
    /// 系列封面 URL（meta.cover，兼容字符串与 `{urls:{...}}` 对象两形态）。
    pub cover_url: String,
    /// 全部话目录 `(novel_id, contentOrder)`，按 contentOrder 升序、容忍跳号。
    pub items: Vec<(i64, i64)>,
}

/// 系列封面 URL：`cover` 兼容字符串与 `{urls:{...}}` 对象两形态（纯函数）。
/// 对象形态按 [`IMAGE_URL_TIERS`] 取档（1200x1200 优先）；两形态皆取不到 → 空串。
pub(crate) fn parse_series_cover(meta: &Value) -> String {
    let cover = match meta.get("cover") {
        Some(Value::String(s)) => return s.clone(),
        Some(v) => v,
        None => return String::new(),
    };
    cover.get("urls").and_then(pick_url_tier).unwrap_or_default()
}

/// 目录游标批次合并（纯函数）：跨批 `novel_id` 去重（保首现），按 contentOrder
/// 稳定升序排序，容忍跳号；跳号不做补齐（产物按实际 order 渲染）。
pub(crate) fn merge_series_batches(batches: &[Vec<(i64, i64)>]) -> Vec<(i64, i64)> {
    let mut seen = std::collections::HashSet::new();
    let mut items: Vec<(i64, i64)> = Vec::new();
    for batch in batches {
        for (nid, order) in batch {
            if seen.insert(*nid) {
                items.push((*nid, *order));
            }
        }
    }
    items.sort_by_key(|(_, order)| *order);
    items
}

/// `/ajax/user/{id}/profile/all` body → ProfileAll（纯函数）。
/// novels/illusts/manga 是对象形（键转 i64，非法键跳过）；novelSeries 是数组形。
pub(crate) fn parse_profile_all(body: &Value) -> ProfileAll {
    let novel_series = body
        .get("novelSeries")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    let sid = s.get("id").and_then(as_i64_loose)?;
                    let title = s.get("title").map(|v| as_str_or(v, "")).unwrap_or_default();
                    Some((sid, title))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    ProfileAll {
        novels: body.get("novels").map(id_map_keys).unwrap_or_default(),
        novel_series,
        illusts: body.get("illusts").map(id_map_keys).unwrap_or_default(),
        manga: body.get("manga").map(id_map_keys).unwrap_or_default(),
    }
}

/// `/ajax/illust/{id}` body → IllustData（纯函数）。
/// 多页键：`meta.pages[].image_urls.original` 优先，条目级兜底 `original`。
pub(crate) fn parse_illust_data(body: &Value) -> IllustData {
    let meta_pages_original = body
        .pointer("/meta/pages")
        .and_then(Value::as_array)
        .map(|pages| {
            pages
                .iter()
                .map(|p| {
                    p.pointer("/image_urls/original")
                        .and_then(Value::as_str)
                        .or_else(|| p.get("original").and_then(Value::as_str))
                        .unwrap_or("")
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default();
    IllustData {
        urls_original_p0: body
            .pointer("/urls/original")
            .map(|v| as_str_or(v, ""))
            .unwrap_or_default(),
        meta_pages_original,
        illust_type: body.get("illustType").and_then(as_i64_loose).unwrap_or(0),
        page_count: body.get("pageCount").and_then(as_i64_loose).unwrap_or(1),
        user_id: body.get("userId").and_then(as_i64_loose).unwrap_or(0),
        user_name: body
            .get("userName")
            .map(|v| as_str_or(v, ""))
            .unwrap_or_default(),
        title: body
            .get("title")
            .map(|v| as_str_or(v, ""))
            .unwrap_or_default(),
    }
}

/// `/ajax/illust/{id}/ugoira_meta` body → UgoiraMeta（纯函数）。
/// originalSrc 优先，zip_urls.original 兜底；两者皆空 → Client 错误。
pub(crate) fn parse_ugoira_meta(id: i64, body: &Value) -> Result<UgoiraMeta, PixivError> {
    let original = body
        .get("originalSrc")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let fallback = body
        .pointer("/zip_urls/original")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let zip_url = original.or(fallback).map(String::from);
    match zip_url {
        Some(zip_url) => Ok(UgoiraMeta { zip_url }),
        None => Err(PixivError::Client(format!("ugoira 作品 {id} 无 zip 原图"))),
    }
}

/// typed API 封装（内部共享同一个 PixivClient 的限速/重试语义）。
pub struct PixivApi {
    client: Arc<PixivClient>,
}

impl PixivApi {
    pub fn new(client: Arc<PixivClient>) -> Self {
        Self { client }
    }

    /// 原始客户端（限速/重试/429 语义全走它）。
    pub fn client(&self) -> &PixivClient {
        self.client.as_ref()
    }

    /// GET /ajax/novel/{id} → NovelData。
    pub async fn get_novel(&self, id: i64) -> Result<NovelData, PixivError> {
        let body = self.client.get_json(&format!("/ajax/novel/{id}")).await?;
        Ok(parse_novel_data(id, &body))
    }

    /// GET /ajax/novel/series_content/{id}?limit=30&last_order=0&order_by=asc&lang=zh
    /// → Vec<(novel_id, contentOrder)>。
    ///
    /// 解析 body.page.seriesContents[]：{"id":"...","series":{"contentOrder":N},"title":...}。
    /// contentOrder 从 1 开始；缺失时用遍历顺序兜底。注意不是 /ajax/novel/series/{id}
    /// （那是系列元信息，无小说列表）。单批语义，整包聚合见 [`Self::get_series_content_all`]。
    pub async fn get_series_content(&self, id: i64) -> Result<Vec<(i64, i64)>, PixivError> {
        let body = self
            .client
            .get_json(&format!(
                "/ajax/novel/series_content/{id}?limit=30&last_order=0&order_by=asc&lang=zh"
            ))
            .await?;
        Ok(parse_series_content(&body))
    }

    /// 系列目录整包聚合：`/ajax/novel/series/{id}` 元数据（标题/作者/封面）
    /// + `series_content` 游标循环取**全部话**（每批 30，`last_order` = 末条
    /// contentOrder；废除旧「≤30 话」假设）。终止条件：空批 / 不足一批 /
    /// 已覆盖 meta.total / 游标不再前进（防御死循环）。
    pub async fn get_series_content_all(&self, id: i64) -> Result<SeriesCatalog, PixivError> {
        const LIMIT: i64 = 30;
        let meta = self
            .client
            .get_json(&format!("/ajax/novel/series/{id}?lang=zh"))
            .await?;
        let total = meta.get("total").and_then(as_i64_loose).unwrap_or(0);
        let mut batches: Vec<Vec<(i64, i64)>> = Vec::new();
        let mut last_order = 0i64;
        loop {
            let body = self
                .client
                .get_json(&format!(
                    "/ajax/novel/series_content/{id}?limit={LIMIT}&last_order={last_order}&order_by=asc&lang=zh"
                ))
                .await?;
            let batch = parse_series_content(&body);
            if batch.is_empty() {
                break;
            }
            let next = batch.last().map(|(_, order)| *order).unwrap_or(last_order);
            batches.push(batch);
            if (batches.last().map(Vec::len).unwrap_or(0) as i64) < LIMIT
                || next <= last_order
                || (total > 0 && next >= total)
            {
                break;
            }
            last_order = next;
        }
        Ok(SeriesCatalog {
            series_title: meta.get("title").map(|v| as_str_or(v, "")).unwrap_or_default(),
            author_name: meta
                .get("userName")
                .map(|v| as_str_or(v, ""))
                .unwrap_or_default(),
            user_id: meta.get("userId").and_then(as_i64_loose).unwrap_or(0),
            cover_url: parse_series_cover(&meta),
            items: merge_series_batches(&batches),
        })
    }

    /// GET /ajax/user/{user_id}/profile/all?sensitiveFilterMode=userSetting&lang=zh
    /// → ProfileAll。匿名访问时内容为空（noLoginData 掩码），需登录态。
    pub async fn get_user_profile_all(&self, user_id: i64) -> Result<ProfileAll, PixivError> {
        let body = self
            .client
            .get_json(&format!(
                "/ajax/user/{user_id}/profile/all?sensitiveFilterMode=userSetting&lang=zh"
            ))
            .await?;
        Ok(parse_profile_all(&body))
    }

    /// GET /ajax/illust/{id} → IllustData。
    pub async fn get_illust(&self, id: i64) -> Result<IllustData, PixivError> {
        let body = self.client.get_json(&format!("/ajax/illust/{id}")).await?;
        Ok(parse_illust_data(&body))
    }

    /// GET /ajax/user/{id} → (name, account)。匿名可读；拿不到时调用方回退 userId。
    pub async fn get_user_info(&self, id: i64) -> Result<(String, String), PixivError> {
        let body = self.client.get_json(&format!("/ajax/user/{id}")).await?;
        Ok((
            body.get("name")
                .map(|v| as_str_or(v, ""))
                .unwrap_or_default(),
            body.get("account")
                .map(|v| as_str_or(v, ""))
                .unwrap_or_default(),
        ))
    }

    /// GET /ajax/illust/{id}/ugoira_meta → UgoiraMeta（originalSrc 优先）。
    pub async fn get_ugoira_meta(&self, id: i64) -> Result<UgoiraMeta, PixivError> {
        let body = self
            .client
            .get_json(&format!("/ajax/illust/{id}/ugoira_meta"))
            .await?;
        parse_ugoira_meta(id, &body)
    }

    /// GET /ajax/user/self?lang=zh → (userData 原始 JSON, csrf token)。
    /// 该端点响应是扁平结构（顶层 userData/token，无 error/body 信封）；
    /// userData 缺失返回 Null、token 缺失返回空串（调用方自行判定）。
    pub async fn get_user_self(&self) -> Result<(Value, String), PixivError> {
        let body = self.client.get_json("/ajax/user/self?lang=zh").await?;
        let user_data = body.get("userData").cloned().unwrap_or(Value::Null);
        let token = body
            .get("token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok((user_data, token))
    }
}

// ----------------------------------------------------------------------
// 单元测试（内嵌样例 JSON，全部离线）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_novel_full_shape() {
        let body = serde_json::json!({
            "title": "相対性理論",
            "userId": "28640",
            "userName": "作者",
            "pageCount": 3,
            "updateDate": "2026-07-21T00:00:00+09:00",
            "content": "正文[line]",
            "seriesNavData": {"seriesId": 1093870, "title": "系列名"},
            "coverUrl": "https://i.pximg.net/c/600x1200/novel-cover/9000012.jpg"
        });
        let d = parse_novel_data(9000012, &body);
        assert_eq!(d.novel_id, 9000012);
        assert_eq!(d.title, "相対性理論");
        assert_eq!(d.user_id, 28640, "userId 是字符串也能解析");
        assert_eq!(d.user_name, "作者");
        assert_eq!(d.page_count, 3);
        assert_eq!(d.content, "正文[line]");
        assert_eq!(d.series_id, Some(1093870));
        assert_eq!(d.series_title.as_deref(), Some("系列名"));
        assert_eq!(
            d.cover_url,
            "https://i.pximg.net/c/600x1200/novel-cover/9000012.jpg"
        );
        assert!(d.text_embedded_images.is_empty(), "无 textEmbeddedImages 时为空表");
    }

    #[test]
    fn parse_novel_missing_series_and_defaults() {
        let d = parse_novel_data(42, &serde_json::json!({"title": "t", "content": "c"}));
        assert_eq!(d.series_id, None);
        assert_eq!(d.series_title, None);
        assert_eq!(d.page_count, 1, "缺 pageCount 兜底 1");
        assert_eq!(d.user_id, 0);
        assert_eq!(d.update_date, "");
        assert_eq!(d.cover_url, "", "缺 coverUrl 兜底空串");
        assert!(d.text_embedded_images.is_empty());
        // 缺 title → 用 id 字符串（对齐 Python data.get("title", str(id))）
        let d = parse_novel_data(7, &serde_json::json!({"content": "c"}));
        assert_eq!(d.title, "7");
    }

    #[test]
    fn parse_novel_embedded_images_tier_fallback() {
        let body = serde_json::json!({
            "textEmbeddedImages": {
                "111": {"novelImageId": "111", "sl": "0", "urls": {
                    "original": "https://i.pximg.net/img/original/111.jpg",
                    "1200x1200": "https://i.pximg.net/img/1200/111.jpg",
                }},
                "222": {"urls": {"480mw": "https://i.pximg.net/img/480/222.jpg",
                                 "240mw": "https://i.pximg.net/img/240/222.jpg"}},
                "333": {"urls": {"128x128": "https://i.pximg.net/img/128/333.jpg"}},
                "444": {"urls": {}},
                "555": {},
            }
        });
        let images = parse_novel_embedded_images(&body);
        assert_eq!(images.len(), 3, "五档全空 / 无 urls 的条目跳过");
        assert_eq!(
            images["111"],
            "https://i.pximg.net/img/1200/111.jpg",
            "1200x1200 档优先"
        );
        assert_eq!(
            images["222"],
            "https://i.pximg.net/img/480/222.jpg",
            "缺 1200x1200/original 按 480mw 回退"
        );
        assert_eq!(images["333"], "https://i.pximg.net/img/128/333.jpg");
        // 空档位字符串（空串视为缺失）与空表容忍
        let body = serde_json::json!({"textEmbeddedImages": {"9": {"urls": {"1200x1200": ""}}}});
        assert!(parse_novel_embedded_images(&body).is_empty());
        assert!(parse_novel_embedded_images(&serde_json::json!({})).is_empty());
        assert!(parse_novel_embedded_images(&serde_json::json!({"textEmbeddedImages": null})).is_empty());
    }

    #[test]
    fn parse_series_cover_string_and_object_forms() {
        // 字符串形态
        let meta = serde_json::json!({"cover": "https://i.pximg.net/series/1.jpg"});
        assert_eq!(parse_series_cover(&meta), "https://i.pximg.net/series/1.jpg");
        // 对象形态：1200x1200 优先
        let meta = serde_json::json!({"cover": {"urls": {
            "original": "https://i.pximg.net/series/2_og.jpg",
            "1200x1200": "https://i.pximg.net/series/2_1200.jpg",
        }}});
        assert_eq!(parse_series_cover(&meta), "https://i.pximg.net/series/2_1200.jpg");
        // 对象形态：档位回退
        let meta = serde_json::json!({"cover": {"urls": {"480mw": "https://i.pximg.net/series/3_480.jpg"}}});
        assert_eq!(parse_series_cover(&meta), "https://i.pximg.net/series/3_480.jpg");
        // 缺失 / 空 urls → 空串
        assert_eq!(parse_series_cover(&serde_json::json!({})), "");
        assert_eq!(parse_series_cover(&serde_json::json!({"cover": {"urls": {}}})), "");
        assert_eq!(parse_series_cover(&serde_json::json!({"cover": null})), "");
    }

    #[test]
    fn merge_series_batches_sorts_dedups_and_tolerates_gaps() {
        // 跨批乱序 + 跳号 + 游标重叠（id 99 两批都出现 → 保首现去重）
        let batches = vec![
            vec![(101, 3), (102, 4)],
            vec![(100, 1), (99, 3), (103, 8)],
            vec![(99, 3), (104, 9)],
        ];
        assert_eq!(
            merge_series_batches(&batches),
            vec![(100, 1), (101, 3), (99, 3), (102, 4), (103, 8), (104, 9)],
            "按 contentOrder 升序、容忍跳号、同 id 跨批去重（保首现；同 order 稳定排序）"
        );
        // 全空
        assert!(merge_series_batches(&[vec![], vec![]]).is_empty());
        // 同批重复 id 也去重
        let batches = vec![vec![(7, 1), (7, 2)]];
        assert_eq!(merge_series_batches(&batches), vec![(7, 1)]);
    }

    #[test]
    fn parse_series_content_real_shape() {
        let body = serde_json::json!({
            "page": {
                "seriesContents": [
                    {"id": "9000012", "series": {"contentOrder": 1}, "title": "第1话"},
                    {"id": "9000013", "series": {"contentOrder": 2}, "title": "第2话"},
                    {"id": "1001", "title": "缺 contentOrder"},
                    {"id": "not-a-number", "series": {"contentOrder": 9}}
                ]
            }
        });
        let items = parse_series_content(&body);
        assert_eq!(
            items,
            vec![(9000012, 1), (9000013, 2), (1001, 3)],
            "缺 contentOrder 用 1-based 遍历序兜底；非法 id 跳过"
        );
    }

    #[test]
    fn parse_series_content_empty_or_missing() {
        assert!(
            parse_series_content(&serde_json::json!({"page": {"seriesContents": []}})).is_empty()
        );
        assert!(parse_series_content(&serde_json::json!({})).is_empty());
        assert!(parse_series_content(&serde_json::json!({"page": {}})).is_empty());
    }

    #[test]
    fn parse_profile_all_mixed_shapes() {
        let body = serde_json::json!({
            "novels": {"111": null, "abc": null, "222": null},
            "illusts": {"1": null, "2": null},
            "manga": {"9": null},
            "novelSeries": [
                {"id": 55, "title": "S1"},
                {"id": "66", "title": "S2"},
                {"title": "无 id 跳过"}
            ]
        });
        let p = parse_profile_all(&body);
        assert_eq!(p.novels, vec![111, 222], "非法键 abc 跳过");
        assert_eq!(p.illusts, vec![1, 2]);
        assert_eq!(p.manga, vec![9]);
        assert_eq!(p.novel_series, vec![(55, "S1".into()), (66, "S2".into())]);
    }

    #[test]
    fn parse_profile_all_empty_body() {
        let p = parse_profile_all(&serde_json::json!({}));
        assert!(p.novels.is_empty());
        assert!(p.novel_series.is_empty());
        assert!(p.illusts.is_empty());
        assert!(p.manga.is_empty());
    }

    #[test]
    fn parse_illust_full_shape() {
        let body = serde_json::json!({
            "urls": {"original": "https://i.pximg.net/img-original/img/x_p0.jpg"},
            "meta": {
                "pages": [
                    {"image_urls": {"original": "https://i.pximg.net/x_p0.jpg"}},
                    {"original": "https://i.pximg.net/x_p1.jpg"},
                    {}
                ]
            },
            "illustType": 0,
            "pageCount": 3,
            "userId": "77",
            "userName": "画师",
            "title": "作品"
        });
        let d = parse_illust_data(&body);
        assert_eq!(
            d.urls_original_p0,
            "https://i.pximg.net/img-original/img/x_p0.jpg"
        );
        assert_eq!(
            d.meta_pages_original,
            vec![
                "https://i.pximg.net/x_p0.jpg".to_string(),
                "https://i.pximg.net/x_p1.jpg".to_string(),
                String::new(),
            ],
            "image_urls.original 优先，条目 original 兜底，缺失留空串"
        );
        assert_eq!(d.illust_type, 0);
        assert_eq!(d.page_count, 3);
        assert_eq!(d.user_id, 77);
        assert_eq!(d.title, "作品");
    }

    #[test]
    fn parse_illust_anonymous_masked_meta() {
        // 匿名访问 meta 被掩码：只剩 p0 直链
        let body = serde_json::json!({
            "urls": {"original": "https://i.pximg.net/y_p0.png"},
            "illustType": 1,
            "pageCount": 2,
            "userId": 9,
            "userName": "u",
            "title": "t"
        });
        let d = parse_illust_data(&body);
        assert!(d.meta_pages_original.is_empty());
        assert_eq!(d.urls_original_p0, "https://i.pximg.net/y_p0.png");
        assert_eq!(d.illust_type, 1);
    }

    #[test]
    fn parse_ugoira_meta_priority_and_error() {
        // originalSrc 优先
        let m = parse_ugoira_meta(
            1,
            &serde_json::json!({"originalSrc": "https://x/a.zip", "zip_urls": {"original": "https://x/b.zip"}}),
        )
        .unwrap();
        assert_eq!(m.zip_url, "https://x/a.zip");
        // 兜底 zip_urls.original
        let m = parse_ugoira_meta(
            1,
            &serde_json::json!({"zip_urls": {"original": "https://x/b.zip"}}),
        )
        .unwrap();
        assert_eq!(m.zip_url, "https://x/b.zip");
        // 皆空 → Client 错误
        match parse_ugoira_meta(1, &serde_json::json!({})) {
            Err(PixivError::Client(msg)) => assert!(msg.contains("ugoira 作品 1 无 zip 原图")),
            other => panic!("预期 Client 错误，实际 {other:?}"),
        }
    }
}
