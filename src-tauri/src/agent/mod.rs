//! 通用 Agent 内核的宿主侧边界。
//!
//! 阶段 1 落模型传输与三协议装配（[`model`]）；本模块补齐与业务无关的运行层：
//! 内置资源、存储拥有者、事件、公平调度、预算、有限恢复、上下文压缩与单次运行。
//!
//! 这一层不依赖 Pixiv、阅读页或翻译术语；翻译服务负责把业务资源、工具与
//! 缓存拥有者注入进来（见 `docs/plans/agent-kernel-and-smart-translation.md`
//! §3～§10）。

pub mod budget;
pub mod compaction;
pub mod events;
pub mod model;
pub mod recovery;
pub mod resources;
pub mod runtime;
pub mod scheduler;
pub mod storage;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

pub use budget::{
    BudgetConfig, BudgetTracker, auto_call_budget, available_input, estimate_tokens, request_fits,
};
pub use compaction::{
    CompactionConfig, CompactionError, CompactionEvent, CompactionJournal, CompactionOutcome,
    CompactionRecord, compact_now, maybe_compact, projected_tokens, should_compact,
    should_compact_tokens,
};
pub use events::{
    EventSink, JobEmitter, JobEvent, RecordingSink, TauriEventSink, log_job, runtime_event,
    warn_job,
};
pub use recovery::{FailureClass, RetryPolicy, backoff, backoff_with_retry_after, classify_http};
pub use resources::{ResourceId, ResourceVersions, load_resource, role_prompt, system_prompt};
pub use runtime::{RunError, RunLimits, RunOutcome, RunRequest, RunStatus, run};
pub use scheduler::{ModelPermit, NovelPermit, RuntimeConfig, Scheduler};
pub use storage::{
    CacheGeneration, JobScope, JobStore, JobSummary, Manifest, StoreError, list_jobs,
};

/// 任务标识：`<yyyymmddHHMMSS>-<6hex>`。
pub type JobId = String;
/// 分块标识：`c0001`。
pub type ChunkId = String;
/// 片段标识：`p003-l012-s0`。
pub type SegmentId = String;
/// 冻结约束版本，冻结一次 +1。
pub type ConstraintVersion = u64;

/// 任务状态。阶段与状态分离；等待确认／暂停／需要处理都可以在检查后恢复。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    AwaitingConfirmation,
    Pausing,
    Paused,
    NeedsAttention,
    Cancelling,
    Cancelled,
    Interrupted,
    Completed,
    /// 已标记删除；此后任何写入都必须拒绝，避免迟到回调复活目录。
    Deleted,
}

impl JobState {
    /// 终态：不会再继续执行，也不会再写产物。
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::Interrupted | Self::Completed | Self::Deleted
        )
    }

    /// 是否允许新写入。只有 `Deleted` 明确拒绝；删除中的目录由 `.deleting` 标记拦截。
    pub fn accepts_writes(self) -> bool {
        !matches!(self, Self::Deleted)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::AwaitingConfirmation => "awaiting_confirmation",
            Self::Pausing => "pausing",
            Self::Paused => "paused",
            Self::NeedsAttention => "needs_attention",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::Completed => "completed",
            Self::Deleted => "deleted",
        }
    }
}

/// 任务阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    PrepareSource,
    PrepareConstraints,
    Translate,
    Review,
    Repair,
    GlobalReview,
    Publish,
}

impl Phase {
    pub const ALL: [Phase; 7] = [
        Phase::PrepareSource,
        Phase::PrepareConstraints,
        Phase::Translate,
        Phase::Review,
        Phase::Repair,
        Phase::GlobalReview,
        Phase::Publish,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrepareSource => "prepare_source",
            Self::PrepareConstraints => "prepare_constraints",
            Self::Translate => "translate",
            Self::Review => "review",
            Self::Repair => "repair",
            Self::GlobalReview => "global_review",
            Self::Publish => "publish",
        }
    }
}

/// Agent 角色。子 Agent 默认一层：只有 Main 可以派发，Translator / Reviewer 不再派生。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Main,
    Translator,
    Reviewer,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Translator => "translator",
            Self::Reviewer => "reviewer",
        }
    }

    /// 角色对应的默认阶段（仅用于日志与预览事件，不改变业务阶段机）。
    pub fn default_phase(self) -> Phase {
        match self {
            Self::Main => Phase::Translate,
            Self::Translator => Phase::Translate,
            Self::Reviewer => Phase::Review,
        }
    }
}

/// 任务计数快照。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct JobCounts {
    pub pages: u32,
    pub chunks: u32,
    pub translated: u32,
    pub reviewed: u32,
    pub needs_repair: u32,
    pub terms: u32,
}

impl JobCounts {
    /// 是否完全没有计数（用于事件合并时保留上一份已知计数）。
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }
}

/// token 用量累计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

impl TokenUsage {
    pub fn add(&mut self, other: TokenUsage) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.total = self.total.saturating_add(other.total);
    }

    /// 从 Rig 的用量映射；缺失的计数按 0 计，不用估算值冒充真实用量。
    pub fn from_rig(usage: &rig_core::completion::Usage) -> Self {
        let input = usage.input_tokens.unwrap_or(0);
        let output = usage.output_tokens.unwrap_or(0);
        let total = usage.total_tokens.unwrap_or_else(|| input + output);
        Self {
            input,
            output,
            total,
        }
    }
}

/// 累计模型尝试预算。恢复、重试、返工都不清零。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AttemptBudget {
    pub model_calls: u32,
    pub limit: u32,
    pub manual_extra: u32,
    pub exits: u32,
}

/// 安全错误摘要：只保留可展示的类别、消息与步骤，不含凭据或正文。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeError {
    pub code: String,
    pub message: String,
    pub step: String,
}

/// 当前工作位置（进度事件用）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentWork {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_id: Option<ChunkId>,
    pub role: Role,
}

impl Default for Role {
    fn default() -> Self {
        Self::Main
    }
}

/// 生成内容预览（仅用于界面实时展示，不落盘、不写日志正文）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub summary: String,
    pub reasoning: String,
}

impl Preview {
    pub fn is_empty(&self) -> bool {
        self.summary.is_empty() && self.reasoning.is_empty()
    }

    /// 预览字节数，用于节流判断。
    pub fn len_bytes(&self) -> usize {
        self.summary.len() + self.reasoning.len()
    }
}

/// 可中断的取消令牌（项目内自实现，避免新增依赖）。
///
/// 语义与 `tokio_util::sync::CancellationToken` 对齐：克隆共享同一个状态，
/// `cancel()` 幂等，`cancelled()` 可被 `tokio::select!` 竞争。
#[derive(Clone, Debug)]
pub struct CancelToken {
    tx: watch::Sender<bool>,
    rx: watch::Receiver<bool>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(false);
        Self { tx, rx }
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    /// 等待取消。已取消时立即返回，不会漏掉取消前的通知。
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let mut rx = self.rx.clone();
        loop {
            if rx.changed().await.is_err() {
                return;
            }
            if *rx.borrow() {
                return;
            }
        }
    }

    /// 派生一个共享同一取消源的令牌（用于父子任务传播）。
    pub fn child(&self) -> Self {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_state_terminal_and_write_rules() {
        assert!(JobState::Completed.is_terminal());
        assert!(!JobState::Running.is_terminal());
        assert!(!JobState::Deleted.accepts_writes());
        assert!(JobState::Cancelled.accepts_writes());
        assert_eq!(
            JobState::AwaitingConfirmation.as_str(),
            "awaiting_confirmation"
        );
    }

    #[tokio::test]
    async fn cancel_token_is_shared_and_idempotent() {
        let token = CancelToken::new();
        let child = token.child();
        assert!(!child.is_cancelled());
        token.cancel();
        token.cancel();
        assert!(child.is_cancelled());
        tokio::time::timeout(std::time::Duration::from_millis(50), child.cancelled())
            .await
            .expect("已取消应立即返回");
    }

    #[tokio::test]
    async fn cancel_token_wakes_waiter() {
        let token = CancelToken::new();
        let waiter = token.clone();
        let handle = tokio::spawn(async move { waiter.cancelled().await });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        token.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(1), handle)
            .await
            .expect("取消应唤醒等待者")
            .expect("等待任务不应 panic");
    }
}
