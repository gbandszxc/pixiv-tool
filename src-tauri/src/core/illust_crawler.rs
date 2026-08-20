//! 插画爬虫内部工具函数（页 URL 收集、`_p0 → _pN` 推导、ugoira zip、
//! 文件名/扩展名/目录命名），供 `core::crawler::run_illust_crawler` 调用。
//!
//! 语义对齐 Python `core/illust_crawler.py` 的同名下划线函数。

use std::sync::OnceLock;

use regex::Regex;

use crate::core::exporter::sanitize_filename;
use crate::pixiv::api::{IllustData, PixivApi};

/// ugoira（动图）的 illustType。
pub(crate) const UGOIRA_TYPE: i64 = 2;

/// 收集作品各页原图 URL（登录态 meta 优先，缺失时从 p0 直链推导兜底）。
///
/// - `meta.pages[].image_urls.original` 非空（列表非空即用，条目可能含空串，
///   下载时跳过——对齐 Python `if urls: return urls`）；
/// - 否则 p0 直链 + `_p0.{ext} → _p{i}.{ext}` 推导 1..page_count；
/// - p0 也缺失 → 空列表（调用方报"没有可用原图 URL"）。
pub(crate) fn collect_page_urls(data: &IllustData) -> Vec<String> {
    if !data.meta_pages_original.is_empty() {
        return data.meta_pages_original.clone();
    }
    if data.urls_original_p0.is_empty() {
        return Vec::new();
    }
    let mut urls = vec![data.urls_original_p0.clone()];
    for page in 1..data.page_count {
        urls.push(derive_page_url(&data.urls_original_p0, page));
    }
    urls
}

/// 多图作品从 p0 直链推导第 N 页原图：`_p0.{ext} → _p{N}.{ext}`。
/// 只替换末尾页码（带 hash 的 URL 同样适用）；无 `_p0.` 后缀时原样返回。
pub(crate) fn derive_page_url(p0_url: &str, page: i64) -> String {
    static P0: OnceLock<Regex> = OnceLock::new();
    let re = P0.get_or_init(|| Regex::new(r"_p0(\.[A-Za-z0-9]+)$").expect("页码正则合法"));
    let replacement = format!("_p{page}$1");
    re.replace_all(p0_url, replacement.as_str()).to_string()
}

/// 从 URL 取扩展名：去查询串/锚点，取**末段路径**中最后一个 `.` 之后的部分；
/// 末段无 `.` 或扩展名为空 → "bin"。
pub(crate) fn url_ext(url: &str) -> String {
    let no_query = url.split(['?', '#']).next().unwrap_or("");
    let last_segment = no_query.rsplit('/').next().unwrap_or("");
    match last_segment.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_string(),
        _ => "bin".to_string(),
    }
}

/// 文件名安全化（标题净化后为空 → 用 artwork id 字符串兜底）。
pub(crate) fn safe_title_or_id(title: &str, artwork_id: i64) -> String {
    let safe = sanitize_filename(title);
    if safe.is_empty() {
        artwork_id.to_string()
    } else {
        safe
    }
}

/// 普通页文件名：`{safe}_{artwork_id}_p{idx}.{ext}`。
pub(crate) fn page_filename(safe_title: &str, artwork_id: i64, idx: usize, url: &str) -> String {
    format!("{safe_title}_{artwork_id}_p{idx}.{}", url_ext(url))
}

/// ugoira zip 文件名：`{safe}_{artwork_id}_ugoira.zip`。
pub(crate) fn ugoira_filename(safe_title: &str, artwork_id: i64) -> String {
    format!("{safe_title}_{artwork_id}_ugoira.zip")
}

/// 用户全集目录名：`{sanitize(作者名)}_{uid}`；作者名取 name||account，
/// 拿不到（接口失败 / 两者为空）退回 `user_{uid}`（对齐 Python `_user_dir_name`）。
pub(crate) async fn user_dir_name(api: &PixivApi, user_id: i64) -> String {
    match api.get_user_info(user_id).await {
        Ok((name, account)) => {
            let picked = if !name.is_empty() { name } else { account };
            if !picked.is_empty() {
                return format!("{}_{}", sanitize_filename(&picked), user_id);
            }
            log::warn!("用户 {user_id} 信息缺 name/account，目录退回 userId");
        }
        Err(err) => {
            log::warn!("获取用户 {user_id} 信息失败，目录退回 userId: {err}");
        }
    }
    format!("user_{user_id}")
}

// ----------------------------------------------------------------------
// 单元测试（全部离线）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn illust(p0: &str, meta_pages: Vec<&str>, page_count: i64) -> IllustData {
        IllustData {
            urls_original_p0: p0.to_string(),
            meta_pages_original: meta_pages.into_iter().map(String::from).collect(),
            illust_type: 0,
            page_count,
            user_id: 1,
            user_name: "u".into(),
            title: "t".into(),
        }
    }

    #[test]
    fn collect_prefers_meta_pages() {
        let d = illust(
            "https://i.pximg.net/x_p0.jpg",
            vec!["https://i.pximg.net/m0.jpg", "https://i.pximg.net/m1.jpg"],
            2,
        );
        assert_eq!(
            collect_page_urls(&d),
            vec!["https://i.pximg.net/m0.jpg", "https://i.pximg.net/m1.jpg"]
        );
    }

    #[test]
    fn collect_derives_from_p0_when_meta_missing() {
        let d = illust("https://i.pximg.net/img-original/img/x_p0.jpg", vec![], 3);
        assert_eq!(
            collect_page_urls(&d),
            vec![
                "https://i.pximg.net/img-original/img/x_p0.jpg",
                "https://i.pximg.net/img-original/img/x_p1.jpg",
                "https://i.pximg.net/img-original/img/x_p2.jpg",
            ]
        );
    }

    #[test]
    fn collect_single_page_only_p0() {
        let d = illust("https://i.pximg.net/x_p0.png", vec![], 1);
        assert_eq!(collect_page_urls(&d), vec!["https://i.pximg.net/x_p0.png"]);
        // pageCount 0/负数同样只有 p0
        let d = illust("https://i.pximg.net/x_p0.png", vec![], 0);
        assert_eq!(collect_page_urls(&d).len(), 1);
    }

    #[test]
    fn collect_empty_when_no_p0_no_meta() {
        assert!(collect_page_urls(&illust("", vec![], 3)).is_empty());
    }

    #[test]
    fn derive_keeps_hash_and_extension() {
        // 带 hash 的 URL：只替换末尾页码
        let p0 =
            "https://i.pximg.net/img-original/img/2026/01/01/00/00/00/147635069-499627f2abc_p0.png";
        assert!(
            derive_page_url(p0, 12).ends_with("147635069-499627f2abc_p12.png"),
            "hash 前缀应保留"
        );
        // 无 _p0 后缀 → 原样
        assert_eq!(derive_page_url("https://x/img.jpg", 1), "https://x/img.jpg");
    }

    #[test]
    fn url_ext_handles_query_and_fragment() {
        assert_eq!(url_ext("https://i.pximg.net/a/b/x_p0.jpg"), "jpg");
        assert_eq!(url_ext("https://i.pximg.net/a/b/x_p0.jpg?v=2&m=3"), "jpg");
        assert_eq!(url_ext("https://i.pximg.net/a/b/x_p0.png#frag"), "png");
        assert_eq!(url_ext("https://i.pximg.net/a/noext"), "bin");
        assert_eq!(url_ext("https://i.pximg.net/a/trailing."), "bin");
    }

    #[test]
    fn filenames_use_safe_title_or_id() {
        assert_eq!(safe_title_or_id("作品<名>", 42), "作品_名");
        assert_eq!(safe_title_or_id("___", 42), "42", "净化后为空 → 用 id");
        assert_eq!(
            page_filename("safe", 1001, 0, "https://x/a_p0.jpg?x=1"),
            "safe_1001_p0.jpg"
        );
        assert_eq!(ugoira_filename("safe", 1001), "safe_1001_ugoira.zip");
    }
}
