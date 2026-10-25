//! token 估算、上下文窗口预算与任务级调用预算。
//!
//! 估算刻意保守：CJK 按 1 token/字符、ASCII 按 1/4，再统一加 10% 余量并向上
//! 取整。它只用于「是否会超限」的决策，不宣称跨模型精确。

use std::{sync::Mutex, time::Duration};

use serde::{Deserialize, Serialize};

use crate::settings::Settings;

/// 除输出预算外额外预留的安全余量（系统提示、工具定义与协议开销）。
pub const SAFETY_MARGIN_TOKENS: u32 = 256;
/// 单条工具结果回传上限；超出由调用方分页或改传产物引用。
pub const TOOL_RESULT_TOKEN_LIMIT: u32 = 8_192;
/// 子任务回传父会话的上限（摘要 + 产物引用）。
pub const SUBAGENT_RETURN_TOKEN_LIMIT: u32 = 8_192;

/// 上下文与压缩参数。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// 模型上下文窗口（token）。
    pub context_window: u32,
    /// 单请求输出上限（token）。
    pub output_limit: u32,
    /// 自动压缩触发比例（可用输入预算的百分比，50~90）。
    pub compact_pct: u32,
    /// 保留近期历史的 token 数。
    pub recent_tokens: u32,
    /// 摘要目标 token 数。
    pub summary_tokens: u32,
    /// 整篇调用预算；0 = 自动公式。
    pub call_budget: u32,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            context_window: 32_768,
            output_limit: 16_384,
            compact_pct: 75,
            recent_tokens: 4_096,
            summary_tokens: 2_048,
            call_budget: 0,
        }
    }
}

impl BudgetConfig {
    /// 从设置快照取运行参数；未知/越界值收敛到推荐区间。
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            context_window: clamp_i64(settings.translation_agent_context_window, 16_384, 2_097_152),
            output_limit: clamp_i64(settings.translation_agent_output_cap, 1_024, 262_144),
            compact_pct: clamp_i64(settings.translation_agent_compact_percent, 50, 90),
            recent_tokens: clamp_i64(
                settings.translation_agent_recent_history_tokens,
                512,
                131_072,
            ),
            summary_tokens: clamp_i64(settings.translation_agent_summary_tokens, 256, 65_536),
            call_budget: clamp_i64(settings.translation_call_budget, 0, 1_000_000),
        }
    }

    /// 输出预算不能超过上下文窗口。
    pub fn normalized(mut self) -> Self {
        self.context_window = self.context_window.max(1_024);
        self.output_limit = self.output_limit.min(self.context_window);
        self.compact_pct = self.compact_pct.clamp(50, 90);
        self
    }

    /// 可用输入预算：窗口 − 输出上限 − 安全余量。
    pub fn available_input(&self) -> u32 {
        available_input(self.context_window, self.output_limit)
    }

    /// 自动压缩触发阈值（token）。
    pub fn compact_threshold(&self) -> u32 {
        let available = self.available_input();
        ((available as u64 * self.compact_pct as u64) / 100) as u32
    }
}

fn clamp_i64(value: i64, min: usize, max: usize) -> u32 {
    value.max(min as i64).min(max as i64) as u32
}

/// 保守 token 估算：CJK 1 token/字符，ASCII 1/4，非 ASCII 其他字符按 1；
/// 最后加 10% 余量并向上取整。空串为 0，非空至少为 1。
pub fn estimate_tokens(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    let mut ascii = 0usize;
    let mut wide = 0usize;
    for ch in text.chars() {
        if ch.is_ascii() {
            ascii += 1;
        } else {
            wide += 1;
        }
    }
    let raw = ascii as f64 / 4.0 + wide as f64;
    let with_margin = raw * 1.1;
    (with_margin.ceil() as u64).max(1) as u32
}

/// 请求是否装得下：输入估算 + 输出上限 + 安全余量 ≤ 窗口。
pub fn request_fits(input_est: u32, output_cap: u32, window: u32) -> bool {
    (input_est as u64) + (output_cap as u64) + (SAFETY_MARGIN_TOKENS as u64) <= window as u64
}

/// 可用输入预算：窗口扣掉输出上限与安全余量。
pub fn available_input(window: u32, output_cap: u32) -> u32 {
    window
        .saturating_sub(output_cap)
        .saturating_sub(SAFETY_MARGIN_TOKENS)
}

/// 自动调用预算：`64 + 32 × 初始分块数`。
/// 每个块至少要经历扫描、翻译、审查，再加上主 Agent 调度、压缩摘要与
/// 失败重派，单块的真实调用量远高于 16；留足余量避免正常任务被预算截断。
pub fn auto_call_budget(chunks: u32) -> u32 {
    64u32.saturating_add(32u32.saturating_mul(chunks))
}

/// 按 token 估算截断文本（用于摘要长度上限），返回不超过 `limit` 的文本。
pub fn truncate_to_tokens(text: &str, limit: u32) -> String {
    if estimate_tokens(text) <= limit {
        return text.to_owned();
    }
    // 保守按字符递减，避免把多字节字符切断。
    let chars: Vec<char> = text.chars().collect();
    let mut low = 0usize;
    let mut high = chars.len();
    while low < high {
        let mid = (low + high).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect();
        if estimate_tokens(&candidate) <= limit {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    chars[..low].iter().collect()
}

/// 一次手动追加预算的记录（只保留数字与原因码，不保留自由文本正文）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualGrant {
    pub amount: u32,
    pub reason: String,
    pub at_ms: u64,
}

/// 预算快照。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    /// 已消耗调用数。
    pub used: u32,
    /// 生效上限（基础上限 + 手动追加）。
    pub limit: u32,
    /// 累计手动追加。
    pub manual_extra: u32,
    /// 剩余调用数。
    pub remaining: u32,
}

struct BudgetState {
    used: u32,
    base_limit: u32,
    manual_extra: u32,
    grants: Vec<ManualGrant>,
}

/// 任务级调用预算；跨重试、返工与恢复不清零。
pub struct BudgetTracker {
    state: Mutex<BudgetState>,
    started: std::time::Instant,
}

impl BudgetTracker {
    /// 使用显式上限；`0` 表示不限制（仅探测/测试），生产应使用 [`Self::auto`]。
    pub fn new(limit: u32) -> Self {
        Self {
            state: Mutex::new(BudgetState {
                used: 0,
                base_limit: limit,
                manual_extra: 0,
                grants: Vec::new(),
            }),
            started: std::time::Instant::now(),
        }
    }

    /// 按初始分块数使用自动公式。
    pub fn auto(chunks: u32) -> Self {
        Self::new(auto_call_budget(chunks))
    }

    /// 消耗一次调用；预算耗尽返回 `false`，且不增加计数。
    pub fn try_consume_call(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        let limit = state.base_limit.saturating_add(state.manual_extra);
        if state.base_limit != 0 && state.used >= limit {
            return false;
        }
        state.used = state.used.saturating_add(1);
        true
    }

    /// 手动追加预算并记录原因；只增不减。返回追加后的快照。
    pub fn add_manual(&self, amount: u32, reason: &str) -> BudgetSnapshot {
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        if amount > 0 {
            state.manual_extra = state.manual_extra.saturating_add(amount);
            state.grants.push(ManualGrant {
                amount,
                reason: reason
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .take(120)
                    .collect(),
                at_ms: self.started.elapsed().as_millis() as u64,
            });
        }
        snapshot_of(&state)
    }

    pub fn snapshot(&self) -> BudgetSnapshot {
        let state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        snapshot_of(&state)
    }

    pub fn manual_grants(&self) -> Vec<ManualGrant> {
        self.state
            .lock()
            .map(|state| state.grants.clone())
            .unwrap_or_default()
    }

    pub fn remaining(&self) -> u32 {
        self.snapshot().remaining
    }

    /// 距离预算耗尽的估算时间（仅用于诊断展示）。
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

fn snapshot_of(state: &BudgetState) -> BudgetSnapshot {
    let limit = state.base_limit.saturating_add(state.manual_extra);
    let remaining = if state.base_limit == 0 {
        u32::MAX
    } else {
        limit.saturating_sub(state.used)
    };
    BudgetSnapshot {
        used: state.used,
        limit,
        manual_extra: state.manual_extra,
        remaining,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_costs_more_than_ascii_and_is_monotone() {
        let ascii = estimate_tokens("hello world");
        let cjk = estimate_tokens("这是一段中文字符");
        assert!(cjk > ascii, "CJK 应按 1 token/字符估算");
        let short = estimate_tokens("短");
        let long = estimate_tokens("短文字更多内容");
        assert!(long > short, "更长的文本估算不应下降");
        assert_eq!(estimate_tokens(""), 0);
        assert!(estimate_tokens("a") >= 1);
        let text = "a".repeat(1_000);
        assert!(estimate_tokens(&text) >= 275, "ASCII 至少 1/4 并含余量");
        assert!(estimate_tokens(&text) <= 300, "ASCII 不应高估超过 10% 余量");
    }

    #[test]
    fn window_budget_accounts_for_output_and_margin() {
        assert!(request_fits(100, 100, 1_000));
        assert!(!request_fits(800, 300, 1_000));
        assert_eq!(available_input(1_000, 300), 444);
        assert_eq!(available_input(100, 300), 0, "不能下溢");
        assert!(request_fits(
            100,
            100,
            available_input(1_000, 100) + 100 + SAFETY_MARGIN_TOKENS
        ));
    }

    #[test]
    fn auto_budget_formula_and_zero_means_auto_only_at_config_level() {
        assert_eq!(auto_call_budget(0), 64);
        assert_eq!(auto_call_budget(3), 160);
        assert_eq!(auto_call_budget(10), 384);
        assert_eq!(BudgetConfig::default().call_budget, 0);
    }

    #[test]
    fn budget_exhausts_and_manual_extra_only_increases() {
        let budget = BudgetTracker::new(2);
        assert!(budget.try_consume_call());
        assert!(budget.try_consume_call());
        assert!(!budget.try_consume_call(), "耗尽后必须拒绝");
        let before = budget.snapshot();
        assert_eq!(before.used, 2);
        assert_eq!(before.remaining, 0);
        let after = budget.add_manual(3, "manual resume");
        assert_eq!(after.manual_extra, 3);
        assert_eq!(after.limit, 5);
        assert_eq!(after.remaining, 3);
        assert_eq!(budget.manual_grants().len(), 1);
        assert!(budget.try_consume_call());
        let zero = budget.add_manual(0, "noop");
        assert_eq!(zero.manual_extra, 3, "0 追加不改变预算");
        assert_eq!(budget.snapshot().used, 3);
    }

    #[test]
    fn unlimited_tracker_never_blocks() {
        let budget = BudgetTracker::new(0);
        for _ in 0..10_000 {
            assert!(budget.try_consume_call());
        }
        assert_eq!(budget.remaining(), u32::MAX);
    }

    #[test]
    fn truncate_to_tokens_keeps_limit_and_unicode_boundaries() {
        let text = "中文".repeat(100);
        let truncated = truncate_to_tokens(&text, 100);
        assert!(estimate_tokens(&truncated) <= 100);
        assert!(text.starts_with(&truncated));
        assert_eq!(truncate_to_tokens("short", 100), "short");
    }
}
