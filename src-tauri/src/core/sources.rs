//! NovelSource / IllustSource —— 解析"来源"得到待抓 id 列表。
//!
//! 语义对齐 Python `core/source.py` / `core/illust_source.py`：
//! - Series：seriesContents 按 contentOrder（缺失用 1-based 遍历序兜底）；
//! - User（小说）：先展开每个 series（序号为 series 内 order），再补不属任何
//!   series 的散篇（order=None）；
//! - User（插画）：illusts + manga 全部 id；
//! - 非法 id（解析失败）跳过。

use std::collections::HashSet;

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
    ///   series 的散篇（order=None，顺序无意义但稳定）
    /// - 非法 id（解析失败）跳过
    pub async fn resolve(&self, api: &PixivApi) -> Result<Vec<(i64, Option<i64>)>> {
        match self {
            NovelSource::Single(id) => Ok(vec![(*id, None)]),
            NovelSource::Series(series_id) => {
                let contents = api.get_series_content(*series_id).await?;
                Ok(contents
                    .into_iter()
                    .map(|(novel_id, order)| (novel_id, Some(order)))
                    .collect())
            }
            NovelSource::User(user_id) => {
                let profile = api.get_user_profile_all(*user_id).await?;
                let mut seen: HashSet<i64> = HashSet::new();
                let mut items: Vec<(i64, Option<i64>)> = Vec::new();
                for (series_id, _title) in &profile.novel_series {
                    // 标题暂不参与解析（NovelData 自带 seriesNavData）
                    let contents = api.get_series_content(*series_id).await?;
                    for (novel_id, order) in contents {
                        if seen.insert(novel_id) {
                            items.push((novel_id, Some(order)));
                        }
                    }
                }
                // 散篇 = profile.novels - 已归入系列的 id
                for novel_id in &profile.novels {
                    if seen.insert(*novel_id) {
                        items.push((*novel_id, None));
                    }
                }
                Ok(items)
            }
        }
    }
}

impl IllustSource {
    /// 解析为 artwork id 列表。Single → [id]；User → illusts + manga
    /// （顺序稳定：illusts 在前、manga 在后，各自保持解析顺序）。
    pub async fn resolve(&self, api: &PixivApi) -> Result<Vec<i64>> {
        match self {
            IllustSource::Single(id) => Ok(vec![*id]),
            IllustSource::User(user_id) => {
                let profile = api.get_user_profile_all(*user_id).await?;
                let mut ids = profile.illusts;
                ids.extend(profile.manga);
                Ok(ids)
            }
        }
    }

    /// 预取任务分母（进度显示用）：user=作品总数（illusts+manga 数量）；
    /// single=pageCount（max(1, pageCount)）。无法确定 → None（调用方退回增量计数；
    /// 网络/接口失败同样返回 None 并记日志，不作为错误中断任务）。
    pub async fn resolve_total(&self, api: &PixivApi) -> Result<Option<i64>> {
        let total = match self {
            IllustSource::User(user_id) => match api.get_user_profile_all(*user_id).await {
                Ok(profile) => Some((profile.illusts.len() + profile.manga.len()) as i64),
                Err(err) => {
                    log::warn!("预取用户 {user_id} 作品总数失败，退回增量计数: {err}");
                    None
                }
            },
            IllustSource::Single(id) => match api.get_illust(*id).await {
                Ok(data) => Some(data.page_count.max(1)),
                Err(err) => {
                    log::warn!("预取插画 {id} 页数失败，退回增量计数: {err}");
                    None
                }
            },
        };
        Ok(total)
    }
}

// ----------------------------------------------------------------------
// 单元测试（Single 分支离线可测；Series/User 依赖网络，其解析逻辑由
// pixiv::api 的 parse_* 纯函数测试覆盖）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::pixiv::client::PixivClient;

    fn api() -> PixivApi {
        PixivApi::new(Arc::new(
            PixivClient::new(&HashMap::new()).expect("离线构造 wreq 客户端"),
        ))
    }

    #[tokio::test]
    async fn novel_single_resolves_without_network() {
        let items = NovelSource::Single(12345).resolve(&api()).await.unwrap();
        assert_eq!(items, vec![(12345, None)]);
    }

    #[tokio::test]
    async fn illust_single_resolves_without_network() {
        let ids = IllustSource::Single(99).resolve(&api()).await.unwrap();
        assert_eq!(ids, vec![99]);
    }

    #[test]
    fn source_enums_are_cloneable_and_comparable() {
        let s = NovelSource::Series(1);
        assert_eq!(s.clone(), NovelSource::Series(1));
        let i = IllustSource::User(2);
        assert_eq!(i.clone(), IllustSource::User(2));
    }
}
