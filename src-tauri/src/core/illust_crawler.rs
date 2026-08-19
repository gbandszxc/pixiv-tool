//! 插画爬虫（与 crawler.rs 平行，共享 EventSink / TaskControls 契约）。
//!
//! **桩说明**：插画爬虫的公共入口是 `crate::core::crawler::run_illust_crawler`
//! + `IllustCrawlArgs`（两类爬虫共用同一入口文件，减少契约分裂）。
//! 本文件预留给 phase C 放插画特有的内部工具函数
//! （页 URL 推导 `_p0 → _pN`、ugoira zip、目录命名等），当前为空。
#![allow(unused)]

// phase C 填充建议（内部工具，非公共契约）：
// - collect_page_urls(&IllustData) -> Vec<String>   meta_pages_original 优先，p0 直链推导兜底
// - derive_page_url(p0: &str, page: i64) -> String  _p0.{ext} → _p{N}.{ext}
// - url_ext(url: &str) -> String                    去查询串取扩展名（默认 "bin"）
// - user_dir_name(api, user_id) -> String           sanitize(作者名)_{uid}，退回 user_{uid}
