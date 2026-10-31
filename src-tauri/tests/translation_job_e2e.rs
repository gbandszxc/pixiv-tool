//! 整篇翻译 Job 的离线端到端验收。
//!
//! 全部走本机脚本化 HTTP mock：不访问真实模型 / Pixiv，不需要 Key。mock 在每次请求
//! 里读回「已经发生的工具调用」，据此决定下一步该调用哪个工具，从而驱动主 Agent
//! 完成 Pass 0～3、整体复核、合并与发布；翻译 / 审查子 Agent 由同一套脚本按角色
//! 分支驱动。
//!
//! 覆盖：
//! 1. 全流程完成（页行映射、合并稿规则、manifest、事件序号、draft/review 产物）；
//! 2. 重复启动加入同一 Job；
//! 3. 暂停 → 恢复从检查点继续（已完成块不重复请求）；
//! 4. 确认模式 awaiting_confirmation → confirm 后继续；
//! 5. 取消立即停止，不产生新请求；
//! 6. 清理缓存后世代递增、旧世代写入被拒且不重建目录；
//! 7. 损坏 manifest 标记 corrupt，不自动请求；
//! 8. 删除整个 Job 目录；
//! 9. 断线（连接中断）、输出截断（缩块重做）、工具参数错误三条失败路径。

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use pixiv_tool_lib::{
    agent::{CacheGeneration, JobScope, JobState, RecordingSink, storage::JobStore},
    paths::AppPaths,
    settings::Settings,
    translation::{
        NovelInput,
        artifacts::{
            Chunk, Draft, Issue, JobArtifacts, PublishStatus, Review, ReviewVerdict, Severity,
        },
        service::{ConstraintEdit, ConstraintTermEdit, StoreIo, TranslationService},
        source::{SegmentKind, parse_source},
    },
};
use rig_core::serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

// ---------------------------------------------------------------------------
// mock HTTP 服务
// ---------------------------------------------------------------------------

/// 一次 mock 响应。
enum Reply {
    /// 逐段写出 SSE 体后关闭。
    Sse(Vec<Vec<u8>>),
    /// 一次性 JSON 响应（非流式请求，如上下文压缩摘要）。
    Json(Value),
    /// 非 2xx：状态码 + JSON 正文。
    Error(u16, Value),
    /// 只发响应头就断开（模拟连接中断 / 流截断）。
    Abort,
    /// 建立响应后持续 keepalive，直到客户端断开（模拟长时间挂起）。
    Hang,
    /// 保持连接不响应，直到测试闸门放行后按同一请求重新计算回复。
    Gated { gate: Arc<Gate>, body: Value },
}

type Handler = Arc<dyn Fn(&Value) -> Reply + Send + Sync>;

/// 测试闸门：默认关闭；`open()` 后放行所有等待中的请求。
#[derive(Default)]
struct Gate {
    opened: AtomicBool,
}

impl Gate {
    fn new() -> Self {
        Self::default()
    }

    fn open(&self) {
        self.opened.store(true, Ordering::SeqCst);
    }

    fn is_open(&self) -> bool {
        self.opened.load(Ordering::SeqCst)
    }
}

struct MockServer {
    address: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl MockServer {
    async fn start(handler: Handler) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let handler = handler.clone();
                let recorded = recorded.clone();
                tokio::spawn(async move {
                    let Some(body) = read_json_request(&mut socket).await else {
                        return;
                    };
                    recorded.lock().unwrap().push(body.clone());
                    let mut reply = handler(&body);
                    // 闸门关闭时先不响应；放行后按同一请求重新计算回复。
                    loop {
                        match reply {
                            Reply::Gated { gate, body: retry } => {
                                while !gate.is_open() {
                                    tokio::time::sleep(Duration::from_millis(5)).await;
                                }
                                reply = handler(&retry);
                            }
                            ready => {
                                write_reply(&mut socket, ready).await;
                                break;
                            }
                        }
                    }
                });
            }
        });
        Self { address, requests }
    }

    fn base(&self) -> String {
        format!("http://{}/v1", self.address)
    }

    fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }

    /// 翻译子 Agent 请求数（按块统计；user prompt 含 `chunk_id: cXXXX`）。
    fn translator_requests_for(&self, chunk_id: &str) -> usize {
        self.requests()
            .iter()
            .filter(|body| {
                role_of_request(body) == "translator"
                    && user_of(body).contains(&format!("chunk_id: {chunk_id}"))
            })
            .count()
    }

    fn translator_requests(&self) -> usize {
        self.requests()
            .iter()
            .filter(|body| role_of_request(body) == "translator")
            .count()
    }

    /// 翻译子 Agent 的运行次数（一次派发 = 一段新会话）。
    ///
    /// 子 Agent 一次运行会发多个请求（读块 / 提交 / 收尾），第一个请求还没有任何
    /// 工具调用，用它精确统计「派发了几次」，比裸请求数更能证明没有整块重译。
    fn translator_runs_for(&self, chunk_id: &str) -> usize {
        self.requests()
            .iter()
            .filter(|body| {
                role_of_request(body) == "translator"
                    && user_of(body).contains(&format!("chunk_id: {chunk_id}"))
                    && !body["messages"].as_array().is_some_and(|messages| {
                        messages.iter().any(|message| {
                            message["tool_calls"]
                                .as_array()
                                .is_some_and(|calls| !calls.is_empty())
                        })
                    })
            })
            .count()
    }

    /// 请求体含指定原文标记的次数（用于区分两本小说的实际处理）。
    fn requests_containing(&self, needle: &str) -> usize {
        self.requests()
            .iter()
            .filter(|body| body.to_string().contains(needle))
            .count()
    }
}

async fn read_json_request(socket: &mut TcpStream) -> Option<Value> {
    let mut raw = Vec::new();
    let (header_end, content_length) = loop {
        let mut buffer = [0u8; 4096];
        let size = socket.read(&mut buffer).await.ok()?;
        if size == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..size]);
        if let Some(end) = raw.windows(4).position(|part| part == b"\r\n\r\n") {
            let header_end = end + 4;
            let headers = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .unwrap_or("0")
                .trim()
                .parse::<usize>()
                .ok()?;
            break (header_end, content_length);
        }
    };
    while raw.len() < header_end + content_length {
        let mut buffer = [0u8; 4096];
        let size = socket.read(&mut buffer).await.ok()?;
        if size == 0 {
            return None;
        }
        raw.extend_from_slice(&buffer[..size]);
    }
    serde_json::from_slice(&raw[header_end..header_end + content_length]).ok()
}

async fn write_sse(socket: &mut TcpStream, chunks: &[Vec<u8>]) {
    if socket
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
              Cache-Control: no-cache\r\nConnection: close\r\n\r\n",
        )
        .await
        .is_err()
    {
        return;
    }
    for chunk in chunks {
        if socket.write_all(chunk).await.is_err() {
            return;
        }
        let _ = socket.flush().await;
    }
}

async fn write_json(socket: &mut TcpStream, body: &Value) {
    let body = body.to_string();
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    if socket.write_all(header.as_bytes()).await.is_err() {
        return;
    }
    let _ = socket.write_all(body.as_bytes()).await;
    let _ = socket.flush().await;
}

async fn write_error(socket: &mut TcpStream, status: u16, body: &Value) {
    let body = body.to_string();
    let _ = socket
        .write_all(
            format!(
                "HTTP/1.1 {status} Error\r\nContent-Type: application/json\r\n\
                 Connection: close\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            )
            .as_bytes(),
        )
        .await;
    let _ = socket.flush().await;
}

async fn write_reply(socket: &mut TcpStream, reply: Reply) {
    match reply {
        Reply::Sse(chunks) => write_sse(socket, &chunks).await,
        Reply::Json(body) => write_json(socket, &body).await,
        Reply::Error(status, body) => write_error(socket, status, &body).await,
        Reply::Abort => {
            let _ = socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                      Connection: close\r\n\r\n",
                )
                .await;
            let _ = socket.flush().await;
            // 直接关闭：客户端读到 EOF 而没有结束帧。
        }
        Reply::Hang => {
            let _ = socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                      Connection: close\r\n\r\n",
                )
                .await;
            let _ = socket.write_all(b": ready\n\n").await;
            let _ = socket.flush().await;
            for _ in 0..3000 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                if socket.write_all(b": keepalive\n\n").await.is_err() {
                    return;
                }
                let _ = socket.flush().await;
            }
        }
        // 闸门在 accept 循环里消化，不会走到写出阶段。
        Reply::Gated { .. } => {}
    }
}

// ---------------------------------------------------------------------------
// 协议分片
// ---------------------------------------------------------------------------

fn chat_chunk(delta: Value, finish: Option<&str>, usage: Option<Value>) -> Vec<u8> {
    let mut chunk = json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "mock-model",
        "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
    });
    if let Some(usage) = usage {
        chunk["usage"] = usage;
    }
    format!("data: {chunk}\n\n").into_bytes()
}

fn tool_call_stream(call_id: &str, tool: &str, arguments: &str) -> Reply {
    Reply::Sse(vec![
        chat_chunk(
            json!({"role": "assistant", "tool_calls": [{
                "index": 0,
                "id": call_id,
                "type": "function",
                "function": {"name": tool, "arguments": arguments}
            }]}),
            None,
            None,
        ),
        chat_chunk(json!({}), Some("tool_calls"), None),
        b"data: [DONE]\n\n".to_vec(),
    ])
}

/// 非流式完成请求（上下文压缩摘要）返回一次性 JSON。
fn json_reply(text: &str) -> Reply {
    Reply::Json(json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "created": 0,
        "model": "mock-model",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 100, "completion_tokens": 30, "total_tokens": 130}
    }))
}

fn text_stream(text: &str) -> Reply {
    Reply::Sse(vec![
        chat_chunk(json!({"role": "assistant", "content": text}), None, None),
        chat_chunk(
            json!({}),
            Some("stop"),
            Some(json!({"prompt_tokens": 20, "completion_tokens": 5, "total_tokens": 25})),
        ),
        b"data: [DONE]\n\n".to_vec(),
    ])
}

/// 角色判定：用角色文档标题，避免被协调者正文里的同名提法误命中。
/// 子 Agent 的系统提示词只含基础翻译提示，角色契约随首条用户消息注入；
/// 协调者的角色契约仍在系统提示词里——两处合并判定。
fn role_of_request(body: &Value) -> &'static str {
    let mut text = system_of(body);
    text.push('\n');
    text.push_str(&user_of(body));
    if text.contains("# 角色：翻译子 Agent（translator）") {
        "translator"
    } else if text.contains("# 角色：审查子 Agent（reviewer）") {
        "reviewer"
    } else {
        "main"
    }
}

/// 把消息 content 归一成纯文本（Rig 会编码成分段数组）。
fn content_text(message: &Value) -> String {
    match &message["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join(
                "
",
            ),
        _ => String::new(),
    }
}

/// 从请求体取系统提示（首条 system 消息的全部文本）。
fn system_of(body: &Value) -> String {
    body["messages"]
        .as_array()
        .map(|messages| {
            messages
                .iter()
                .filter(|message| message["role"] == "system")
                .map(content_text)
                .collect::<Vec<_>>()
                .join(
                    "
",
                )
        })
        .unwrap_or_default()
}

fn user_of(body: &Value) -> String {
    body["messages"]
        .as_array()
        .map(|messages| {
            messages
                .iter()
                .filter(|message| message["role"] == "user")
                .filter_map(|message| message["content"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// 已发生的工具调用（名称, 参数）与工具结果（按工具名归并，取最后一次）。
fn tool_history(body: &Value) -> (Vec<(String, Value)>, BTreeMap<String, Value>) {
    let mut calls = Vec::new();
    let mut call_names: BTreeMap<String, String> = BTreeMap::new();
    let mut results = BTreeMap::new();
    if let Some(messages) = body["messages"].as_array() {
        for message in messages {
            if let Some(list) = message["tool_calls"].as_array() {
                for call in list {
                    let name = call["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let args = call["function"]["arguments"]
                        .as_str()
                        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                        .unwrap_or(Value::Null);
                    if let Some(id) = call["id"].as_str() {
                        call_names.insert(id.to_string(), name.clone());
                    }
                    calls.push((name, args));
                }
            }
            if message["role"] == "tool"
                && let Some(id) = message["tool_call_id"].as_str()
                && let Some(name) = call_names.get(id)
            {
                let raw = content_text(message);
                let content = serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null);
                results.insert(name.clone(), content);
            }
        }
    }
    (calls, results)
}

fn called(calls: &[(String, Value)], name: &str) -> bool {
    calls.iter().any(|(tool, _)| tool == name)
}

fn call_count(calls: &[(String, Value)], name: &str) -> usize {
    calls.iter().filter(|(tool, _)| tool == name).count()
}

// ---------------------------------------------------------------------------
// 脚本化「模型」
// ---------------------------------------------------------------------------

/// 主 Agent 的下一步（只依赖会话里已发生的工具调用与结果）。
fn main_next(calls: &[(String, Value)], results: &BTreeMap<String, Value>) -> Reply {
    let chunk_ids: Vec<String> = results
        .get("prepare_source")
        .and_then(|value| value["chunk_ids"].as_array())
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    if !called(calls, "prepare_source") {
        return tool_call_stream("m1", "prepare_source", "{}");
    }

    // Pass 1：逐块 read_chunk + submit_scan。
    let scanned: Vec<String> = calls
        .iter()
        .filter(|(tool, _)| tool == "submit_scan")
        .filter_map(|(_, args)| args["chunk_id"].as_str().map(str::to_string))
        .collect();
    for chunk_id in &chunk_ids {
        if !scanned.contains(chunk_id) {
            let read = calls.iter().any(|(tool, args)| {
                tool == "read_chunk" && args["chunk_id"].as_str() == Some(chunk_id.as_str())
            });
            if !read {
                return tool_call_stream(
                    "m2",
                    "read_chunk",
                    &json!({"chunk_id": chunk_id, "include_context": true}).to_string(),
                );
            }
            let scan = json!({
                "chunk_id": chunk_id,
                "terms": [{
                    "source_term": "勇者",
                    "locked_translation": "勇士",
                    "term_type": "character",
                    "notes": "主角称谓",
                }],
                "style": {"text_type": "小说", "formality": "书面语，对白偏口语"},
                "notes": "叙述视角为第三人称",
                "confidence": "high",
            });
            return tool_call_stream("m3", "submit_scan", &scan.to_string());
        }
    }

    if !called(calls, "freeze_constraints") {
        return tool_call_stream("m4", "freeze_constraints", "{}");
    }

    // Pass 2 / 3：用 job_status 的待办列表自适应（细分会改变块集合），最多 4 轮。
    let status_rounds = call_count(calls, "job_status");
    if status_rounds < 4 {
        let status = results.get("job_status").cloned().unwrap_or(Value::Null);
        let pending = |key: &str| -> Vec<String> {
            status[key]
                .as_array()
                .map(|ids| {
                    ids.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        let last_was_status = calls
            .last()
            .map(|(tool, _)| tool == "job_status")
            .unwrap_or(false);
        if !last_was_status {
            return tool_call_stream("m5", "job_status", "{}");
        }
        let pending_translate = pending("pending_translate");
        let pending_review = pending("pending_review");
        if !pending_translate.is_empty() {
            return tool_call_stream(
                "m6",
                "dispatch_translation",
                &json!({"chunk_ids": pending_translate}).to_string(),
            );
        }
        if !pending_review.is_empty() {
            return tool_call_stream(
                "m7",
                "dispatch_review",
                &json!({"chunk_ids": pending_review}).to_string(),
            );
        }
    }

    if !called(calls, "read_review_summary") {
        return tool_call_stream("m9", "read_review_summary", "{}");
    }
    if !called(calls, "merge_drafts") {
        return tool_call_stream("m10", "merge_drafts", "{}");
    }
    if !called(calls, "record_global_review") {
        // 抽样必须用当前计划内的块（细分会改变块集合）。
        let current: Vec<String> = results
            .get("job_status")
            .and_then(|status| status["chunk_ids"].as_array())
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .filter(|ids: &Vec<String>| !ids.is_empty())
            .unwrap_or_else(|| chunk_ids.clone());
        let sample = current.iter().take(8).cloned().collect::<Vec<_>>();
        let review = json!({
            "sample_chunk_ids": if sample.is_empty() { json!(["c0001"]) } else { json!(sample) },
            "notes": "术语、风格、对白、纯净度、标记五项已核",
            "passed": true,
        });
        return tool_call_stream("m11", "record_global_review", &review.to_string());
    }
    if !called(calls, "publish") {
        return tool_call_stream("m12", "publish", "{}");
    }
    text_stream("done")
}

/// 翻译子 Agent：read_chunk → submit_translation → 结束。
fn translator_next(calls: &[(String, Value)], results: &BTreeMap<String, Value>) -> Reply {
    let chunk_id = calls
        .iter()
        .find(|(tool, _)| tool == "read_chunk")
        .and_then(|(_, args)| args["chunk_id"].as_str())
        .map(str::to_string)
        .or_else(|| {
            results
                .get("chunk_id_hint")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "c0001".to_string());
    if !called(calls, "read_chunk") {
        return tool_call_stream(
            "t1",
            "read_chunk",
            &json!({"chunk_id": chunk_id, "include_context": true}).to_string(),
        );
    }
    if !called(calls, "submit_translation") {
        let segments = results
            .get("read_chunk")
            .and_then(|value| value["segments"].as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            !segments.is_empty(),
            "mock 读不到 read_chunk 的 segments：{:?}",
            results.keys().collect::<Vec<_>>()
        );
        let translated: Vec<Value> = segments
            .iter()
            .map(|segment| {
                let id = segment["id"].as_str().unwrap_or_default();
                let text = segment["text"].as_str().unwrap_or_default();
                let kind = segment["kind"].as_str().unwrap_or("text");
                let output = match kind {
                    "markup" => text.to_string(),
                    "chapter" => "[chapter:中文标题]".to_string(),
                    "ruby" => "[rb:汉字>hanzi]呀".to_string(),
                    _ => "这是本段的中文译文".to_string(),
                };
                json!({"id": id, "text": output})
            })
            .collect();
        let submit = json!({"chunk_id": chunk_id, "segments": translated});
        return tool_call_stream("t2", "submit_translation", &submit.to_string());
    }
    text_stream("translated")
}

/// 审查子 Agent：read_chunk → read_translation → submit_review(pass) → 结束。
fn reviewer_next(calls: &[(String, Value)], results: &BTreeMap<String, Value>) -> Reply {
    reviewer_next_with(calls, results, |chunk_id, _version, _translation| {
        json!({
            "chunk_id": chunk_id,
            "verdict": "pass",
            "issues": [],
            "notes": "逐片段对照通过",
        })
    })
}

/// 审查子 Agent 的公共读取序列：先读原文与译文，再由 `verdict_for` 决定结论。
///
/// `verdict_for` 收到（块 ID、当前草稿版本、`read_translation` 的工具结果），
/// 与真实模型一样只依据读到的内容作判断。
fn reviewer_next_with(
    calls: &[(String, Value)],
    results: &BTreeMap<String, Value>,
    verdict_for: impl Fn(&str, u64, &Value) -> Value,
) -> Reply {
    let chunk_id = reviewer_chunk_id(results);
    if !called(calls, "read_chunk") {
        return tool_call_stream(
            "r1",
            "read_chunk",
            &json!({"chunk_id": chunk_id, "include_context": true}).to_string(),
        );
    }
    if !called(calls, "read_translation") {
        return tool_call_stream(
            "r2",
            "read_translation",
            &json!({"chunk_id": chunk_id}).to_string(),
        );
    }
    if !called(calls, "submit_review") {
        let translation = results
            .get("read_translation")
            .cloned()
            .unwrap_or(Value::Null);
        let version = translation["draft_version"].as_u64().unwrap_or(1);
        let review = verdict_for(&chunk_id, version, &translation);
        return tool_call_stream("r3", "submit_review", &review.to_string());
    }
    text_stream("reviewed")
}

/// 本次审查的块 ID：首轮取 user prompt 里的提示，之后取 read_chunk 的结果。
fn reviewer_chunk_id(results: &BTreeMap<String, Value>) -> String {
    results
        .get("chunk_id_hint")
        .and_then(Value::as_str)
        .or_else(|| {
            results
                .get("read_chunk")
                .and_then(|value| value["chunk_id"].as_str())
        })
        .unwrap_or("c0001")
        .to_string()
}

/// 审查点名的片段：优先普通文本（补丁可安全整体替换），否则退回首个片段。
fn review_target_segment(results: &BTreeMap<String, Value>) -> String {
    let picked = results
        .get("read_chunk")
        .and_then(|value| value["segments"].as_array())
        .and_then(|segments| {
            segments
                .iter()
                .find(|segment| segment["kind"] == "text")
                .or_else(|| segments.first())
        });
    picked
        .and_then(|segment| segment["id"].as_str())
        .unwrap_or("p001-l001-s0")
        .to_string()
}

/// 通用脚本：按角色分派；`hint` 从 user prompt 取 chunk_id 供首轮使用。
fn scripted(body: &Value) -> Reply {
    scripted_roles(body, reviewer_next, main_next)
}

/// 与 [`scripted`] 同构，但审查 / 协调者脚本可替换（翻译子 Agent 固定）。
fn scripted_roles(
    body: &Value,
    reviewer: fn(&[(String, Value)], &BTreeMap<String, Value>) -> Reply,
    main: fn(&[(String, Value)], &BTreeMap<String, Value>) -> Reply,
) -> Reply {
    // 上下文压缩的摘要请求不带工具定义：返回一次性 JSON 摘要。
    let has_tools = body["tools"]
        .as_array()
        .is_some_and(|tools| !tools.is_empty());
    if !has_tools {
        return json_reply(
            "1. 目标：完成整篇翻译；2. 约束：已冻结；3. 产物：见块产物；             4. 进度：按检查点；5. 未决：无；6. 下一步：继续派发待办块。",
        );
    }
    let (calls, mut results) = tool_history(body);
    let hint = user_of(body)
        .split("chunk_id: ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .map(|id| id.trim().to_string());
    if role_of_request(body) == "translator" {
        if let Some(id) = hint {
            results.insert("chunk_id_hint".into(), json!(id));
        }
        return translator_next(&calls, &results);
    }
    if role_of_request(body) == "reviewer" {
        if let Some(id) = hint {
            results.insert("chunk_id_hint".into(), json!(id));
        }
        return reviewer(&calls, &results);
    }
    main(&calls, &results)
}

// ---------------------------------------------------------------------------
// 测试夹具
// ---------------------------------------------------------------------------

struct Fixture {
    dir: std::path::PathBuf,
    paths: AppPaths,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(tag: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("pixiv-tj-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    Fixture {
        paths: AppPaths {
            data_dir: dir.clone(),
            config_dir: dir.join("config"),
            logs_dir: dir.join("logs"),
        },
        dir,
    }
}

/// 测试用设置：mock 端点、中文目标、快速退避、充足轮次。
fn settings_for(server: &MockServer) -> Settings {
    // 流程用例不触发压缩（压缩本身由 agent 运行层单测覆盖）。
    Settings {
        translation_api_url: server.base(),
        translation_api_format: "chat_completions".into(),
        translation_model: "mock-model".into(),
        translation_target_language: "zh-CN".into(),
        translation_timeout_seconds: 30,
        translation_extra: json!({}),
        translation_agent_max_turns: 64,
        translation_agent_stall_limit: 32,
        translation_agent_retry_limit: 2,
        translation_agent_backoff_base_ms: 10,
        translation_agent_backoff_max_seconds: 10,
        translation_agent_context_window: 200_000,
        translation_agent_output_cap: 16_384,
        translation_agent_recent_history_tokens: 131_072,
        translation_agent_global_concurrency: 4,
        translation_agent_novel_concurrency: 4,
        ..Settings::default()
    }
}

/// 2 页原文：章节、ruby、图片标记、超长单行（会被拆成多个片段）。
fn novel_text() -> String {
    let long_line = "あ".repeat(2_000);
    format!(
        "[chapter:序章]\n[rb:漢字>かんじ]の勇者が現れた\n[pixivimage:12345]\n{long_line}\n[newpage]\n第二頁の本文です\n終わりの行"
    )
}

fn novel() -> NovelInput {
    NovelInput {
        novel_id: 5150,
        title: "测试小说".into(),
        tags: vec!["测试".into()],
        description: "离线端到端".into(),
        content: novel_text(),
    }
}

/// 两本内容、页数都不同的小说，用于并行与隔离断言。
fn novel_a() -> NovelInput {
    NovelInput {
        novel_id: 6001,
        title: "并行甲".into(),
        tags: vec![],
        description: String::new(),
        content:
            "[chapter:甲章]\n甲の勇者が現れた\n甲の二行目\n[newpage]\n甲の第二頁です\n甲の最後の行"
                .into(),
    }
}

fn novel_b() -> NovelInput {
    NovelInput {
        novel_id: 6002,
        title: "并行乙".into(),
        tags: vec![],
        description: String::new(),
        content: "[chapter:乙章]\n乙の剣士が笑った\n乙の最後の行".into(),
    }
}

/// 多页正文（每页约 1000 字符），用于让主会话历史稳定越过低压缩阈值。
fn novel_compact() -> NovelInput {
    let page = format!("[chapter:圧縮試験]\n{}", "圧縮対象の本文です".repeat(100));
    NovelInput {
        novel_id: 5151,
        title: "压缩测试".into(),
        tags: vec![],
        description: String::new(),
        content: [page.clone(), page.clone(), page.clone(), page].join("\n[newpage]\n"),
    }
}

fn journal_records(paths: &AppPaths, novel_id: i64, job_id: &str) -> Vec<Value> {
    let path = job_dir(paths, novel_id, job_id).join("sessions/journal.jsonl");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

fn service(paths: &AppPaths, settings: Settings, sink: Arc<RecordingSink>) -> TranslationService {
    TranslationService::for_tests(paths.clone(), settings, sink, Some("test-key".into()))
}

/// 等待某 Job 达到指定状态；超时返回最后一次快照。
async fn wait_for(
    service: &TranslationService,
    job_id: &str,
    wanted: &[JobState],
) -> pixiv_tool_lib::translation::service::TranslationJobSnapshot {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let snapshot = service.snapshot(job_id).await.expect("任务快照可读");
        if wanted.contains(&snapshot.state) {
            return snapshot;
        }
        if tokio::time::Instant::now() > deadline {
            return snapshot;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_until(mut check: impl FnMut() -> bool, millis: u64) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(millis);
    loop {
        if check() {
            return true;
        }
        if tokio::time::Instant::now() > deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn manifest_of(paths: &AppPaths, novel_id: i64, job_id: &str) -> Value {
    let path = paths
        .data_dir
        .join("translations")
        .join("v2")
        .join(novel_id.to_string())
        .join(job_id)
        .join("manifest.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn job_dir(paths: &AppPaths, novel_id: i64, job_id: &str) -> std::path::PathBuf {
    paths
        .data_dir
        .join("translations")
        .join("v2")
        .join(novel_id.to_string())
        .join(job_id)
}

// ---------------------------------------------------------------------------
// 1. 全流程
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_job_completes_with_stable_page_line_mapping() {
    let fixture = fixture("full");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let sink = Arc::new(RecordingSink::new());
    let service = service(&fixture.paths, settings_for(&server), sink.clone());

    let started = service.start(novel(), false).await.expect("启动成功");
    assert_eq!(started.state, JobState::Queued, "start 立即返回 queued");
    assert!(!started.already_target_language);

    let done = wait_for(
        &service,
        &started.job_id,
        &[
            JobState::Completed,
            JobState::NeedsAttention,
            JobState::Cancelled,
            JobState::Paused,
        ],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "全流程应完成，得到 {:?} ({}) error={:?} requests={} manifest={:?}",
        done.state,
        done.message,
        done.error_code,
        server.requests().len(),
        manifest_of(&fixture.paths, 5150, &started.job_id)
    );

    // manifest 终态。
    let manifest = manifest_of(&fixture.paths, 5150, &started.job_id);
    assert_eq!(manifest["state"], "completed");
    assert_eq!(manifest["phase"], "publish");
    assert_eq!(manifest["target_language"], "zh-CN");
    assert_eq!(manifest["source_language"], "ja");
    assert!(manifest["counts"]["chunks"].as_u64().unwrap() >= 2);

    // 快照必须带分块计划的块 ID：细分后 ID 不连续，前端不能按序号拼。
    let plan = std::fs::read_to_string(
        fixture
            .paths
            .data_dir
            .join("translations")
            .join("v2")
            .join("5150")
            .join(&started.job_id)
            .join("preparation")
            .join("chunk-plan.json"),
    )
    .expect("读取分块计划");
    let plan: Value = rig_core::serde_json::from_str(&plan).expect("分块计划是 JSON");
    let expected: Vec<String> = plan["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|entry| entry["id"].as_str().map(str::to_string))
        .collect();
    assert!(!expected.is_empty());
    assert_eq!(
        done.chunk_ids, expected,
        "快照 chunk_ids 必须与分块计划顺序一致"
    );

    // 阅读器投影：页行映射与原文一致。
    let result = service
        .result(5150)
        .await
        .expect("读取结果成功")
        .expect("应有正式结果");
    assert_eq!(result.status, PublishStatus::Complete);
    assert_eq!(result.unresolved, 0);
    let source = parse_source(&novel_text(), "zh-CN").unwrap();
    assert_eq!(result.pages.len(), source.pages.len(), "页数与原文一致");
    for page in &source.pages {
        let lines = result.pages.get(&page.page).expect("每页都有译文");
        let expected: Vec<u32> = page.lines.iter().map(|line| line.line).collect();
        let actual: Vec<u32> = lines.iter().map(|line| line.line).collect();
        assert_eq!(actual, expected, "第 {} 页行号映射一致", page.page);
        assert!(lines.iter().all(|line| !line.text.trim().is_empty()));
        // 同页同一行的多个片段按 s 序号拼接（超长行被拆开）。
        for line in &page.lines {
            if line.segments.len() > 1 {
                let translated = lines.iter().find(|item| item.line == line.line).unwrap();
                assert_eq!(
                    translated.text.matches("中文译文").count(),
                    line.segments.len(),
                    "超长行的每个片段都应进入同一行译文"
                );
            }
        }
    }

    // 合并稿规则。
    let merged = std::fs::read_to_string(
        job_dir(&fixture.paths, 5150, &started.job_id).join("output/merged.md"),
    )
    .expect("应写入合并稿");
    assert!(merged.contains("## 中文标题"), "章节标记渲染为标题");
    assert!(merged.contains("汉字(hanzi)呀"), "ruby 渲染为 A(B)");
    assert!(merged.contains("<!-- 图片占位 -->"), "图片行渲染为占位注释");
    assert!(merged.contains("\n---\n"), "页间分隔");

    // 产物齐全。
    let dir = job_dir(&fixture.paths, 5150, &started.job_id);
    for rel in [
        "manifest.json",
        "settings_snapshot.json",
        "resources.json",
        "source/source.json",
        "source/input.json",
        "preparation/constraints.json",
        "preparation/chunk-plan.json",
        "reviews/merge-report.json",
        "output/result.json",
        "output/merged.md",
        "output/global-review.json",
        "checkpoints/latest.json",
    ] {
        assert!(dir.join(rel).is_file(), "缺少产物 {rel}");
    }
    let (chunks, drafts, reviews) = pixiv_tool_lib::translation::service::read_job_artifacts(
        &fixture.paths,
        &serde_json::from_value(manifest.clone()).unwrap(),
    )
    .expect("读取产物");
    assert!(!chunks.is_empty());
    assert_eq!(drafts.len(), chunks.len(), "每块都有草稿");
    assert_eq!(reviews.len(), chunks.len(), "每块都有审查结论");
    assert!(drafts.iter().all(|draft| draft.version >= 1));
    assert!(drafts.iter().all(|draft| draft.reviewed));

    // 事件序号单调递增，且终态为 completed。
    let events = sink.events();
    assert!(events.len() > 5, "应有进度事件");
    let mut last = 0;
    for event in &events {
        assert!(
            event.seq > last,
            "事件 seq 必须单调递增：{last} -> {}",
            event.seq
        );
        last = event.seq;
        assert_eq!(event.job_id, started.job_id);
        assert_eq!(event.novel_id, 5150);
    }
    assert_eq!(events.last().unwrap().state, JobState::Completed);
    assert!(events.iter().any(|event| event.counts.translated > 0));

    // 模型请求都带测试 Key（凭据只走请求头，不落盘）。
    for body in server.requests() {
        assert!(
            !serde_json::to_string(&body).unwrap().contains("test-key"),
            "请求体不得包含凭据"
        );
    }
    let snapshot_json = serde_json::to_string(&manifest).unwrap();
    assert!(!snapshot_json.contains("test-key"), "manifest 不得包含凭据");
}

// ---------------------------------------------------------------------------
// 2. 重复启动加入同一 Job
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplicate_start_joins_the_existing_job() {
    let fixture = fixture("join");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let first = service.start(novel(), false).await.expect("首次启动");
    let second = service.start(novel(), false).await.expect("重复启动加入");
    assert_eq!(first.job_id, second.job_id, "同输入同语言应加入既有 Job");

    // force 重译创建新 Job。
    let third = service.start(novel(), true).await.expect("强制重译");
    assert_ne!(third.job_id, first.job_id, "force 必须创建新 Job");

    let list = service.jobs().await;
    let ids: Vec<&str> = list.jobs.iter().map(|job| job.job_id.as_str()).collect();
    assert!(ids.contains(&first.job_id.as_str()));
    assert!(ids.contains(&third.job_id.as_str()));
}

// ---------------------------------------------------------------------------
// 3. 暂停 → 恢复
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_then_resume_continues_without_redoing_chunks() {
    let fixture = fixture("pause");
    let blocked = Arc::new(AtomicBool::new(true));
    let handler = {
        let blocked = blocked.clone();
        Arc::new(move |body: &Value| {
            if role_of_request(body) == "translator"
                && blocked.load(Ordering::SeqCst)
                && user_of(body).contains("chunk_id: c0002")
            {
                return Reply::Hang;
            }
            scripted(body)
        }) as Handler
    };
    let server = MockServer::start(handler).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动");
    // 等 c0001 完成翻译（其他块继续），再暂停。
    assert!(
        wait_until(|| server.translator_requests_for("c0001") >= 2, 30_000).await,
        "c0001 应先完成翻译"
    );
    service.pause(&started.job_id).await.expect("暂停成功");
    let paused = service.snapshot(&started.job_id).await.unwrap();
    assert_eq!(paused.state, JobState::Paused, "暂停后状态为 paused");

    let before_c1 = server.translator_requests_for("c0001");
    assert!(before_c1 >= 2, "c0001 已请求过翻译");
    blocked.store(false, Ordering::SeqCst);
    service.resume(&started.job_id).await.expect("恢复成功");

    let done = wait_for(
        &service,
        &started.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "恢复后应完成：{}",
        done.message
    );
    assert_eq!(
        server.translator_requests_for("c0001"),
        before_c1,
        "已完成且有效的块不得重复请求"
    );
    assert!(
        server.translator_requests_for("c0002") >= 1,
        "未完成块应重新请求"
    );
}

// ---------------------------------------------------------------------------
// 4. 确认模式
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn confirmation_mode_waits_for_confirm_then_continues() {
    let fixture = fixture("confirm");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let mut settings = settings_for(&server);
    settings.translation_confirm_before_start = true;
    let service = service(&fixture.paths, settings, Arc::new(RecordingSink::new()));

    let started = service.start(novel(), false).await.expect("启动");
    let waiting = wait_for(
        &service,
        &started.job_id,
        &[
            JobState::AwaitingConfirmation,
            JobState::Completed,
            JobState::NeedsAttention,
        ],
    )
    .await;
    assert_eq!(
        waiting.state,
        JobState::AwaitingConfirmation,
        "确认模式应停在 awaiting_confirmation：{}",
        waiting.message
    );
    assert_eq!(server.translator_requests(), 0, "确认前不得开始翻译");
    assert!(waiting.constraint_version >= 1, "冻结后应有约束版本");

    // 只读约束命令：确认前应看到冻结版本与等待确认标志。
    let before = service
        .constraints(&started.job_id)
        .await
        .expect("确认前可读取约束");
    assert!(before.frozen, "冻结后 frozen=true");
    assert!(before.awaiting_confirmation, "确认模式等待确认");
    assert!(before.version >= 1);
    let hero = before
        .terms
        .iter()
        .find(|term| term.source_term == "勇者")
        .expect("冻结术语可见");
    assert_eq!(hero.locked_translation, "勇士", "冻结时的推荐译名");

    // 用户修改术语：用户值优先。
    let edits = ConstraintEdit {
        terms: vec![ConstraintTermEdit {
            source_term: "勇者".into(),
            locked_translation: "英杰".into(),
            term_type: "character".into(),
            aliases: vec![],
            notes: "用户确认".into(),
        }],
        style: "第三人称，书面语".into(),
    };
    service
        .confirm(&started.job_id, edits)
        .await
        .expect("确认成功");

    let done = wait_for(
        &service,
        &started.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "确认后应完成：{}",
        done.message
    );

    let constraints: Value = serde_json::from_slice(
        &std::fs::read(
            job_dir(&fixture.paths, 5150, &started.job_id).join("preparation/constraints.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let terms = constraints["data"]["terms"].as_array().unwrap();
    let hero = terms
        .iter()
        .find(|term| term["source_term"] == "勇者")
        .expect("术语保留");
    assert_eq!(hero["locked_translation"], "英杰", "用户值优先");
    assert_eq!(constraints["data"]["style"], "第三人称，书面语");
    assert!(constraints["data"]["version"].as_u64().unwrap() >= 2);

    // 只读约束命令：确认后版本递增、等待确认标志清除、用户值优先。
    let after = service
        .constraints(&started.job_id)
        .await
        .expect("确认后可读取约束");
    assert!(!after.awaiting_confirmation, "确认后不再等待");
    assert!(after.frozen);
    assert!(after.version >= 2, "用户确认递增版本");
    assert_eq!(after.style, "第三人称，书面语");
    let hero = after
        .terms
        .iter()
        .find(|term| term.source_term == "勇者")
        .expect("术语保留");
    assert_eq!(hero.locked_translation, "英杰", "用户值优先");

    // 未知 Job 返回明确错误，不 panic。
    assert!(service.constraints("20260101000000-abcdef").await.is_err());
    assert!(service.issues("20260101000000-abcdef").await.is_err());
}

// ---------------------------------------------------------------------------
// 4b. 重启后的等待确认
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn awaiting_confirmation_survives_process_restart() {
    let fixture = fixture("confirm-restart");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let mut settings = settings_for(&server);
    settings.translation_confirm_before_start = true;
    let sink = Arc::new(RecordingSink::new());
    let first = service(&fixture.paths, settings.clone(), sink.clone());

    let started = first.start(novel(), false).await.expect("启动");
    let waiting = wait_for(
        &first,
        &started.job_id,
        &[
            JobState::AwaitingConfirmation,
            JobState::Completed,
            JobState::NeedsAttention,
        ],
    )
    .await;
    assert_eq!(
        waiting.state,
        JobState::AwaitingConfirmation,
        "应停在等待确认：{}",
        waiting.message
    );
    assert_eq!(server.translator_requests(), 0, "确认前不得开始翻译");

    // 进入等待确认时发一条系统通知（默认开启，ADR 0046）。
    assert!(
        wait_until(|| sink.notifications().len() == 1, 5_000).await,
        "等待确认应发送一次系统通知"
    );
    let notification = &sink.notifications()[0];
    assert!(
        notification.title.contains("术语"),
        "标题说明待确认：{}",
        notification.title
    );
    assert!(
        notification.body.contains("测试小说"),
        "正文带小说标题：{}",
        notification.body
    );

    // 模拟关闭软件：内存注册表随旧进程消失，任务只剩磁盘状态。
    drop(first);
    let restarted = service(&fixture.paths, settings, Arc::new(RecordingSink::new()));
    let visible = restarted
        .constraints(&started.job_id)
        .await
        .expect("重启后仍能看到约束");
    assert!(visible.awaiting_confirmation, "磁盘状态仍是等待确认");

    restarted
        .confirm(
            &started.job_id,
            ConstraintEdit {
                terms: Vec::new(),
                style: "第一人称，口语".into(),
            },
        )
        .await
        .expect("重启后仍可确认");

    let done = wait_for(
        &restarted,
        &started.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "确认后应完成：{}",
        done.message
    );
}

// ---------------------------------------------------------------------------
// 5. 取消
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_stops_immediately_and_keeps_checkpoint() {
    let fixture = fixture("cancel");
    let handler = Arc::new(|body: &Value| {
        if role_of_request(body) == "translator" {
            return Reply::Hang;
        }
        scripted(body)
    }) as Handler;
    let server = MockServer::start(handler).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动");
    assert!(
        wait_until(|| server.translator_requests() >= 1, 20_000).await,
        "应至少发起一次翻译请求"
    );
    let before = server.requests().len();
    service.cancel(&started.job_id).await.expect("取消成功");
    let cancelled = service.snapshot(&started.job_id).await.unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);

    // 取消瞬间可能已有请求体被服务端读入（取消无法撤回已发出的字节）；
    // 先等其落地，再断言此后完全静止，不得继续请求模型。
    tokio::time::sleep(Duration::from_millis(300)).await;
    let settled = server.requests().len();
    assert!(settled >= before);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        server.requests().len(),
        settled,
        "取消后不得继续请求模型（取消前已发出的在途请求除外）"
    );

    // 检查点保留（目录与 manifest 仍在，可查看已提交产物）。
    let dir = job_dir(&fixture.paths, 5150, &started.job_id);
    assert!(dir.join("manifest.json").is_file());
    assert_eq!(
        manifest_of(&fixture.paths, 5150, &started.job_id)["state"],
        "cancelled"
    );
}

// ---------------------------------------------------------------------------
// 6. 缓存清理
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cache_clear_bumps_generation_and_rejects_stale_writes() {
    let fixture = fixture("cache");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动");
    assert!(!started.job_id.is_empty());
    let scope = JobScope {
        novel_id: 5150,
        job_id: started.job_id.clone(),
        generation: service.generation().get(),
    };
    // 模拟一个仍持有旧世代的写入者（例如迟到的子任务回调）。
    let generation: CacheGeneration = service.generation();
    let stale = JobStore::create_guarded(&fixture.paths, &scope, generation.clone()).unwrap();
    stale
        .write_atomic("chunks/stale.json", b"{}")
        .expect("清前可写");

    service.invalidate_cache();
    let root = stale.root().to_path_buf();
    let _ = std::fs::remove_dir_all(&root);

    // 旧世代写入被拒，且不重建目录。
    let error = stale.write_atomic("chunks/stale.json", b"{}").unwrap_err();
    assert!(
        error.to_string().contains("世代不一致") || error.to_string().contains("不存在"),
        "旧世代写入必须失败：{error}"
    );
    assert!(!root.exists(), "不得重建已清理的 Job 目录");

    // 索引归零：清单里不再有该 Job。
    assert!(
        service.jobs().await.jobs.is_empty(),
        "清理后索引必须归零，不保留幽灵任务"
    );
    assert!(
        service.generation().get() > scope.generation,
        "世代必须递增"
    );
}

// ---------------------------------------------------------------------------
// 7. 损坏 manifest
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn corrupt_manifest_is_flagged_and_never_requested() {
    let fixture = fixture("corrupt");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let dir = fixture
        .paths
        .data_dir
        .join("translations/v2/4242/20260101000000-abcdef");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.json"), b"{ not json").unwrap();

    let recovered = service.recover_interrupted().await.expect("恢复扫描");
    assert!(recovered.is_empty(), "损坏 manifest 不作为可恢复任务");

    let list = service.jobs().await;
    let job = list
        .jobs
        .iter()
        .find(|job| job.job_id == "20260101000000-abcdef")
        .expect("损坏目录应作为 corrupt 条目出现");
    assert_eq!(job.state, JobState::Interrupted);
    assert_eq!(job.error_code.as_deref(), Some("corrupt_manifest"));

    let snapshot = service
        .snapshot("20260101000000-abcdef")
        .await
        .expect("快照可读");
    assert_eq!(snapshot.message, "corrupt");
    assert!(
        service.resume("20260101000000-abcdef").await.is_err(),
        "损坏任务不可恢复"
    );
    assert!(server.requests().is_empty(), "不得自动发起任何模型请求");
}

// ---------------------------------------------------------------------------
// 8. 删除
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn delete_removes_whole_job_tree() {
    let fixture = fixture("delete");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(
        &service,
        &started.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(done.state, JobState::Completed);
    let dir = job_dir(&fixture.paths, 5150, &started.job_id);
    assert!(dir.join("output/merged.md").is_file());

    service.delete(&started.job_id).await.expect("删除成功");
    assert!(!dir.exists(), "删除必须清掉整个 Job 目录");
    assert!(
        service.snapshot(&started.job_id).await.is_err(),
        "删除后快照不可读"
    );
    assert!(service.jobs().await.jobs.is_empty());
}

// ---------------------------------------------------------------------------
// 9. 三条失败路径
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropped_connection_fails_the_chunk_without_completing_the_job() {
    let fixture = fixture("drop");
    let handler = Arc::new(|body: &Value| {
        // 翻译 c0002 时只发响应头就断开。
        if role_of_request(body) == "translator" && user_of(body).contains("chunk_id: c0002") {
            return Reply::Abort;
        }
        scripted(body)
    }) as Handler;
    let server = MockServer::start(handler).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(
        &service,
        &started.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::NeedsAttention,
        "断线后不得冒充完成：{}",
        done.message
    );
    let manifest = manifest_of(&fixture.paths, 5150, &started.job_id);
    assert_eq!(manifest["state"], "needs_attention");
    assert!(manifest["last_error"]["code"].is_string());

    let (chunks, drafts, _) = pixiv_tool_lib::translation::service::read_job_artifacts(
        &fixture.paths,
        &serde_json::from_value(manifest).unwrap(),
    )
    .unwrap();
    assert!(
        drafts.len() < chunks.len(),
        "失败块的草稿不得被记为完成（{}/{}）",
        drafts.len(),
        chunks.len()
    );
    assert!(
        service
            .result(5150)
            .await
            .unwrap()
            .is_none_or(|result| result.status != PublishStatus::Complete),
        "未通过审查不得产出正式完整结果"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn truncated_output_splits_the_chunk_and_still_completes() {
    let fixture = fixture("split");
    let fired = Arc::new(AtomicUsize::new(0));
    let handler = {
        let fired = fired.clone();
        Arc::new(move |body: &Value| {
            // 第一次翻译 c0001（含开场与超长行）时返回「输出超限」，触发缩块重做。
            if role_of_request(body) == "translator"
                && user_of(body).contains("chunk_id: c0001")
                && fired.fetch_add(1, Ordering::SeqCst) == 0
            {
                return Reply::Error(
                    400,
                    json!({"error": {
                        "message": "max_output_tokens exceeded",
                        "type": "invalid_request_error",
                        "code": "output_too_long"
                    }}),
                );
            }
            scripted(body)
        }) as Handler
    };
    let server = MockServer::start(handler).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(
        &service,
        &started.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "缩块后应完成：{}",
        done.message
    );

    // 计划里应出现派生块（原 c0001 被两个派生块取代）。
    let plan: Value = serde_json::from_slice(
        &std::fs::read(
            job_dir(&fixture.paths, 5150, &started.job_id).join("preparation/chunk-plan.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let entries = plan["entries"].as_array().unwrap();
    let derived: Vec<&Value> = entries
        .iter()
        .filter(|entry| entry["parent"].is_string())
        .collect();
    assert_eq!(derived.len(), 2, "超限块应拆成两个派生块");
    assert!(derived.iter().all(|entry| entry["depth"] == 1));
    assert_eq!(
        fired.load(Ordering::SeqCst),
        1,
        "超限请求只发生一次，不原样重发"
    );

    // 派生块也要有完整译文与审查，最终投影仍覆盖全部页行。
    let result = service.result(5150).await.unwrap().unwrap();
    assert_eq!(result.status, PublishStatus::Complete);
    let source = parse_source(&novel_text(), "zh-CN").unwrap();
    for page in &source.pages {
        let lines = result.pages.get(&page.page).unwrap();
        assert_eq!(lines.len(), page.lines.len());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_tool_arguments_stop_after_corrections() {
    let fixture = fixture("args");
    let handler = Arc::new(|body: &Value| {
        if role_of_request(body) == "translator" {
            // 参数不是合法 JSON 对象：运行层有限纠正后停止。
            return Reply::Sse(vec![
                chat_chunk(
                    json!({"role": "assistant", "tool_calls": [{
                        "index": 0,
                        "id": "bad",
                        "type": "function",
                        "function": {"name": "submit_translation", "arguments": "{not json"}
                    }]}),
                    None,
                    None,
                ),
                chat_chunk(json!({}), Some("tool_calls"), None),
                b"data: [DONE]\n\n".to_vec(),
            ]);
        }
        scripted(body)
    }) as Handler;
    let server = MockServer::start(handler).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(
        &service,
        &started.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::NeedsAttention,
        "参数错误不得完成：{}",
        done.message
    );

    let manifest = manifest_of(&fixture.paths, 5150, &started.job_id);
    let (_, drafts, _) = pixiv_tool_lib::translation::service::read_job_artifacts(
        &fixture.paths,
        &serde_json::from_value(manifest).unwrap(),
    )
    .unwrap();
    assert!(drafts.is_empty(), "无效参数不得写入草稿");
    // 纠正次数有限：每个块的翻译请求不会无限增长。
    assert!(
        server.translator_requests() < 40,
        "纠正必须有上限，实际 {} 次",
        server.translator_requests()
    );
}

// ---------------------------------------------------------------------------
// 10. 重启后恢复（不依赖进程内 JobHandle）
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_after_restart_rebuilds_job_from_disk() {
    let fixture = fixture("restart");
    let blocked = Arc::new(AtomicBool::new(true));
    let handler = {
        let blocked = blocked.clone();
        Arc::new(move |body: &Value| {
            // 卡住 c0002 的翻译，模拟「进程在 c0001 已完成后退出」。
            if role_of_request(body) == "translator"
                && blocked.load(Ordering::SeqCst)
                && user_of(body).contains("chunk_id: c0002")
            {
                return Reply::Hang;
            }
            scripted(body)
        }) as Handler
    };
    let server = MockServer::start(handler).await;
    let settings = settings_for(&server);
    let sink = Arc::new(RecordingSink::new());
    let first = service(&fixture.paths, settings.clone(), sink.clone());

    let started = first.start(novel(), false).await.expect("启动");
    assert!(
        wait_until(|| server.translator_requests_for("c0001") >= 2, 30_000).await,
        "c0001 应先完成翻译"
    );
    first.pause(&started.job_id).await.expect("暂停");
    let before_c1 = server.translator_requests_for("c0001");
    let paused = first.snapshot(&started.job_id).await.unwrap();
    assert_eq!(paused.state, JobState::Paused);
    // 暂停时派发批次可能还没整批结束，计数未必刷新；直接看磁盘产物。
    assert!(
        job_dir(&fixture.paths, 5150, &started.job_id)
            .join("chunks/c0001.draft.json")
            .is_file(),
        "c0001 的草稿应已提交到磁盘"
    );

    // 模拟应用重启：全新的服务实例（不同世代实例，同一数据目录）+ 恢复扫描。
    blocked.store(false, Ordering::SeqCst);
    let sink2 = Arc::new(RecordingSink::new());
    let restarted = service(&fixture.paths, settings, sink2.clone());
    restarted.recover_interrupted().await.expect("恢复扫描");
    let after_scan = restarted
        .snapshot(&started.job_id)
        .await
        .expect("重启后可见");
    // 暂停已是稳定检查点，恢复扫描不改它；仍在跑的状态才转 interrupted。
    assert_eq!(after_scan.state, JobState::Paused, "暂停状态是稳定检查点");
    assert!(
        restarted.recover_interrupted().await.unwrap().is_empty(),
        "暂停任务不在「未结束」集合里"
    );

    // 重建上下文并续跑：已完成块不重复请求。
    restarted
        .resume(&started.job_id)
        .await
        .expect("重启后恢复成功");
    let done = wait_until_async(
        &restarted,
        &started.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "重启后应能完成：{} error={:?}",
        done.message,
        done.error_code
    );
    assert_eq!(
        server.translator_requests_for("c0001"),
        before_c1,
        "重启恢复不得重复请求已完成块"
    );
    assert!(
        server.translator_requests_for("c0002") >= 1,
        "未完成块应重新请求"
    );

    // 重建后的预算沿用累计值（不清零）。
    assert!(done.budget.used > 0, "预算用量应从 manifest 还原");
    assert!(done.budget.limit > 0, "恢复后的预算上限应有效");
    // 恢复过程有新事件（新实例的事件序号从 1 起）。
    assert!(!sink2.events().is_empty(), "重启后的实例应广播事件");
    let final_result = restarted.result(5150).await.unwrap().unwrap();
    assert_eq!(final_result.status, PublishStatus::Complete);
}

async fn wait_until_async(
    service: &TranslationService,
    job_id: &str,
    wanted: &[JobState],
) -> pixiv_tool_lib::translation::service::TranslationJobSnapshot {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let snapshot = service.snapshot(job_id).await.expect("任务快照可读");
        if wanted.contains(&snapshot.state) {
            return snapshot;
        }
        if tokio::time::Instant::now() > deadline {
            return snapshot;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ---------------------------------------------------------------------------
// 11. 会话记录（sessions/journal.jsonl）
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn session_journal_records_facts_without_content() {
    let fixture = fixture("journal");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(
        &service,
        &started.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(done.state, JobState::Completed);

    let journal = std::fs::read_to_string(
        job_dir(&fixture.paths, 5150, &started.job_id).join("sessions/journal.jsonl"),
    )
    .expect("应写入会话记录");
    let records: Vec<Value> = journal
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    assert!(!records.is_empty(), "会话记录不应为空");
    assert!(
        records
            .iter()
            .any(|record| record["kind"] == "subtask_translate_done"),
        "应有翻译子任务记录"
    );
    assert!(
        records
            .iter()
            .any(|record| record["kind"] == "subtask_review_done"),
        "应有审查子任务记录"
    );
    assert!(
        records.iter().any(|record| record["kind"] == "main_run"),
        "应有主运行记录"
    );
    for record in &records {
        let keys: Vec<&String> = record.as_object().unwrap().keys().collect();
        for forbidden in [
            "text",
            "content",
            "prompt",
            "summary",
            "source",
            "translation",
        ] {
            assert!(
                !keys.iter().any(|key| key.as_str() == forbidden),
                "会话记录不得包含正文字段 {forbidden}: {record}"
            );
        }
        let serialized = record.to_string();
        assert!(!serialized.contains("这是本段的中文译文"), "不得记录译文");
        assert!(!serialized.contains("第二頁の本文です"), "不得记录原文");
        assert!(!serialized.contains("test-key"), "不得记录凭据");
    }
    // 清理后会话记录随 Job 一起消失。
    service.delete(&started.job_id).await.expect("删除");
    assert!(!job_dir(&fixture.paths, 5150, &started.job_id).exists());
}

// ---------------------------------------------------------------------------
// 12. 压缩记录（compact_start / compact_done / compact_failed）
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compaction_writes_journal_records_without_content() {
    let fixture = fixture("compact-journal");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let mut settings = settings_for(&server);
    // 低阈值：小上下文 + 最小输出上限 + 50% 触发 + 最小保留窗口。
    settings.translation_agent_context_window = 32_768;
    settings.translation_agent_output_cap = 1_024;
    settings.translation_agent_compact_percent = 50;
    settings.translation_agent_recent_history_tokens = 512;
    settings.translation_agent_summary_tokens = 256;
    let service = service(&fixture.paths, settings, Arc::new(RecordingSink::new()));

    let started = service
        .start(novel_compact(), false)
        .await
        .expect("启动成功");
    let done = wait_for(
        &service,
        &started.job_id,
        &[
            JobState::Completed,
            JobState::NeedsAttention,
            JobState::Paused,
        ],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "低阈值下仍应完成：{} error={:?}",
        done.message,
        done.error_code
    );

    let records = journal_records(&fixture.paths, 5151, &started.job_id);
    assert!(
        records
            .iter()
            .any(|record| record["kind"] == "compact_start"),
        "应记录 compact_start：{records:?}"
    );
    let completed = records
        .iter()
        .find(|record| record["kind"] == "compact_done")
        .expect("低阈值应触发一次成功压缩");
    let before = completed["before_est"].as_u64().expect("before_est 计数");
    let after = completed["after_est"].as_u64().expect("after_est 计数");
    assert!(before > 0, "压缩前估算应为正数");
    assert!(after <= before, "压缩后估算不得高于压缩前");
    assert!(completed["elapsed_ms"].is_number(), "应记录耗时");
    assert!(completed["retained"].as_u64().is_some(), "应记录保留条数");
    assert!(
        completed["keep_from"].as_u64().is_some(),
        "应记录保留区起点"
    );

    for record in records.iter().filter(|record| {
        record["kind"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("compact"))
    }) {
        assert_eq!(record["agent"], "main");
        assert!(record["phase"].is_string(), "应附阶段关联");
        let keys: Vec<&String> = record.as_object().unwrap().keys().collect();
        for forbidden in [
            "text",
            "content",
            "summary",
            "prompt",
            "source",
            "translation",
            "original",
            "key",
        ] {
            assert!(
                !keys.iter().any(|key| key.as_str() == forbidden),
                "压缩记录不得包含正文字段 {forbidden}: {record}"
            );
        }
        let serialized = record.to_string();
        assert!(!serialized.contains("圧縮対象の本文です"), "不得记录原文");
        assert!(!serialized.contains("这是本段的中文译文"), "不得记录译文");
        assert!(!serialized.contains("test-key"), "不得记录凭据");
        assert!(
            !serialized.contains("目标：完成整篇翻译"),
            "不得记录摘要正文: {record}"
        );
    }
}

// ---------------------------------------------------------------------------
// 13. 审查问题只读命令
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn issues_command_aggregates_reviews_and_excludes_passed_chunks() {
    let fixture = fixture("issues");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let initial = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = initial.start(novel(), false).await.expect("启动成功");
    let done = wait_for(
        &initial,
        &started.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "基础流程应完成：{}",
        done.message
    );

    // 翻转 c0001 的审查结论，制造一个有问题块；c0002 保持通过。
    let manifest: pixiv_tool_lib::agent::Manifest =
        serde_json::from_value(manifest_of(&fixture.paths, 5150, &started.job_id)).unwrap();
    let scope = JobScope {
        novel_id: manifest.novel_id,
        job_id: manifest.job_id.clone(),
        generation: manifest.generation,
    };
    let store = Arc::new(JobStore::open(&fixture.paths, &scope).unwrap());
    let io = StoreIo::new(store);
    let artifacts = JobArtifacts::new(&io);
    let draft = artifacts
        .read_draft("c0001")
        .expect("读取草稿")
        .expect("c0001 有草稿");
    artifacts
        .write_review(&Review {
            chunk_id: "c0001".into(),
            draft_version: draft.version,
            verdict: ReviewVerdict::NeedsFix,
            issues: vec![
                Issue {
                    segment_id: "p001-l001-s0".into(),
                    kind: "style".into(),
                    severity: Severity::Minor,
                    detail: "语气与冻结风格不一致".into(),
                },
                Issue {
                    segment_id: "p001-l001-s0".into(),
                    kind: "omission".into(),
                    severity: Severity::Critical,
                    detail: "漏译一个片段".into(),
                },
                Issue {
                    segment_id: "p001-l002-s0".into(),
                    kind: "term".into(),
                    severity: Severity::Major,
                    detail: "术语与冻结约束不一致".into(),
                },
            ],
            notes: "存在阻断问题".into(),
        })
        .unwrap();

    // 新实例（无内存 JobHandle）走磁盘路径，验证命令不依赖运行态。
    let fresh = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );
    let view = fresh.issues(&started.job_id).await.expect("问题汇总可读");
    assert_eq!(view.issues.len(), 3);
    let severities: Vec<Severity> = view.issues.iter().map(|issue| issue.severity).collect();
    assert_eq!(
        severities,
        vec![Severity::Critical, Severity::Major, Severity::Minor],
        "按 severity critical → major → minor 稳定排序"
    );
    assert!(view.issues.iter().all(|issue| issue.chunk_id == "c0001"));
    assert_eq!(view.affected_chunks, vec!["c0001"]);
    assert!(view.unresolved_chunks.contains(&"c0001".to_string()));
    assert!(
        !view.unresolved_chunks.contains(&"c0002".to_string()),
        "通过块不得进入 unresolved：{:?}",
        view.unresolved_chunks
    );
    // c0001 已被翻成 needs_fix：按 ADR 0038，reviewed 只统计结论为 pass 的块
    // （与进度计数 `reviewed = review_pass` 同一口径）。
    assert_eq!(
        view.reviewed + 1,
        view.total,
        "只有 pass 的块计入 reviewed"
    );
    assert!(view.total >= 2);
    for issue in &view.issues {
        assert!(issue.detail.chars().count() <= 301, "detail 必须限长");
    }
    let serialized = serde_json::to_string(&view).unwrap();
    assert!(!serialized.contains("第二頁の本文です"), "汇总不得含原文");
    assert!(!serialized.contains("这是本段的中文译文"), "汇总不得含译文");
}

// ---------------------------------------------------------------------------
// 14. 两本小说并行 + 取消隔离
// ---------------------------------------------------------------------------

/// 等待两本小说都进入 Running 且调度器同时持有两个活跃小说槽位。
async fn wait_two_running(
    service: &TranslationService,
    server: &MockServer,
    first: &str,
    second: &str,
) -> (
    pixiv_tool_lib::translation::service::TranslationJobSnapshot,
    pixiv_tool_lib::translation::service::TranslationJobSnapshot,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let a = service.snapshot(first).await.expect("甲快照可读");
        let b = service.snapshot(second).await.expect("乙快照可读");
        if a.state == JobState::Running
            && b.state == JobState::Running
            && service.scheduler().active_novel_count() == 2
            && server.translator_requests() >= 2
        {
            return (a, b);
        }
        assert!(
            tokio::time::Instant::now() <= deadline,
            "两本小说未能同时运行：甲={:?} 乙={:?} active={} translator_requests={}",
            a.state,
            b.state,
            service.scheduler().active_novel_count(),
            server.translator_requests()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_novels_run_concurrently_without_crossing_jobs() {
    let fixture = fixture("parallel");
    let gate = Arc::new(Gate::new());
    let handler = {
        let gate = gate.clone();
        Arc::new(move |body: &Value| {
            if role_of_request(body) == "translator" && !gate.is_open() {
                return Reply::Gated {
                    gate: gate.clone(),
                    body: body.clone(),
                };
            }
            scripted(body)
        }) as Handler
    };
    let server = MockServer::start(handler).await;
    let mut settings = settings_for(&server);
    settings.translation_agent_active_novels = 2;
    settings.translation_agent_global_concurrency = 4;
    let sink = Arc::new(RecordingSink::new());
    let service = service(&fixture.paths, settings, sink.clone());

    let a = service.start(novel_a(), false).await.expect("甲启动");
    let b = service.start(novel_b(), false).await.expect("乙启动");
    assert_ne!(a.job_id, b.job_id, "不同小说必须拥有不同 Job");
    wait_two_running(&service, &server, &a.job_id, &b.job_id).await;
    gate.open();

    let done_a = wait_for(
        &service,
        &a.job_id,
        &[
            JobState::Completed,
            JobState::NeedsAttention,
            JobState::Cancelled,
        ],
    )
    .await;
    let done_b = wait_for(
        &service,
        &b.job_id,
        &[
            JobState::Completed,
            JobState::NeedsAttention,
            JobState::Cancelled,
        ],
    )
    .await;
    assert_eq!(
        done_a.state,
        JobState::Completed,
        "甲应完成：{}",
        done_a.message
    );
    assert_eq!(
        done_b.state,
        JobState::Completed,
        "乙应完成：{}",
        done_b.message
    );

    // 各自目录 / manifest / 结果互不覆盖。
    let dir_a = job_dir(&fixture.paths, 6001, &a.job_id);
    let dir_b = job_dir(&fixture.paths, 6002, &b.job_id);
    assert_ne!(dir_a, dir_b);
    assert!(dir_a.join("output/result.json").is_file());
    assert!(dir_b.join("output/result.json").is_file());
    assert_eq!(
        manifest_of(&fixture.paths, 6001, &a.job_id)["novel_id"],
        6001
    );
    assert_eq!(
        manifest_of(&fixture.paths, 6002, &b.job_id)["novel_id"],
        6002
    );

    let result_a = service.result(6001).await.unwrap().expect("甲结果");
    let result_b = service.result(6002).await.unwrap().expect("乙结果");
    assert_eq!(result_a.novel_id, 6001);
    assert_eq!(result_b.novel_id, 6002);
    let source_a = parse_source(&novel_a().content, "zh-CN").unwrap();
    let source_b = parse_source(&novel_b().content, "zh-CN").unwrap();
    assert_eq!(result_a.pages.len(), source_a.pages.len());
    assert_eq!(result_b.pages.len(), source_b.pages.len());
    assert_ne!(
        result_a.pages.len(),
        result_b.pages.len(),
        "两本页数不同，能区分结果归属"
    );

    // 事件 seq 每个 Job 各自独立、严格递增。
    let events = sink.events();
    for job_id in [&a.job_id, &b.job_id] {
        let seqs: Vec<u64> = events
            .iter()
            .filter(|event| &event.job_id == job_id)
            .map(|event| event.seq)
            .collect();
        assert!(seqs.len() > 3, "每个 Job 都应有进度事件");
        let mut last = 0;
        for seq in seqs {
            assert!(
                seq > last,
                "同一 Job 的事件 seq 必须单调递增：{last} -> {seq}"
            );
            last = seq;
        }
    }
    assert!(
        events
            .iter()
            .filter(|event| event.job_id == a.job_id)
            .next_back()
            .is_some_and(|event| event.state == JobState::Completed)
    );
    assert!(
        events
            .iter()
            .filter(|event| event.job_id == b.job_id)
            .next_back()
            .is_some_and(|event| event.state == JobState::Completed)
    );

    // 产物不串页 / 串块：每块的原文都来自本 Job 的小说，草稿片段与块一一对应。
    for (manifest_value, novel, label) in [
        (
            manifest_of(&fixture.paths, 6001, &a.job_id),
            novel_a(),
            "甲",
        ),
        (
            manifest_of(&fixture.paths, 6002, &b.job_id),
            novel_b(),
            "乙",
        ),
    ] {
        let manifest: pixiv_tool_lib::agent::Manifest =
            serde_json::from_value(manifest_value).unwrap();
        let (chunks, drafts, reviews) =
            pixiv_tool_lib::translation::service::read_job_artifacts(&fixture.paths, &manifest)
                .expect("读取产物");
        assert!(!chunks.is_empty());
        assert_eq!(drafts.len(), chunks.len(), "{label} 每块都有草稿");
        assert_eq!(reviews.len(), chunks.len(), "{label} 每块都有审查");
        for chunk in &chunks {
            for segment in &chunk.segments {
                assert!(
                    novel.content.contains(&segment.text),
                    "{label} Job 混入其他小说原文：{}",
                    segment.text
                );
            }
            let draft = drafts
                .iter()
                .find(|draft| draft.chunk_id == chunk.id)
                .expect("块有对应草稿");
            let chunk_ids: Vec<&String> = chunk.segments.iter().map(|item| &item.id).collect();
            let draft_ids: Vec<&String> = draft.segments.iter().map(|item| &item.id).collect();
            assert_eq!(
                chunk_ids, draft_ids,
                "{label} {} 片段顺序必须一致",
                chunk.id
            );
        }
    }
    assert!(server.requests_containing("甲") > 0, "甲小说确实被处理");
    assert!(server.requests_containing("乙") > 0, "乙小说确实被处理");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn active_novels_limit_queues_second_novel_until_first_releases() {
    let fixture = fixture("fair");
    let gate = Arc::new(Gate::new());
    let handler = {
        let gate = gate.clone();
        Arc::new(move |body: &Value| {
            if role_of_request(body) == "translator" && !gate.is_open() {
                return Reply::Gated {
                    gate: gate.clone(),
                    body: body.clone(),
                };
            }
            scripted(body)
        }) as Handler
    };
    let server = MockServer::start(handler).await;
    let mut settings = settings_for(&server);
    settings.translation_agent_active_novels = 1;
    settings.translation_agent_global_concurrency = 4;
    let service = service(&fixture.paths, settings, Arc::new(RecordingSink::new()));

    let a = service.start(novel_a(), false).await.expect("甲启动");
    assert!(
        wait_until(|| server.translator_requests() >= 1, 30_000).await,
        "甲应先进入翻译并被闸门挡住"
    );
    let b = service.start(novel_b(), false).await.expect("乙启动");

    // 甲被闸门挡住期间，乙必须留在队列，且不产生任何含乙原文的请求。
    for _ in 0..10 {
        let snapshot = service.snapshot(&b.job_id).await.expect("乙快照可读");
        assert_eq!(snapshot.state, JobState::Queued, "乙应在队列等待");
        assert_eq!(
            service.snapshot(&a.job_id).await.unwrap().state,
            JobState::Running,
            "甲仍占用唯一的活跃小说槽位"
        );
        assert_eq!(
            server.requests_containing("乙"),
            0,
            "甲未释放前乙不得发起任何请求"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    gate.open();
    let done_a = wait_for(
        &service,
        &a.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(
        done_a.state,
        JobState::Completed,
        "甲应完成：{}",
        done_a.message
    );
    let done_b = wait_for(
        &service,
        &b.job_id,
        &[JobState::Completed, JobState::NeedsAttention],
    )
    .await;
    assert_eq!(
        done_b.state,
        JobState::Completed,
        "乙应在甲释放后完成：{}",
        done_b.message
    );
    assert!(
        server.requests_containing("乙") > 0,
        "乙必须在甲释放后才真正开始"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_one_novel_does_not_affect_the_other() {
    let fixture = fixture("cancel-isolated");
    let gate = Arc::new(Gate::new());
    let handler = {
        let gate = gate.clone();
        Arc::new(move |body: &Value| {
            if role_of_request(body) == "translator" && !gate.is_open() {
                return Reply::Gated {
                    gate: gate.clone(),
                    body: body.clone(),
                };
            }
            scripted(body)
        }) as Handler
    };
    let server = MockServer::start(handler).await;
    let mut settings = settings_for(&server);
    settings.translation_agent_active_novels = 2;
    settings.translation_agent_global_concurrency = 4;
    let service = service(&fixture.paths, settings, Arc::new(RecordingSink::new()));

    let a = service.start(novel_a(), false).await.expect("甲启动");
    let b = service.start(novel_b(), false).await.expect("乙启动");
    wait_two_running(&service, &server, &a.job_id, &b.job_id).await;

    service.cancel(&a.job_id).await.expect("取消甲");
    let cancelled = service.snapshot(&a.job_id).await.unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert_eq!(
        service.snapshot(&b.job_id).await.unwrap().state,
        JobState::Running,
        "取消甲不得改变乙的运行状态"
    );

    gate.open();
    let done_b = wait_for(
        &service,
        &b.job_id,
        &[
            JobState::Completed,
            JobState::NeedsAttention,
            JobState::Cancelled,
        ],
    )
    .await;
    assert_eq!(
        done_b.state,
        JobState::Completed,
        "乙不受甲取消影响，应完成：{}",
        done_b.message
    );
    assert_eq!(
        manifest_of(&fixture.paths, 6001, &a.job_id)["state"],
        "cancelled"
    );
    assert_eq!(
        manifest_of(&fixture.paths, 6002, &b.job_id)["state"],
        "completed"
    );
    assert!(
        service.result(6002).await.unwrap().is_some(),
        "乙有正式结果"
    );
}

// ---------------------------------------------------------------------------
// 运行层防护：单轮工具结果预算与任务续跑
// ---------------------------------------------------------------------------

/// 一条 assistant 消息携带多个工具调用（并行读块场景）。
fn multi_tool_call_stream(calls: Vec<(String, String, String)>) -> Reply {
    let tool_calls: Vec<Value> = calls
        .iter()
        .enumerate()
        .map(|(index, (id, tool, args))| {
            json!({
                "index": index,
                "id": id,
                "type": "function",
                "function": {"name": tool, "arguments": args}
            })
        })
        .collect();
    Reply::Sse(vec![
        chat_chunk(
            json!({"role": "assistant", "tool_calls": tool_calls}),
            None,
            None,
        ),
        chat_chunk(json!({}), Some("tool_calls"), None),
        b"data: [DONE]\n\n".to_vec(),
    ])
}

/// 按调用 id 精确收集拿到成功结果（JSON 正文）的 read_chunk 块 ID；
/// 被单轮预算跳过的调用正文是纯文本提示，不会计入。
fn successful_read_chunks(body: &Value) -> Vec<String> {
    let mut args_by_call: BTreeMap<String, Value> = BTreeMap::new();
    let mut read_ok = Vec::new();
    if let Some(messages) = body["messages"].as_array() {
        for message in messages {
            if let Some(list) = message["tool_calls"].as_array() {
                for call in list {
                    if call["function"]["name"] == "read_chunk"
                        && let Some(id) = call["id"].as_str()
                    {
                        let args = call["function"]["arguments"]
                            .as_str()
                            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                            .unwrap_or(Value::Null);
                        args_by_call.insert(id.to_string(), args);
                    }
                }
            }
        }
        for message in messages {
            if message["role"] == "tool"
                && let Some(id) = message["tool_call_id"].as_str()
                && let Some(args) = args_by_call.get(id)
            {
                let raw = content_text(message);
                if serde_json::from_str::<Value>(&raw).is_ok()
                    && let Some(chunk_id) = args["chunk_id"].as_str()
                {
                    read_ok.push(chunk_id.to_string());
                }
            }
        }
    }
    read_ok
}

/// 多页大正文（约 12 页，切成约 12 块），用于触发单轮工具结果预算。
fn novel_throttle() -> NovelInput {
    let page = format!("[chapter:流量試験]\n{}", "しぼう対象の本文です".repeat(100));
    NovelInput {
        novel_id: 5152,
        title: "节流测试".into(),
        tags: vec![],
        description: String::new(),
        content: std::iter::repeat_n(page.clone(), 12)
            .collect::<Vec<_>>()
            .join("\n[newpage]\n"),
    }
}

#[tokio::test]
async fn main_run_continuation_completes_after_idle_end() {
    let fixture = fixture("continuation");
    let main_calls = Arc::new(AtomicUsize::new(0));
    let counter = main_calls.clone();
    // 收集续跑轮的请求体，断言运行层注入了明确的下一步指令。
    let continuation_bodies = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = continuation_bodies.clone();
    let handler: Handler = Arc::new(move |body: &Value| {
        if body["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty())
        {
            let role = role_of_request(body);
            if role != "translator" && role != "reviewer" {
                let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                if n == 2 {
                    // 第一轮运行里模型输出纯文本收工（不再调用工具）。
                    return text_stream("当前阶段已准备完毕，本轮先汇报到这里。");
                }
                if n >= 3 {
                    seen.lock().unwrap().push(body.to_string());
                }
            }
        }
        scripted(body)
    });
    let server = MockServer::start(handler).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动成功");
    let done = wait_for(
        &service,
        &started.job_id,
        &[
            JobState::Completed,
            JobState::NeedsAttention,
            JobState::Cancelled,
        ],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "模型提前收工后应自动续跑到完成：{} error={:?}",
        done.message,
        done.error_code
    );

    // 会话里应有两次 main_run 记录（第一轮 + 续跑轮）。
    let runs: Vec<Value> = journal_records(&fixture.paths, 5150, &started.job_id)
        .into_iter()
        .filter(|record| record["kind"] == "main_run")
        .collect();
    assert!(
        runs.len() >= 2,
        "应至少两次主运行记录，实际 {}：{runs:?}",
        runs.len()
    );

    let bodies = continuation_bodies.lock().unwrap().join("\n");
    assert!(
        bodies.contains("续跑指令"),
        "续跑轮应收到运行层生成的明确指令：{bodies}"
    );
    assert!(
        bodies.contains("必须包含工具调用"),
        "续跑指令应禁止只输出文本：{bodies}"
    );
}

#[tokio::test]
async fn retryable_provider_error_is_retried_before_giving_up() {
    let fixture = fixture("provider-retry");
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let handler: Handler = Arc::new(move |body: &Value| {
        if role_of_request(body) == "main" && counter.fetch_add(1, Ordering::SeqCst) == 0 {
            // 首次主 Agent 请求返回 429：任务级应退避重试而不是直接转 needs_attention。
            return Reply::Error(429, json!({"error": {"message": "cell is at capacity"}}));
        }
        scripted(body)
    });
    let server = MockServer::start(handler).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动成功");
    let done = wait_for(
        &service,
        &started.job_id,
        &[
            JobState::Completed,
            JobState::NeedsAttention,
            JobState::Cancelled,
        ],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "429 应被退避重试并最终完成：{} error={:?}",
        done.message,
        done.error_code
    );
    let main_calls = server
        .requests()
        .iter()
        .filter(|body| role_of_request(body) == "main")
        .count();
    assert!(
        main_calls >= 2,
        "主 Agent 应至少重试一次，实际 {main_calls} 次请求"
    );
}

#[tokio::test]
async fn zero_tool_turns_surface_the_model_message_as_reason() {
    let fixture = fixture("declined");
    // 模型每轮都只回文本（内容审核拒绝的典型形态）：两轮后应停止并把原因透出。
    let handler: Handler =
        Arc::new(|_body: &Value| text_stream("我无法继续处理这篇内容，涉及未成年角色的露骨描写。"));
    let server = MockServer::start(handler).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动成功");
    let done = wait_for(
        &service,
        &started.job_id,
        &[
            JobState::NeedsAttention,
            JobState::Completed,
            JobState::Cancelled,
        ],
    )
    .await;
    assert_eq!(done.state, JobState::NeedsAttention);
    assert_eq!(
        done.error_code.as_deref(),
        Some("model_declined"),
        "零工具调用的收敛应使用 model_declined：{:?}",
        done.error_code
    );
    assert!(
        done.message.contains("无法继续处理"),
        "应把模型说明作为用户可见原因：{}",
        done.message
    );
}

#[tokio::test]
async fn bulk_read_chunk_throttled_by_turn_budget_still_completes() {
    let fixture = fixture("throttle");
    // 有状态机：跨压缩保留进度（真实模型依据压缩摘要与检查点继续推进）。
    let state = Arc::new(Mutex::new(BulkState::default()));
    let handler: Handler = {
        let state = state.clone();
        Arc::new(move |body: &Value| {
            let has_tools = body["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty());
            if !has_tools {
                return json_reply(
                    "1. 目标：完成整篇翻译；2. 约束：已冻结；3. 产物：见块产物；             4. 进度：按检查点；5. 未决：无；6. 下一步：继续派发待办块。",
                );
            }
            let role = role_of_request(body);
            if role == "translator" || role == "reviewer" {
                return scripted(body);
            }
            let (_calls, results) = tool_history(body);
            let mut state = state.lock().unwrap();
            if let Some(ids) = results
                .get("prepare_source")
                .and_then(|value| value["chunk_ids"].as_array())
                .map(|ids| {
                    ids.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                && !ids.is_empty()
            {
                state.chunk_ids = ids;
            }
            if state.chunk_ids.is_empty() {
                return tool_call_stream("m1", "prepare_source", "{}");
            }
            {
                let mut memory = state.read.lock().unwrap();
                memory.extend(successful_read_chunks(body));
            }
            let unscanned: Vec<String> = state
                .chunk_ids
                .iter()
                .filter(|id| !state.scanned.contains(id.as_str()))
                .cloned()
                .collect();
            if !unscanned.is_empty() {
                let unread: Vec<String> = unscanned
                    .iter()
                    .filter(|id| !state.read.lock().unwrap().contains(id.as_str()))
                    .cloned()
                    .collect();
                if !unread.is_empty() {
                    let batch: Vec<(String, String, String)> = unread
                        .iter()
                        .enumerate()
                        .map(|(index, id)| {
                            (
                                format!("mr-{index}"),
                                "read_chunk".into(),
                                json!({"chunk_id": id, "include_context": true}).to_string(),
                            )
                        })
                        .collect();
                    return multi_tool_call_stream(batch);
                }
                let chunk_id = unscanned[0].clone();
                state.scanned.insert(chunk_id.clone());
                let scan = json!({
                    "chunk_id": chunk_id,
                    "terms": [{
                        "source_term": "勇者",
                        "locked_translation": "勇士",
                        "term_type": "character",
                        "notes": "主角称谓",
                    }],
                    "style": {"text_type": "小说", "formality": "书面语，对白偏口语"},
                    "notes": "叙述视角为第三人称",
                    "confidence": "high",
                });
                return tool_call_stream("m3", "submit_scan", &scan.to_string());
            }
            if !state.frozen {
                state.frozen = true;
                return tool_call_stream("m4", "freeze_constraints", "{}");
            }
            if !state.translated {
                state.translated = true;
                let ids = state.chunk_ids.clone();
                return tool_call_stream(
                    "m6",
                    "dispatch_translation",
                    &json!({"chunk_ids": ids}).to_string(),
                );
            }
            if !state.reviewed {
                state.reviewed = true;
                let ids = state.chunk_ids.clone();
                return tool_call_stream(
                    "m7",
                    "dispatch_review",
                    &json!({"chunk_ids": ids}).to_string(),
                );
            }
            if !state.review_summary {
                state.review_summary = true;
                return tool_call_stream("m9", "read_review_summary", "{}");
            }
            if !state.merged {
                state.merged = true;
                return tool_call_stream("m10", "merge_drafts", "{}");
            }
            if !state.global_reviewed {
                state.global_reviewed = true;
                let sample = state.chunk_ids.iter().take(8).cloned().collect::<Vec<_>>();
                let review = json!({
                    "sample_chunk_ids": if sample.is_empty() { json!(["c0001"]) } else { json!(sample) },
                    "notes": "术语、风格、对白、纯净度、标记五项已核",
                    "passed": true,
                });
                return tool_call_stream("m11", "record_global_review", &review.to_string());
            }
            if !state.published {
                state.published = true;
                return tool_call_stream("m12", "publish", "{}");
            }
            text_stream("done")
        })
    };
    let server = MockServer::start(handler).await;

    // 用较小的输出上限压低单轮工具结果预算（约 18k）：12 块（每块约 2563
    // tokens）并行读取时第 8 个起会被跳过；历史保留用生产默认（4k），
    // 旧块结果由正常压缩摘要，紧急截断只兜底。
    let mut settings = settings_for(&server);
    settings.translation_agent_context_window = 32_768;
    settings.translation_agent_output_cap = 4_096;
    settings.translation_agent_recent_history_tokens = 4_096;
    settings.translation_agent_summary_tokens = 2_048;
    let service = service(&fixture.paths, settings, Arc::new(RecordingSink::new()));

    let started = service
        .start(novel_throttle(), false)
        .await
        .expect("启动成功");
    let done = wait_for(
        &service,
        &started.job_id,
        &[
            JobState::Completed,
            JobState::NeedsAttention,
            JobState::Cancelled,
        ],
    )
    .await;
    let artifacts_summary = {
        let manifest = manifest_of(&fixture.paths, 5152, &started.job_id);
        pixiv_tool_lib::translation::service::read_job_artifacts(
            &fixture.paths,
            &serde_json::from_value::<pixiv_tool_lib::agent::Manifest>(manifest).unwrap(),
        )
        .map(|(chunks, drafts, reviews)| {
            format!(
                "chunks={} drafts={} reviews={}",
                chunks.len(),
                drafts.len(),
                reviews.len()
            )
        })
        .unwrap_or_else(|| "read artifacts failed".into())
    };
    assert_eq!(
        done.state,
        JobState::Completed,
        "批量读块被预算节流后任务仍应完成：{} error={:?} artifacts=[{artifacts_summary}]",
        done.message,
        done.error_code
    );
    let manifest = manifest_of(&fixture.paths, 5152, &started.job_id);
    assert_eq!(manifest["state"], "completed");
    assert!(
        server.requests_containing("本轮工具结果预算已用尽") > 0,
        "应有至少一次预算跳过提示回传给模型（chunks={}，含 read_chunk 的请求 {} 次）",
        manifest["counts"]["chunks"],
        server
            .requests()
            .iter()
            .filter(|body| body.to_string().contains("read_chunk"))
            .count()
    );
}

/// bulk 测试的有状态进度：跨压缩（历史被摘要）保留各阶段进度。
#[derive(Default)]
struct BulkState {
    chunk_ids: Vec<String>,
    read: Arc<Mutex<BTreeSet<String>>>,
    scanned: BTreeSet<String>,
    frozen: bool,
    translated: bool,
    reviewed: bool,
    review_summary: bool,
    merged: bool,
    global_reviewed: bool,
    published: bool,
}

// ---------------------------------------------------------------------------
// 15. 审查分级、局部修订与按块返工（ADR 0038 端到端回归）
// ---------------------------------------------------------------------------

/// 读取 Job 全部块产物（chunks / drafts / reviews）。
fn read_artifacts(paths: &AppPaths, novel_id: i64, job_id: &str) -> (Vec<Chunk>, Vec<Draft>, Vec<Review>) {
    let manifest: pixiv_tool_lib::agent::Manifest =
        serde_json::from_value(manifest_of(paths, novel_id, job_id)).expect("manifest 可解析");
    pixiv_tool_lib::translation::service::read_job_artifacts(paths, &manifest).expect("读取产物")
}

/// 与翻译子 Agent 脚本一致的预期译文（用于证明未点名的片段没有被改动）。
fn translator_output(kind: SegmentKind, source: &str) -> String {
    match kind {
        SegmentKind::Markup => source.to_string(),
        SegmentKind::Chapter => "[chapter:中文标题]".to_string(),
        SegmentKind::Ruby => "[rb:汉字>hanzi]呀".to_string(),
        SegmentKind::Text => "这是本段的中文译文".to_string(),
    }
}

fn planned_chunk_ids(results: &BTreeMap<String, Value>) -> Vec<String> {
    results
        .get("prepare_source")
        .and_then(|value| value["chunk_ids"].as_array())
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Pass 0～1：准备原文、逐块 read_chunk + submit_scan、冻结约束。
///
/// 已全部完成时返回 `None`。与 [`main_next`] 的前半段同一套动作，只是少了
/// Pass 2 之后的自适应逻辑，便于各回归场景只替换「返工怎么处理」。
fn main_prepare(calls: &[(String, Value)], results: &BTreeMap<String, Value>) -> Option<Reply> {
    let chunk_ids = planned_chunk_ids(results);
    if !called(calls, "prepare_source") {
        return Some(tool_call_stream("m1", "prepare_source", "{}"));
    }
    let scanned: Vec<String> = calls
        .iter()
        .filter(|(tool, _)| tool == "submit_scan")
        .filter_map(|(_, args)| args["chunk_id"].as_str().map(str::to_string))
        .collect();
    for chunk_id in &chunk_ids {
        if !scanned.contains(chunk_id) {
            let read = calls.iter().any(|(tool, args)| {
                tool == "read_chunk" && args["chunk_id"].as_str() == Some(chunk_id.as_str())
            });
            if !read {
                return Some(tool_call_stream(
                    "m2",
                    "read_chunk",
                    &json!({"chunk_id": chunk_id, "include_context": true}).to_string(),
                ));
            }
            let scan = json!({
                "chunk_id": chunk_id,
                "terms": [{
                    "source_term": "勇者",
                    "locked_translation": "勇士",
                    "term_type": "character",
                    "notes": "主角称谓",
                }],
                "style": {"text_type": "小说", "formality": "书面语，对白偏口语"},
                "notes": "叙述视角为第三人称",
                "confidence": "high",
            });
            return Some(tool_call_stream("m3", "submit_scan", &scan.to_string()));
        }
    }
    if !called(calls, "freeze_constraints") {
        return Some(tool_call_stream("m4", "freeze_constraints", "{}"));
    }
    None
}

/// 审查：`c0001` 以 `pass` + 一条 `minor` 通过，其余块 `pass` 且无问题。
fn reviewer_minor_pass(
    calls: &[(String, Value)],
    results: &BTreeMap<String, Value>,
) -> Reply {
    reviewer_next_with(calls, results, |chunk_id, _version, _translation| {
        if chunk_id == "c0001" {
            json!({
                "chunk_id": chunk_id,
                "verdict": "pass",
                "issues": [{
                    "segment_id": review_target_segment(results),
                    "kind": "style",
                    "severity": "minor",
                    "detail": "表达可再润色，不改变原意",
                }],
                "notes": "覆盖与事实核对通过，仅一条润色建议",
            })
        } else {
            json!({
                "chunk_id": chunk_id,
                "verdict": "pass",
                "issues": [],
                "notes": "逐片段对照通过",
            })
        }
    })
}

/// 审查：`c0002` 的首版草稿（v1）判 `needs_fix` + 一条 `major`，修订后（v2）通过。
fn reviewer_fix_then_pass(
    calls: &[(String, Value)],
    results: &BTreeMap<String, Value>,
) -> Reply {
    reviewer_next_with(calls, results, |chunk_id, version, _translation| {
        if chunk_id == "c0002" && version < 2 {
            json!({
                "chunk_id": chunk_id,
                "verdict": "needs_fix",
                "issues": [{
                    "segment_id": review_target_segment(results),
                    "kind": "mistranslation",
                    "severity": "major",
                    "detail": "术语与冻结约束不一致，需要定点修复",
                }],
                "notes": "存在阻塞级问题",
            })
        } else {
            json!({
                "chunk_id": chunk_id,
                "verdict": "pass",
                "issues": [],
                "notes": "复检通过",
            })
        }
    })
}

/// 审查：`c0002` 每轮都判 `needs_fix` + 一条 `major`；`c0001` 正常通过。
fn reviewer_always_fix(
    calls: &[(String, Value)],
    results: &BTreeMap<String, Value>,
) -> Reply {
    reviewer_next_with(calls, results, |chunk_id, _version, _translation| {
        if chunk_id == "c0002" {
            json!({
                "chunk_id": chunk_id,
                "verdict": "needs_fix",
                "issues": [{
                    "segment_id": review_target_segment(results),
                    "kind": "mistranslation",
                    "severity": "major",
                    "detail": "术语与冻结约束不一致，整块仍未收敛",
                }],
                "notes": "仍有阻塞级问题",
            })
        } else {
            json!({
                "chunk_id": chunk_id,
                "verdict": "pass",
                "issues": [],
                "notes": "逐片段对照通过",
            })
        }
    })
}

/// 审查汇总里第一条阻塞问题的（块, 片段）。
fn blocking_issue_target(results: &BTreeMap<String, Value>) -> Option<(String, String)> {
    let summary = results.get("read_review_summary")?;
    for chunk in summary["chunks"].as_array()? {
        if chunk["verdict"].as_str() != Some("needs_fix") {
            continue;
        }
        let chunk_id = chunk["chunk_id"].as_str()?.to_string();
        let segment_id = chunk["issues"].as_array()?.first()?["segment_id"]
            .as_str()?
            .to_string();
        return Some((chunk_id, segment_id));
    }
    None
}

/// 协调者：首轮派发 → 首轮审查 → 读汇总 → `revise_draft` 定点修复 → 复检 → 收尾发布。
///
/// 一次派发翻译、一次整块重译都没有；修复只通过 `revise_draft` 完成。
fn main_next_revise(calls: &[(String, Value)], results: &BTreeMap<String, Value>) -> Reply {
    if let Some(reply) = main_prepare(calls, results) {
        return reply;
    }
    let chunk_ids = planned_chunk_ids(results);
    if call_count(calls, "dispatch_translation") == 0 {
        return tool_call_stream(
            "m6",
            "dispatch_translation",
            &json!({"chunk_ids": chunk_ids}).to_string(),
        );
    }
    if call_count(calls, "dispatch_review") == 0 {
        return tool_call_stream(
            "m7",
            "dispatch_review",
            &json!({"chunk_ids": chunk_ids}).to_string(),
        );
    }
    if !called(calls, "read_review_summary") {
        return tool_call_stream("m9", "read_review_summary", "{}");
    }
    let Some((chunk_id, segment_id)) = blocking_issue_target(results) else {
        return text_stream("审查汇总里没有可定位的阻塞问题，停止继续修复");
    };
    if !called(calls, "revise_draft") {
        let patch = json!({
            "chunk_id": chunk_id,
            "patches": [{"segment_id": segment_id, "text": "定点修订后的译文"}],
        });
        return tool_call_stream("m13", "revise_draft", &patch.to_string());
    }
    // 修订推进了草稿版本，旧审查失效：必须复检同一块。
    if call_count(calls, "dispatch_review") == 1 {
        return tool_call_stream(
            "m7",
            "dispatch_review",
            &json!({"chunk_ids": [chunk_id]}).to_string(),
        );
    }
    if !called(calls, "merge_drafts") {
        return tool_call_stream("m10", "merge_drafts", "{}");
    }
    if !called(calls, "record_global_review") {
        let review = json!({
            "sample_chunk_ids": chunk_ids,
            "notes": "术语、风格、对白、纯净度、标记五项已核",
            "passed": true,
        });
        return tool_call_stream("m11", "record_global_review", &review.to_string());
    }
    if !called(calls, "publish") {
        return tool_call_stream("m12", "publish", "{}");
    }
    text_stream("done")
}

/// 最新一次派发里是否有块被返工额度拒绝。
fn rework_limit_reported(results: &BTreeMap<String, Value>) -> bool {
    results
        .get("dispatch_translation")
        .and_then(|value| value["chunks"].as_array())
        .is_some_and(|chunks| {
            chunks
                .iter()
                .any(|chunk| chunk["status"] == "rework_limit")
        })
}

/// 协调者：按 `job_status` 待办推进；返工额度用尽由运行层收尾（见
/// [`exhausted_after_rework_limit`]），这里只负责正常阶段。
fn main_next_rework_exhausted(
    calls: &[(String, Value)],
    results: &BTreeMap<String, Value>,
) -> Reply {
    if let Some(reply) = main_prepare(calls, results) {
        return reply;
    }
    let last_was_status = calls
        .last()
        .map(|(tool, _)| tool == "job_status")
        .unwrap_or(false);
    if !last_was_status {
        return tool_call_stream("m5", "job_status", "{}");
    }
    let status = results.get("job_status").cloned().unwrap_or(Value::Null);
    let pending = |key: &str| -> Vec<String> {
        status[key]
            .as_array()
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let pending_translate = pending("pending_translate");
    let pending_rework = pending("pending_rework");
    let pending_review = pending("pending_review");
    if !pending_translate.is_empty() {
        return tool_call_stream(
            "m6",
            "dispatch_translation",
            &json!({"chunk_ids": pending_translate}).to_string(),
        );
    }
    if !pending_rework.is_empty() {
        return tool_call_stream(
            "m6",
            "dispatch_translation",
            &json!({"chunk_ids": pending_rework}).to_string(),
        );
    }
    if !pending_review.is_empty() {
        return tool_call_stream(
            "m7",
            "dispatch_review",
            &json!({"chunk_ids": pending_review}).to_string(),
        );
    }
    text_stream("没有剩余待办块")
}

/// 返工额度用尽后的协调者行为：每轮续跑先查一次 `job_status`，再如实上报。
///
/// 不能连续两轮只回文本：运行层会把「空轮 + 有说明」收敛成 `model_declined`，
/// 掩盖真实卡点。这里模拟真实模型「查看状态 → 报告用户」的收尾。
fn exhausted_after_rework_limit(body: &Value) -> Option<Reply> {
    let has_tools = body["tools"]
        .as_array()
        .is_some_and(|tools| !tools.is_empty());
    if !has_tools || role_of_request(body) != "main" {
        return None;
    }
    let (_, results) = tool_history(body);
    if !rework_limit_reported(&results) {
        return None;
    }
    // 新一轮续跑的第一个请求以 user 提示结尾；先查状态，再在下一个回合上报。
    let last_is_user = body["messages"]
        .as_array()
        .and_then(|messages| messages.last())
        .is_some_and(|message| message["role"] == "user");
    if last_is_user {
        Some(tool_call_stream("m5", "job_status", "{}"))
    } else {
        Some(text_stream(
            "返工额度已用尽，按未解决项上报需要人工处理。",
        ))
    }
}

// ---------------------------------------------------------------------------
// 场景 1：minor-only 可以交付
// ---------------------------------------------------------------------------

/// 保护「审查分级放行」：`pass` 携带的 `minor` 不计入未解决、不阻塞发布。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn minor_only_review_still_publishes() {
    let fixture = fixture("minor-pass");
    let handler: Handler =
        Arc::new(|body: &Value| scripted_roles(body, reviewer_minor_pass, main_next));
    let server = MockServer::start(handler).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(
        &service,
        &started.job_id,
        &[
            JobState::Completed,
            JobState::NeedsAttention,
            JobState::Cancelled,
        ],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "仅 minor 建议不得阻断交付：{} error={:?}",
        done.message,
        done.error_code
    );

    // 正式产物齐全：合并稿、阅读器结果与 manifest 终态。
    let dir = job_dir(&fixture.paths, 5150, &started.job_id);
    assert!(
        dir.join("output/merged.md").is_file(),
        "交付必须产出合并稿"
    );
    assert!(
        dir.join("output/result.json").is_file(),
        "交付必须产出正式结果"
    );
    assert_eq!(
        manifest_of(&fixture.paths, 5150, &started.job_id)["state"],
        "completed"
    );
    let result = service.result(5150).await.unwrap().expect("应有正式结果");
    assert_eq!(result.status, PublishStatus::Complete);
    assert_eq!(result.unresolved, 0, "minor 不计入未解决项");

    // novel_translation_issues 视图：带 minor 的块计入 reviewed，但不属于 unresolved。
    let view = service.issues(&started.job_id).await.expect("问题汇总可读");
    assert!(view.total >= 2, "夹具至少两块");
    assert_eq!(
        view.reviewed, view.total,
        "pass（可带 minor）的块都应计入 reviewed"
    );
    assert!(
        view.issues
            .iter()
            .any(|issue| issue.chunk_id == "c0001" && issue.severity == Severity::Minor),
        "minor 建议仍应被记录：{:?}",
        view.issues
    );
    assert!(
        !view
            .unresolved_chunks
            .contains(&"c0001".to_string()),
        "只有 minor 的块不得进入 unresolved：{:?}",
        view.unresolved_chunks
    );

    let (_, drafts, reviews) = read_artifacts(&fixture.paths, 5150, &started.job_id);
    let draft = drafts
        .iter()
        .find(|draft| draft.chunk_id == "c0001")
        .expect("c0001 有草稿");
    assert!(draft.reviewed, "pass 的块草稿必须标记已审查");
    let review = reviews
        .iter()
        .find(|review| review.chunk_id == "c0001")
        .expect("c0001 有审查");
    assert_eq!(review.verdict, ReviewVerdict::Pass);
    assert!(
        review
            .issues
            .iter()
            .all(|issue| issue.severity == Severity::Minor),
        "pass 携带的问题只能是 minor"
    );
}

// ---------------------------------------------------------------------------
// 场景 2：revise_draft 定点修复后交付
// ---------------------------------------------------------------------------

/// 保护「局部修订替代整块重译」：只替换审查点名的片段，其余片段原样保留，
/// 草稿版本递增使旧审查失效，复检通过后正常交付。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revise_draft_fixes_pointed_segment_without_retranslating() {
    let fixture = fixture("revise");
    let handler: Handler = Arc::new(|body: &Value| {
        scripted_roles(body, reviewer_fix_then_pass, main_next_revise)
    });
    let server = MockServer::start(handler).await;
    let service = service(
        &fixture.paths,
        settings_for(&server),
        Arc::new(RecordingSink::new()),
    );

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(
        &service,
        &started.job_id,
        &[
            JobState::Completed,
            JobState::NeedsAttention,
            JobState::Cancelled,
        ],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::Completed,
        "定点修复后应完成交付：{} error={:?}",
        done.message,
        done.error_code
    );

    // 没有第二次整块翻译：c0002 只派发过一次翻译子 Agent。
    assert_eq!(
        server.translator_runs_for("c0002"),
        1,
        "定点修复不得触发整块重译（请求 {} 次）",
        server.translator_requests_for("c0002")
    );

    let (chunks, drafts, reviews) = read_artifacts(&fixture.paths, 5150, &started.job_id);
    let chunk = chunks
        .iter()
        .find(|chunk| chunk.id == "c0002")
        .expect("c0002 块存在");
    let draft = drafts
        .iter()
        .find(|draft| draft.chunk_id == "c0002")
        .expect("c0002 有草稿");
    let review = reviews
        .iter()
        .find(|review| review.chunk_id == "c0002")
        .expect("c0002 有审查");

    assert_eq!(draft.version, 2, "revise_draft 必须把草稿版本推到 2");
    assert_eq!(review.draft_version, 2, "复检必须针对修订后的版本");
    assert_eq!(review.verdict, ReviewVerdict::Pass);
    assert_eq!(
        draft.segments.len(),
        chunk.segments.len(),
        "定点修订不得增删片段"
    );

    let patched_id = chunk
        .segments
        .iter()
        .find(|segment| segment.kind == SegmentKind::Text)
        .expect("c0002 含普通文本片段")
        .id
        .clone();
    let mut patched_seen = false;
    for (source, translated) in chunk.segments.iter().zip(&draft.segments) {
        assert_eq!(source.id, translated.id, "片段顺序不得改变");
        if source.id == patched_id {
            assert_eq!(
                translated.text, "定点修订后的译文",
                "被审查点名的片段应被替换"
            );
            patched_seen = true;
        } else {
            assert_eq!(
                translated.text,
                translator_output(source.kind, &source.text),
                "未被点名的片段必须保持原译：{}",
                source.id
            );
        }
    }
    assert!(patched_seen, "审查点名的片段必须出现在草稿里");

    // 合并稿与正式结果同样存在，说明修复后走完了正常收尾。
    let dir = job_dir(&fixture.paths, 5150, &started.job_id);
    assert!(dir.join("output/merged.md").is_file());
    let result = service.result(5150).await.unwrap().expect("应有正式结果");
    assert_eq!(result.status, PublishStatus::Complete);
}

// ---------------------------------------------------------------------------
// 场景 3：按块返工额度耗尽给出正确错误码
// ---------------------------------------------------------------------------

/// 保护「按块额度 + 收尾错误码」：某块反复不收敛只耗尽自己的额度，
/// 收尾报 `rework_exhausted`（step=review），其余块照常通过审查。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn per_chunk_rework_exhaustion_reports_rework_exhausted() {
    let fixture = fixture("rework-exhausted");
    let handler: Handler = Arc::new(|body: &Value| {
        if let Some(reply) = exhausted_after_rework_limit(body) {
            return reply;
        }
        scripted_roles(body, reviewer_always_fix, main_next_rework_exhausted)
    });
    let server = MockServer::start(handler).await;
    let mut settings = settings_for(&server);
    // 每块只允许一次整块返工，减少测试轮次。
    settings.translation_agent_rework_rounds = 1;
    let service = service(&fixture.paths, settings, Arc::new(RecordingSink::new()));

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(
        &service,
        &started.job_id,
        &[
            JobState::NeedsAttention,
            JobState::Completed,
            JobState::Cancelled,
        ],
    )
    .await;
    assert_eq!(
        done.state,
        JobState::NeedsAttention,
        "额度耗尽后不得冒充完成：{}",
        done.message
    );

    // 额度 1：首轮翻译 + 一次重译；第三次派发被拒绝，不再发翻译请求。
    assert_eq!(
        server.translator_runs_for("c0002"),
        2,
        "额度用尽后不得继续整块重译（请求 {} 次）",
        server.translator_requests_for("c0002")
    );
    assert_eq!(
        server.translator_runs_for("c0001"),
        1,
        "另一块只翻译一次"
    );
    assert!(
        server.requests_containing("rework_limit") > 0,
        "协调者必须收到 rework_limit 的工具结果"
    );

    // 块级隔离：c0001 正常通过审查并标记 reviewed，未解决的只有 c0002。
    let (_, drafts, reviews) = read_artifacts(&fixture.paths, 5150, &started.job_id);
    let good_draft = drafts
        .iter()
        .find(|draft| draft.chunk_id == "c0001")
        .expect("c0001 有草稿");
    assert!(good_draft.reviewed, "正常块应完成审查");
    let good_review = reviews
        .iter()
        .find(|review| review.chunk_id == "c0001")
        .expect("c0001 有审查");
    assert_eq!(good_review.verdict, ReviewVerdict::Pass);

    let bad_review = reviews
        .iter()
        .find(|review| review.chunk_id == "c0002")
        .expect("c0002 有审查");
    assert_eq!(bad_review.verdict, ReviewVerdict::NeedsFix);
    assert!(
        bad_review
            .issues
            .iter()
            .any(|issue| issue.severity == Severity::Major)
    );

    let view = service.issues(&started.job_id).await.expect("问题汇总可读");
    assert!(
        view.unresolved_chunks.contains(&"c0002".to_string()),
        "未收敛块应保留在未解决清单：{:?}",
        view.unresolved_chunks
    );
    assert!(
        !view.unresolved_chunks.contains(&"c0001".to_string()),
        "通过块不得被拖入未解决清单：{:?}",
        view.unresolved_chunks
    );

    // 未发布：没有合并稿，也没有正式结果。
    let dir = job_dir(&fixture.paths, 5150, &started.job_id);
    assert!(
        !dir.join("output/merged.md").is_file(),
        "未收敛的任务不得产出合并稿"
    );
    assert!(
        service.result(5150).await.unwrap().is_none(),
        "未收敛的任务不得产出正式结果"
    );

    // 收尾错误码：额度用尽的阻塞块必须报 rework_exhausted / review，
    // 而不是笼统的 incomplete_translate 或整体复核。
    let manifest = manifest_of(&fixture.paths, 5150, &started.job_id);
    assert_eq!(manifest["state"], "needs_attention");
    assert_eq!(
        done.error_code.as_deref(),
        Some("rework_exhausted"),
        "快照错误码应指向返工耗尽：{:?}（manifest={:?}）",
        done.error_code,
        manifest
    );
    assert_eq!(
        manifest["last_error"]["code"], "rework_exhausted",
        "实际 last_error：{:?}",
        manifest["last_error"]
    );
    assert_eq!(
        manifest["last_error"]["step"], "review",
        "错误必须落在真实卡住的审查阶段：{:?}",
        manifest["last_error"]
    );
    assert_ne!(
        manifest["last_error"]["code"], "incomplete_global_review",
        "块级审查失败不得再报整体复核"
    );
}
