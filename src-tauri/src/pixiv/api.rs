//! Pixiv Ajax API 的 typed 封装（serde struct + 从 JSON 解析）。
//!
//! **桩（phase B 实现）**：本文件只定稿公共契约（结构体字段 + 方法签名 +
//! 接口路径与解析语义），函数体待填充。
#![allow(unused)]

use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use super::client::{PixivClient, PixivError};

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
    /// 缺失时从 p0 直链按 _p0 → _pN 推导兜底）。
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
        todo!("phase B")
    }

    /// GET /ajax/novel/series_content/{id}?limit=30&last_order=0&order_by=asc&lang=zh
    /// → Vec<(novel_id, contentOrder)>。
    ///
    /// 解析 body.page.seriesContents[]：{"id":"...","series":{"contentOrder":N},"title":...}。
    /// contentOrder 从 1 开始；缺失时用遍历顺序兜底。注意不是 /ajax/novel/series/{id}
    /// （那是系列元信息，无小说列表）。V1 假设系列 ≤ 30 话。
    pub async fn get_series_content(&self, id: i64) -> Result<Vec<(i64, i64)>, PixivError> {
        todo!("phase B")
    }

    /// GET /ajax/user/{user_id}/profile/all?sensitiveFilterMode=userSetting&lang=zh
    /// → ProfileAll。匿名访问时内容为空（noLoginData 掩码），需登录态。
    pub async fn get_user_profile_all(&self, user_id: i64) -> Result<ProfileAll, PixivError> {
        todo!("phase B")
    }

    /// GET /ajax/illust/{id} → IllustData。
    pub async fn get_illust(&self, id: i64) -> Result<IllustData, PixivError> {
        todo!("phase B")
    }

    /// GET /ajax/user/{id} → (name, account)。匿名可读；拿不到时调用方回退 userId。
    pub async fn get_user_info(&self, id: i64) -> Result<(String, String), PixivError> {
        todo!("phase B")
    }

    /// GET /ajax/illust/{id}/ugoira_meta → UgoiraMeta。
    pub async fn get_ugoira_meta(&self, id: i64) -> Result<UgoiraMeta, PixivError> {
        todo!("phase B")
    }

    /// GET /ajax/user/self?lang=zh → (userData 原始 JSON, csrf token)。
    pub async fn get_user_self(&self) -> Result<(Value, String), PixivError> {
        todo!("phase B")
    }
}
