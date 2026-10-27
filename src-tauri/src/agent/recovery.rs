//! 有限恢复：失败分类、退避与取消。
//!
//! 分类只依赖状态码、机器码与脱敏后的消息；重试次数、缩块、返工共享任务总
//! 预算。`UnknownOutcome` 默认不自动重放（由调用方按设置决定）。

use std::{future::Future, time::Duration};

use crate::agent::CancelToken;

/// 失败分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FailureClass {
    /// 明确未发送的连接失败：可安全重试。
    NotSent,
    /// 限流、暂时性服务错误：退避后重试。
    Retryable,
    /// 提供商内容过滤拒绝：过滤判断含采样随机性，按可重试退避重发；
    /// 重试耗尽后转人工（更换模型 / 网关）。
    ContentFiltered,
    /// 鉴权、余额、模型不存在、配置不兼容：暂停并提示修正设置。
    Permanent,
    /// 已发送但结果未知：保留现场，默认等待用户恢复。
    UnknownOutcome,
    /// 输入超过上下文：压缩历史或细分工作单元。
    InputTooLarge,
    /// 输出被截断：缩块重做，不原样重发。
    OutputTruncated,
    /// 工具参数 / 结构错误：有限纠正。
    InvalidToolArgs,
    /// 质检问题：只返工受影响块。
    ReviewIssues,
}

impl std::fmt::Display for FailureClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FailureClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotSent => "not_sent",
            Self::Retryable => "retryable",
            Self::ContentFiltered => "content_filtered",
            Self::Permanent => "permanent",
            Self::UnknownOutcome => "unknown_outcome",
            Self::InputTooLarge => "input_too_large",
            Self::OutputTruncated => "output_truncated",
            Self::InvalidToolArgs => "invalid_tool_args",
            Self::ReviewIssues => "review_issues",
        }
    }

    /// 是否允许自动重试同一请求。
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::NotSent | Self::Retryable | Self::ContentFiltered)
    }

    /// 是否可以自动恢复（不重放同一请求，而是压缩/缩块/纠正）。
    pub fn is_recoverable(self) -> bool {
        !matches!(self, Self::Permanent | Self::UnknownOutcome)
    }
}

/// 根据 HTTP 状态、机器码与消息分类。
///
/// 顺序很重要：先识别输入/输出超限与工具参数错误（它们可能是 400），再按
/// 状态码判定永久/可重试。
pub fn classify_http(status: Option<u16>, code: Option<&str>, message: &str) -> FailureClass {
    let code_lower = code.unwrap_or_default().to_ascii_lowercase();
    let message_lower = message.to_ascii_lowercase();

    let mentions = |needles: &[&str]| needles.iter().any(|needle| message_lower.contains(needle));
    let code_matches = |needles: &[&str]| needles.iter().any(|needle| code_lower.contains(needle));

    if code_matches(&[
        "context_length_exceeded",
        "context_window_exceeded",
        "input_too_long",
        "prompt_too_long",
        "request_too_large",
        "string_above_max_length",
    ]) || mentions(&[
        "context length",
        "context_length",
        "maximum context",
        "context window",
        "too many tokens",
        "prompt is too long",
        "input is too long",
        "request is too large",
    ]) {
        return FailureClass::InputTooLarge;
    }

    if code_matches(&["max_output_tokens", "output_too_long", "length_limit"])
        || mentions(&[
            "max_tokens",
            "max_output_tokens",
            "max_completion_tokens",
            "output limit",
            "output was truncated",
            "finish_reason=length",
            "length limit",
        ])
    {
        return FailureClass::OutputTruncated;
    }

    if code_matches(&["content_filter", "content_filtering"])
        || mentions(&[
            "finish_reason: content_filter",
            "finish_reason=content_filter",
            "content_filter",
            "content filter",
            "content policy",
            "content filtering",
        ])
    {
        return FailureClass::ContentFiltered;
    }

    if code_matches(&[
        "invalid_tool",
        "tool_arguments",
        "tool_call_parse",
        "malformed_function_call",
    ]) || mentions(&["tool arguments", "tool_call", "function.arguments"])
        && mentions(&["invalid", "malformed", "parse"])
    {
        return FailureClass::InvalidToolArgs;
    }

    if code_matches(&[
        "insufficient_quota",
        "billing_hard_limit_reached",
        "invalid_api_key",
        "model_not_found",
        "account_deactivated",
        "authentication_error",
        "permission_denied",
    ]) {
        return FailureClass::Permanent;
    }

    match status {
        Some(401 | 402 | 403) => FailureClass::Permanent,
        Some(404) => FailureClass::Permanent,
        Some(408 | 409 | 425 | 429) => FailureClass::Retryable,
        Some(status) if (500..=599).contains(&status) => FailureClass::Retryable,
        Some(status) if (400..=499).contains(&status) => FailureClass::Permanent,
        Some(_) => FailureClass::Permanent,
        None => {
            if mentions(&[
                "connection refused",
                "connection reset",
                "dns",
                "no route",
                "failed to connect",
                "connect error",
                "proxy",
                "tls",
            ]) {
                FailureClass::NotSent
            } else if mentions(&["timeout", "timed out", "deadline", "stream", "eof"]) {
                FailureClass::UnknownOutcome
            } else if mentions(&["not sent", "before sending", "request not sent"]) {
                FailureClass::NotSent
            } else {
                FailureClass::UnknownOutcome
            }
        }
    }
}

/// 重试策略：`retries` 不含首次；退避从 `base_ms` 指数增长，上限 `max_ms`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub retries: u32,
    pub base_ms: u64,
    pub max_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            retries: 2,
            base_ms: 1_000,
            max_ms: 120_000,
        }
    }
}

impl RetryPolicy {
    pub fn new(retries: u32, base_ms: u64, max_ms: u64) -> Self {
        Self {
            retries,
            base_ms: base_ms.max(1),
            max_ms: max_ms.max(base_ms.max(1)),
        }
    }

    /// 是否还允许第 `attempt` 次重试（`attempt` 从 0 开始）。
    pub fn allows(&self, attempt: u32) -> bool {
        attempt < self.retries
    }
}

/// 确定性抖动：由 `attempt` 推导 −25%..+25% 的基点，便于测试与复现。
pub fn jitter_basis_points(attempt: u32) -> i32 {
    // 简单整数散列即可：同一 attempt 恒等，范围严格落在 ±2500 基点。
    let mixed = attempt
        .wrapping_mul(2_654_435_761)
        .wrapping_add(1_013_904_223);
    let span = 5_001i64;
    (mixed as i64 % span) as i32 - 2_500
}

/// 计算下一次退避延迟（纯函数，便于单测）。
///
/// `retry_after` 优先：服务明确要求等待时按它执行；超过 `max_ms` 返回 `None`
/// （调用方暂停，而不是提前重发）。
pub fn next_delay(
    policy: &RetryPolicy,
    attempt: u32,
    retry_after: Option<Duration>,
) -> Option<Duration> {
    if !policy.allows(attempt) {
        return None;
    }
    if let Some(retry_after) = retry_after {
        if retry_after > Duration::from_millis(policy.max_ms) {
            return None;
        }
        return Some(retry_after);
    }
    let exponent = attempt.min(16);
    let base = policy
        .base_ms
        .saturating_mul(1u64.checked_shl(exponent).unwrap_or(u64::MAX));
    let jitter = jitter_basis_points(attempt) as i64;
    let adjusted = (base as i128 * (10_000 + jitter) as i128 / 10_000) as u64;
    Some(Duration::from_millis(
        adjusted.clamp(1, policy.max_ms.max(1)),
    ))
}

/// 退避等待；可被取消中断。返回 `None` 表示不应再重试（次数用尽或要求暂停）。
pub async fn backoff(policy: &RetryPolicy, attempt: u32, cancel: &CancelToken) -> Option<Duration> {
    backoff_with_retry_after(policy, attempt, None, cancel).await
}

/// 尊重 `Retry-After` 的退避等待。
pub async fn backoff_with_retry_after(
    policy: &RetryPolicy,
    attempt: u32,
    retry_after: Option<Duration>,
    cancel: &CancelToken,
) -> Option<Duration> {
    let delay = next_delay(policy, attempt, retry_after)?;
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);
    tokio::select! {
        () = cancel.cancelled() => None,
        () = &mut sleep => Some(delay),
    }
}

/// 包装一次可能被取消的异步调用（工具/请求边界用）。
pub async fn with_cancel<T, F>(cancel: &CancelToken, future: F) -> Option<T>
where
    F: Future<Output = T>,
{
    tokio::select! {
        () = cancel.cancelled() => None,
        value = future => Some(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn classification_matrix() {
        assert_eq!(
            classify_http(Some(401), None, "invalid api key"),
            FailureClass::Permanent
        );
        assert_eq!(
            classify_http(Some(403), None, "forbidden"),
            FailureClass::Permanent
        );
        assert_eq!(
            classify_http(Some(402), None, "payment required"),
            FailureClass::Permanent
        );
        assert_eq!(
            classify_http(Some(404), Some("model_not_found"), "no such model"),
            FailureClass::Permanent
        );
        assert_eq!(
            classify_http(Some(429), Some("rate_limit_exceeded"), "slow down"),
            FailureClass::Retryable
        );
        assert_eq!(
            classify_http(Some(408), None, "timeout"),
            FailureClass::Retryable
        );
        assert_eq!(
            classify_http(Some(503), None, "service unavailable"),
            FailureClass::Retryable
        );
        assert_eq!(
            classify_http(Some(400), Some("context_length_exceeded"), "too long"),
            FailureClass::InputTooLarge
        );
        assert_eq!(
            classify_http(Some(400), None, "maximum context length exceeded"),
            FailureClass::InputTooLarge
        );
        assert_eq!(
            classify_http(Some(200), Some("max_output_tokens"), "truncated"),
            FailureClass::OutputTruncated
        );
        assert_eq!(
            classify_http(
                None,
                None,
                "the provider failed the turn: Provider finish_reason: content_filter"
            ),
            FailureClass::ContentFiltered
        );
        assert_eq!(
            classify_http(Some(400), Some("content_filter"), "blocked by policy"),
            FailureClass::ContentFiltered
        );
        assert!(FailureClass::ContentFiltered.is_retryable());
        assert_eq!(
            classify_http(Some(400), Some("invalid_tool_arguments"), "bad tool"),
            FailureClass::InvalidToolArgs
        );
        assert_eq!(
            classify_http(None, None, "connection refused"),
            FailureClass::NotSent
        );
        assert_eq!(
            classify_http(None, None, "request timed out after 600s"),
            FailureClass::UnknownOutcome
        );
        assert_eq!(
            classify_http(Some(400), None, "bad request"),
            FailureClass::Permanent
        );
        assert!(FailureClass::Retryable.is_retryable());
        assert!(!FailureClass::UnknownOutcome.is_retryable());
        assert!(!FailureClass::Permanent.is_recoverable());
    }

    #[test]
    fn retry_after_wins_and_over_max_means_pause() {
        let policy = RetryPolicy::default();
        let retry_after = Some(Duration::from_secs(2));
        assert_eq!(
            next_delay(&policy, 0, retry_after),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            next_delay(&policy, 0, Some(Duration::from_secs(600))),
            None,
            "超过 max_ms 必须暂停而不是提前重发"
        );
        assert_eq!(next_delay(&policy, 2, None), None, "重试次数用尽");
    }

    #[test]
    fn jitter_stays_within_quarter_and_grows_exponentially() {
        let policy = RetryPolicy::new(5, 1_000, 120_000);
        for attempt in 0..5 {
            let delay = next_delay(&policy, attempt, None)
                .expect("有剩余重试")
                .as_millis() as u64;
            let base = 1_000u64 << attempt;
            assert!(
                delay >= base * 3 / 4 && delay <= base * 5 / 4,
                "attempt {attempt}: {delay} 应落在 {base} 的 ±25%"
            );
            let bp = jitter_basis_points(attempt);
            assert!((-2_500..=2_500).contains(&bp));
        }
        let capped = next_delay(&policy, 4, None).unwrap();
        let expected = (16_000i128 * (10_000 + jitter_basis_points(4) as i128) / 10_000) as u64;
        assert_eq!(
            capped.as_millis() as u64,
            expected,
            "远未到上限时按指数增长"
        );
        let policy = RetryPolicy::new(10, 60_000, 120_000);
        let capped = next_delay(&policy, 5, None).unwrap();
        assert!(capped.as_millis() as u64 <= 120_000);
    }

    #[tokio::test]
    async fn cancellation_interrupts_backoff() {
        let policy = RetryPolicy::new(2, 30_000, 60_000);
        let cancel = CancelToken::new();
        let waiter = cancel.clone();
        let handle = tokio::spawn(async move { backoff(&policy, 0, &waiter).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("取消应立即中断退避")
            .expect("任务不应 panic");
        assert_eq!(result, None);
    }
}
