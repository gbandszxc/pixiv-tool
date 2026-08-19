//! NovelSource / IllustSource —— 解析"来源"得到待抓 id 列表。
//!
//! **桩（phase C 实现）**：本文件只定稿公共契约，函数体待填充。
#![allow(unused)]

use anyhow::Result;

use crate::pixiv::api::PixivApi;

/// 小说来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NovelSource {
    /// 单篇。
    Single(i64),
    /// 系列。
    Series(i64),
    /// 用户全集。
    User(i64),
}

/// 插画来源（插画没有"系列抓取"：pixiv 系列插画本身是一个多页作品）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IllustSource {
    /// 单幅作品。
    Single(i64),
    /// 用户全部插画/漫画。
    User(i64),
}

impl NovelSource {
    /// 解析为 (novel_id, series_order) 列表（单篇 series_order=None）。
    ///
    /// - Single → [(id, None)]
    /// - Series → seriesContents 按 contentOrder
    /// - User → 先展开每个 series（序号为 series 内 order，去重），再补不属任何
    ///   series 的散篇（order=None，顺序无意义）
    /// - 非法 id（解析失败）跳过
    pub async fn resolve(&self, api: &PixivApi) -> Result<Vec<(i64, Option<i64>)>> {
        todo!("phase C")
    }
}

impl IllustSource {
    /// 解析为 artwork id 列表。Single → [id]；User → illusts + manga
    /// （排序任意但要稳定）。
    pub async fn resolve(&self, api: &PixivApi) -> Result<Vec<i64>> {
        todo!("phase C")
    }

    /// 预取任务分母（进度显示用）：user=作品总数（illusts+manga 数量）；
    /// single=pageCount（max(1, pageCount)）。无法确定 → None（调用方退回增量计数）。
    pub async fn resolve_total(&self, api: &PixivApi) -> Result<Option<i64>> {
        todo!("phase C")
    }
}
