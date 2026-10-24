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
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use pixiv_tool_lib::{
    agent::{
        CacheGeneration, JobScope, JobState, RecordingSink, storage::JobStore,
    },
    paths::AppPaths,
    settings::Settings,
    translation::{
        NovelInput,
        artifacts::PublishStatus,
        service::{ConstraintEdit, ConstraintTermEdit, TranslationService},
        source::parse_source,
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
}

type Handler = Arc<dyn Fn(&Value) -> Reply + Send + Sync>;

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
                    match handler(&body) {
                        Reply::Sse(chunks) => write_sse(&mut socket, &chunks).await,
                        Reply::Json(body) => write_json(&mut socket, &body).await,
                        Reply::Error(status, body) => {
                            write_error(&mut socket, status, &body).await;
                        }
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
                role_of(&system_of(body)) == "translator"
                    && user_of(body).contains(&format!("chunk_id: {chunk_id}"))
            })
            .count()
    }

    fn translator_requests(&self) -> usize {
        self.requests()
            .iter()
            .filter(|body| role_of(&system_of(body)) == "translator")
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
fn role_of(system: &str) -> &'static str {
    if system.contains("# 角色：翻译子 Agent（translator）") {
        "translator"
    } else if system.contains("# 角色：审查子 Agent（reviewer）") {
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
            .join("
"),
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
                .join("
")
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
                    let name = call["function"]["name"].as_str().unwrap_or_default().to_string();
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
    let chunk_id = results
        .get("read_chunk")
        .and_then(|value| value["chunk_id"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| "c0001".to_string());
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
        let review = json!({
            "chunk_id": chunk_id,
            "verdict": "pass",
            "issues": [],
            "notes": "逐片段对照通过",
        });
        return tool_call_stream("r3", "submit_review", &review.to_string());
    }
    text_stream("reviewed")
}

/// 通用脚本：按角色分派；`hint` 从 user prompt 取 chunk_id 供首轮使用。
fn scripted(body: &Value) -> Reply {
    // 上下文压缩的摘要请求不带工具定义：返回一次性 JSON 摘要。
    let has_tools = body["tools"].as_array().is_some_and(|tools| !tools.is_empty());
    if !has_tools {
        return json_reply(
            "1. 目标：完成整篇翻译；2. 约束：已冻结；3. 产物：见块产物；             4. 进度：按检查点；5. 未决：无；6. 下一步：继续派发待办块。",
        );
    }
    let system = system_of(body);
    let (calls, mut results) = tool_history(body);
    if role_of(&system) == "translator" {
        let hint = user_of(body)
            .split("chunk_id: ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .map(|id| id.trim().to_string());
        if let Some(id) = hint {
            results.insert("chunk_id_hint".into(), json!(id));
        }
        return translator_next(&calls, &results);
    }
    if role_of(&system) == "reviewer" {
        let hint = user_of(body)
            .split("chunk_id: ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .map(|id| id.trim().to_string());
        if let Some(id) = hint {
            results.insert("read_chunk".into(), json!({"chunk_id": id}));
        }
        return reviewer_next(&calls, &results);
    }
    main_next(&calls, &results)
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

    let done = wait_for(&service, &started.job_id, &[
        JobState::Completed,
        JobState::NeedsAttention,
        JobState::Cancelled,
        JobState::Paused,
    ])
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

    // 阅读器投影：页行映射与原文一致。
    let result = service
        .result(5150)
        .await
        .expect("读取结果成功")
        .expect("应有正式结果");
    assert_eq!(result.status, PublishStatus::Complete);
    assert_eq!(result.unresolved, 0);
    let source = parse_source(&novel_text(), "zh-CN").unwrap();
    assert_eq!(
        result.pages.len(),
        source.pages.len(),
        "页数与原文一致"
    );
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
    let merged = std::fs::read_to_string(job_dir(&fixture.paths, 5150, &started.job_id).join("output/merged.md"))
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
    let (chunks, drafts, reviews) =
        pixiv_tool_lib::translation::service::read_job_artifacts(
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
        assert!(event.seq > last, "事件 seq 必须单调递增：{last} -> {}", event.seq);
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
    let service = service(&fixture.paths, settings_for(&server), Arc::new(RecordingSink::new()));

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
            if role_of(&system_of(body)) == "translator"
                && blocked.load(Ordering::SeqCst)
                && user_of(body).contains("chunk_id: c0002")
            {
                return Reply::Hang;
            }
            scripted(body)
        }) as Handler
    };
    let server = MockServer::start(handler).await;
    let service = service(&fixture.paths, settings_for(&server), Arc::new(RecordingSink::new()));

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

    let done = wait_for(&service, &started.job_id, &[
        JobState::Completed,
        JobState::NeedsAttention,
    ])
    .await;
    assert_eq!(done.state, JobState::Completed, "恢复后应完成：{}", done.message);
    assert_eq!(
        server.translator_requests_for("c0001"),
        before_c1,
        "已完成且有效的块不得重复请求"
    );
    assert!(server.translator_requests_for("c0002") >= 1, "未完成块应重新请求");
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
    let waiting = wait_for(&service, &started.job_id, &[
        JobState::AwaitingConfirmation,
        JobState::Completed,
        JobState::NeedsAttention,
    ])
    .await;
    assert_eq!(
        waiting.state,
        JobState::AwaitingConfirmation,
        "确认模式应停在 awaiting_confirmation：{}",
        waiting.message
    );
    assert_eq!(server.translator_requests(), 0, "确认前不得开始翻译");
    assert!(waiting.constraint_version >= 1, "冻结后应有约束版本");

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

    let done = wait_for(&service, &started.job_id, &[
        JobState::Completed,
        JobState::NeedsAttention,
    ])
    .await;
    assert_eq!(done.state, JobState::Completed, "确认后应完成：{}", done.message);

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
}

// ---------------------------------------------------------------------------
// 5. 取消
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_stops_immediately_and_keeps_checkpoint() {
    let fixture = fixture("cancel");
    let handler = Arc::new(|body: &Value| {
        if role_of(&system_of(body)) == "translator" {
            return Reply::Hang;
        }
        scripted(body)
    }) as Handler;
    let server = MockServer::start(handler).await;
    let service = service(&fixture.paths, settings_for(&server), Arc::new(RecordingSink::new()));

    let started = service.start(novel(), false).await.expect("启动");
    assert!(
        wait_until(|| server.translator_requests() >= 1, 20_000).await,
        "应至少发起一次翻译请求"
    );
    let before = server.requests().len();
    service.cancel(&started.job_id).await.expect("取消成功");
    let cancelled = service.snapshot(&started.job_id).await.unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);

    // 取消后不再产生新请求。
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.requests().len(), before, "取消后不得继续请求模型");

    // 检查点保留（目录与 manifest 仍在，可查看已提交产物）。
    let dir = job_dir(&fixture.paths, 5150, &started.job_id);
    assert!(dir.join("manifest.json").is_file());
    assert_eq!(manifest_of(&fixture.paths, 5150, &started.job_id)["state"], "cancelled");
}

// ---------------------------------------------------------------------------
// 6. 缓存清理
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cache_clear_bumps_generation_and_rejects_stale_writes() {
    let fixture = fixture("cache");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let service = service(&fixture.paths, settings_for(&server), Arc::new(RecordingSink::new()));

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
    stale.write_atomic("chunks/stale.json", b"{}").expect("清前可写");

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
    assert!(service.generation().get() > scope.generation, "世代必须递增");
}

// ---------------------------------------------------------------------------
// 7. 损坏 manifest
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn corrupt_manifest_is_flagged_and_never_requested() {
    let fixture = fixture("corrupt");
    let server = MockServer::start(Arc::new(|body: &Value| scripted(body))).await;
    let service = service(&fixture.paths, settings_for(&server), Arc::new(RecordingSink::new()));

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
    let service = service(&fixture.paths, settings_for(&server), Arc::new(RecordingSink::new()));

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(&service, &started.job_id, &[
        JobState::Completed,
        JobState::NeedsAttention,
    ])
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
        if role_of(&system_of(body)) == "translator"
            && user_of(body).contains("chunk_id: c0002")
        {
            return Reply::Abort;
        }
        scripted(body)
    }) as Handler;
    let server = MockServer::start(handler).await;
    let service = service(&fixture.paths, settings_for(&server), Arc::new(RecordingSink::new()));

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(&service, &started.job_id, &[
        JobState::Completed,
        JobState::NeedsAttention,
    ])
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
        service.result(5150).await.unwrap().is_none_or(|result| result.status != PublishStatus::Complete),
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
            if role_of(&system_of(body)) == "translator"
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
    let service = service(&fixture.paths, settings_for(&server), Arc::new(RecordingSink::new()));

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(&service, &started.job_id, &[
        JobState::Completed,
        JobState::NeedsAttention,
    ])
    .await;
    assert_eq!(done.state, JobState::Completed, "缩块后应完成：{}", done.message);

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
    assert_eq!(fired.load(Ordering::SeqCst), 1, "超限请求只发生一次，不原样重发");

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
        if role_of(&system_of(body)) == "translator" {
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
    let service = service(&fixture.paths, settings_for(&server), Arc::new(RecordingSink::new()));

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(&service, &started.job_id, &[
        JobState::Completed,
        JobState::NeedsAttention,
    ])
    .await;
    assert_eq!(done.state, JobState::NeedsAttention, "参数错误不得完成：{}", done.message);

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
            if role_of(&system_of(body)) == "translator"
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
    let after_scan = restarted.snapshot(&started.job_id).await.expect("重启后可见");
    // 暂停已是稳定检查点，恢复扫描不改它；仍在跑的状态才转 interrupted。
    assert_eq!(after_scan.state, JobState::Paused, "暂停状态是稳定检查点");
    assert!(
        restarted.recover_interrupted().await.unwrap().is_empty(),
        "暂停任务不在「未结束」集合里"
    );

    // 重建上下文并续跑：已完成块不重复请求。
    restarted.resume(&started.job_id).await.expect("重启后恢复成功");
    let done = wait_until_async(&restarted, &started.job_id, &[
        JobState::Completed,
        JobState::NeedsAttention,
    ])
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
    assert!(server.translator_requests_for("c0002") >= 1, "未完成块应重新请求");

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
    let service = service(&fixture.paths, settings_for(&server), Arc::new(RecordingSink::new()));

    let started = service.start(novel(), false).await.expect("启动");
    let done = wait_for(&service, &started.job_id, &[
        JobState::Completed,
        JobState::NeedsAttention,
    ])
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
        records.iter().any(|record| record["kind"] == "subtask_translate_done"),
        "应有翻译子任务记录"
    );
    assert!(
        records.iter().any(|record| record["kind"] == "subtask_review_done"),
        "应有审查子任务记录"
    );
    assert!(
        records.iter().any(|record| record["kind"] == "main_run"),
        "应有主运行记录"
    );
    for record in &records {
        let keys: Vec<&String> = record.as_object().unwrap().keys().collect();
        for forbidden in ["text", "content", "prompt", "summary", "source", "translation"] {
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
