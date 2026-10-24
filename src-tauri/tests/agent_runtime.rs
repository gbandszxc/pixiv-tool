//! 通用运行层离线验收：本地脚本化 HTTP mock 驱动 Chat 协议工具循环。
//!
//! 不访问真实模型 / Pixiv，不需要 Key。覆盖：工具调用后最终文本、事件与用量、
//! 取消、预算耗尽、上下文压缩（历史工具配对）与无进展停止。

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use pixiv_tool_lib::agent::{
    BudgetConfig, BudgetTracker, CancelToken, CompactionConfig, EventSink, JobScope, Phase,
    RecordingSink, Role, RunError, RunLimits, RunRequest, RunStatus, RuntimeConfig, Scheduler,
    model::providers::{ApiFormat, ProviderConfig},
    runtime::run,
};
use rig_agent::tool::{DynamicTool, ToolOutput};
use rig_core::{
    DynModel,
    message::ToolName,
    operation::Completion,
    serde_json::{Value, json},
    tool::ToolExecutionError,
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
    /// 一次性 JSON 响应（非流式请求，如压缩摘要）。
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
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
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
                        for _ in 0..600 {
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

// ---------------------------------------------------------------------------
// Chat 协议脚本
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

fn chat_tool_call_stream(call_id: &str, tool: &str, arguments: &str) -> Vec<Vec<u8>> {
    vec![
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
    ]
}

fn chat_text_stream(text: &str) -> Vec<Vec<u8>> {
    vec![
        chat_chunk(json!({"role": "assistant", "content": text}), None, None),
        chat_chunk(
            json!({}),
            Some("stop"),
            Some(json!({"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15})),
        ),
        b"data: [DONE]\n\n".to_vec(),
    ]
}

fn chat_json_reply(text: &str) -> Value {
    json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "created": 0,
        "model": "mock-model",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 50, "completion_tokens": 20, "total_tokens": 70}
    })
}

// ---------------------------------------------------------------------------
// 运行装配
// ---------------------------------------------------------------------------

fn scope() -> JobScope {
    JobScope {
        novel_id: 11,
        job_id: "20261024210000-abc123".into(),
        generation: 1,
    }
}

fn chat_model(server: &MockServer) -> DynModel<Completion> {
    ProviderConfig::new(
        ApiFormat::ChatCompletions,
        server.base("/v1"),
        "mock-model",
        "test-key",
        json!({}),
        60,
    )
    .expect("provider config")
    .chat_model()
    .expect("chat model")
    .erase()
}

fn tool(name: &str, result: String) -> DynamicTool {
    DynamicTool::new(
        ToolName::new(name).expect("非空工具名"),
        "测试工具：返回固定结果",
        json!({"type": "object", "properties": {}, "required": []}),
        move |_args: Value| {
            let result = result.clone();
            Box::pin(async move { Ok::<_, ToolExecutionError>(ToolOutput::text(result)) })
        },
    )
}

fn request(
    server: &MockServer,
    sink: Arc<RecordingSink>,
    result: String,
    limits: RunLimits,
) -> RunRequest {
    RunRequest::new(
        Role::Translator,
        "你是测试角色。 ".to_owned(),
        "translate the chunk".to_owned(),
        chat_model(server),
        vec![tool("translate_chunk", result)],
        limits,
        sink,
        scope(),
    )
    .with_phase(Phase::Translate)
}

fn enter_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

#[test]
fn tool_call_then_text_completes_and_reports_usage() {
    enter_runtime().block_on(async {
        let server = MockServer::start(vec![
            Script::Sse(chat_tool_call_stream("call_1", "translate_chunk", "{}")),
            Script::Sse(chat_text_stream("translated text")),
        ])
        .await;
        let sink = Arc::new(RecordingSink::new());
        let request = request(&server, sink.clone(), "ok".into(), RunLimits::default());
        let outcome = run(request, CancelToken::new()).await.expect("运行成功");

        assert_eq!(outcome.text, "translated text");
        assert_eq!(outcome.status, RunStatus::Completed);
        assert_eq!(outcome.tool_calls, 1);
        assert_eq!(outcome.usage.input, 10, "使用模型上报的用量");
        assert_eq!(outcome.usage.output, 5);
        assert_eq!(outcome.usage.total, 15);

        let requests = server.requests();
        assert_eq!(requests.len(), 2, "一次工具轮次 + 一次最终回答");
        for request in &requests {
            assert_eq!(request.method, "POST");
            assert_eq!(request.path, "/v1/chat/completions");
            assert_eq!(request.header("authorization"), Some("Bearer test-key"));
        }
        let first = requests[0].body_json();
        assert_eq!(first["max_tokens"], 16_384, "运行层写入输出上限");
        assert_eq!(first["tools"][0]["function"]["name"], "translate_chunk");
        let second = requests[1].body_json();
        assert!(
            second["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool"),
            "第二轮必须回传工具结果"
        );

        let events = sink.events();
        assert!(
            events
                .iter()
                .any(|event| event.preview.summary.contains("translated")),
            "应产生生成预览事件"
        );
        let last = events.last().expect("至少一个事件");
        assert_eq!(last.state, pixiv_tool_lib::agent::JobState::Completed);
        assert_eq!(last.usage.total, 15);
    });
}

#[test]
fn cancellation_drops_the_stream_and_returns_cancelled() {
    enter_runtime().block_on(async {
        let server = MockServer::start(vec![Script::Hang]).await;
        let sink = Arc::new(RecordingSink::new());
        let cancel = CancelToken::new();
        let request = request(&server, sink.clone(), "ok".into(), RunLimits::default());
        let handle = tokio::spawn(run(request, cancel.clone()));
        tokio::time::sleep(Duration::from_millis(150)).await;
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("取消后应尽快结束")
            .expect("任务不应 panic");
        assert!(matches!(result, Err(RunError::Cancelled)), "got {result:?}");

        let last = sink.last().expect("取消事件");
        assert_eq!(last.state, pixiv_tool_lib::agent::JobState::Cancelled);
        assert_eq!(last.error_code.as_deref(), Some("cancelled"));

        tokio::time::timeout(Duration::from_secs(5), async {
            while !server.disconnected.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("取消后必须关闭流连接");
    });
}

#[test]
fn budget_exhaustion_stops_before_the_next_model_call() {
    enter_runtime().block_on(async {
        let server = MockServer::start(vec![
            Script::Sse(chat_tool_call_stream("call_1", "translate_chunk", "{}")),
            Script::Sse(chat_text_stream("never requested")),
        ])
        .await;
        let sink = Arc::new(RecordingSink::new());
        let budget = Arc::new(BudgetTracker::new(1));
        let request = request(&server, sink.clone(), "ok".into(), RunLimits::default())
            .with_budget(budget.clone());
        let result = run(request, CancelToken::new()).await;
        assert!(
            matches!(result, Err(RunError::BudgetExceeded)),
            "预算耗尽必须停止，got {result:?}"
        );
        assert_eq!(server.requests().len(), 1, "第二次模型请求不得发出");
        assert_eq!(budget.snapshot().used, 1);
        let last = sink.last().expect("失败事件");
        assert_eq!(last.error_code.as_deref(), Some("budget_exhausted"));
    });
}

#[test]
fn compaction_summarizes_middle_history_and_keeps_tool_pairing() {
    enter_runtime().block_on(async {
        let server = MockServer::start(vec![
            Script::Sse(chat_tool_call_stream("call_1", "translate_chunk", "{}")),
            Script::Sse(chat_tool_call_stream("call_2", "translate_chunk", "{}")),
            Script::Json(chat_json_reply(
                "目标：翻译；约束：无；进度：2/2；下一步：复核",
            )),
            Script::Sse(chat_text_stream("done")),
        ])
        .await;
        let sink = Arc::new(RecordingSink::new());
        let budget = BudgetConfig {
            context_window: 4_096,
            output_limit: 1_024,
            compact_pct: 50,
            recent_tokens: 1_100,
            summary_tokens: 128,
            call_budget: 0,
        };
        let scheduler = Arc::new(Scheduler::new(&RuntimeConfig {
            global_concurrency: 1,
            novel_concurrency: 1,
            active_novels: 2,
        }));
        // 每次工具返回约 1100 token，两个轮次后必然触发压缩。
        let request = request(
            &server,
            sink.clone(),
            "y".repeat(4_000),
            RunLimits {
                max_turns: 6,
                ..RunLimits::default()
            },
        )
        .with_budget_config(budget.clone())
        .with_compaction(CompactionConfig::new(budget, scope()))
        .with_scheduler(scheduler.clone());
        let outcome = run(request, CancelToken::new()).await.expect("运行成功");
        assert_eq!(outcome.text, "done");

        let requests = server.requests();
        assert_eq!(requests.len(), 4, "工具 ×2 + 摘要 + 最终回答");
        let summary = requests[2].body_json();
        assert_ne!(summary["stream"], true, "摘要请求必须是非流式完成请求");
        let messages = summary["messages"].as_array().expect("messages");
        assert_eq!(messages[0]["role"], "system");
        assert!(
            summary["messages"][0]["content"]
                .to_string()
                .contains("只输出结构化摘要"),
            "摘要请求必须使用固定系统提示"
        );

        let final_body = requests[3].body_json();
        let messages = final_body["messages"].as_array().expect("messages");
        assert!(
            messages.iter().any(|message| message["content"]
                .as_str()
                .is_some_and(|content| content.contains("【历史摘要"))),
            "最终请求应携带摘要消息"
        );
        // 工具调用与结果必须成对：assistant.tool_calls 后紧跟 role=tool。
        for (index, message) in messages.iter().enumerate() {
            if message.get("tool_calls").is_some() {
                let next = messages.get(index + 1).expect("工具结果必须紧跟调用");
                assert_eq!(next["role"], "tool");
                assert_eq!(
                    next["tool_call_id"], message["tool_calls"][0]["id"],
                    "tool_call_id 必须配对"
                );
            }
        }
        // 原历史仍完整：工具调用与结果数量一致。
        let calls = messages
            .iter()
            .filter(|message| message.get("tool_calls").is_some())
            .count();
        let results = messages
            .iter()
            .filter(|message| message["role"] == "tool")
            .count();
        assert_eq!(calls, results);
        assert_eq!(
            scheduler.available_global(),
            1,
            "运行结束后调度许可必须全部归还（含压缩请求）"
        );
    });
}

#[test]
fn repeated_invalid_tool_calls_stop_on_no_progress() {
    enter_runtime().block_on(async {
        let server = MockServer::start(vec![
            Script::Sse(chat_tool_call_stream("call_1", "ghost", "{}")),
            Script::Sse(chat_tool_call_stream("call_2", "ghost", "{}")),
        ])
        .await;
        let sink = Arc::new(RecordingSink::new());
        let request = request(
            &server,
            sink.clone(),
            "ok".into(),
            RunLimits {
                max_turns: 8,
                max_no_progress: 2,
                max_tool_corrections: 10,
            },
        );
        let result = run(request, CancelToken::new()).await;
        assert!(
            matches!(result, Err(RunError::NoProgress)),
            "连续无派发应停止，got {result:?}"
        );
        assert_eq!(server.requests().len(), 2, "第三次调用前已停止");
        let last = sink.last().expect("失败事件");
        assert_eq!(last.error_code.as_deref(), Some("no_progress"));
    });
}

#[test]
fn scheduler_and_novel_permit_are_reusable_by_the_host() {
    enter_runtime().block_on(async {
        let scheduler = Arc::new(Scheduler::new(&RuntimeConfig {
            global_concurrency: 2,
            novel_concurrency: 2,
            active_novels: 1,
        }));
        let novel = scheduler.enter_novel(1).await;
        let permit = scheduler.model_permit(1).await;
        assert!(permit.has_permits());
        drop(permit);
        drop(novel);
        assert_eq!(scheduler.active_novel_count(), 0);
        assert_eq!(scheduler.available_global(), 2);
    });
}

#[test]
fn recording_sink_is_send_sync_for_host_state() {
    fn assert_sink<T: EventSink + Send + Sync + 'static>() {}
    assert_sink::<RecordingSink>();
    let sink = Arc::new(RecordingSink::new());
    assert!(sink.is_empty());
    let _: Arc<dyn EventSink> = sink;
}
