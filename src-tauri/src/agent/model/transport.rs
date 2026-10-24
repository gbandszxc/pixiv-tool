//! wreq → Rig 传输层。
//!
//! Rig 的 provider 只负责协议编解码，真正发包由 [`HttpClientExt`] 实现承担。
//! 这里把 wreq（项目既有的 Chrome 指纹客户端）接到 Rig：
//!
//! - **端点固定**：Rig 会按自己的习惯给基址追加 `/v1/...` 后缀；本项目允许用户
//!   配置任意 OpenAI/Anthropic 兼容网关，因此每个请求的 URI 路径整体改写为
//!   [`ProviderConfig::endpoint`](crate::agent::model::providers::ProviderConfig)
//!   推导出的端点，只保留 scheme/host/port/path，丢弃 Rig 追加的部分。
//! - **请求头叠加**：保留 Rig 生成的 Authorization / x-api-key / anthropic-version /
//!   content-type，再叠加调用方静态头（如 `x-opencode-session`）。
//! - **全局请求闸门**：可注入 [`tokio::sync::Semaphore`]，许可只在单次 HTTP 请求
//!   期间持有；非流式请求覆盖到响应体读完，流式请求由返回的 stream 持有到流结束
//!   或调用方 drop。future 被取消时 `OwnedSemaphorePermit` 随 future drop 释放。
//! - **超时与重定向**：由调用方配置的 [`wreq::Client`] 决定，本层不另设策略。
//! - **错误脱敏**：传输错误只保留脱敏后的类别文本，不带 URL 查询、Key、Cookie；
//!   非 2xx 保留 status/headers 与 ≤64 KiB 响应体供 Rig 的错误路径判断。

use std::{future::Future, sync::Arc};

use bytes::Bytes;
use futures::{StreamExt, stream};
use http::header::HeaderName;
use rig_core::http_client::{
    BoxedStream, Error as HttpError, HeaderMap, HeaderValue, HttpClientExt, LazyBody,
    MultipartForm, Request, Response, StreamingResponse,
};
use rig_core::wasm_compat::WasmCompatSend;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// 非 2xx 响应最多保留的正文长度。Rig 的错误路径会读取它做诊断，但完整请求体
/// 与无界响应体都不应进内存或日志。
const MAX_ERROR_BODY: usize = 64 * 1024;

/// 基于 wreq 的 Rig HTTP 传输。可克隆：provider 会把它交给 `DynHttpClient`，
/// 每次请求共享同一个 wreq 连接池与闸门。
#[derive(Clone)]
pub struct WreqTransport {
    client: wreq::Client,
    /// 用户配置推导出的最终端点（含完整 path）。
    endpoint: tauri::Url,
    /// 调用方静态头（例如 opencode 会话标识）。
    static_headers: HeaderMap,
    /// 全局模型请求闸门；`None` 表示不限流。
    gate: Option<Arc<Semaphore>>,
}

impl WreqTransport {
    /// 用已配置好的 wreq client 与最终端点构造传输。
    pub fn new(client: wreq::Client, endpoint: tauri::Url) -> Self {
        Self {
            client,
            endpoint,
            static_headers: HeaderMap::new(),
            gate: None,
        }
    }

    /// 叠加一个静态请求头；同名时覆盖 Rig 生成的头。
    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.static_headers.insert(name, value);
        self
    }

    /// 注入全局请求闸门。许可只在单次请求期间持有。
    pub fn with_gate(mut self, gate: Option<Arc<Semaphore>>) -> Self {
        self.gate = gate;
        self
    }

    /// 最终端点（供上游诊断/断言使用；不要把它当基址再拼后缀）。
    pub fn endpoint(&self) -> &tauri::Url {
        &self.endpoint
    }
}

impl HttpClientExt for WreqTransport {
    fn send<T, U>(
        &self,
        req: Request<T>,
    ) -> impl Future<Output = Result<Response<LazyBody<U>>, HttpError>> + WasmCompatSend + 'static
    where
        T: Into<Bytes> + WasmCompatSend,
        U: From<Bytes> + WasmCompatSend + 'static,
    {
        let client = self.client.clone();
        let endpoint = self.endpoint.clone();
        let static_headers = self.static_headers.clone();
        let gate = self.gate.clone();
        let (parts, body) = req.into_parts();
        let body: Bytes = body.into();

        async move {
            // 非流式请求：许可覆盖「建立响应 + 读完响应体」，函数返回时释放。
            let _permit = acquire(&gate).await?;
            let response = send_once(&client, &endpoint, parts, &static_headers, body).await?;
            let status = response.status();
            let headers = response.headers().clone();
            if !status.is_success() {
                let body = read_error_body(response).await;
                return Err(HttpError::non_success_with_details(status, headers, body));
            }
            let bytes = response.bytes().await.map_err(transport_error)?;
            let lazy: LazyBody<U> = Box::pin(async move { Ok(U::from(bytes)) });
            let mut response = Response::new(lazy);
            *response.status_mut() = status;
            *response.headers_mut() = headers;
            Ok(response)
        }
    }

    fn send_multipart<U>(
        &self,
        _req: Request<MultipartForm>,
    ) -> impl Future<Output = Result<Response<LazyBody<U>>, HttpError>> + WasmCompatSend + 'static
    where
        U: From<Bytes> + WasmCompatSend + 'static,
    {
        // Agent / 翻译只发 JSON；音频、图片上传不在本功能的依赖范围内。
        async move {
            Err(unsupported_error(
                "multipart requests are not supported by the wreq transport",
            ))
        }
    }

    fn send_streaming<T>(
        &self,
        req: Request<T>,
    ) -> impl Future<Output = Result<StreamingResponse, HttpError>> + WasmCompatSend
    where
        T: Into<Bytes> + WasmCompatSend,
    {
        let client = self.client.clone();
        let endpoint = self.endpoint.clone();
        let static_headers = self.static_headers.clone();
        let gate = self.gate.clone();
        let (parts, body) = req.into_parts();
        let body: Bytes = body.into();

        async move {
            let permit = acquire(&gate).await?;
            let response = send_once(&client, &endpoint, parts, &static_headers, body).await?;
            let status = response.status();
            let headers = response.headers().clone();
            if !status.is_success() {
                let body = read_error_body(response).await;
                return Err(HttpError::non_success_with_details(status, headers, body));
            }
            // 许可进入 stream 状态：流读完自然释放；调用方 drop stream 时
            // 连同 wreq 响应体一起 drop，连接关闭、许可立即归还。
            let inner = response.bytes_stream();
            let body = stream::unfold((inner, permit), |(mut inner, permit)| async move {
                match inner.next().await {
                    Some(Ok(bytes)) => Some((Ok(bytes), (inner, permit))),
                    Some(Err(error)) => Some((Err(transport_error(error)), (inner, permit))),
                    None => None,
                }
            });
            let body: BoxedStream = Box::pin(body);
            let mut response = Response::new(body);
            *response.status_mut() = status;
            *response.headers_mut() = headers;
            Ok(response)
        }
    }
}

/// 获取闸门许可。`acquire_owned` 在 future 被取消时会移除等待者，不会泄漏许可。
async fn acquire(gate: &Option<Arc<Semaphore>>) -> Result<Option<OwnedSemaphorePermit>, HttpError> {
    match gate {
        Some(gate) => gate
            .clone()
            .acquire_owned()
            .await
            .map(Some)
            .map_err(|_| HttpError::instance(std::io::Error::other("request gate closed"))),
        None => Ok(None),
    }
}

/// 把 Rig 的请求改写为「用户配置端点 + 原有方法/头 + 静态头」后交给 wreq。
async fn send_once(
    client: &wreq::Client,
    endpoint: &tauri::Url,
    mut parts: http::request::Parts,
    static_headers: &HeaderMap,
    body: Bytes,
) -> Result<wreq::Response, HttpError> {
    let uri: http::Uri = endpoint.as_str().parse().map_err(|_| invalid_endpoint())?;
    // 只保留配置端点的 scheme/host/port/path；Rig 追加的 path 与查询全部丢弃。
    for (name, value) in static_headers {
        parts.headers.insert(name.clone(), value.clone());
    }
    let mut request = wreq::Request::new(parts.method, uri);
    *request.headers_mut() = parts.headers;
    *request.body_mut() = Some(wreq::Body::from(body));
    client.execute(request).await.map_err(transport_error)
}

/// 有界读取非 2xx 正文：最多 [`MAX_ERROR_BODY`]，超限即截断。
async fn read_error_body(response: wreq::Response) -> String {
    let mut body = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(next) = chunks.next().await {
        let Ok(chunk) = next else { break };
        let remaining = MAX_ERROR_BODY.saturating_sub(body.len());
        if remaining == 0 {
            break;
        }
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            break;
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8_lossy(&body).into_owned()
}

/// 传输错误映射：只保留脱敏文本，绝不回显 URL 查询、Key 或 Cookie。
fn transport_error(error: wreq::Error) -> HttpError {
    let message = crate::translation_diagnostics::redact(&error.to_string(), "");
    HttpError::instance(std::io::Error::other(message))
}

fn invalid_endpoint() -> HttpError {
    HttpError::instance(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "invalid model endpoint",
    ))
}

fn unsupported_error(message: &str) -> HttpError {
    HttpError::instance(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        message.to_owned(),
    ))
}
