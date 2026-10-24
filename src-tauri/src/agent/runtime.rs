//! 单次 Agent 运行：Rig 工具循环 + 取消 + 预算 + 压缩 + 有限纠正。
//!
//! 运行层与业务无关：工具由调用方注入，子 Agent 只能通过调用方提供的
//! `dispatch_*` 工具启动（runtime 不内置派生能力）。
//!
//! 事件：预览节流由 [`JobEmitter`](crate::agent::events::JobEmitter) 负责；本模块
//! 只按「首条、≥120ms、终态」产生预览事件，避免逐 token 刷屏。

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use futures::StreamExt;
use rig_agent::AgentBuilder;
use rig_agent::agent::{
    AgentHook, CompletionCallAction, CompletionCallEvent, DispatchAction, DispatchEvent,
    HookContext, InvalidToolCallAction, InvalidToolCallContext, InvalidToolCallReason,
    MultiTurnStreamItem, ObservationAction, OutcomeAction, OutcomeEvent, ReasoningDelta,
    RequestPatch, TextDelta,
};
use rig_agent::completion::PromptError;
use rig_core::{
    DynModel, completion::Usage, message::Message, operation::Completion, tool::DynamicTool,
};

use crate::agent::{
    BudgetConfig, BudgetTracker, CancelToken, CurrentWork, FailureClass, JobScope, JobState,
    ModelPermit, Phase, Preview, Role, Scheduler, TokenUsage,
    budget::{estimate_tokens, request_fits},
    compaction::{CompactionConfig, compact_now, should_compact_tokens},
    events::{EventSink, debug_job, log_job, runtime_event, safe_text, warn_job},
    recovery::classify_http,
};

/// 预览事件的最小间隔（events 层还有 300ms 的合并窗口）。
const PREVIEW_EMIT_INTERVAL: Duration = Duration::from_millis(120);

/// 单次运行限制。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunLimits {
    /// 模型请求轮次上限（一轮 = 一次模型调用，不含传输层重试）。
    pub max_turns: u32,
    /// 连续无进展轮次上限。
    pub max_no_progress: u32,
    /// 工具参数 / 结构纠正次数上限。
    pub max_tool_corrections: u32,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_turns: 24,
            max_no_progress: 8,
            max_tool_corrections: 2,
        }
    }
}

/// 运行终态（仅 `Completed` 会带回 [`RunOutcome`]；其余以 [`RunError`] 返回）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Completed,
    Cancelled,
    BudgetExceeded,
    NoProgress,
    MaxTurns,
    CompactionFailed,
    InputTooLarge,
    Failed,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::BudgetExceeded => "budget_exceeded",
            Self::NoProgress => "no_progress",
            Self::MaxTurns => "max_turns",
            Self::CompactionFailed => "compaction_failed",
            Self::InputTooLarge => "input_too_large",
            Self::Failed => "failed",
        }
    }
}

/// 运行错误。`Cancelled` 与业务失败分开，调用方据此决定 Job 状态。
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("运行已取消")]
    Cancelled,
    #[error("模型调用预算耗尽")]
    BudgetExceeded,
    #[error("连续无进展，停止运行")]
    NoProgress,
    #[error("上下文压缩失败: {0}")]
    CompactionFailed(String),
    #[error("输入超出上下文窗口")]
    InputTooLarge,
    #[error("工具参数纠正次数用尽")]
    InvalidToolArgs,
    #[error("达到模型轮次上限 {0}")]
    MaxTurns(u32),
    #[error("模型请求失败（{}）：{}", class.as_str(), message)]
    Provider {
        class: FailureClass,
        code: Option<String>,
        message: String,
    },
    #[error("运行内部错误: {0}")]
    Internal(String),
}

impl RunError {
    /// 安全错误码（写事件 / 日志用）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::BudgetExceeded => "budget_exhausted",
            Self::NoProgress => "no_progress",
            Self::CompactionFailed(_) => "compaction_failed",
            Self::InputTooLarge => "input_too_large",
            Self::InvalidToolArgs => "invalid_tool_args",
            Self::MaxTurns(_) => "max_turns",
            Self::Provider { class, .. } => class.as_str(),
            Self::Internal(_) => "internal",
        }
    }

    pub fn status(&self) -> RunStatus {
        match self {
            Self::Cancelled => RunStatus::Cancelled,
            Self::BudgetExceeded => RunStatus::BudgetExceeded,
            Self::NoProgress => RunStatus::NoProgress,
            Self::CompactionFailed(_) => RunStatus::CompactionFailed,
            Self::InputTooLarge => RunStatus::InputTooLarge,
            Self::MaxTurns(_) => RunStatus::MaxTurns,
            Self::InvalidToolArgs | Self::Provider { .. } | Self::Internal(_) => RunStatus::Failed,
        }
    }
}

/// 单次运行结果。
#[derive(Clone, Debug)]
pub struct RunOutcome {
    /// 最终回答文本（可能为空，但不能作为“已完成”的业务判据）。
    pub text: String,
    pub usage: TokenUsage,
    pub tool_calls: u32,
    pub status: RunStatus,
}

/// 运行请求。业务资源与工具由调用方装配；`system` 必须包含角色定义、工具契约
/// 与数据边界。
pub struct RunRequest {
    pub role: Role,
    pub system: String,
    pub prompt: String,
    pub model: DynModel<Completion>,
    pub tools: Vec<DynamicTool>,
    pub limits: RunLimits,
    pub sink: Arc<dyn EventSink>,
    pub scope: JobScope,
    /// 任务级调用预算；`None` 表示本次运行不额外计数（生产应始终传入）。
    pub budget: Option<Arc<BudgetTracker>>,
    /// 上下文预算与压缩参数。
    pub budget_config: BudgetConfig,
    /// 压缩配置；`None` 表示不做自动压缩（超限直接停止）。
    pub compaction: Option<CompactionConfig>,
    /// 模型请求调度器；传入后每次模型请求取单小说 + 全局许可。
    pub scheduler: Option<Arc<Scheduler>>,
    /// 事件与日志使用的阶段。
    pub phase: Phase,
    /// 本次运行使用的输出上限；`None` 取 [`BudgetConfig::output_limit`]。
    pub max_tokens: Option<u64>,
    /// 调用方按协议归一化后的请求扩展字段（不含凭据与保留字段）。
    pub additional_params: Option<serde_json::Value>,
}

impl RunRequest {
    /// 冻结接口的最小构造；其余字段使用推荐默认值。
    pub fn new(
        role: Role,
        system: String,
        prompt: String,
        model: DynModel<Completion>,
        tools: Vec<DynamicTool>,
        limits: RunLimits,
        sink: Arc<dyn EventSink>,
        scope: JobScope,
    ) -> Self {
        Self {
            phase: role.default_phase(),
            role,
            system,
            prompt,
            model,
            tools,
            limits,
            sink,
            scope,
            budget: None,
            budget_config: BudgetConfig::default(),
            compaction: None,
            scheduler: None,
            max_tokens: None,
            additional_params: None,
        }
    }

    /// 设置本次请求的输出上限与协议扩展字段。
    pub fn with_request_params(
        mut self,
        max_tokens: Option<u64>,
        additional_params: Option<serde_json::Value>,
    ) -> Self {
        self.max_tokens = max_tokens;
        self.additional_params = additional_params;
        self
    }

    pub fn with_budget(mut self, budget: Arc<BudgetTracker>) -> Self {
        self.budget = Some(budget);
        self
    }

    pub fn with_budget_config(mut self, budget_config: BudgetConfig) -> Self {
        self.budget_config = budget_config;
        self
    }

    pub fn with_compaction(mut self, compaction: CompactionConfig) -> Self {
        self.compaction = Some(compaction);
        self
    }

    /// 注入模型请求调度器（压缩摘要会复用同一调度器与预算）。
    pub fn with_scheduler(mut self, scheduler: Arc<Scheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }

    pub fn with_phase(mut self, phase: Phase) -> Self {
        self.phase = phase;
        self
    }
}

struct CompactionCache {
    /// 上一次压缩得到的完整投影。
    compacted: Vec<Arc<Message>>,
    /// 生成该投影时的原始历史长度；其后的消息原样追加。
    original_len: usize,
}

struct RunState {
    scope: JobScope,
    role: Role,
    phase: Phase,
    sink: Arc<dyn EventSink>,
    limits: RunLimits,
    budget: Option<Arc<BudgetTracker>>,
    budget_config: BudgetConfig,
    compaction: Option<CompactionConfig>,
    scheduler: Option<Arc<Scheduler>>,
    /// 当前模型请求持有的调度许可；在 on_outcome 释放，避免工具执行期间占用。
    model_permit: Mutex<Option<ModelPermit>>,
    model: DynModel<Completion>,
    /// 系统提示词 + 工具 schema 的固定输入开销。
    overhead_tokens: u32,
    cancel: CancelToken,
    started: Instant,
    usage: Mutex<TokenUsage>,
    tool_calls: AtomicU32,
    tool_dispatches: AtomicU32,
    last_turn_dispatches: AtomicU32,
    model_calls: AtomicU32,
    stall: AtomicU32,
    corrections: AtomicU32,
    stop_code: Mutex<Option<String>>,
    preview: Mutex<Preview>,
    last_preview_emit: Mutex<Option<Instant>>,
    compaction_cache: Mutex<Option<CompactionCache>>,
}

impl RunState {
    fn usage(&self) -> TokenUsage {
        self.usage.lock().map(|usage| *usage).unwrap_or_default()
    }

    fn add_usage(&self, usage: &Usage) {
        self.add_tokens(TokenUsage::from_rig(usage));
    }

    fn add_tokens(&self, tokens: TokenUsage) {
        if let Ok(mut total) = self.usage.lock() {
            total.add(tokens);
        }
    }

    fn set_usage(&self, usage: TokenUsage) {
        if let Ok(mut total) = self.usage.lock() {
            *total = usage;
        }
    }

    /// 记录当前实现触发的停止原因（用于事件与日志）。
    fn stop(&self, code: &str) -> CompletionCallAction {
        if let Ok(mut stop) = self.stop_code.lock() {
            *stop = Some(code.to_owned());
        }
        // 终态事件由 run() 的统一收尾发出，避免同一次停止重复发终态。
        CompletionCallAction::stop(code)
    }

    fn emit(&self, state: JobState, preview: Preview, message: &str, error_code: Option<&str>) {
        let event = runtime_event(
            &self.scope,
            state,
            self.phase,
            self.usage(),
            preview,
            message,
            error_code,
        );
        self.sink.emit(event);
    }

    fn maybe_emit_preview(&self, force: bool) {
        let now = Instant::now();
        let should_emit = {
            let mut last = self
                .last_preview_emit
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            match *last {
                None => {
                    *last = Some(now);
                    true
                }
                Some(previous)
                    if force || now.duration_since(previous) >= PREVIEW_EMIT_INTERVAL =>
                {
                    *last = Some(now);
                    true
                }
                Some(_) => false,
            }
        };
        if should_emit {
            let preview = self
                .preview
                .lock()
                .map(|preview| preview.clone())
                .unwrap_or_default();
            self.emit(JobState::Running, preview, "preview", None);
        }
    }

    fn append_preview(&self, text: &str, reasoning: bool) {
        if let Ok(mut preview) = self.preview.lock() {
            if reasoning {
                preview.reasoning.push_str(text);
            } else {
                preview.summary.push_str(text);
            }
            // 预览只用于界面；限制内存占用，不在这里截断业务产物。
            const MAX_PREVIEW_BYTES: usize = 32 * 1024;
            if preview.summary.len() > MAX_PREVIEW_BYTES {
                let tail: String = preview
                    .summary
                    .chars()
                    .rev()
                    .take(MAX_PREVIEW_BYTES / 4)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                preview.summary = tail;
            }
            if preview.reasoning.len() > MAX_PREVIEW_BYTES {
                preview.reasoning.clear();
            }
        }
    }

    /// 当前上下文投影（压缩缓存 + 压缩后新增的消息）。
    fn projection(&self, history: &[Arc<Message>]) -> Vec<Arc<Message>> {
        let cache = self
            .compaction_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        match cache.as_ref() {
            Some(cache) if history.len() >= cache.original_len => {
                let mut projection = cache.compacted.clone();
                projection.extend(history[cache.original_len..].iter().cloned());
                projection
            }
            _ => history.to_vec(),
        }
    }

    fn release_model_permit(&self) {
        if let Ok(mut permit) = self.model_permit.lock() {
            *permit = None;
        }
    }

    fn store_projection(&self, compacted: Vec<Arc<Message>>, original_len: usize) {
        if let Ok(mut cache) = self.compaction_cache.lock() {
            *cache = Some(CompactionCache {
                compacted,
                original_len,
            });
        }
    }
}

/// 运行层 hook：预算、压缩、预览、工具观察与有限纠正。
struct RuntimeHooks {
    state: Arc<RunState>,
}

impl AgentHook for RuntimeHooks {
    fn name(&self) -> Option<String> {
        Some(format!("runtime:{}", self.state.role.as_str()))
    }

    async fn on_completion_call(
        &self,
        _ctx: &HookContext,
        event: CompletionCallEvent<'_>,
    ) -> CompletionCallAction {
        let state = &self.state;
        if state.cancel.is_cancelled() {
            return state.stop("cancelled");
        }

        // 无进展检测：上一次模型调用之后没有任何新的工具派发。
        // 用「模型调用次数」而不是 turn 判断，重试同一轮也会累计。
        let turn = event.turn as u32;
        let calls = state.model_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if calls > 1 {
            let dispatches = state.tool_dispatches.load(Ordering::SeqCst);
            let previous_dispatches = state
                .last_turn_dispatches
                .swap(dispatches, Ordering::SeqCst);
            if dispatches == previous_dispatches {
                let stall = state.stall.fetch_add(1, Ordering::SeqCst) + 1;
                if stall >= state.limits.max_no_progress.max(1) {
                    return state.stop("no_progress");
                }
            } else {
                state.stall.store(0, Ordering::SeqCst);
            }
        }

        if let Some(budget) = &state.budget
            && !budget.try_consume_call()
        {
            warn_job(
                &state.scope,
                state.role.as_str(),
                state.phase,
                None,
                "budget_exhausted",
                &[("turn", &turn.to_string())],
            );
            return state.stop("budget_exhausted");
        }

        let prompt = event.prompt;
        let history = event.history;
        let mut projection = state.projection(history);
        let mut input_est =
            projected_input(&projection, Some(prompt)).saturating_add(state.overhead_tokens);

        if let Some(config) = &state.compaction
            && should_compact_tokens(input_est, config)
        {
            match compact_now(&state.model, &projection, config, &state.cancel).await {
                Ok(outcome) => {
                    state.add_tokens(outcome.usage);
                    state.store_projection(outcome.history.clone(), history.len());
                    projection = outcome.history;
                    input_est = projected_input(&projection, Some(prompt))
                        .saturating_add(state.overhead_tokens);
                    log_job(
                        &state.scope,
                        state.role.as_str(),
                        state.phase,
                        None,
                        "completion_compacted",
                        &[
                            ("turn", &turn.to_string()),
                            ("after_est", &input_est.to_string()),
                        ],
                    );
                }
                Err(crate::agent::CompactionError::Cancelled) => {
                    return state.stop("cancelled");
                }
                Err(crate::agent::CompactionError::BudgetExhausted) => {
                    return state.stop("budget_exhausted");
                }
                Err(crate::agent::CompactionError::NothingToCompact) => {
                    // 没有可切分的旧历史：交给后面的窗口预算检查决定是否停止。
                    warn_job(
                        &state.scope,
                        state.role.as_str(),
                        state.phase,
                        None,
                        "compact_skipped",
                        &[("reason", "nothing_to_compact")],
                    );
                }
                Err(error) => {
                    warn_job(
                        &state.scope,
                        state.role.as_str(),
                        state.phase,
                        None,
                        "compaction_failed",
                        &[("turn", &turn.to_string()), ("reason", error.code())],
                    );
                    return state.stop("compaction_failed");
                }
            }
        }

        let config = state.budget_config.clone().normalized();
        if !request_fits(input_est, config.output_limit, config.context_window) {
            warn_job(
                &state.scope,
                state.role.as_str(),
                state.phase,
                None,
                "input_too_large",
                &[
                    ("turn", &turn.to_string()),
                    ("input_est", &input_est.to_string()),
                    ("window", &config.context_window.to_string()),
                ],
            );
            return state.stop("input_too_large");
        }

        debug_job(
            &state.scope,
            state.role.as_str(),
            state.phase,
            None,
            "completion_call",
            &[
                ("turn", &turn.to_string()),
                ("input_est", &input_est.to_string()),
            ],
        );

        // 每次模型请求都要经过调度器；工具执行与等待子任务期间不持有许可。
        if let Some(scheduler) = &state.scheduler {
            state.release_model_permit();
            let permit = scheduler.model_permit(state.scope.novel_id).await;
            if let Ok(mut held) = state.model_permit.lock() {
                *held = Some(permit);
            }
        }

        if projection.len() == history.len() && state.compaction_cache_is_empty() {
            CompletionCallAction::continue_run()
        } else {
            CompletionCallAction::patch(RequestPatch::new().history(projection))
        }
    }

    async fn on_text_delta(&self, _ctx: &HookContext, event: TextDelta<'_>) -> ObservationAction {
        self.state.append_preview(event.delta, false);
        if self.state.cancel.is_cancelled() {
            return ObservationAction::stop("cancelled");
        }
        self.state.maybe_emit_preview(false);
        ObservationAction::continue_run()
    }

    async fn on_reasoning_delta(
        &self,
        _ctx: &HookContext,
        event: ReasoningDelta<'_>,
    ) -> ObservationAction {
        self.state.append_preview(event.delta, true);
        if self.state.cancel.is_cancelled() {
            return ObservationAction::stop("cancelled");
        }
        self.state.maybe_emit_preview(false);
        ObservationAction::continue_run()
    }

    async fn on_dispatch(&self, _ctx: &HookContext, event: DispatchEvent<'_>) -> DispatchAction {
        if let Some(name) = event.tool_name() {
            self.state.tool_dispatches.fetch_add(1, Ordering::SeqCst);
            debug_job(
                &self.state.scope,
                self.state.role.as_str(),
                self.state.phase,
                None,
                "tool_start",
                &[("turn", &event.turn.to_string()), ("tool", name)],
            );
        }
        DispatchAction::proceed()
    }

    async fn on_outcome(&self, _ctx: &HookContext, event: OutcomeEvent<'_>) -> OutcomeAction {
        if event.completion().is_some() {
            // 用量由流里的 CompletionCall 统一累计（避免与 hook 重复计数）；
            // 模型回答已到：立即归还调度许可，随后可能进入工具执行。
            self.state.release_model_permit();
        }
        if let Some(name) = event.tool_name() {
            let failed = event
                .tool_result()
                .map(|result| result.is_error())
                .unwrap_or(true);
            debug_job(
                &self.state.scope,
                self.state.role.as_str(),
                self.state.phase,
                None,
                "tool_end",
                &[
                    ("turn", &event.turn.to_string()),
                    ("tool", name),
                    ("status", if failed { "error" } else { "ok" }),
                ],
            );
        }
        OutcomeAction::proceed()
    }

    async fn on_invalid_tool_call(
        &self,
        _ctx: &HookContext,
        event: &InvalidToolCallContext,
    ) -> Option<InvalidToolCallAction> {
        let corrections = self.state.corrections.fetch_add(1, Ordering::SeqCst) + 1;
        let available = event.available_tools.join(",");
        warn_job(
            &self.state.scope,
            self.state.role.as_str(),
            self.state.phase,
            None,
            "invalid_tool_call",
            &[
                ("tool", &event.tool_name),
                ("attempt", &corrections.to_string()),
                ("limit", &self.state.limits.max_tool_corrections.to_string()),
            ],
        );
        if corrections > self.state.limits.max_tool_corrections {
            if let Ok(mut stop) = self.state.stop_code.lock() {
                *stop = Some("invalid_tool_args".into());
            }
            return Some(InvalidToolCallAction::stop("invalid_tool_args"));
        }
        Some(InvalidToolCallAction::retry(format!(
            "工具调用无效：{}。只使用已提供的工具（{}），参数必须是合法 JSON 对象。",
            invalid_reason(event),
            if available.is_empty() {
                "无"
            } else {
                &available
            }
        )))
    }
}

impl RunState {
    fn compaction_cache_is_empty(&self) -> bool {
        self.compaction_cache
            .lock()
            .map(|cache| cache.is_none())
            .unwrap_or(true)
    }
}

fn invalid_reason(event: &InvalidToolCallContext) -> &'static str {
    match &event.reason {
        InvalidToolCallReason::UnknownTool => "未知工具",
        InvalidToolCallReason::DisallowedByToolChoice => "当前轮次不允许该工具",
        InvalidToolCallReason::MalformedArguments { .. } => "参数不是合法 JSON 对象",
        _ => "工具调用无效",
    }
}

fn tool_tokens(tool: &DynamicTool) -> u32 {
    let definition = tool.definition();
    estimate_tokens(&definition.name.to_string())
        .saturating_add(estimate_tokens(&definition.description))
        .saturating_add(estimate_tokens(&definition.parameters.to_string()))
        .saturating_add(16)
}

fn projected_input(history: &[Arc<Message>], prompt: Option<&Message>) -> u32 {
    let history_tokens = history
        .iter()
        .map(|message| message_tokens(message))
        .fold(0u32, |acc, tokens| acc.saturating_add(tokens));
    let prompt_tokens = prompt.map(|prompt| message_tokens(prompt)).unwrap_or(0);
    history_tokens.saturating_add(prompt_tokens)
}

/// 按真实文本长度估算消息 token；工具结果必须计入正文长度，否则预算会被低估。
fn message_tokens(message: &Message) -> u32 {
    match message {
        Message::System { content } => estimate_tokens(content),
        Message::User { content } => content
            .iter()
            .map(|part| match part {
                rig_core::message::UserContent::Text(text) => estimate_tokens(&text.text),
                rig_core::message::UserContent::ToolResult(result) => result
                    .content
                    .iter()
                    .map(|content| match content {
                        rig_core::message::ToolResultContent::Text(text) => {
                            estimate_tokens(&text.text)
                        }
                        rig_core::message::ToolResultContent::Json { value } => {
                            estimate_tokens(&value.to_string())
                        }
                        _ => 16,
                    })
                    .fold(16u32, |acc, tokens| acc.saturating_add(tokens)),
                _ => 16,
            })
            .fold(0u32, |acc, tokens| acc.saturating_add(tokens)),
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .map(|part| match part {
                rig_core::message::AssistantContent::Text(text) => estimate_tokens(&text.text),
                rig_core::message::AssistantContent::ToolCall(call) => {
                    estimate_tokens(&call.function.name.to_string()).saturating_add(16)
                }
                _ => 0,
            })
            .fold(0u32, |acc, tokens| acc.saturating_add(tokens)),
    }
}

/// 执行一次 Agent 运行。
pub async fn run(req: RunRequest, cancel: CancelToken) -> Result<RunOutcome, RunError> {
    let overhead_tokens =
        estimate_tokens(&req.system).saturating_add(req.tools.iter().map(tool_tokens).sum::<u32>());
    let state = Arc::new(RunState {
        scope: req.scope.clone(),
        role: req.role,
        phase: req.phase,
        sink: req.sink.clone(),
        limits: req.limits,
        budget: req.budget.clone(),
        budget_config: req.budget_config.clone(),
        compaction: {
            let mut compaction = req.compaction.clone();
            if let (Some(config), Some(scheduler)) = (compaction.as_mut(), req.scheduler.as_ref()) {
                if config.scheduler.is_none() {
                    config.scheduler = Some(scheduler.clone());
                }
            }
            compaction
        },
        scheduler: req.scheduler.clone(),
        model_permit: Mutex::new(None),
        model: req.model.clone(),
        overhead_tokens,
        cancel: cancel.clone(),
        started: Instant::now(),
        usage: Mutex::new(TokenUsage::default()),
        tool_calls: AtomicU32::new(0),
        tool_dispatches: AtomicU32::new(0),
        last_turn_dispatches: AtomicU32::new(0),
        model_calls: AtomicU32::new(0),
        stall: AtomicU32::new(0),
        corrections: AtomicU32::new(0),
        stop_code: Mutex::new(None),
        preview: Mutex::new(Preview::default()),
        last_preview_emit: Mutex::new(None),
        compaction_cache: Mutex::new(None),
    });

    log_job(
        &state.scope,
        state.role.as_str(),
        state.phase,
        None,
        "run_start",
        &[
            ("max_turns", &state.limits.max_turns.to_string()),
            ("tools", &req.tools.len().to_string()),
        ],
    );
    state.emit(JobState::Running, Preview::default(), "run_start", None);

    let max_tokens = req
        .max_tokens
        .unwrap_or(req.budget_config.normalized().output_limit as u64);
    let mut builder = AgentBuilder::new(req.model)
        .preamble(req.system)
        .max_tokens(max_tokens)
        .default_max_turns(req.limits.max_turns.max(1) as usize)
        .add_hook(RuntimeHooks {
            state: state.clone(),
        });
    if let Some(params) = req.additional_params.clone() {
        builder = builder.additional_params(params);
    }
    for tool in req.tools {
        builder = builder.dynamic_tool(tool);
    }
    let agent = builder.build();

    let stream = agent
        .prompt(req.prompt)
        .max_turns(req.limits.max_turns.max(1) as usize)
        .max_invalid_tool_call_retries(req.limits.max_tool_corrections as usize)
        .stream();
    let mut stream = std::pin::pin!(stream);

    let mut final_text = String::new();
    let mut final_usage: Option<Usage> = None;
    let mut outcome: Option<Result<(), RunError>> = None;

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                outcome = Some(Err(RunError::Cancelled));
                break;
            }
            item = stream.next() => {
                let Some(item) = item else { break };
                match item {
                    Ok(MultiTurnStreamItem::CompletionCall(call)) => state.add_usage(&call.usage),
                    Ok(MultiTurnStreamItem::ToolCall { .. }) => {
                        state.tool_calls.fetch_add(1, Ordering::SeqCst);
                    }
                    Ok(MultiTurnStreamItem::FinalResponse(response)) => {
                        final_text = response.output();
                        final_usage = Some(response.usage);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        outcome = Some(Err(map_prompt_error(error, &state)));
                        break;
                    }
                }
            }
        }
    }
    drop(stream);

    if let Some(Err(error)) = outcome {
        return finish_error(&state, error);
    }
    if cancel.is_cancelled() {
        return finish_error(&state, RunError::Cancelled);
    }
    match final_usage {
        Some(usage) => {
            // 已按 CompletionCall 累计（含压缩摘要）；仅当一次都没上报时才回落到
            // 聚合用量，避免覆盖压缩等运行外调用的计数。
            let accumulated = state.usage();
            if accumulated.input == 0 && accumulated.output == 0 && accumulated.total == 0 {
                state.set_usage(TokenUsage::from_rig(&usage));
            }
        }
        None => {
            return finish_error(
                &state,
                RunError::Internal("模型流结束时没有最终响应".into()),
            );
        }
    }

    let outcome = RunOutcome {
        text: final_text.clone(),
        usage: state.usage(),
        tool_calls: state.tool_calls.load(Ordering::SeqCst),
        status: RunStatus::Completed,
    };
    log_job(
        &state.scope,
        state.role.as_str(),
        state.phase,
        None,
        "run_done",
        &[
            ("tool_calls", &outcome.tool_calls.to_string()),
            ("input", &outcome.usage.input.to_string()),
            ("output", &outcome.usage.output.to_string()),
            (
                "elapsed_ms",
                &state.started.elapsed().as_millis().to_string(),
            ),
        ],
    );
    state.emit(
        JobState::Completed,
        Preview {
            summary: final_text.chars().take(2_000).collect(),
            reasoning: String::new(),
        },
        "completed",
        None,
    );
    Ok(outcome)
}

fn finish_error(state: &Arc<RunState>, error: RunError) -> Result<RunOutcome, RunError> {
    let (job_state, event) = match &error {
        RunError::Cancelled => (JobState::Cancelled, "run_cancelled"),
        _ => (JobState::NeedsAttention, "run_failed"),
    };
    let code = error.code();
    if matches!(&error, RunError::Cancelled) {
        log_job(
            &state.scope,
            state.role.as_str(),
            state.phase,
            None,
            event,
            &[
                ("reason", code),
                (
                    "elapsed_ms",
                    &state.started.elapsed().as_millis().to_string(),
                ),
            ],
        );
    } else {
        warn_job(
            &state.scope,
            state.role.as_str(),
            state.phase,
            None,
            event,
            &[
                ("reason", code),
                ("error", &safe_text(&error.to_string(), 160)),
                (
                    "elapsed_ms",
                    &state.started.elapsed().as_millis().to_string(),
                ),
            ],
        );
    }
    state.emit(
        job_state,
        state
            .preview
            .lock()
            .map(|preview| preview.clone())
            .unwrap_or_default(),
        event,
        Some(code),
    );
    Err(error)
}

fn map_prompt_error(error: PromptError, state: &RunState) -> RunError {
    match error {
        PromptError::Cancelled { reason, .. } => match reason.as_str() {
            "budget_exhausted" => RunError::BudgetExceeded,
            "no_progress" => RunError::NoProgress,
            "compaction_failed" => RunError::CompactionFailed("摘要请求失败".into()),
            "input_too_large" => RunError::InputTooLarge,
            "invalid_tool_args" => RunError::InvalidToolArgs,
            "cancelled" => RunError::Cancelled,
            other => {
                let recorded = state.stop_code.lock().ok().and_then(|code| code.clone());
                match recorded.as_deref() {
                    Some("budget_exhausted") => RunError::BudgetExceeded,
                    Some("no_progress") => RunError::NoProgress,
                    Some("compaction_failed") => RunError::CompactionFailed("摘要请求失败".into()),
                    Some("input_too_large") => RunError::InputTooLarge,
                    Some("invalid_tool_args") => RunError::InvalidToolArgs,
                    Some("cancelled") => RunError::Cancelled,
                    _ => RunError::Internal(safe_text(other, 160)),
                }
            }
        },
        PromptError::MaxTurns { max_turns, .. } => RunError::MaxTurns(max_turns as u32),
        PromptError::Provider(error) => classify_provider_error(&error),
        PromptError::Report(report) => {
            let class = classify_http(report.http_status, report.code.as_deref(), &report.message);
            RunError::Provider {
                class,
                code: report.code,
                message: safe_text(&report.message, 240),
            }
        }
        PromptError::Memory { error, .. } => RunError::Internal(safe_text(&error.to_string(), 200)),
        other => RunError::Internal(safe_text(&other.to_string(), 200)),
    }
}

fn classify_provider_error(error: &rig_core::error::ProviderError) -> RunError {
    use rig_core::error::ProviderError;
    match error {
        ProviderError::ProviderResponse(response)
        | ProviderError::InvalidAuthentication(response) => {
            let status = response.status.as_ref().map(|status| status.as_u16());
            let code = response.machine_code();
            let body = safe_text(&response.body, 240);
            let class = classify_http(status, code.as_deref(), &body);
            RunError::Provider {
                class,
                code,
                message: body,
            }
        }
        ProviderError::CacheExpired { response, .. } => {
            let status = response.status.as_ref().map(|status| status.as_u16());
            RunError::Provider {
                class: classify_http(status, response.machine_code().as_deref(), &response.body),
                code: response.machine_code(),
                message: safe_text(&response.body, 240),
            }
        }
        other => {
            let message = safe_text(&other.to_string(), 240);
            let class = classify_http(None, None, &message);
            RunError::Provider {
                class,
                code: None,
                message,
            }
        }
    }
}

/// 运行状态里的当前工作（供调用方组装进度事件）。
pub fn current_work(role: Role, page: Option<u32>, chunk_id: Option<String>) -> CurrentWork {
    CurrentWork {
        page,
        chunk_id,
        role,
    }
}
