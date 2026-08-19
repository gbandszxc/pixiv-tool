//! PixivClient —— 带 TLS 指纹伪装 + 限速 + 重试 + 429 全队列暂停的 HTTP 客户端。
//!
//! **桩（phase B 实现）**：本文件只定稿公共契约，函数体待填充。
//!
//! 底层用 wreq（Chrome147 emulation，spike 已验证通过 pixiv 风控）：
//! - `PixivClient::new`：wreq Client + wreq_util Chrome147 指纹；
//!   cookie jar 预置 pixiv.net 域（除 x-csrf-token 外的全部 cookie，含 PHPSESSID）；
//!   x-csrf-token 拆出放默认 header；UA 用 Chrome147 Windows 版。
#![allow(unused)]

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

/// 并发上限（V1 写死，不暴露给用户）。
pub const CONCURRENCY: usize = 2;
/// 同一信号量持有期内的最小请求间隔（毫秒）。sleep 必须在 semaphore 持有期间
/// 执行，保证间隔真正生效（Python 版曾因缩进在块外导致限速失效触发风控）。
pub const REQUEST_INTERVAL_MS: u64 = 400;
/// 单请求超时（秒）。
pub const REQUEST_TIMEOUT_SECS: u64 = 15;
/// 最大重试次数（429/5xx/网络错误）。
pub const MAX_RETRIES: u32 = 3;
/// 命中 429 后整个客户端的暂停时长（秒）。
pub const PAUSE_ON_429_SECS: u64 = 60;
/// 重试退避（秒），按尝试次数取。
pub const RETRY_BACKOFF_SECS: [u64; 3] = [1, 2, 4];

pub const BASE_URL: &str = "https://www.pixiv.net";
pub const AJAX_URL: &str = "https://www.pixiv.net/ajax";
/// Chrome 147（Windows）UA，与 wreq Chrome147 指纹伪装配套。
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";

/// Pixiv API 错误分类（消息中文化）。
#[derive(thiserror::Error, Debug)]
pub enum PixivError {
    /// 401/403 认证失败。
    #[error("认证失败：登录态无效或已过期")]
    Auth,
    /// 404 资源不存在。
    #[error("资源不存在")]
    NotFound,
    /// 429 请求过多（触发全队列暂停）。
    #[error("请求过于频繁（429），已触发全队列暂停")]
    RateLimit,
    /// 5xx 服务端错误。
    #[error("Pixiv 服务端错误")]
    Server,
    /// 其它 4xx / API error 响应。
    #[error("客户端错误: {0}")]
    Client(String),
    /// 网络/超时错误。
    #[error("网络错误: {0}")]
    Network(String),
}

pub struct PixivClient {
    /// wreq Client（Chrome147 emulation）。
    pub(crate) http: wreq::Client,
    /// 并发闸门（CONCURRENCY 个许可）。
    pub(crate) semaphore: Arc<tokio::sync::Semaphore>,
    /// 429 暂停闸门：true=暂停，所有请求进入前等它变回 false。
    pub(crate) pause_gate: Arc<tokio::sync::watch::Sender<bool>>,
    /// 默认 header（x-csrf-token 等）。
    pub(crate) default_headers: wreq::header::HeaderMap,
}

impl PixivClient {
    /// 构造客户端（Chrome147 emulation；cookie jar 预置 pixiv.net 域；
    /// x-csrf-token 拆出放默认 header；UA=Chrome147 Windows 版）。
    pub fn new(cookies: &HashMap<String, String>) -> anyhow::Result<Self> {
        todo!("phase B: wreq-util Chrome147 指纹 + cookie jar + csrf header + UA")
    }

    /// GET `{BASE_URL}{path}` 并解析 JSON，返回 body 字段（无 body 时返回整个对象）。
    ///
    /// 语义（phase B 实现）：
    /// - 所有请求共享 CONCURRENCY 信号量 + REQUEST_INTERVAL_MS 间隔
    ///   （间隔 sleep 在 semaphore 持有期间执行）
    /// - 先等 429 暂停闸门解除
    /// - 429 → 清空闸门、安排 PAUSE_ON_429_SECS 后恢复，本次按 RateLimit 进重试
    /// - 401/403 → Auth（不重试）；404 → NotFound（不重试）
    /// - 5xx / 网络错误 → RETRY_BACKOFF_SECS 退避重试，共 MAX_RETRIES 次
    /// - 响应 body.error 非空 → Client(body.message)
    pub async fn get_json(&self, path: &str) -> Result<Value, PixivError> {
        todo!("phase B")
    }

    /// 下载二进制（i.pximg.net 原图 / ugoira zip）。额外带
    /// `Referer: https://www.pixiv.net/`（pximg 防盗链校验）。
    /// 与 get_json 共用限速/重试/429 暂停。
    pub async fn download_bytes(&self, url: &str) -> Result<Vec<u8>, PixivError> {
        todo!("phase B")
    }
}
