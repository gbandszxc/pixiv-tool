//! 上下文压缩：在完整请求 / 工具批次之间做结构化摘要。
//!
//! 规则（计划 §6、§14.4）：
//! - 触发：`estimate_tokens(投影) > compact_pct × 可用输入`;
//! - 摘要请求使用固定系统提示，只输出结构化摘要，不含原文 / 译文；
//! - 保留最近 `recent_history_tokens`，按「assistant tool_call + 对应 tool result」
//!   的配对边界切分，绝不拆散；
//! - 原始历史不被覆盖：成功只返回新的投影，失败返回 `Err` 由调用方保留原记录并
//!   停止运行（不无限压缩）；
//! - 日志只记录估算计数、耗时与保留范围，不记录正文。

use std::sync::Arc;

use std::{future::Future, pin::Pin};

use rig_core::{
    DynModel,
    completion::{CompletionRequest, CompletionResponse},
    error::ProviderError,
    message::{AssistantContent, Message, ToolResultContent, UserContent},
    operation::Completion,
};

use crate::agent::{
    BudgetConfig, BudgetTracker, CancelToken, JobScope, Phase, Scheduler, TokenUsage,
    budget::{available_input, estimate_tokens, truncate_to_tokens},
    events::{log_job, warn_job},
};

/// 发一次普通完成请求的能力。
///
/// 生产实现是 [`DynModel<Completion>`]；单元测试可注入脚本化实现，避免依赖
/// 真实网络或额外的 test-utils feature。
pub trait CompletionCaller: Send + Sync {
    fn complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, ProviderError>> + Send + 'a>>;
}

impl CompletionCaller for DynModel<Completion> {
    fn complete<'a>(
        &'a self,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, ProviderError>> + Send + 'a>> {
        Box::pin(self.call(request))
    }
}

/// 摘要请求的固定系统提示。不得改成「自由总结」。
pub const SUMMARY_SYSTEM: &str = "\
你是上下文压缩器。只输出结构化摘要，不含原文、译文或任何逐字引用。\n\
摘要必须依次覆盖：\n\
1. 目标（当前任务要完成什么）；\n\
2. 已确认约束（术语、风格、用户确认结果及其版本）；\n\
3. 有效产物版本（已提交/已审查/待修复的块与版本号）；\n\
4. 进度（完成到哪一步、当前块）；\n\
5. 未决问题（需要后续处理或用户确认的事项）；\n\
6. 下一步（接下来应执行的最小动作）。\n\
控制在给定输出上限内；信息不足时写「未知」，不得编造。";

/// 压缩生命周期事件（只用于结构化记录，不携带正文）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompactionEvent {
    Start,
    Done,
    Failed,
}

impl CompactionEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "compact_start",
            Self::Done => "compact_done",
            Self::Failed => "compact_failed",
        }
    }
}

/// 一次压缩事件的结构化记录。
///
/// 只含估算计数、耗时、保留范围与结果码；**不包含**摘要正文、原文、译文或
/// 提示词。宿主可据此写入 Job 会话记录（`sessions/journal.jsonl`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactionRecord {
    pub event: CompactionEvent,
    /// 压缩前 / 压缩后的上下文估算 token。
    pub before_tokens: Option<u32>,
    pub after_tokens: Option<u32>,
    /// 保留区起点（历史消息下标）。
    pub keep_from: Option<usize>,
    /// 保留的原始消息条数。
    pub retained: Option<usize>,
    pub elapsed_ms: u64,
    /// 失败原因码；成功与开始事件为 `None`。
    pub code: Option<&'static str>,
}

/// 压缩记录回调。实现方负责落到自己的日志 / 会话存储。
pub type CompactionJournal = Arc<dyn Fn(&CompactionRecord) + Send + Sync>;

/// 压缩参数与依赖。
#[derive(Clone)]
pub struct CompactionConfig {
    pub budget: BudgetConfig,
    /// 压缩摘要同样经过全局模型请求闸门。
    pub scheduler: Option<Arc<Scheduler>>,
    /// 摘要调用计入任务总预算。
    pub tracker: Option<Arc<BudgetTracker>>,
    /// 可选的压缩事件记录钩子（不改变压缩策略或语义）。
    pub journal: Option<CompactionJournal>,
    pub scope: JobScope,
}

impl CompactionConfig {
    pub fn new(budget: BudgetConfig, scope: JobScope) -> Self {
        Self {
            budget,
            scheduler: None,
            tracker: None,
            journal: None,
            scope,
        }
    }

    pub fn with_scheduler(mut self, scheduler: Arc<Scheduler>) -> Self {
        self.scheduler = Some(scheduler);
        self
    }

    pub fn with_tracker(mut self, tracker: Arc<BudgetTracker>) -> Self {
        self.tracker = Some(tracker);
        self
    }

    /// 注入压缩事件记录钩子；记录失败不影响压缩本身。
    pub fn with_journal(mut self, journal: CompactionJournal) -> Self {
        self.journal = Some(journal);
        self
    }

    fn record(&self, record: CompactionRecord) {
        if let Some(journal) = &self.journal {
            journal(&record);
        }
    }

    fn available_input(&self) -> u32 {
        available_input(self.budget.context_window, self.budget.output_limit)
    }

    fn threshold(&self) -> u32 {
        (self.available_input() as u64 * self.budget.compact_pct.clamp(50, 90) as u64 / 100) as u32
    }
}

/// 压缩错误。
#[derive(Debug, thiserror::Error)]
pub enum CompactionError {
    #[error("压缩已取消")]
    Cancelled,
    #[error("任务预算耗尽，无法发起摘要请求")]
    BudgetExhausted,
    #[error("摘要请求失败: {0}")]
    Model(String),
    #[error("没有可压缩的历史")]
    NothingToCompact,
}

impl CompactionError {
    /// 安全原因码（日志 / 事件用）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::BudgetExhausted => "budget_exhausted",
            Self::Model(_) => "model_error",
            Self::NothingToCompact => "nothing_to_compact",
        }
    }
}

/// 压缩结果：新的上下文投影，原历史保持不变。
#[derive(Clone, Debug)]
pub struct CompactionOutcome {
    /// `[摘要消息] + 最近历史`。
    pub history: Vec<Arc<Message>>,
    pub summary: String,
    pub before_tokens: u32,
    pub after_tokens: u32,
    /// 保留的原始消息条数。
    pub retained: usize,
    /// 摘要请求的用量（计入任务总用量）。
    pub usage: TokenUsage,
    pub elapsed_ms: u64,
}

/// 投影 token 估算。
pub fn projected_tokens(history: &[Arc<Message>], prompt: Option<&Message>) -> u32 {
    let history_tokens: u32 = history
        .iter()
        .map(|message| estimate_tokens(&render_message(message)))
        .fold(0u32, |acc, tokens| acc.saturating_add(tokens));
    prompt
        .map(|prompt| estimate_tokens(&render_message(prompt)))
        .map(|prompt_tokens| history_tokens.saturating_add(prompt_tokens))
        .unwrap_or(history_tokens)
}

/// 是否应触发压缩。
pub fn should_compact(
    history: &[Arc<Message>],
    prompt: Option<&Message>,
    config: &CompactionConfig,
) -> bool {
    should_compact_tokens(projected_tokens(history, prompt), config)
}

/// 以调用方计算的总输入估算（含系统提示与工具 schema）判断是否触发压缩。
pub fn should_compact_tokens(projected: u32, config: &CompactionConfig) -> bool {
    if config.available_input() == 0 {
        return projected > 0;
    }
    projected > config.threshold()
}

/// 触发检查 + 压缩。未达阈值返回 `Ok(None)`。
pub async fn maybe_compact(
    model: &dyn CompletionCaller,
    history: &[Arc<Message>],
    prompt: Option<&Message>,
    config: &CompactionConfig,
    cancel: &CancelToken,
) -> Result<Option<CompactionOutcome>, CompactionError> {
    if !should_compact(history, prompt, config) {
        return Ok(None);
    }
    compact_now(model, history, config, cancel).await.map(Some)
}

/// 强制压缩一次（不做阈值判断）。
pub async fn compact_now(
    model: &dyn CompletionCaller,
    history: &[Arc<Message>],
    config: &CompactionConfig,
    cancel: &CancelToken,
) -> Result<CompactionOutcome, CompactionError> {
    let started = std::time::Instant::now();
    if cancel.is_cancelled() {
        journal_failure(config, None, None, None, started, "cancelled");
        return Err(CompactionError::Cancelled);
    }
    let before_tokens = projected_tokens(history, None);
    let recent_start = recent_start(history, config.budget.recent_tokens);
    if recent_start == 0 {
        journal_failure(
            config,
            Some(before_tokens),
            Some(0),
            Some(0),
            started,
            "nothing_to_compact",
        );
        return Err(CompactionError::NothingToCompact);
    }
    let retained = &history[recent_start..];
    let summarizable = &history[..recent_start];

    log_job(
        &config.scope,
        "main",
        Phase::Translate,
        None,
        "compact_start",
        &[
            ("context_est", &before_tokens.to_string()),
            ("trigger", &config.threshold().to_string()),
            ("keep_from", &recent_start.to_string()),
        ],
    );
    config.record(CompactionRecord {
        event: CompactionEvent::Start,
        before_tokens: Some(before_tokens),
        after_tokens: None,
        keep_from: Some(recent_start),
        retained: Some(retained.len()),
        elapsed_ms: 0,
        code: None,
    });

    if let Some(tracker) = &config.tracker
        && !tracker.try_consume_call()
    {
        warn_job(
            &config.scope,
            "main",
            Phase::Translate,
            None,
            "compact_failed",
            &[("reason", "budget_exhausted")],
        );
        journal_failure(
            config,
            Some(before_tokens),
            Some(recent_start),
            Some(retained.len()),
            started,
            "budget_exhausted",
        );
        return Err(CompactionError::BudgetExhausted);
    }

    let transcript_budget = config
        .available_input()
        .saturating_sub(config.budget.summary_tokens)
        .max(256);
    let transcript = clip_middle(&render_transcript(summarizable), transcript_budget);
    if transcript.trim().is_empty() {
        journal_failure(
            config,
            Some(before_tokens),
            Some(recent_start),
            Some(retained.len()),
            started,
            "nothing_to_compact",
        );
        return Err(CompactionError::NothingToCompact);
    }

    let mut request = CompletionRequest::new(transcript).preamble(SUMMARY_SYSTEM);
    request.max_tokens = Some(config.budget.summary_tokens.max(256) as u64);
    request.temperature = Some(0.0);

    let response = if let Some(scheduler) = &config.scheduler {
        let permit = {
            let acquire = scheduler.model_permit(config.scope.novel_id);
            tokio::pin!(acquire);
            tokio::select! {
                () = cancel.cancelled() => {
                    journal_failure(
                        config,
                        Some(before_tokens),
                        Some(recent_start),
                        Some(retained.len()),
                        started,
                        "cancelled",
                    );
                    return Err(CompactionError::Cancelled);
                }
                permit = &mut acquire => permit,
            }
        };
        let call = model.complete(request);
        tokio::pin!(call);
        let result = tokio::select! {
            () = cancel.cancelled() => {
                journal_failure(
                    config,
                    Some(before_tokens),
                    Some(recent_start),
                    Some(retained.len()),
                    started,
                    "cancelled",
                );
                return Err(CompactionError::Cancelled);
            }
            result = &mut call => result,
        };
        drop(permit);
        result
    } else {
        let call = model.complete(request);
        tokio::pin!(call);
        tokio::select! {
            () = cancel.cancelled() => {
                journal_failure(
                    config,
                    Some(before_tokens),
                    Some(recent_start),
                    Some(retained.len()),
                    started,
                    "cancelled",
                );
                return Err(CompactionError::Cancelled);
            }
            result = &mut call => result,
        }
    };

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let message = safe_error(&error.to_string());
            warn_job(
                &config.scope,
                "main",
                Phase::Translate,
                None,
                "compact_failed",
                &[
                    ("reason", "model_error"),
                    ("error", &message),
                    ("elapsed_ms", &started.elapsed().as_millis().to_string()),
                ],
            );
            journal_failure(
                config,
                Some(before_tokens),
                Some(recent_start),
                Some(retained.len()),
                started,
                "model_error",
            );
            return Err(CompactionError::Model(message));
        }
    };

    let summary = truncate_to_tokens(
        &response_text(&response),
        config.budget.summary_tokens.max(64),
    );
    if summary.trim().is_empty() {
        warn_job(
            &config.scope,
            "main",
            Phase::Translate,
            None,
            "compact_failed",
            &[("reason", "empty_summary")],
        );
        journal_failure(
            config,
            Some(before_tokens),
            Some(recent_start),
            Some(retained.len()),
            started,
            "empty_summary",
        );
        return Err(CompactionError::Model("摘要为空".into()));
    }

    let mut new_history = Vec::with_capacity(retained.len() + 1);
    new_history.push(Arc::new(Message::user(format!(
        "【历史摘要（已压缩，原始记录仍保留在产物中）】\n{summary}"
    ))));
    new_history.extend(retained.iter().cloned());
    let after_tokens = projected_tokens(&new_history, None);
    let usage = TokenUsage::from_rig(&response.usage);

    log_job(
        &config.scope,
        "main",
        Phase::Translate,
        None,
        "compact_done",
        &[
            ("before_est", &before_tokens.to_string()),
            ("after_est", &after_tokens.to_string()),
            ("retained", &retained.len().to_string()),
            ("elapsed_ms", &started.elapsed().as_millis().to_string()),
        ],
    );
    config.record(CompactionRecord {
        event: CompactionEvent::Done,
        before_tokens: Some(before_tokens),
        after_tokens: Some(after_tokens),
        keep_from: Some(recent_start),
        retained: Some(retained.len()),
        elapsed_ms: started.elapsed().as_millis() as u64,
        code: None,
    });

    Ok(CompactionOutcome {
        history: new_history,
        summary,
        before_tokens,
        after_tokens,
        retained: retained.len(),
        usage,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// 记录一次压缩失败；不改变错误返回路径。
fn journal_failure(
    config: &CompactionConfig,
    before_tokens: Option<u32>,
    keep_from: Option<usize>,
    retained: Option<usize>,
    started: std::time::Instant,
    code: &'static str,
) {
    config.record(CompactionRecord {
        event: CompactionEvent::Failed,
        before_tokens,
        after_tokens: None,
        keep_from,
        retained,
        elapsed_ms: started.elapsed().as_millis() as u64,
        code: Some(code),
    });
}

/// 按「assistant 工具调用 + 对应 tool result」的配对边界，返回保留区起点。
///
/// 从尾部向前累积到 `recent_tokens`，只按整组切分；若起点仍是孤立的 tool result，
/// 继续向前扩一组，保证配对完整。
pub fn recent_start(history: &[Arc<Message>], recent_tokens: u32) -> usize {
    if history.is_empty() {
        return 0;
    }
    let units = turn_units(history);
    let mut total = 0u32;
    let mut chosen = units.len();
    for (index, (start, end)) in units.iter().enumerate().rev() {
        let cost: u32 = history[*start..*end]
            .iter()
            .map(|message| estimate_tokens(&render_message(message)))
            .fold(0u32, |acc, tokens| acc.saturating_add(tokens));
        if total > 0 && total.saturating_add(cost) > recent_tokens {
            break;
        }
        total = total.saturating_add(cost);
        chosen = index;
        if total >= recent_tokens {
            break;
        }
    }
    let mut start = units.get(chosen).map(|(start, _)| *start).unwrap_or(0);
    // 不把孤立的 tool result 放进保留区开头。
    while start > 0 && start < history.len() && is_tool_result(&history[start]) {
        start = units
            .iter()
            .rev()
            .find(|(unit_start, _)| *unit_start < start)
            .map(|(unit_start, _)| *unit_start)
            .unwrap_or(0);
    }
    start
}

/// 按「一个 assistant 回合 + 其后的全部 tool result」分组。
fn turn_units(history: &[Arc<Message>]) -> Vec<(usize, usize)> {
    let mut units = Vec::new();
    let mut index = 0;
    while index < history.len() {
        let start = index;
        let has_calls = matches!(
            &*history[index],
            Message::Assistant(assistant)
                if assistant.content.iter().any(|part| matches!(part, AssistantContent::ToolCall(_)))
        );
        index += 1;
        if has_calls {
            while index < history.len() && is_tool_result(&history[index]) {
                index += 1;
            }
        }
        units.push((start, index));
    }
    units
}

fn is_tool_result(message: &Message) -> bool {
    matches!(
        message,
        Message::User { content }
            if content.iter().any(|part| matches!(part, UserContent::ToolResult(_)))
    )
}

fn render_transcript(messages: &[Arc<Message>]) -> String {
    let mut out = String::new();
    for message in messages {
        out.push_str(&render_message(message));
        out.push('\n');
    }
    out
}

fn render_message(message: &Message) -> String {
    match message {
        Message::System { content } => format!("[system] {content}"),
        Message::User { content } => {
            let mut parts = Vec::new();
            for part in content {
                match part {
                    UserContent::Text(text) => parts.push(text.text.clone()),
                    UserContent::ToolResult(result) => {
                        let mut body = String::new();
                        for content in &result.content {
                            match content {
                                rig_core::message::ToolResultContent::Text(text) => {
                                    body.push_str(&text.text);
                                }
                                rig_core::message::ToolResultContent::Json { value } => {
                                    body.push_str(&value.to_string());
                                }
                                _ => body.push_str("[非文本工具结果]"),
                            }
                        }
                        parts.push(format!("[tool_result {}] {body}", result.name));
                    }
                    UserContent::Image(_) => parts.push("[image]".into()),
                    UserContent::Audio(_) => parts.push("[audio]".into()),
                    UserContent::Video(_) => parts.push("[video]".into()),
                    UserContent::Document(_) => parts.push("[document]".into()),
                }
            }
            format!("[user] {}", parts.join("\n"))
        }
        Message::Assistant(assistant) => {
            let mut parts = Vec::new();
            for part in &assistant.content {
                match part {
                    AssistantContent::Text(text) => parts.push(text.text.clone()),
                    AssistantContent::ToolCall(call) => {
                        parts.push(format!("[tool_call {}]", call.function.name))
                    }
                    AssistantContent::Reasoning(_) => {}
                    AssistantContent::Image(_) => parts.push("[image]".into()),
                    AssistantContent::Opaque(_) => {}
                    _ => {}
                }
            }
            format!("[assistant] {}", parts.join("\n"))
        }
    }
}

fn response_text(response: &rig_core::completion::CompletionResponse) -> String {
    response
        .choice
        .iter()
        .filter_map(|part| match part {
            AssistantContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 紧急截断：常规压缩救不回来时，从最旧到最新把历史中超大的成功工具结果
/// 正文截短（保留头尾、标注省略），直到总输入估算降到 `target_tokens` 以内。
///
/// 一轮截完仍不达标时按半深迭代收紧单条上限（最低 64 tokens），保证极小窗口
/// 下历史也能压到位。不调用模型、不改变消息结构（工具调用与结果保持配对）；
/// 错误结果不截。返回（截断后的历史, 截断条数）。
pub fn emergency_truncate(
    history: &[Arc<Message>],
    current_tokens: u32,
    target_tokens: u32,
    per_result_tokens: u32,
) -> (Vec<Arc<Message>>, usize) {
    if current_tokens <= target_tokens || history.is_empty() {
        return (history.to_vec(), 0);
    }
    let mut excess = current_tokens - target_tokens;
    let mut per = per_result_tokens;
    let mut messages = history.to_vec();
    let mut total_truncated = 0usize;
    loop {
        let (next, count) = truncate_pass(&messages, &mut excess, per);
        total_truncated += count;
        messages = next;
        if excess == 0 || count == 0 || per <= 64 {
            return (messages, total_truncated);
        }
        per /= 2;
    }
}

/// 单遍截断：从旧到新把超出 `per_result_tokens` 的工具结果正文替换为截断
/// 版本；返回（新历史, 修改的消息条数）。`excess` 按腾出的估算 token 递减。
fn truncate_pass(
    history: &[Arc<Message>],
    excess: &mut u32,
    per_result_tokens: u32,
) -> (Vec<Arc<Message>>, usize) {
    let mut messages: Vec<Arc<Message>> = Vec::with_capacity(history.len());
    let mut truncated = 0usize;
    for message in history {
        let replaced = replace_oversized_result(message, per_result_tokens, excess);
        if replaced.is_some() {
            truncated += 1;
        }
        messages.push(replaced.unwrap_or_else(|| message.clone()));
        if *excess == 0 {
            messages.extend(history[messages.len()..].iter().cloned());
            break;
        }
    }
    (messages, truncated)
}

/// 把单条消息中超出 `per_result_tokens` 的工具结果正文替换为截断版本；
/// 返回 `None` 表示该消息无需修改。`excess` 按腾出的估算 token 递减。
fn replace_oversized_result(
    message: &Arc<Message>,
    per_result_tokens: u32,
    excess: &mut u32,
) -> Option<Arc<Message>> {
    let Message::User { content } = message.as_ref() else {
        return None;
    };
    let mut changed = false;
    let mut new_content = Vec::with_capacity(content.len());
    for part in content {
        let UserContent::ToolResult(result) = part else {
            new_content.push(part.clone());
            continue;
        };
        if result.is_error || *excess == 0 {
            new_content.push(part.clone());
            continue;
        }
        let text = result
            .content
            .iter()
            .map(|item| match item {
                ToolResultContent::Text(text) => text.text.clone(),
                ToolResultContent::Json { value } => value.to_string(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        let text_tokens = estimate_tokens(&text);
        if text_tokens <= per_result_tokens {
            new_content.push(part.clone());
            continue;
        }
        let clipped = format!(
            "{}\n[已截断：正文过长，中段省略；如需完整内容请重新读取]",
            clip_middle(&text, per_result_tokens)
        );
        let saved = text_tokens.saturating_sub(per_result_tokens + 48);
        *excess = excess.saturating_sub(saved);
        new_content.push(UserContent::ToolResult(rig_core::message::ToolResult {
            call: result.call.clone(),
            name: result.name.clone(),
            content: vec![ToolResultContent::text(clipped)],
            is_error: false,
        }));
        changed = true;
    }
    if changed {
        Some(Arc::new(Message::User {
            content: new_content,
        }))
    } else {
        None
    }
}

/// 超长输入保留头尾，中间省略；头部保留目标与约束，尾部保留最近进度。
fn clip_middle(text: &str, limit: u32) -> String {
    if estimate_tokens(text) <= limit {
        return text.to_owned();
    }
    let head_budget = (limit / 3).max(64);
    let tail_budget = limit.saturating_sub(head_budget).max(64);
    let head = truncate_to_tokens(text, head_budget);
    let chars: Vec<char> = text.chars().collect();
    let (mut low, mut high) = (0usize, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        let tail: String = chars[chars.len() - mid..].iter().collect();
        if estimate_tokens(&tail) <= tail_budget {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    let tail: String = chars[chars.len() - low..].iter().collect();
    format!("{head}\n…（中间历史省略）…\n{tail}")
}

fn safe_error(text: &str) -> String {
    let redacted = crate::translation_diagnostics::redact(text, "");
    redacted.chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use crate::agent::turn_tool_budget;

    use rig_core::{
        completion::Usage,
        message::{Api, AssistantMessage, CallId, Origin, ToolCall, ToolFunction, ToolName},
        serde_json::json,
    };

    /// 脚本化的普通完成请求：成功返回固定摘要，或返回错误。
    struct FakeCaller {
        reply: Option<String>,
        requests: Mutex<Vec<CompletionRequest>>,
    }

    impl FakeCaller {
        fn reply(text: &str) -> Self {
            Self {
                reply: Some(text.to_owned()),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn failing() -> Self {
            Self {
                reply: None,
                requests: Mutex::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<CompletionRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl CompletionCaller for FakeCaller {
        fn complete<'a>(
            &'a self,
            request: CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, ProviderError>> + Send + 'a>>
        {
            self.requests.lock().unwrap().push(request);
            let reply = self.reply.clone();
            Box::pin(async move {
                match reply {
                    Some(text) => Ok(CompletionResponse::new(
                        vec![AssistantContent::text(text)],
                        Usage::default(),
                        Origin::new(Api::from("chat"), "mock", "mock-model"),
                        serde_json::Value::Null,
                    )),
                    None => Err(ProviderError::Provider("mock failure".into())),
                }
            })
        }
    }

    fn scope() -> JobScope {
        JobScope {
            novel_id: 5,
            job_id: "20261024210000-abc123".into(),
            generation: 1,
        }
    }

    fn long_text(chars: usize) -> String {
        "中".repeat(chars)
    }

    fn user(text: &str) -> Arc<Message> {
        Arc::new(Message::user(text))
    }

    fn tool_name() -> ToolName {
        ToolName::new("translate").expect("非空工具名")
    }

    fn assistant_call(id: &str) -> Arc<Message> {
        Arc::new(Message::Assistant(AssistantMessage::new(vec![
            AssistantContent::ToolCall(ToolCall::from_wire(
                id,
                ToolFunction::new(tool_name(), json!({"chunk": "c0001"})),
            )),
        ])))
    }

    fn tool_result(call: &str) -> Arc<Message> {
        Arc::new(Message::tool_result(
            CallId::from_wire(call),
            tool_name(),
            long_text(200),
        ))
    }

    fn config() -> CompactionConfig {
        CompactionConfig::new(
            BudgetConfig {
                context_window: 4_096,
                output_limit: 1_024,
                compact_pct: 50,
                recent_tokens: 64,
                summary_tokens: 128,
                call_budget: 0,
            },
            scope(),
        )
    }

    #[test]
    fn trigger_uses_projected_tokens_over_threshold() {
        let config = config();
        let small = vec![user("short")];
        assert!(!should_compact(&small, None, &config));
        let large = vec![user(&long_text(20_000))];
        assert!(should_compact(&large, None, &config));
    }

    #[test]
    fn recent_start_never_splits_tool_pairs() {
        let history = vec![
            user("goal"),
            assistant_call("call_1"),
            tool_result("call_1"),
            user("next"),
            assistant_call("call_2"),
            tool_result("call_2"),
        ];
        for budget in [10, 30, 64, 128] {
            let start = recent_start(&history, budget);
            if start < history.len() {
                assert!(
                    !is_tool_result(&history[start]),
                    "保留区不能以孤立 tool result 开头（budget {budget}, start {start}）"
                );
                if start > 0 && matches!(&*history[start], Message::Assistant(_)) {
                    assert!(
                        start + 1 >= history.len() || is_tool_result(&history[start + 1]),
                        "assistant tool_call 后必须紧跟结果（budget {budget}）"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn compaction_returns_new_history_and_keeps_original() {
        let original = vec![
            user("goal"),
            assistant_call("call_1"),
            tool_result("call_1"),
            assistant_call("call_2"),
            tool_result("call_2"),
        ];
        let snapshot_len = original.len();
        let model = FakeCaller::reply("目标：翻译；约束：无；进度：2/2；下一步：复核");
        let cancel = CancelToken::new();
        let outcome = compact_now(&model, &original, &config(), &cancel)
            .await
            .expect("摘要成功");
        assert!(!outcome.summary.is_empty());
        assert!(outcome.after_tokens <= outcome.before_tokens);
        assert!(outcome.history[0].rag_text().is_some(), "摘要作为首条消息");
        assert_eq!(original.len(), snapshot_len, "原历史不被覆盖");
        let calls = outcome
            .history
            .iter()
            .filter(|message| {
                matches!(&***message, Message::Assistant(assistant)
                    if assistant.content.iter().any(|part| matches!(part, AssistantContent::ToolCall(_))))
            })
            .count();
        let results = outcome
            .history
            .iter()
            .filter(|message| is_tool_result(message))
            .count();
        assert_eq!(calls, results, "保留区工具调用与结果必须成对");
        assert!(calls >= 1, "至少保留一组完整调用");
        let request = &model.requests()[0];
        assert_eq!(
            request.system_instructions(),
            Some(SUMMARY_SYSTEM),
            "摘要请求使用固定系统提示"
        );
        assert!(request.max_tokens.unwrap() >= 256);
    }

    #[tokio::test]
    async fn failure_keeps_original_history() {
        let model = FakeCaller::failing();
        let original = vec![user("goal"), user(&long_text(100))];
        let cancel = CancelToken::new();
        let error = compact_now(&model, &original, &config(), &cancel)
            .await
            .expect_err("模型失败应返回错误");
        assert!(matches!(error, CompactionError::Model(_)));
        assert_eq!(original.len(), 2, "失败时原历史必须保留");
    }

    #[tokio::test]
    async fn cancellation_stops_summary_request() {
        let model = FakeCaller::reply("不会到达");
        let cancel = CancelToken::new();
        cancel.cancel();
        let history = vec![user("goal"), user(&long_text(100))];
        let error = compact_now(&model, &history, &config(), &cancel)
            .await
            .expect_err("已取消");
        assert!(matches!(error, CompactionError::Cancelled));
        assert!(model.requests().is_empty(), "取消后不得发出摘要请求");
    }

    #[test]
    fn summary_length_is_bounded() {
        let text = long_text(10_000);
        let clipped = truncate_to_tokens(&text, 128);
        assert!(estimate_tokens(&clipped) <= 128);
        let clipped = clip_middle(&text, 128);
        assert!(clipped.contains("中间历史省略"));
    }

    #[tokio::test]
    async fn below_threshold_returns_none_without_request() {
        let model = FakeCaller::reply("unused");
        let cancel = CancelToken::new();
        let history = vec![user("small")];
        assert!(
            maybe_compact(&model, &history, None, &config(), &cancel)
                .await
                .unwrap()
                .is_none()
        );
        assert!(model.requests().is_empty(), "未达阈值不得发起请求");
    }

    fn big_tool_result(call: &str, chars: usize) -> Arc<Message> {
        Arc::new(Message::tool_result(
            CallId::from_wire(call),
            tool_name(),
            long_text(chars),
        ))
    }

    fn error_tool_result(call: &str, chars: usize) -> Arc<Message> {
        Arc::new(Message::User {
            content: vec![UserContent::ToolResult(rig_core::message::ToolResult {
                call: CallId::from_wire(call),
                name: tool_name(),
                content: vec![rig_core::message::ToolResultContent::text(long_text(chars))],
                is_error: true,
            })],
        })
    }

    #[test]
    fn emergency_truncate_shrinks_results_and_keeps_structure() {
        let history = vec![
            user("goal"),
            assistant_call("call_1"),
            tool_result("call_1"),
            assistant_call("call_2"),
            big_tool_result("call_2", 2_000),
            assistant_call("call_3"),
            big_tool_result("call_3", 3_000),
        ];
        let current = projected_tokens(&history, None);
        let (truncated, count) = emergency_truncate(&history, current, current / 3, 256);
        assert!(count >= 2, "两个大结果都应被截断（实际 {count}）");
        assert_eq!(truncated.len(), history.len(), "消息条数不变");
        assert!(
            matches!(&*truncated[1], Message::Assistant(_)),
            "配对结构不变"
        );
        assert!(matches!(&*truncated[2], Message::User { .. }));
        let after = projected_tokens(&truncated, None);
        assert!(after < current, "截断后估算必须下降（{current} → {after}）");
        let rendered = render_transcript(&truncated);
        assert!(rendered.contains("已截断"), "截断标注必须可见");
        assert!(
            rendered.contains("[tool_result translate]"),
            "未截的小结果原样保留"
        );
    }

    #[test]
    fn emergency_truncate_never_touches_error_results() {
        let history = vec![
            user("goal"),
            assistant_call("call_1"),
            error_tool_result("call_1", 2_000),
        ];
        let current = projected_tokens(&history, None);
        let (truncated, count) = emergency_truncate(&history, current, 64, 256);
        assert_eq!(count, 0, "错误结果不得截断");
        assert_eq!(
            projected_tokens(&truncated, None),
            current,
            "历史应原样保留"
        );
    }

    #[test]
    fn emergency_truncate_noop_when_already_within_target() {
        let history = vec![
            user("goal"),
            assistant_call("call_1"),
            tool_result("call_1"),
        ];
        let current = projected_tokens(&history, None);
        let (truncated, count) = emergency_truncate(&history, current, current, 256);
        assert_eq!(count, 0);
        assert_eq!(truncated.len(), history.len());
    }

    #[test]
    fn turn_tool_budget_scales_with_window_and_has_floor() {
        let small = BudgetConfig {
            context_window: 32_768,
            output_limit: 16_384,
            compact_pct: 75,
            recent_tokens: 4_096,
            summary_tokens: 2_048,
            call_budget: 0,
        };
        // 32k 窗口扣掉输出上限与安全余量后空间很小：16 128 − 8 238 − 2 048。
        assert_eq!(turn_tool_budget(&small, 8_238), 5_842);
        let large = BudgetConfig {
            context_window: 65_536,
            ..small
        };
        // 48 896（可用输入）− 8 238（固定开销）− 2 048（历史保留）。
        assert_eq!(turn_tool_budget(&large, 8_238), 38_610);
    }
}
