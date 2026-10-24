//! 三协议流式工具调用的离线线协议验收。
//!
//! 全部走本机 `TcpListener` mock：不访问真实 pixiv / 模型服务，不需要 Key。
//! 每个 mock 只按脚本逐条响应，并记录收到的请求（路径、头、JSON 体）供断言。

use std::{
    convert::Infallible,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use futures::StreamExt;
use pixiv_tool_lib::agent::model::providers::{
    ApiFormat, OPENCODE_SESSION_HEADER, ProviderConfig, request_defaults, session_header_for,
};
use rig_agent::{AgentBuilder, agent::MultiTurnStreamItem};
use rig_core::{
    http_client::HttpClientExt,
    message::{Message, StopReason},
    serde_json::{Value, json},
    streaming::{Item, StreamEvent},
    tool::{Tool, ToolContext},
    wasm_compat::WasmCompatSend,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

// ---------------------------------------------------------------------------
// 离线 mock HTTP 服务
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct CapturedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl CapturedRequest {
    fn body_json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("请求体是 JSON")
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

enum Script {
    /// 按顺序逐段写出 SSE 体后关闭连接。
    Sse(Vec<Vec<u8>>),
    /// 一次性 JSON 响应（非流式请求）。
    Json(Value),
    /// 建立响应后持续写 keepalive；客户端断开时置位 `disconnected`。
    Hang,
}

struct MockServer {
    address: String,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    disconnected: Arc<AtomicBool>,
}

impl MockServer {
    async fn start(scripts: Vec<Script>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let disconnected = Arc::new(AtomicBool::new(false));
        let captured = requests.clone();
        let seen_disconnect = disconnected.clone();

        tokio::spawn(async move {
            for script in scripts {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = read_request(&mut socket).await;
                captured.lock().unwrap().push(request);
                match script {
                    Script::Sse(chunks) => write_sse(&mut socket, &chunks).await,
                    Script::Json(body) => {
                        let body = body.to_string();
                        socket
                            .write_all(
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                                     Connection: close\r\nContent-Length: {}\r\n\r\n{}",
                                    body.len(),
                                    body
                                )
                                .as_bytes(),
                            )
                            .await
                            .unwrap();
                    }
                    Script::Hang => {
                        socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                                  Connection: close\r\n\r\n",
                            )
                            .await
                            .unwrap();
                        socket.write_all(b": ready\n\n").await.unwrap();
                        socket.flush().await.unwrap();
                        for _ in 0..400 {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            if socket.write_all(b": keepalive\n\n").await.is_err()
                                || socket.flush().await.is_err()
                            {
                                seen_disconnect.store(true, Ordering::SeqCst);
                                return;
                            }
                        }
                    }
                }
            }
        });

        Self {
            address,
            requests,
            disconnected,
        }
    }

    fn base(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn request(&self, index: usize) -> CapturedRequest {
        self.requests.lock().unwrap()[index].clone()
    }

    fn requests(&self) -> Vec<CapturedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

async fn read_request(socket: &mut TcpStream) -> CapturedRequest {
    let mut raw = Vec::new();
    let (header_end, content_length) = loop {
        let mut buffer = [0u8; 4096];
        let size = socket.read(&mut buffer).await.unwrap();
        assert!(size > 0, "连接在请求头完成前关闭");
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
                .unwrap();
            break (header_end, content_length);
        }
    };
    while raw.len() < header_end + content_length {
        let mut buffer = [0u8; 4096];
        let size = socket.read(&mut buffer).await.unwrap();
        assert!(size > 0, "请求体未读完整");
        raw.extend_from_slice(&buffer[..size]);
    }
    let head = String::from_utf8_lossy(&raw[..header_end]).into_owned();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect();
    CapturedRequest {
        method,
        path,
        headers,
        body: raw[header_end..header_end + content_length].to_vec(),
    }
}

async fn write_sse(socket: &mut TcpStream, chunks: &[Vec<u8>]) {
    socket
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
              Cache-Control: no-cache\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    for chunk in chunks {
        socket.write_all(chunk).await.unwrap();
        socket.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

fn sse_event(event: &str, data: &Value) -> Vec<u8> {
    format!("event: {event}\ndata: {data}\n\n").into_bytes()
}

fn chat_chunk(delta: Value, finish: Option<&str>) -> Vec<u8> {
    let chunk = json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "mock-model",
        "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
    });
    format!("data: {chunk}\n\n").into_bytes()
}

fn chat_tool_call_stream() -> Vec<Vec<u8>> {
    vec![
        chat_chunk(
            json!({"role": "assistant", "tool_calls": [{
                "index": 0,
                "id": "call_1",
                "type": "function",
                "function": {"name": "add", "arguments": "{\"a\":"}
            }]}),
            None,
        ),
        chat_chunk(
            json!({"tool_calls": [{"index": 0, "function": {"arguments": "2,\"b\":3}"}}]}),
            None,
        ),
        chat_chunk(json!({}), Some("tool_calls")),
        b"data: [DONE]\n\n".to_vec(),
    ]
}

fn chat_text_stream(text: &str) -> Vec<Vec<u8>> {
    vec![
        chat_chunk(json!({"role": "assistant", "content": text}), None),
        chat_chunk(json!({}), Some("stop")),
        b"data: [DONE]\n\n".to_vec(),
    ]
}

fn anthropic_tool_call_stream() -> Vec<Vec<u8>> {
    vec![
        sse_event(
            "message_start",
            &json!({"type": "message_start", "message": {
                "id": "msg_mock", "type": "message", "role": "assistant",
                "model": "mock-model", "content": [], "stop_reason": null,
                "stop_sequence": null, "usage": {"input_tokens": 3, "output_tokens": 1}
            }}),
        ),
        sse_event(
            "content_block_start",
            &json!({"type": "content_block_start", "index": 0, "content_block": {
                "type": "tool_use", "id": "call_1", "name": "add", "input": {}
            }}),
        ),
        sse_event(
            "content_block_delta",
            &json!({"type": "content_block_delta", "index": 0, "delta": {
                "type": "input_json_delta", "partial_json": "{\"a\":2,"
            }}),
        ),
        sse_event(
            "content_block_delta",
            &json!({"type": "content_block_delta", "index": 0, "delta": {
                "type": "input_json_delta", "partial_json": "\"b\":3}"
            }}),
        ),
        sse_event(
            "content_block_stop",
            &json!({"type": "content_block_stop", "index": 0}),
        ),
        sse_event(
            "message_delta",
            &json!({"type": "message_delta", "delta": {
                "stop_reason": "tool_use", "stop_sequence": null
            }, "usage": {"output_tokens": 5}}),
        ),
        sse_event("message_stop", &json!({"type": "message_stop"})),
    ]
}

fn anthropic_text_stream(text: &str) -> Vec<Vec<u8>> {
    vec![
        sse_event(
            "message_start",
            &json!({"type": "message_start", "message": {
                "id": "msg_mock", "type": "message", "role": "assistant",
                "model": "mock-model", "content": [], "stop_reason": null,
                "stop_sequence": null, "usage": {"input_tokens": 3, "output_tokens": 1}
            }}),
        ),
        sse_event(
            "content_block_start",
            &json!({"type": "content_block_start", "index": 0, "content_block": {
                "type": "text", "text": ""
            }}),
        ),
        sse_event(
            "content_block_delta",
            &json!({"type": "content_block_delta", "index": 0, "delta": {
                "type": "text_delta", "text": text
            }}),
        ),
        sse_event(
            "content_block_stop",
            &json!({"type": "content_block_stop", "index": 0}),
        ),
        sse_event(
            "message_delta",
            &json!({"type": "message_delta", "delta": {
                "stop_reason": "end_turn", "stop_sequence": null
            }, "usage": {"output_tokens": 5}}),
        ),
        sse_event("message_stop", &json!({"type": "message_stop"})),
    ]
}

fn function_call_item(arguments: &str) -> Value {
    json!({
        "type": "function_call",
        "id": "fc_1",
        "call_id": "call_1",
        "name": "add",
        "arguments": arguments,
        "status": "completed"
    })
}

fn message_item(text: &str) -> Value {
    json!({
        "type": "message",
        "id": "msg_1",
        "role": "assistant",
        "status": "completed",
        "content": [{"type": "output_text", "annotations": [], "logprobs": [], "text": text}]
    })
}

fn responses_base_response(status: &str, output: Value) -> Value {
    json!({
        "id": "resp_mock",
        "object": "response",
        "created_at": 0,
        "status": status,
        "model": "mock-model",
        "output": output,
        "tools": []
    })
}

fn responses_tool_call_stream() -> Vec<Vec<u8>> {
    vec![
        sse_event(
            "response.created",
            &json!({"type": "response.created", "sequence_number": 0,
                "response": responses_base_response("in_progress", json!([]))}),
        ),
        sse_event(
            "response.output_item.added",
            &json!({"type": "response.output_item.added", "output_index": 0,
            "sequence_number": 1, "item": {
                "type": "function_call", "id": "fc_1", "call_id": "call_1",
                "name": "add", "arguments": ""
            }}),
        ),
        sse_event(
            "response.function_call_arguments.delta",
            &json!({"type": "response.function_call_arguments.delta", "output_index": 0,
                "sequence_number": 2, "delta": "{\"a\":2,"}),
        ),
        sse_event(
            "response.function_call_arguments.delta",
            &json!({"type": "response.function_call_arguments.delta", "output_index": 0,
                "sequence_number": 3, "delta": "\"b\":3}"}),
        ),
        sse_event(
            "response.output_item.done",
            &json!({"type": "response.output_item.done", "output_index": 0,
                "sequence_number": 4, "item": function_call_item("{\"a\":2,\"b\":3}")}),
        ),
        sse_event(
            "response.completed",
            &json!({"type": "response.completed", "sequence_number": 5,
            "response": responses_base_response(
                "completed",
                json!([function_call_item("{\"a\":2,\"b\":3}")])
            )}),
        ),
    ]
}

fn responses_text_stream(text: &str) -> Vec<Vec<u8>> {
    let mut added = message_item(text);
    added["content"] = json!([]);
    added["status"] = json!("in_progress");
    vec![
        sse_event(
            "response.created",
            &json!({"type": "response.created", "sequence_number": 0,
                "response": responses_base_response("in_progress", json!([]))}),
        ),
        sse_event(
            "response.output_item.added",
            &json!({"type": "response.output_item.added", "output_index": 0,
                "sequence_number": 1, "item": added}),
        ),
        sse_event(
            "response.output_text.delta",
            &json!({"type": "response.output_text.delta", "item_id": "msg_1",
                "output_index": 0, "content_index": 0, "sequence_number": 2,
                "delta": text}),
        ),
        sse_event(
            "response.output_item.done",
            &json!({"type": "response.output_item.done", "output_index": 0,
                "sequence_number": 3, "item": message_item(text)}),
        ),
        sse_event(
            "response.completed",
            &json!({"type": "response.completed", "sequence_number": 4,
                "response": responses_base_response("completed", json!([message_item(text)]))}),
        ),
    ]
}

// ---------------------------------------------------------------------------
// 测试工具与模型
// ---------------------------------------------------------------------------

struct Add;

impl Tool for Add {
    const NAME: &'static str = "add";
    type Args = Value;
    type Output = Value;
    type Error = Infallible;

    fn description(&self) -> String {
        "Add two integers".to_owned()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "a": {"type": "integer"},
                "b": {"type": "integer"}
            },
            "required": ["a", "b"]
        })
    }

    fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> impl Future<Output = Result<Value, Infallible>> + WasmCompatSend {
        async move {
            let a = args.get("a").and_then(Value::as_i64).unwrap_or_default();
            let b = args.get("b").and_then(Value::as_i64).unwrap_or_default();
            Ok(json!(a + b))
        }
    }
}

#[derive(Default)]
struct Streamed {
    argument_deltas: String,
    tool_calls: usize,
    tool_results: usize,
    final_output: String,
    stop_reason: Option<StopReason>,
}

async fn run_streamed(agent: &rig_agent::Agent) -> Streamed {
    let mut stream = std::pin::pin!(agent.prompt("What is 2 + 3?").max_turns(4).stream());
    let mut collected = Streamed::default();
    while let Some(item) = stream.next().await {
        match item.expect("stream item") {
            MultiTurnStreamItem::StreamAssistantItem(Item::Event(StreamEvent::Arguments {
                json,
                ..
            })) => collected.argument_deltas.push_str(&json),
            MultiTurnStreamItem::ToolCall { tool_call } => {
                assert_eq!(tool_call.function.name.as_str(), "add");
                assert_eq!(tool_call.id.wire(), "call_1");
                collected.tool_calls += 1;
            }
            MultiTurnStreamItem::ToolResult { tool_result } => {
                assert_eq!(tool_result.call.wire(), "call_1");
                collected.tool_results += 1;
            }
            MultiTurnStreamItem::FinalResponse(response) => {
                collected.stop_reason =
                    response
                        .messages
                        .iter()
                        .rev()
                        .find_map(|message| match message {
                            Message::Assistant(assistant) => assistant.stop.clone(),
                            _ => None,
                        });
                collected.final_output = response.output();
            }
            _ => {}
        }
    }
    collected
}

fn build_agent<M>(model: M, max_tokens: Option<u64>, extra: Value) -> rig_agent::Agent
where
    M: Into<rig_core::DynModel<rig_core::operation::Completion>>,
{
    let mut builder = AgentBuilder::new(model).tool(Add);
    if let Some(cap) = max_tokens {
        builder = builder.max_tokens(cap);
    }
    builder = builder.additional_params(extra);
    builder.build()
}

fn request_patch(config: &ProviderConfig) -> (Option<u64>, Value) {
    let defaults = request_defaults(config, 1_000);
    (defaults.max_tokens, defaults.additional_params)
}

#[tokio::test]
async fn chat_streaming_tool_call_runs_and_pins_endpoint() {
    let server = MockServer::start(vec![
        Script::Sse(chat_tool_call_stream()),
        Script::Sse(chat_text_stream("2 + 3 = 5")),
    ])
    .await;
    let config = ProviderConfig::new(
        ApiFormat::ChatCompletions,
        server.base("/v1"),
        "mock-model",
        "test-key",
        json!({
            "max_completion_tokens": 1234,
            "reasoning_effort": "high",
            "custom_flag": true
        }),
        600,
    )
    .unwrap();
    let model = config.chat_model().unwrap();
    let (max_tokens, extra) = request_patch(&config);
    let agent = build_agent(model, max_tokens, extra);
    let collected = run_streamed(&agent).await;

    assert_eq!(collected.argument_deltas, "{\"a\":2,\"b\":3}");
    assert_eq!(collected.tool_calls, 1);
    assert_eq!(collected.tool_results, 1);
    assert_eq!(collected.final_output, "2 + 3 = 5");
    assert_eq!(
        collected.stop_reason,
        Some(rig_core::message::StopReason::Stop),
        "工具调用后应进入正常终态"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 2, "一轮工具调用 + 一次最终回答");
    for request in &requests {
        assert_eq!(request.method, "POST");
        assert_eq!(
            request.path, "/v1/chat/completions",
            "Rig 的路径被配置端点整体替换"
        );
        assert_eq!(request.header("authorization"), Some("Bearer test-key"));
        assert!(request.header(OPENCODE_SESSION_HEADER).is_none());
    }
    let first = requests[0].body_json();
    assert_eq!(first["max_completion_tokens"], 1234, "用户显式上限优先");
    assert_eq!(first["reasoning_effort"], "high", "用户显式思考优先");
    assert_eq!(first["custom_flag"], true, "additional_params 合并");
    assert_eq!(first["stream"], true);
    assert_eq!(first["tools"][0]["type"], "function");
    assert_eq!(first["tools"][0]["function"]["name"], "add");
    assert_eq!(
        first["tools"][0]["function"]["parameters"]["required"],
        json!(["a", "b"])
    );
    let second = requests[1].body_json();
    assert!(
        second["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool"),
        "第二轮必须回传工具结果"
    );
}

#[tokio::test]
async fn responses_streaming_tool_call_uses_protocol_fields() {
    let server = MockServer::start(vec![
        Script::Sse(responses_tool_call_stream()),
        Script::Sse(responses_text_stream("2 + 3 = 5")),
    ])
    .await;
    let config = ProviderConfig::new(
        ApiFormat::Responses,
        server.base("/responses"),
        "mock-model",
        "test-key",
        json!({
            "max_output_tokens": 2048,
            "reasoning_effort": "low",
            "custom_flag": true
        }),
        600,
    )
    .unwrap();
    let model = config.responses_model().unwrap();
    let (max_tokens, extra) = request_patch(&config);
    let agent = build_agent(model, max_tokens, extra);
    let collected = run_streamed(&agent).await;

    assert_eq!(collected.argument_deltas, "{\"a\":2,\"b\":3}");
    assert_eq!(collected.tool_calls, 1);
    assert_eq!(collected.tool_results, 1);
    assert_eq!(collected.final_output, "2 + 3 = 5");
    assert_eq!(
        collected.stop_reason,
        Some(StopReason::Stop),
        "工具调用后应进入正常终态"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.path, "/responses", "配置端点固定为 /responses");
        assert_eq!(request.header("authorization"), Some("Bearer test-key"));
    }
    let first = requests[0].body_json();
    assert_eq!(first["max_output_tokens"], 2048);
    assert_eq!(first["reasoning"]["effort"], "low");
    assert_eq!(first["reasoning"]["summary"], "auto");
    assert_eq!(first["custom_flag"], true);
    assert_eq!(first["stream"], true);
    assert_eq!(first["tools"][0]["type"], "function");
    assert_eq!(first["tools"][0]["name"], "add");
}

#[tokio::test]
async fn anthropic_streaming_tool_call_keeps_no_v1_gateway_path() {
    let server = MockServer::start(vec![
        Script::Sse(anthropic_tool_call_stream()),
        Script::Sse(anthropic_text_stream("2 + 3 = 5")),
    ])
    .await;
    // 自定义网关直接给出 /messages 完整端点（不带 /v1）：路径必须原样固定。
    let config = ProviderConfig::new(
        ApiFormat::Anthropic,
        server.base("/messages"),
        "mock-model",
        "test-key",
        json!({
            "max_tokens": 4096,
            "thinking": {"type": "enabled", "budget_tokens": 1024},
            "temperature": 0.2,
            "custom_flag": true
        }),
        600,
    )
    .unwrap();
    let model = config.anthropic_model().unwrap();
    let (max_tokens, extra) = request_patch(&config);
    let agent = build_agent(model, max_tokens, extra);
    let collected = run_streamed(&agent).await;

    assert_eq!(collected.argument_deltas, "{\"a\":2,\"b\":3}");
    assert_eq!(collected.tool_calls, 1);
    assert_eq!(collected.tool_results, 1);
    assert_eq!(collected.final_output, "2 + 3 = 5");
    assert_eq!(
        collected.stop_reason,
        Some(StopReason::Stop),
        "工具调用后应进入正常终态"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.path, "/messages", "不得被 Rig 改写成 /v1/messages");
        assert_eq!(request.header("x-api-key"), Some("test-key"));
        assert_eq!(request.header("anthropic-version"), Some("2023-06-01"));
        assert!(request.header("authorization").is_none());
    }
    let first = requests[0].body_json();
    assert_eq!(first["max_tokens"], 4096);
    assert_eq!(
        first["thinking"],
        json!({"type": "enabled", "budget_tokens": 1024})
    );
    assert_eq!(first["temperature"], 1, "开启思考后采样参数被归一化");
    assert_eq!(first["custom_flag"], true);
    assert_eq!(first["stream"], true);
    assert_eq!(first["tools"][0]["name"], "add");
    assert_eq!(
        first["tools"][0]["input_schema"]["required"],
        json!(["a", "b"])
    );
}

#[tokio::test]
async fn session_header_is_ignored_for_non_opencode_hosts() {
    let server = MockServer::start(vec![Script::Json(json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "created": 0,
        "model": "mock-model",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "ok"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    }))])
    .await;
    let endpoint = tauri::Url::parse(&server.base("/v1")).unwrap();
    assert!(session_header_for(&endpoint, "session-1").is_none());

    let config = ProviderConfig::new(
        ApiFormat::ChatCompletions,
        server.base("/v1"),
        "mock-model",
        "test-key",
        json!({}),
        600,
    )
    .unwrap()
    .with_session_header(Some((OPENCODE_SESSION_HEADER.into(), "session-1".into())));
    assert!(
        config.session_header.is_none(),
        "非 opencode host 不附加会话头"
    );
    let model = config.chat_model().unwrap();
    let agent = AgentBuilder::new(model).build();
    let response = agent.prompt("hi").await.expect("completion");
    assert_eq!(response.output(), "ok");
    let request = server.request(0);
    assert!(request.header(OPENCODE_SESSION_HEADER).is_none());
    // opencode 判定本身仍为真，供宿主持久化会话 ID 后使用。
    let opencode = tauri::Url::parse("https://opencode.ai/zen/go/v1").unwrap();
    assert_eq!(
        session_header_for(&opencode, "session-1"),
        Some((OPENCODE_SESSION_HEADER.into(), "session-1".into()))
    );
}

#[tokio::test]
async fn dropping_stream_closes_connection_and_releases_gate() {
    let server = MockServer::start(vec![Script::Hang]).await;
    let gate = Arc::new(tokio::sync::Semaphore::new(1));
    let config = ProviderConfig::new(
        ApiFormat::ChatCompletions,
        server.base("/v1"),
        "mock-model",
        "test-key",
        json!({}),
        600,
    )
    .unwrap()
    .with_gate(Some(gate.clone()));
    let transport = config.transport().unwrap();
    assert_eq!(gate.available_permits(), 1);

    let request = http::Request::post("http://invalid.example/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Bytes::from_static(b"{}"))
        .unwrap();
    let response = transport.send_streaming(request).await.expect("stream");
    let mut body = response.into_body();
    assert!(body.next().await.is_some(), "先读到首个 chunk");
    assert_eq!(gate.available_permits(), 0, "请求期间持有闸门许可");
    drop(body);

    tokio::time::timeout(Duration::from_secs(5), async {
        while !server.disconnected.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("drop 流后 mock 侧应观察到连接关闭");

    let _permit = tokio::time::timeout(Duration::from_secs(2), gate.acquire())
        .await
        .expect("取消后许可必须归还")
        .expect("gate 未关闭");
}
