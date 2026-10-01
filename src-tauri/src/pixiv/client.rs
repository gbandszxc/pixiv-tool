//! PixivClient —— 带 TLS 指纹伪装 + 限速 + 重试 + 429 全队列暂停的 HTTP 客户端。
//!
//! 底层 wreq（Chrome147 emulation，spike 已验证通过 pixiv 风控）。
//! 语义逐条对齐 Python `core/pixiv_client.py`：
//! - 并发闸门（CONCURRENCY 个许可），进入临界区后先等 429 暂停闸门；
//! - 无论成败，在 semaphore 持有期间 sleep REQUEST_INTERVAL_MS（Python 版曾因
//!   finally 缩进错误导致限速失效触发风控，此处为修正后的行为）；
//! - 401/403 → Auth（立即终止）、404 → NotFound（立即终止）、
//!   429 → 置全队列暂停 + 60s 自动恢复 + RateLimit（当次不重试）、
//!   5xx / 其他非 200 / 网络错误 → 记 last_err 退避重试，共 MAX_RETRIES 次；
//! - 响应体 `error` 为真值 → `Client("API error: {message}")`，
//!   否则返回 `body` 字段（缺失时返回整个对象）。
//!
//! Cookie 处理：`x-csrf-token` 拆出放默认 header（Pixiv 专用 header）；
//! 其余 cookie（含 PHPSESSID）拼成 `Cookie: k=v; k2=v2`，**只在 ajax 请求
//! （get_json 系）携带**，download_bytes 不带（避免下载 i.pximg.net 时误带）。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use wreq::header::{HeaderMap, HeaderValue};
use wreq_util::Emulation;

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
/// 重试退避（秒），按尝试次数取。第 3 次（index 2）的 4s 不可达——最后一次
/// 失败直接抛 last_err——保留常量对齐 Python `RETRY_BACKOFF = [1, 2, 4]`。
pub const RETRY_BACKOFF_SECS: [u64; 3] = [1, 2, 4];

pub const BASE_URL: &str = "https://www.pixiv.net";
pub const AJAX_URL: &str = "https://www.pixiv.net/ajax";
/// Chrome 147（Windows）UA，与 wreq Chrome147 指纹伪装配套。
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";
/// pximg 防盗链要求的 Referer。
pub const REFERER: &str = "https://www.pixiv.net/";
pub const ACCEPT_LANGUAGE: &str = "zh-CN,zh;q=0.9,en;q=0.8";

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

/// 状态码 → 错误分类（200 → None 表示成功）。与 Python `_request` 的分支一一对应：
/// 只有精确 200 走成功路径（2xx 其他码按 Python 语义进可重试的 Client 分支）。
pub fn classify_status(status: u16) -> Option<PixivError> {
    match status {
        200 => None,
        401 | 403 => Some(PixivError::Auth),
        404 => Some(PixivError::NotFound),
        429 => Some(PixivError::RateLimit),
        500..=599 => Some(PixivError::Server),
        other => Some(PixivError::Client(format!("HTTP {other}"))),
    }
}

/// JSON 真值语义（对齐 Python truthiness：null/false/0/""/[]/{} 为假）。
pub(crate) fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// ajax 响应体提取：`error` 为真值 → `Client("API error: {message}")`；
/// 否则返回 `body` 字段（缺失时返回整个对象）。对齐 Python `_get`。
pub(crate) fn extract_ajax_body(body: Value) -> Result<Value, PixivError> {
    if body.get("error").is_some_and(json_truthy) {
        let message = match body.get("message") {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => String::new(),
        };
        return Err(PixivError::Client(format!("API error: {message}")));
    }
    Ok(body.get("body").cloned().unwrap_or(body))
}

/// 日志脱敏：去掉查询串（Python `_mask_url`）。
pub(crate) fn mask_url(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

fn network_err(err: wreq::Error, url: &str) -> PixivError {
    PixivError::Network(format!("{err}: {}", mask_url(url)))
}

/// header 值可安全入 HTTP 头（无控制字符 / DEL）。
fn header_safe(s: &str) -> bool {
    !s.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7f)
}

pub struct PixivClient {
    /// wreq Client（Chrome147 emulation）。
    pub(crate) http: wreq::Client,
    /// 并发闸门（CONCURRENCY 个许可）。
    pub(crate) semaphore: Arc<tokio::sync::Semaphore>,
    /// 429 暂停闸门：true=暂停，所有请求进入前等它变回 false。
    pub(crate) pause_gate: Arc<tokio::sync::watch::Sender<bool>>,
    /// 默认 header（UA / Referer / Accept-Language / x-csrf-token）。
    /// 请求实际由 wreq builder 配置携带；保留字段供诊断与单测断言。
    #[allow(dead_code)]
    pub(crate) default_headers: HeaderMap,
    /// ajax 请求附加的 `Cookie: k=v; k2=v2` 头（不含 x-csrf-token）。
    /// 只在 get_json 系请求携带；download_bytes 不带。
    pub(crate) cookie_header: Option<String>,
}

impl PixivClient {
    /// 构造客户端（Chrome147 emulation；cookie 拆分：x-csrf-token → 默认 header，
    /// 其余 → ajax 专用 Cookie 头；UA/Referer/Accept-Language 走默认 header）。
    pub fn new(cookies: &HashMap<String, String>) -> anyhow::Result<Self> {
        let mut default_headers = HeaderMap::new();
        default_headers.insert("user-agent", HeaderValue::from_static(USER_AGENT));
        default_headers.insert("referer", HeaderValue::from_static(REFERER));
        default_headers.insert("accept-language", HeaderValue::from_static(ACCEPT_LANGUAGE));

        let mut pairs: Vec<String> = Vec::new();
        for (name, value) in cookies {
            if !header_safe(name) || !header_safe(value) {
                log::warn!("cookie 含非法字符，忽略: {name}");
                continue;
            }
            if name == "x-csrf-token" {
                if !value.is_empty() {
                    default_headers.insert("x-csrf-token", HeaderValue::from_str(value)?);
                }
            } else if !name.is_empty() && !value.is_empty() {
                pairs.push(format!("{name}={value}"));
            }
        }
        pairs.sort(); // HashMap 迭代序不定，排序保证 Cookie 头稳定
        let cookie_header = (!pairs.is_empty()).then(|| pairs.join("; "));

        let http = wreq::Client::builder()
            .emulation(Emulation::Chrome147)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .default_headers(default_headers.clone())
            .build()?;

        log::debug!(
            "PixivClient 就绪（默认 header {} 项，cookie {} 项）",
            default_headers.len(),
            cookie_header
                .as_deref()
                .map(|c| c.matches('=').count())
                .unwrap_or(0)
        );

        Ok(Self {
            http,
            semaphore: Arc::new(tokio::sync::Semaphore::new(CONCURRENCY)),
            pause_gate: Arc::new(tokio::sync::watch::channel(false).0),
            default_headers,
            cookie_header,
        })
    }

    /// 429 全队列暂停：闸门置 true，并安排 PAUSE_ON_429_SECS 秒后自动恢复。
    /// 多次触发各自计时，重复恢复幂等。
    pub(crate) fn trigger_global_pause(&self) {
        log::warn!("429 Rate Limit — 全队列暂停 {PAUSE_ON_429_SECS}s");
        // send_replace 而非 send：watch 的 send 在无接收者（当前没有请求在等
        // 闸门）时是 no-op，会丢失这次全队列暂停。
        self.pause_gate.send_replace(true);
        let gate = self.pause_gate.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(PAUSE_ON_429_SECS)).await;
            gate.send_replace(false);
            log::info!("429 暂停结束，恢复请求");
        });
    }

    /// 等暂停闸门解除（paused=true 时挂起），返回剩余部分由调用方计时。
    pub(crate) async fn wait_not_paused(&self) {
        let mut rx = self.pause_gate.subscribe();
        loop {
            if !*rx.borrow_and_update() {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }

    /// 限速 + 重试核心管线（Python `_request`）：
    /// 1. 取信号量许可 → 2. 等 429 闸门 → 3. 最多 MAX_RETRIES 次尝试
    ///    （Auth/NotFound/RateLimit 立即终止；Server/Client/Network 退避重试）
    ///    → 4. finally：无论成败在 semaphore 持有期间 sleep REQUEST_INTERVAL_MS。
    ///
    /// `pub(crate)`：csrf.rs 的主页 token 抓取复用同一闸门/限速语义。
    pub(crate) async fn run_gated<T, F, Fut>(&self, mut attempt: F) -> Result<T, PixivError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, PixivError>>,
    {
        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| PixivError::Network("并发闸门已关闭".into()))?;
        self.wait_not_paused().await;

        let mut last_err: Option<PixivError> = None;
        for idx in 0..MAX_RETRIES {
            match attempt().await {
                Ok(value) => {
                    // finally 语义：成功路径也限速（持锁期间 sleep）
                    tokio::time::sleep(Duration::from_millis(REQUEST_INTERVAL_MS)).await;
                    return Ok(value);
                }
                Err(err) => {
                    let abort = matches!(
                        err,
                        PixivError::Auth | PixivError::NotFound | PixivError::RateLimit
                    );
                    last_err = Some(err);
                    if abort || idx + 1 == MAX_RETRIES {
                        break;
                    }
                    let wait = RETRY_BACKOFF_SECS[(idx as usize).min(RETRY_BACKOFF_SECS.len() - 1)];
                    log::info!("重试 {}/{MAX_RETRIES}，等待 {wait}s", idx + 1);
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                }
            }
        }
        // finally 语义：失败路径同样限速（semaphore 持有期间）
        tokio::time::sleep(Duration::from_millis(REQUEST_INTERVAL_MS)).await;
        Err(last_err.unwrap_or(PixivError::Network("请求失败".into())))
    }

    /// 发 ajax GET（带 Cookie 头）。非 200 走 classify_status 分类；
    /// 429 额外触发全队列暂停。
    ///
    /// `pub(crate)`：csrf.rs 的主页 token 抓取（HTML 响应）复用同一发送语义。
    pub(crate) async fn send_ajax(&self, url: &str) -> Result<wreq::Response, PixivError> {
        let mut req = self.http.get(url);
        if let Some(cookie) = self.cookie_header.as_deref() {
            req = req.header("cookie", cookie);
        }
        let resp = req.send().await.map_err(|e| network_err(e, url))?;
        let status = resp.status().as_u16();
        log::info!("GET {} → {status}", mask_url(url));
        if let Some(err) = classify_status(status) {
            if status == 429 {
                self.trigger_global_pause();
            }
            return Err(err);
        }
        Ok(resp)
    }

    /// 发下载 GET（只额外带 Referer，不带 pixiv cookie）。
    async fn send_download(&self, url: &str) -> Result<wreq::Response, PixivError> {
        let resp = self
            .http
            .get(url)
            .header("referer", REFERER)
            .send()
            .await
            .map_err(|e| network_err(e, url))?;
        let status = resp.status().as_u16();
        log::info!("GET {} → {status}", mask_url(url));
        if let Some(err) = classify_status(status) {
            if status == 429 {
                self.trigger_global_pause();
            }
            return Err(err);
        }
        Ok(resp)
    }

    /// GET `{BASE_URL}{path}` 并解析 JSON，返回 body 字段（无 body 时返回整个对象）。
    /// JSON 解析与 error/body 提取在重试管线之外（对齐 Python：解析失败不重试）。
    pub async fn get_json(&self, path: &str) -> Result<Value, PixivError> {
        let url = format!("{BASE_URL}{path}");
        let text = self
            .run_gated(|| async {
                let resp = self.send_ajax(&url).await?;
                let text = resp.text().await.map_err(|e| network_err(e, &url))?;
                Ok::<String, PixivError>(text)
            })
            .await?;
        let body: Value = serde_json::from_str(&text)
            .map_err(|e| PixivError::Client(format!("Pixiv 响应不是有效 JSON: {e}")))?;
        extract_ajax_body(body)
    }

    /// POST `{BASE_URL}{path}`（JSON body + `x-csrf-token` 头，镜像 get_json 的
    /// cookie/闸门/重试/限速/error-body 语义；浏览层 street 接口专用）。
    ///
    /// - `csrf_token`：来自主站 `__NEXT_DATA__` 的会话令牌（browse_api 层缓存）。
    ///   wreq 的 `.header()` 为替换语义，会覆盖默认 header 里的旧 x-csrf-token。
    /// - JSON 解析与 error/body 提取在重试管线之外（对齐 get_json）。
    pub async fn post_json(
        &self,
        path: &str,
        csrf_token: &str,
        body: &Value,
    ) -> Result<Value, PixivError> {
        let url = format!("{BASE_URL}{path}");
        let payload = serde_json::to_vec(body)
            .map_err(|e| PixivError::Client(format!("请求体序列化失败: {e}")))?;
        let text = self
            .run_gated(|| async {
                let mut req = self
                    .http
                    .post(&url)
                    .header("content-type", "application/json")
                    .header("x-csrf-token", csrf_token);
                if let Some(cookie) = self.cookie_header.as_deref() {
                    req = req.header("cookie", cookie);
                }
                let resp = req
                    .body(payload.clone())
                    .send()
                    .await
                    .map_err(|e| network_err(e, &url))?;
                let status = resp.status().as_u16();
                log::info!("POST {} → {status}", mask_url(&url));
                if let Some(err) = classify_status(status) {
                    if status == 429 {
                        self.trigger_global_pause();
                    }
                    return Err(err);
                }
                let text = resp.text().await.map_err(|e| network_err(e, &url))?;
                Ok::<String, PixivError>(text)
            })
            .await?;
        let body: Value = serde_json::from_str(&text)
            .map_err(|e| PixivError::Client(format!("Pixiv 响应不是有效 JSON: {e}")))?;
        extract_ajax_body(body)
    }

    /// POST `{BASE_URL}{path}` form 体（`application/x-www-form-urlencoded`），
    /// 镜像 post_json 的 cookie/闸门/重试/限速语义。收藏删除接口专用：
    /// - 插画 `/ajax/illusts/bookmarks/delete`：ajax 端点，`csrf_token` 传 Some
    ///   附加 `x-csrf-token` 头，响应为 JSON（解析交调用方走 extract_ajax_body）；
    /// - 小说 `/novel/bookmark_setting.php`：旧式表单端点，token 放表单 `tt`
    ///   字段，`csrf_token` 传 None 不加头；成功以 302 跳转表示（wreq 自动跟随
    ///   重定向，最终落 200 HTML）。因此本方法对 2xx/3xx 一律放行，返回响应
    ///   文本，是否按 JSON 解析由调用方决定。
    pub async fn post_form(
        &self,
        path: &str,
        csrf_token: Option<&str>,
        form: &str,
    ) -> Result<String, PixivError> {
        let url = format!("{BASE_URL}{path}");
        let text = self
            .run_gated(|| async {
                let mut req = self
                    .http
                    .post(&url)
                    .header("content-type", "application/x-www-form-urlencoded");
                if let Some(token) = csrf_token {
                    req = req.header("x-csrf-token", token);
                }
                if let Some(cookie) = self.cookie_header.as_deref() {
                    req = req.header("cookie", cookie);
                }
                let resp = req
                    .body(form.to_string())
                    .send()
                    .await
                    .map_err(|e| network_err(e, &url))?;
                let status = resp.status().as_u16();
                log::info!("POST {} → {status}", mask_url(&url));
                // 2xx/3xx 放行（3xx = 旧式表单端点的成功跳转，wreq 会跟随到最终页）
                if !(200..400).contains(&status) {
                    if status == 429 {
                        self.trigger_global_pause();
                    }
                    return Err(classify_status(status)
                        .unwrap_or_else(|| PixivError::Client(format!("HTTP {status}"))));
                }
                let text = resp.text().await.map_err(|e| network_err(e, &url))?;
                Ok::<String, PixivError>(text)
            })
            .await?;
        Ok(text)
    }

    /// 下载二进制（i.pximg.net 原图 / ugoira zip）。额外带
    /// `Referer: https://www.pixiv.net/`（pximg 防盗链校验）。
    /// 与 get_json 共用限速/重试/429 暂停。空 body → Client 错误。
    pub async fn download_bytes(&self, url: &str) -> Result<Vec<u8>, PixivError> {
        let bytes = self
            .run_gated(|| async {
                let resp = self.send_download(url).await?;
                let bytes = resp.bytes().await.map_err(|e| network_err(e, url))?;
                Ok::<Vec<u8>, PixivError>(bytes.to_vec())
            })
            .await?;
        if bytes.is_empty() {
            return Err(PixivError::Client(format!("响应无内容: {}", mask_url(url))));
        }
        Ok(bytes)
    }

    /// 登录态探测专用：GET JSON 但保留 HTTP 状态码（csrf.rs 需要按精确状态码
    /// 分类 Invalid/Csrf，不能走 get_json 的 Auth/NotFound 映射）。
    /// 任何 HTTP 响应（含 4xx/5xx）都作为 Ok 返回 `(status, 解析后的 JSON)`，
    /// 只有网络错误走重试管线。
    pub(crate) async fn get_json_probe(
        &self,
        path: &str,
    ) -> Result<(u16, Option<Value>), PixivError> {
        let url = format!("{BASE_URL}{path}");
        let (status, text) = self
            .run_gated(|| async {
                let mut req = self.http.get(&url);
                if let Some(cookie) = self.cookie_header.as_deref() {
                    req = req.header("cookie", cookie);
                }
                let resp = req.send().await.map_err(|e| network_err(e, &url))?;
                let status = resp.status().as_u16();
                log::info!("GET {} → {status}", mask_url(&url));
                let text = resp.text().await.unwrap_or_default();
                Ok::<(u16, String), PixivError>((status, text))
            })
            .await?;
        Ok((status, serde_json::from_str(&text).ok()))
    }
}

// ----------------------------------------------------------------------
// 单元测试（全部离线，不发真实网络）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> PixivClient {
        PixivClient::new(&HashMap::new()).expect("wreq client 构造不应触网")
    }

    #[test]
    fn classify_status_branch_table() {
        // 成功：仅精确 200（对齐 Python）
        assert!(classify_status(200).is_none());
        // 认证 / 不存在 / 限流
        assert!(matches!(classify_status(401), Some(PixivError::Auth)));
        assert!(matches!(classify_status(403), Some(PixivError::Auth)));
        assert!(matches!(classify_status(404), Some(PixivError::NotFound)));
        assert!(matches!(classify_status(429), Some(PixivError::RateLimit)));
        // 5xx
        for status in [500, 502, 503, 599] {
            assert!(matches!(classify_status(status), Some(PixivError::Server)));
        }
        // 其他非 200（含 2xx 其他码）→ 可重试 Client
        assert!(matches!(classify_status(400), Some(PixivError::Client(_))));
        assert!(matches!(classify_status(409), Some(PixivError::Client(_))));
        assert!(matches!(classify_status(201), Some(PixivError::Client(_))));
        match classify_status(418) {
            Some(PixivError::Client(msg)) => assert_eq!(msg, "HTTP 418"),
            other => panic!("预期 Client 错误，实际 {other:?}"),
        }
    }

    #[test]
    fn json_truthy_python_semantics() {
        for falsy in [
            Value::Null,
            Value::Bool(false),
            serde_json::json!(0),
            serde_json::json!(""),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            assert!(!json_truthy(&falsy), "{falsy} 应为假值");
        }
        for truthy in [
            Value::Bool(true),
            serde_json::json!(1),
            serde_json::json!(-1),
            serde_json::json!("x"),
            serde_json::json!([0]),
            serde_json::json!({"a": 1}),
        ] {
            assert!(json_truthy(&truthy), "{truthy} 应为真值");
        }
    }

    #[test]
    fn extract_ajax_body_rules() {
        // 正常：error=false + body → 提取 body
        let body = serde_json::json!({"error": false, "message": "", "body": {"id": 1}});
        assert_eq!(
            extract_ajax_body(body).unwrap(),
            serde_json::json!({"id": 1})
        );
        // 无 error 键、无 body 键 → 返回整个对象
        let whole = serde_json::json!({"userData": {}, "token": "t"});
        assert_eq!(extract_ajax_body(whole.clone()).unwrap(), whole);
        // error 真值 + message → Client("API error: ...")
        match extract_ajax_body(serde_json::json!({"error": true, "message": "限流"})) {
            Err(PixivError::Client(msg)) => assert_eq!(msg, "API error: 限流"),
            other => panic!("预期 Client 错误，实际 {other:?}"),
        }
        // Python 假值语义：error=null / "" / 0 / [] 都不算错误
        for error in [
            Value::Null,
            serde_json::json!(""),
            serde_json::json!(0),
            serde_json::json!([]),
        ] {
            let body = serde_json::json!({"error": error, "body": "ok"});
            assert!(extract_ajax_body(body).is_ok(), "error={error} 应为成功");
        }
        // error 真值但缺 message → 空消息
        match extract_ajax_body(serde_json::json!({"error": {"code": 1}})) {
            Err(PixivError::Client(msg)) => assert_eq!(msg, "API error: "),
            other => panic!("预期 Client 错误，实际 {other:?}"),
        }
    }

    #[test]
    fn new_splits_csrf_and_cookie_header() {
        let mut cookies = HashMap::new();
        cookies.insert("PHPSESSID".to_string(), "12345_abc".to_string());
        cookies.insert("x-csrf-token".to_string(), "tok".to_string());
        cookies.insert("device_token".to_string(), "dv".to_string());
        let c = PixivClient::new(&cookies).unwrap();
        // csrf token 进默认 header
        assert_eq!(
            c.default_headers
                .get("x-csrf-token")
                .and_then(|v| v.to_str().ok()),
            Some("tok")
        );
        // Cookie 头含 PHPSESSID / device_token，不含 x-csrf-token
        let cookie = c.cookie_header.expect("应有 Cookie 头");
        assert!(cookie.contains("PHPSESSID=12345_abc"), "cookie={cookie}");
        assert!(cookie.contains("device_token=dv"), "cookie={cookie}");
        assert!(!cookie.contains("csrf"), "cookie={cookie}");
    }

    #[test]
    fn new_without_cookies_has_no_cookie_header() {
        let c = PixivClient::new(&HashMap::new()).unwrap();
        assert!(c.cookie_header.is_none());
        assert!(c.default_headers.get("x-csrf-token").is_none());
        assert!(c.default_headers.get("user-agent").is_some());
    }

    #[tokio::test]
    async fn rate_limit_sets_pause_gate() {
        let c = client();
        assert!(!*c.pause_gate.borrow(), "初始不应暂停");
        c.trigger_global_pause();
        assert!(*c.pause_gate.borrow(), "429 后闸门应为暂停态");
        // 手动恢复（测试不等 60s 定时器；无接收者时必须 send_replace 才落值）
        c.pause_gate.send_replace(false);
        assert!(!*c.pause_gate.borrow());
    }

    #[tokio::test]
    async fn run_gated_retries_then_succeeds() {
        let c = client();
        let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let shared = counter.clone();
        let result: Result<u32, PixivError> = c
            .run_gated(move || {
                let counter = shared.clone();
                async move {
                    let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    if n < 3 {
                        Err(PixivError::Server)
                    } else {
                        Ok(n)
                    }
                }
            })
            .await;
        assert_eq!(result.unwrap(), 3);
        assert_eq!(
            counter.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "失败两次后第三次成功"
        );
    }

    #[tokio::test]
    async fn run_gated_auth_aborts_immediately() {
        let c = client();
        let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let shared = counter.clone();
        let result: Result<(), PixivError> = c
            .run_gated(move || {
                let counter = shared.clone();
                async move {
                    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Err(PixivError::Auth)
                }
            })
            .await;
        assert!(matches!(result, Err(PixivError::Auth)));
        assert_eq!(
            counter.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "401/403 不应重试"
        );
    }

    #[tokio::test]
    async fn run_gated_exhausts_retries_with_last_error() {
        let c = client();
        let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let shared = counter.clone();
        let result: Result<(), PixivError> = c
            .run_gated(move || {
                let counter = shared.clone();
                async move {
                    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Err(PixivError::Server)
                }
            })
            .await;
        assert!(matches!(result, Err(PixivError::Server)));
        assert_eq!(
            counter.load(std::sync::atomic::Ordering::SeqCst),
            MAX_RETRIES
        );
    }

    #[tokio::test]
    async fn run_gated_rate_limit_does_not_retry() {
        let c = client();
        let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let shared = counter.clone();
        let result: Result<(), PixivError> = c
            .run_gated(move || {
                let counter = shared.clone();
                async move {
                    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Err(PixivError::RateLimit)
                }
            })
            .await;
        assert!(matches!(result, Err(PixivError::RateLimit)));
        assert_eq!(
            counter.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "429 当次抛出、不重试"
        );
    }

    #[tokio::test]
    async fn wait_not_paused_returns_when_resumed() {
        let c = client();
        let _ = c.pause_gate.send(true);
        // 100ms 后恢复
        let gate = c.pause_gate.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = gate.send(false);
        });
        c.wait_not_paused().await;
        assert!(!*c.pause_gate.borrow());
    }
}
