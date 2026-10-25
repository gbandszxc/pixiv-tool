//! 整篇小说翻译 Job 服务：生命周期、调度、恢复、预算与 IPC 快照。
//!
//! 设计要点（计划 §10、§11 与冻结接口 §8）：
//! - 后端是状态真相源：`start` 快速返回 Job 快照，后台 tokio 任务驱动主 Agent
//!   运行；一轮运行正常结束但完成校验未通过时自动续跑（带压缩后历史与最新检查点），
//!   连续无进展达到上限才转 needs_attention。阶段、计数、用量与终态由本服务写入
//!   manifest 并广播事件。
//! - 同一小说同一原文版本、同一目标语言的重复启动加入既有 Job；重译（force）
//!   创建新 Job，旧结果保留到新结果通过为止。
//! - 暂停 = 停止派发并停在最近检查点；取消 = 立即传播取消令牌，保留检查点。
//!   两者都不清预算与重试记录。产物写入全部走 `JobStore::write_atomic`
//!   （临时文件 + rename），所以中断不会留下半截文件。
//! - 缓存清理由宿主调用 [`TranslationService::invalidate_cache`]：递增世代、取消
//!   在途任务、清空索引；此后的旧世代写入被 [`JobStore`] 直接拒绝，且不重建目录。
//! - 启动恢复只把未结束状态改写为 `interrupted`，绝不自动发起付费请求。

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::{
    agent::{
        AttemptBudget, BudgetConfig, BudgetTracker, CacheGeneration, CancelToken, EventSink,
        JobCounts, JobEvent, JobId, JobScope, JobState, Phase, Preview, Role, RunError, RunLimits,
        RunRequest, SafeError, Scheduler, TokenUsage, backoff,
        events::{JobEmitter, log_job, work_event},
        model::providers::{
            ApiFormat, ProviderConfig, request_defaults, resolve_api_format, session_header_for,
            session_id,
        },
        resources::{ResourceVersions, role_prompt, system_prompt},
        runtime,
        storage::{JobStore, list_jobs},
    },
    paths::AppPaths,
    settings::Settings,
    translation::{
        artifacts::{
            AgentTerm, ArtifactIo, Chunk, Constraints, Draft, JobArtifacts, MergeReport,
            PublishStatus, Review, ReviewVerdict, Severity, SourceDoc, TranslatedLine,
            TranslationResult,
        },
        chunking::ChunkConfig,
        publish::{publish_status_with_language, render_markdown},
        source::{parse_segment_id, parse_source},
        tools,
        validation::{merge_pages, validate_draft_with_language, validate_merge_with_language},
    },
    translation_legacy as legacy,
};

/// 无事件出口（无 Tauri 宿主或单元测试构造 AppState 时使用）。
pub struct NoopSink;

impl EventSink for NoopSink {
    fn emit(&self, _event: JobEvent) {}
}

// ----------------------------------------------------------------------
// 凭据与模型装配
// ----------------------------------------------------------------------

/// 凭据来源：生产走系统凭据存储，离线测试注入固定 Key。
#[derive(Clone)]
pub enum CredentialProvider {
    Keyring,
    Fixed(String),
}

impl CredentialProvider {
    fn read(&self) -> Result<Option<String>, String> {
        match self {
            Self::Keyring => legacy::read_api_key(),
            Self::Fixed(key) => Ok(Some(key.clone())),
        }
    }
}

/// 本 Job 使用的模型配置（不提供明文 Key 的序列化入口）。
#[derive(Clone)]
pub struct ModelConfig {
    pub format: String,
    pub base_url: String,
    pub model: String,
    key: String,
    pub extra_json: Value,
    pub timeout_seconds: u64,
    pub session_seed: String,
}

impl std::fmt::Debug for ModelConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelConfig")
            .field("format", &self.format)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("timeout_seconds", &self.timeout_seconds)
            .field("session_seed", &self.session_seed)
            .finish_non_exhaustive()
    }
}

impl ModelConfig {
    fn provider(&self) -> Result<ProviderConfig, String> {
        let format = resolve_api_format(&self.format)?;
        let provider = ProviderConfig::new(
            format,
            self.base_url.clone(),
            self.model.clone(),
            self.key.clone(),
            self.extra_json.clone(),
            self.timeout_seconds,
        )?;
        let session = session_id(&self.session_seed);
        let header = session_header_for(&provider.endpoint, &session);
        Ok(provider.with_session_header(header))
    }

    /// 每次 Agent 运行使用独立的模型句柄（共享连接池）。
    pub(crate) fn model(
        &self,
    ) -> Result<rig_core::DynModel<rig_core::operation::Completion>, String> {
        let provider = self.provider()?;
        let erased = match provider.format {
            ApiFormat::ChatCompletions => provider.chat_model()?.erase(),
            ApiFormat::Responses => provider.responses_model()?.erase(),
            ApiFormat::Anthropic => provider.anthropic_model()?.erase(),
        };
        Ok(erased)
    }

    /// 输出上限与按协议归一化的扩展字段（思考参数见 ADR 0032）。
    pub(crate) fn request_params(
        &self,
        input_chars: usize,
    ) -> Result<(Option<u64>, Option<Value>), String> {
        let provider = self.provider()?;
        let defaults = request_defaults(&provider, input_chars);
        let mut params = defaults
            .additional_params
            .as_object()
            .cloned()
            .unwrap_or_default();
        let cap = match provider.format {
            // Chat：运行层写 `max_tokens`；这里取等值上限并移除
            // `max_completion_tokens`，避免两个输出上限字段同时下发。
            ApiFormat::ChatCompletions => params
                .remove("max_completion_tokens")
                .and_then(|value| value.as_u64())
                .or(defaults.max_tokens),
            _ => defaults.max_tokens,
        };
        Ok((cap, Some(Value::Object(params))))
    }
}

// ----------------------------------------------------------------------
// 控制位（暂停 / 取消 / 等待确认）
// ----------------------------------------------------------------------

/// 派发控制位。
///
/// - `pause`：停止派发（派发工具在每块前检查）；
/// - `stop`：让主运行的 supervisor 尽快停在最近的检查点；
/// - `cancel`：传播到运行层与子任务，立即中止请求流。
#[derive(Default)]
pub struct Control {
    pause: AtomicBool,
    awaiting_confirmation: AtomicBool,
    confirmed: AtomicBool,
    rework_rounds: std::sync::atomic::AtomicU32,
    reset_notes: StdMutex<Vec<String>>,
    stop: StdMutex<CancelToken>,
    pub cancel: CancelToken,
}

impl Control {
    pub fn pause(&self) {
        self.pause.store(true, Ordering::SeqCst);
        self.request_stop();
    }

    pub fn resume(&self) {
        self.pause.store(false, Ordering::SeqCst);
        self.reset_stop();
    }

    pub fn pause_requested(&self) -> bool {
        self.pause.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
        self.request_stop();
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// 让主运行停在最近一个检查点（不丢已提交产物）。
    pub fn request_stop(&self) {
        if let Ok(token) = self.stop.lock() {
            token.cancel();
        }
    }

    fn reset_stop(&self) {
        if let Ok(mut token) = self.stop.lock() {
            *token = CancelToken::new();
        }
    }

    fn stop_signal(&self) -> CancelToken {
        self.stop
            .lock()
            .map(|token| token.clone())
            .unwrap_or_else(|error| error.into_inner().clone())
    }

    pub fn set_awaiting_confirmation(&self, value: bool) {
        self.awaiting_confirmation.store(value, Ordering::SeqCst);
        if value {
            self.request_stop();
        }
    }

    pub fn awaiting_confirmation(&self) -> bool {
        self.awaiting_confirmation.load(Ordering::SeqCst)
    }

    pub fn mark_confirmed(&self) {
        self.confirmed.store(true, Ordering::SeqCst);
    }

    pub fn confirmed(&self) -> bool {
        self.confirmed.load(Ordering::SeqCst)
    }

    pub fn bump_rework(&self) -> u32 {
        self.rework_rounds.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn rework_rounds(&self) -> u32 {
        self.rework_rounds.load(Ordering::SeqCst)
    }

    pub fn note_reset(&self, phase: Phase, reason: &str) {
        if let Ok(mut notes) = self.reset_notes.lock() {
            notes.push(json!({"phase": phase, "reason": reason}).to_string());
        }
    }

    pub fn reset_notes(&self) -> Vec<String> {
        self.reset_notes
            .lock()
            .map(|notes| notes.clone())
            .unwrap_or_default()
    }
}

// ----------------------------------------------------------------------
// 存储适配与 Job 上下文
// ----------------------------------------------------------------------

/// `JobStore` → [`crate::translation::artifacts::ArtifactIo`]（保留世代校验）。
#[derive(Clone)]
pub struct StoreIo {
    store: Arc<JobStore>,
}

impl StoreIo {
    pub fn new(store: Arc<JobStore>) -> Self {
        Self { store }
    }
}

impl crate::translation::artifacts::ArtifactIo for StoreIo {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        self.store.read(path).map_err(|error| error.to_string())
    }

    fn write_atomic(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.store
            .write_atomic(path, bytes)
            .map_err(|error| error.to_string())
    }

    fn exists(&self, path: &str) -> bool {
        self.store.exists(path)
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, String> {
        let path = self.store.root().join(dir);
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.to_string()),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let meta = entry.metadata().map_err(|error| error.to_string())?;
            if meta.is_file()
                && let Some(name) = entry.file_name().to_str()
            {
                names.push(name.to_string());
            }
        }
        names.sort();
        Ok(names)
    }
}

/// 一个 Job 的只读上下文 + 运行期共享状态。
pub struct JobContext {
    pub scope: JobScope,
    pub store: Arc<JobStore>,
    pub paths: AppPaths,
    /// 启动时冻结的设置快照。
    pub settings: Settings,
    pub target_language: String,
    pub target_language_name: String,
    pub source_language: String,
    pub budget: Arc<BudgetTracker>,
    pub budget_config: BudgetConfig,
    pub scheduler: Arc<Scheduler>,
    pub model: ModelConfig,
    pub emitter: Arc<JobEmitter>,
    pub control: Control,
    pub confirm_before_start: bool,
    /// manifest 读改写串行化。
    manifest_lock: StdMutex<()>,
    /// `sessions/journal.jsonl` 追加串行化（主运行与子任务并发写入时防丢行）。
    journal_lock: StdMutex<()>,
    /// 单 Job 子任务并发闸门。
    pub dispatch_gate: Arc<Semaphore>,
    /// 本 Job 累计用量（跨多次运行）。
    usage: StdMutex<TokenUsage>,
    /// 结果未知且按设置不自动重放的块：再次派发直接跳过，避免续跑时反复
    /// 重发同一请求；用户手动恢复时清空。
    unknown_blocked: StdMutex<BTreeSet<String>>,
    /// 产物访问器（持有 `Arc<JobStore>`，与上下文同寿命）。
    io: StoreIo,
}

impl JobContext {
    /// 在同步作用域内访问产物访问器。
    ///
    /// `JobArtifacts` 借用 `dyn ArtifactIo`（非 `Send`），因此所有产物读写都在
    /// 同步闭包里完成，不能跨 `await` 持有。
    pub fn with_artifacts<T>(&self, f: impl FnOnce(&JobArtifacts<'_>) -> T) -> T {
        let artifacts = JobArtifacts::new(&self.io);
        f(&artifacts)
    }

    pub fn chunk_config(&self) -> ChunkConfig {
        ChunkConfig {
            target_chars: self.settings.translation_agent_chunk_target_chars.max(1) as usize,
            max_chars: self.settings.translation_agent_chunk_max_chars.max(1) as usize,
            context_chars: self.settings.translation_agent_context_chars.max(0) as usize,
        }
    }

    pub fn run_limits(&self) -> RunLimits {
        RunLimits {
            max_turns: self.settings.translation_agent_max_turns.clamp(4, 128) as u32,
            max_no_progress: self.settings.translation_agent_stall_limit.clamp(3, 32) as u32,
            max_tool_corrections: self.settings.translation_agent_tool_corrections.clamp(0, 3)
                as u32,
        }
    }

    /// 该块上次结果未知且按设置不自动重放：再次派发直接跳过。
    pub fn is_unknown_blocked(&self, chunk_id: &str) -> bool {
        self.unknown_blocked
            .lock()
            .map(|set| set.contains(chunk_id))
            .unwrap_or(false)
    }

    /// 标记块结果未知；后续派发不再自动请求，直到用户手动恢复任务。
    pub fn block_unknown(&self, chunk_id: &str) {
        if let Ok(mut set) = self.unknown_blocked.lock() {
            set.insert(chunk_id.to_string());
        }
    }

    /// 用户显式恢复任务时清空，允许重新请求这些块。
    pub fn clear_unknown_blocks(&self) {
        if let Ok(mut set) = self.unknown_blocked.lock() {
            set.clear();
        }
    }

    /// 子运行的事件出口：子运行的终态不能冒充 Job 终态。
    pub fn run_sink(&self) -> Arc<dyn EventSink> {
        Arc::new(JobProgressSink {
            emitter: self.emitter.clone(),
        })
    }

    pub fn usage(&self) -> TokenUsage {
        self.usage.lock().map(|usage| *usage).unwrap_or_default()
    }

    pub fn add_usage(&self, usage: TokenUsage) {
        if let Ok(mut total) = self.usage.lock() {
            total.add(usage);
        }
    }

    pub fn read_manifest(&self) -> Result<crate::agent::Manifest, String> {
        let _guard = self
            .manifest_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.store
            .read_manifest()
            .map_err(|error| error.to_string())
    }

    pub fn update_manifest(
        &self,
        f: impl FnOnce(&mut crate::agent::Manifest),
    ) -> Result<(), String> {
        let _guard = self
            .manifest_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.store
            .update_manifest(f)
            .map_err(|error| error.to_string())
    }

    pub fn touch_checkpoint(&self, phase: Phase) -> Result<(), String> {
        let _guard = self
            .manifest_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.store
            .touch_checkpoint(phase)
            .map_err(|error| error.to_string())
    }

    /// 追加一条会话记录到 `sessions/journal.jsonl`。
    ///
    /// 只写结构化事实（角色、块 ID、状态、耗时、长度、用量），不写原文、译文、
    /// 提示词或凭据。`sessions/` 属于 Job 缓存拥有者，随 Job 一起删除。
    ///
    /// 主运行的上下文压缩经 `CompactionConfig::with_journal` 注入本方法，记录
    /// `compact_start / compact_done / compact_failed`，只含估算 token、耗时、
    /// 保留范围与结果码。
    pub fn append_session(&self, record: &Value) {
        const PATH: &str = "sessions/journal.jsonl";
        let line = match serde_json::to_string(record) {
            Ok(line) => line,
            Err(_) => return,
        };
        // 主运行（含压缩记录）与子任务完成记录可能并发到达；串行化读改写。
        let _guard = self
            .journal_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut bytes = self.store.read(PATH).ok().flatten().unwrap_or_default();
        // 单 Job 最多保留 4000 行，超出后丢弃最旧的记录（保留最近事实）。
        const MAX_LINES: usize = 4_000;
        const NEWLINE: u8 = 0x0A;
        if bytes.iter().filter(|byte| **byte == NEWLINE).count() >= MAX_LINES
            && let Some(cut) = bytes.iter().position(|byte| *byte == NEWLINE)
        {
            bytes.drain(..=cut);
        }
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(NEWLINE);
        let _ = self.store.write_atomic(PATH, &bytes);
    }

    /// 重新计算计数并通过事件广播（不改变 Job 状态）。
    pub fn refresh_counts(&self) -> JobCounts {
        let counts = tools::compute_counts(self);
        let (phase, usage) = match self.read_manifest() {
            Ok(manifest) => (manifest.phase, manifest.usage),
            Err(_) => (Phase::PrepareSource, TokenUsage::default()),
        };
        let _ = self.update_manifest(|manifest| manifest.counts = counts);
        self.emitter.emit(work_event(
            &self.scope,
            JobState::Running,
            phase,
            counts,
            None,
            usage,
            "progress",
        ));
        counts
    }
}

/// 运行层事件过滤：Running 与预览原样通过，终态改写为 Running。
struct JobProgressSink {
    emitter: Arc<JobEmitter>,
}

impl EventSink for JobProgressSink {
    fn emit(&self, mut event: JobEvent) {
        if !matches!(event.state, JobState::Running) {
            event.state = JobState::Running;
        }
        self.emitter.emit(event);
    }
}

// ----------------------------------------------------------------------
// 快照与前端契约
// ----------------------------------------------------------------------

/// 预算快照（浮层展示用）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    pub used: u32,
    pub limit: u32,
    pub remaining: u32,
    pub manual_extra: u32,
}

/// Job 快照：前端 store 的单一数据结构。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranslationJobSnapshot {
    pub job_id: JobId,
    pub novel_id: i64,
    pub state: JobState,
    pub phase: Phase,
    pub counts: JobCounts,
    pub current: Option<crate::agent::CurrentWork>,
    pub usage: TokenUsage,
    pub elapsed_ms: u64,
    pub preview: Preview,
    pub message: String,
    pub error_code: Option<String>,
    pub seq: u64,
    pub constraint_version: u64,
    pub budget: BudgetSnapshot,
    /// 原文已是目标语言、未创建 Job。
    pub already_target_language: bool,
    pub target_language: String,
    /// 分块计划的块 ID（顺序即合并顺序）。细分后 ID 不再连续，前端不能按
    /// 序号拼 ID；缺失时为空，由前端回退到旧行为。
    #[serde(default)]
    pub chunk_ids: Vec<String>,
}

/// 任务列表。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JobList {
    /// 所有 Job 的最大事件序号。
    pub seq: u64,
    pub jobs: Vec<TranslationJobSnapshot>,
}

/// 用户确认时对单个术语的修改。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ConstraintTermEdit {
    pub source_term: String,
    pub locked_translation: String,
    #[serde(default)]
    pub term_type: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

/// 确认请求体。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ConstraintEdit {
    #[serde(default)]
    pub terms: Vec<ConstraintTermEdit>,
    #[serde(default)]
    pub style: String,
}

/// 浮层预览：原文 / 译文对照（只读）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkPreview {
    pub chunk_id: String,
    pub order: u32,
    pub segments: Vec<PreviewSegment>,
    pub translated: Vec<PreviewTranslation>,
    pub context_prev: String,
    pub context_next: String,
    pub draft_version: u64,
    pub reviewed: bool,
    pub needs_repair: bool,
    pub review: Option<PreviewReview>,
}

/// 预览里的原文片段。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewSegment {
    pub id: String,
    pub page: u32,
    pub line: u32,
    pub text: String,
    pub kind: String,
}

/// 预览里的译文片段。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewTranslation {
    pub id: String,
    pub text: String,
}

/// 预览里的审查结论。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewReview {
    pub verdict: ReviewVerdict,
    pub notes: String,
    pub issues: Vec<crate::translation::artifacts::Issue>,
}

/// 约束浮层的只读视图（不产生副作用，也不阻塞运行中的 Job）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConstraintsView {
    pub version: u64,
    pub terms: Vec<AgentTerm>,
    pub style: String,
    pub conflicts: Vec<ConflictGroupView>,
    pub unresolved: Vec<String>,
    pub frozen: bool,
    pub awaiting_confirmation: bool,
}

/// 同一术语 / 类型的冲突候选值。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictValueView {
    pub value: String,
    /// 提出该候选值的块；既有锁定译名没有来源块，记为空串。
    pub chunk_id: String,
}

/// 术语冲突分组。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictGroupView {
    pub source_term: String,
    pub term_type: String,
    pub values: Vec<ConflictValueView>,
}

/// 审查问题浮层的只读视图。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IssueView {
    pub chunk_id: String,
    pub segment_id: String,
    pub kind: String,
    pub severity: Severity,
    /// 已限长并压平控制字符；不含原文 / 译文。
    pub detail: String,
}

/// 全部单块审查问题的汇总。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IssuesView {
    pub issues: Vec<IssueView>,
    /// 有问题的块（按块顺序）。
    pub affected_chunks: Vec<String>,
    /// 仍有未解决问题的块（审查未通过或有阻断问题，按块顺序）。
    pub unresolved_chunks: Vec<String>,
    /// 已有审查记录的块数。
    pub reviewed: u32,
    /// 计划内块总数（没有计划时退化为已落盘块数）。
    pub total: u32,
}

// ----------------------------------------------------------------------
// 服务主体
// ----------------------------------------------------------------------

struct ServiceInner {
    /// 会话级详细诊断开关（默认关闭，重启复位）。
    diagnostic: AtomicBool,
    paths: AppPaths,
    settings: Arc<StdMutex<Settings>>,
    generation: CacheGeneration,
    scheduler: Arc<Scheduler>,
    sink: Arc<dyn EventSink>,
    credentials: CredentialProvider,
    jobs: StdMutex<HashMap<JobId, Arc<JobHandle>>>,
    /// 同一小说串行：一次只允许一个 Job 运行。
    novel_locks: StdMutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
}

/// 翻译 Job 服务。克隆共享同一索引与世代。
#[derive(Clone)]
pub struct TranslationService {
    inner: Arc<ServiceInner>,
}

struct JobHandle {
    ctx: Arc<JobContext>,
    started: Instant,
    runtime: StdMutex<JobRuntime>,
    running: AtomicBool,
}

struct JobRuntime {
    state: JobState,
    phase: Phase,
    counts: JobCounts,
    current: Option<crate::agent::CurrentWork>,
    usage: TokenUsage,
    preview: Preview,
    message: String,
    error_code: Option<String>,
    constraint_version: u64,
}

impl TranslationService {
    /// 生产构造：Keyring 凭据、无事件出口（宿主可用 [`Self::with_sink`] 注入）。
    pub fn new(
        paths: AppPaths,
        settings: Arc<StdMutex<Settings>>,
        generation: CacheGeneration,
    ) -> Self {
        Self::build(
            paths,
            settings,
            generation,
            Arc::new(NoopSink),
            CredentialProvider::Keyring,
        )
    }

    /// 宿主构造：注入 Tauri 事件出口。
    pub fn with_sink(
        paths: AppPaths,
        settings: Arc<StdMutex<Settings>>,
        generation: CacheGeneration,
        sink: Arc<dyn EventSink>,
    ) -> Self {
        Self::build(
            paths,
            settings,
            generation,
            sink,
            CredentialProvider::Keyring,
        )
    }

    /// 离线测试构造：固定凭据、可注入记录出口。
    pub fn for_tests(
        paths: AppPaths,
        settings: Settings,
        sink: Arc<dyn EventSink>,
        api_key: Option<String>,
    ) -> Self {
        Self::for_tests_with_generation(paths, settings, sink, api_key, CacheGeneration::new(0))
    }

    /// 离线测试构造（显式世代）。
    pub fn for_tests_with_generation(
        paths: AppPaths,
        settings: Settings,
        sink: Arc<dyn EventSink>,
        api_key: Option<String>,
        generation: CacheGeneration,
    ) -> Self {
        let settings = Arc::new(StdMutex::new(settings));
        let credentials = match api_key {
            Some(key) => CredentialProvider::Fixed(key),
            None => CredentialProvider::Keyring,
        };
        Self::build(paths, settings, generation, sink, credentials)
    }

    fn build(
        paths: AppPaths,
        settings: Arc<StdMutex<Settings>>,
        generation: CacheGeneration,
        sink: Arc<dyn EventSink>,
        credentials: CredentialProvider,
    ) -> Self {
        let snapshot = settings.lock().map(|s| s.clone()).unwrap_or_default();
        let scheduler = Arc::new(Scheduler::new(&crate::agent::RuntimeConfig::from_settings(
            &snapshot,
        )));
        Self {
            inner: Arc::new(ServiceInner {
                diagnostic: AtomicBool::new(false),
                paths,
                settings,
                generation,
                scheduler,
                sink,
                credentials,
                jobs: StdMutex::new(HashMap::new()),
                novel_locks: StdMutex::new(HashMap::new()),
            }),
        }
    }

    /// 会话级详细诊断开关：开启后 DEBUG 级别生效（工具 / 请求 / 子任务起止与长度），
    /// 关闭恢复 INFO。开关只影响日志级别，不改变任何业务行为，也不写入设置文件。
    pub fn set_diagnostic(&self, enabled: bool) -> bool {
        self.inner.diagnostic.store(enabled, Ordering::SeqCst);
        let _ = crate::logging::set_diagnostic(enabled);
        log::info!("翻译诊断开关：{}", if enabled { "debug" } else { "info" });
        enabled
    }

    pub fn diagnostic(&self) -> bool {
        self.inner.diagnostic.load(Ordering::SeqCst)
    }

    pub fn generation(&self) -> CacheGeneration {
        self.inner.generation.clone()
    }

    pub fn paths(&self) -> &AppPaths {
        &self.inner.paths
    }

    pub fn scheduler(&self) -> Arc<Scheduler> {
        self.inner.scheduler.clone()
    }

    fn settings_snapshot(&self) -> Settings {
        self.inner
            .settings
            .lock()
            .map(|settings| settings.clone())
            .unwrap_or_default()
    }

    /// 仍在运行（含收尾中）的任务数；维护清理据此拒绝。
    pub fn active_jobs(&self) -> usize {
        let Ok(jobs) = self.inner.jobs.lock() else {
            return 0;
        };
        jobs.values()
            .filter(|handle| {
                handle.running.load(Ordering::SeqCst)
                    || matches!(
                        handle.runtime.lock().map(|runtime| runtime.state),
                        Ok(JobState::Running
                            | JobState::Queued
                            | JobState::Pausing
                            | JobState::Cancelling)
                    )
            })
            .count()
    }

    /// 任务是否仍在写入（供清理门判定）。
    pub fn has_inflight(&self, job_id: &str) -> bool {
        self.handle(job_id)
            .is_some_and(|handle| handle.running.load(Ordering::SeqCst))
    }

    fn novel_lock(&self, novel_id: i64) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .inner
            .novel_locks
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        locks
            .entry(novel_id)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    fn handle(&self, job_id: &str) -> Option<Arc<JobHandle>> {
        self.inner
            .jobs
            .lock()
            .ok()
            .and_then(|jobs| jobs.get(job_id).cloned())
    }

    // ------------------------------------------------------------------
    // 启动
    // ------------------------------------------------------------------

    /// 启动（或加入）一个整篇翻译 Job。
    pub async fn start(
        &self,
        novel: legacy::NovelInput,
        force: bool,
    ) -> Result<TranslationJobSnapshot, String> {
        let settings = self.settings_snapshot();
        // 1. 启动前校验：设置、URL、模型、Key；任何失败都不创建 Job。
        legacy::validate_settings(&settings)?;
        if settings.translation_api_url.trim().is_empty()
            || settings.translation_model.trim().is_empty()
        {
            return Err("请先在设置的小说翻译中配置 API URL、API Key 和模型 ID".into());
        }
        let key = self
            .inner
            .credentials
            .read()?
            .filter(|key| !key.trim().is_empty())
            .ok_or("请先配置翻译 API Key")?;
        if key.len() > 8_192 || key.chars().any(|ch| ch.is_control()) {
            return Err("翻译 API Key 格式无效".into());
        }
        if novel.novel_id <= 0 || novel.content.len() > 8_000_000 {
            return Err("小说翻译输入无效或过大".into());
        }
        let (target_code, target_name) = legacy::resolve_target_language(&settings)?;
        let source = parse_source(&novel.content, target_code)?;
        let source_language = legacy::detect_language(&novel.content);
        if !force && legacy::same_target_language(source_language, target_code) {
            // 原文已是目标语言：不创建 Job，返回 completed 的空快照。
            return Ok(TranslationJobSnapshot {
                job_id: String::new(),
                novel_id: novel.novel_id,
                state: JobState::Completed,
                phase: Phase::Publish,
                counts: JobCounts {
                    pages: source.page_count(),
                    ..JobCounts::default()
                },
                current: None,
                usage: TokenUsage::default(),
                elapsed_ms: 0,
                preview: Preview::default(),
                message: "already_target_language".into(),
                error_code: None,
                seq: 0,
                constraint_version: 0,
                budget: BudgetSnapshot::default(),
                already_target_language: true,
                target_language: target_code.to_string(),
                chunk_ids: Vec::new(),
            });
        }

        // 2. 同一小说、同一原文版本、同一目标语言：加入既有 Job。
        //    加入的是失败 / 暂停 / 中断的任务时，用户意图明确是继续翻译：自动恢复。
        //    恢复校验（世代、资源版本）不通过时保留原状态返回，不阻断启动语义。
        if !force
            && let Some(existing_job_id) =
                self.find_reusable_job(novel.novel_id, &source.input_hash, target_code)?
        {
            let snapshot = self.snapshot(&existing_job_id).await?;
            if matches!(
                snapshot.state,
                JobState::NeedsAttention | JobState::Paused | JobState::Interrupted
            ) && self.resume(&existing_job_id).await.is_ok()
            {
                return self.snapshot(&existing_job_id).await;
            }
            return Ok(snapshot);
        }

        // 3. 新 Job：先估算块数与预算，再创建目录。
        let chunk_config = ChunkConfig {
            target_chars: settings.translation_agent_chunk_target_chars.max(1) as usize,
            max_chars: settings.translation_agent_chunk_max_chars.max(1) as usize,
            context_chars: settings.translation_agent_context_chars.max(0) as usize,
        };
        let chunks = crate::translation::chunking::chunk_source(&source, &chunk_config);
        let call_limit = if settings.translation_call_budget > 0 {
            settings.translation_call_budget as u32
        } else {
            crate::agent::auto_call_budget(chunks.len() as u32)
        };
        let budget = Arc::new(BudgetTracker::new(call_limit));
        let job_id = self.new_job_id(novel.novel_id);
        let scope = JobScope {
            novel_id: novel.novel_id,
            job_id: job_id.clone(),
            generation: self.inner.generation.get(),
        };
        let store = Arc::new(
            JobStore::create_guarded(&self.inner.paths, &scope, self.inner.generation.clone())
                .map_err(|error| format!("无法创建翻译任务目录：{error}"))?,
        );
        let model = ModelConfig {
            format: settings.translation_api_format.clone(),
            base_url: settings.translation_api_url.clone(),
            model: settings.translation_model.clone(),
            key,
            extra_json: settings.translation_extra.clone(),
            timeout_seconds: legacy::translation_timeout_seconds(
                settings.translation_timeout_seconds,
            ),
            session_seed: format!("pixiv-tool-novel-{}", novel.novel_id),
        };
        let settings_json = settings_snapshot_json(&settings, target_code, &model);
        let resources = ResourceVersions::current();
        {
            let io = StoreIo::new(store.clone());
            let artifacts = JobArtifacts::new(&io);
            artifacts.write_source(&source)?;
        }
        // 原文正文单独保存：重启或重置 Pass 0 后不再依赖前端重新提交。
        let input_bytes = serde_json::to_vec(&json!({
            "title": novel.title,
            "tags": novel.tags,
            "description": novel.description,
            "content": novel.content,
        }))
        .map_err(|_| "无法序列化翻译原文".to_string())?;
        store
            .write_atomic("source/input.json", &input_bytes)
            .map_err(|error| format!("无法写入翻译原文：{error}"))?;
        store
            .write_atomic(
                "settings_snapshot.json",
                &serde_json::to_vec_pretty(&settings_json).unwrap_or_default(),
            )
            .map_err(|error| format!("无法写入设置快照：{error}"))?;
        store
            .write_atomic(
                "resources.json",
                &serde_json::to_vec_pretty(&resources).unwrap_or_default(),
            )
            .map_err(|error| format!("无法写入资源版本：{error}"))?;
        let counts = JobCounts {
            pages: source.page_count(),
            chunks: chunks.len() as u32,
            ..JobCounts::default()
        };
        store
            .update_manifest(|manifest| {
                manifest.input_hash = source.input_hash.clone();
                manifest.target_language = target_code.to_string();
                manifest.source_language = language_code(source_language).to_string();
                manifest.settings_snapshot = settings_json.clone();
                manifest.counts = counts;
                manifest.attempts = AttemptBudget {
                    model_calls: 0,
                    limit: call_limit,
                    manual_extra: 0,
                    exits: 0,
                };
                manifest.state = JobState::Queued;
                manifest.phase = Phase::PrepareSource;
            })
            .map_err(|error| format!("无法写入任务清单：{error}"))?;

        let emitter = Arc::new(JobEmitter::new(self.inner.sink.clone(), scope.clone()));
        let dispatch_gate = Arc::new(Semaphore::new(
            settings.translation_agent_novel_concurrency.clamp(1, 16) as usize,
        ));
        let ctx = Arc::new(JobContext {
            scope: scope.clone(),
            store: store.clone(),
            paths: self.inner.paths.clone(),
            settings: settings.clone(),
            target_language: target_code.to_string(),
            target_language_name: target_name.to_string(),
            source_language: language_code(source_language).to_string(),
            budget,
            budget_config: BudgetConfig::from_settings(&settings),
            scheduler: self.inner.scheduler.clone(),
            model,
            emitter,
            control: Control::default(),
            confirm_before_start: settings.translation_confirm_before_start,
            manifest_lock: StdMutex::new(()),
            journal_lock: StdMutex::new(()),
            dispatch_gate,
            usage: StdMutex::new(TokenUsage::default()),
            unknown_blocked: StdMutex::new(BTreeSet::new()),
            io: StoreIo::new(store),
        });
        let handle = Arc::new(JobHandle {
            ctx,
            started: Instant::now(),
            runtime: StdMutex::new(JobRuntime {
                state: JobState::Queued,
                phase: Phase::PrepareSource,
                counts,
                current: None,
                usage: TokenUsage::default(),
                preview: Preview::default(),
                message: "queued".into(),
                error_code: None,
                constraint_version: 0,
            }),
            running: AtomicBool::new(false),
        });
        if let Ok(mut jobs) = self.inner.jobs.lock() {
            jobs.insert(job_id.clone(), handle.clone());
        }
        let snapshot = self.handle_snapshot(&handle);
        self.spawn_supervisor(handle);
        Ok(snapshot)
    }

    fn new_job_id(&self, novel_id: i64) -> JobId {
        let directory = self
            .inner
            .paths
            .data_dir
            .join("translations")
            .join("v2")
            .join(novel_id.to_string());
        for _ in 0..32 {
            let suffix = rand::random::<u32>() & 0x00ff_ffff;
            let job_id = format!("{}-{suffix:06x}", Utc::now().format("%Y%m%d%H%M%S"));
            if !directory.join(&job_id).exists() {
                return job_id;
            }
        }
        format!(
            "{}-{:06x}",
            Utc::now().format("%Y%m%d%H%M%S"),
            rand::random::<u32>() & 0x00ff_ffff
        )
    }

    fn find_reusable_job(
        &self,
        novel_id: i64,
        input_hash: &str,
        target_language: &str,
    ) -> Result<Option<JobId>, String> {
        let jobs = list_jobs(&self.inner.paths, novel_id).map_err(|error| error.to_string())?;
        for summary in jobs {
            if summary.corrupt || summary.pending_delete {
                continue;
            }
            let Some(manifest) = read_manifest_at(&summary.path) else {
                continue;
            };
            if manifest.input_hash == input_hash
                && manifest.target_language == target_language
                && !matches!(manifest.state, JobState::Deleted | JobState::Cancelled)
            {
                return Ok(Some(manifest.job_id));
            }
        }
        Ok(None)
    }

    // ------------------------------------------------------------------
    // 查询
    // ------------------------------------------------------------------

    pub async fn snapshot(&self, job_id: &str) -> Result<TranslationJobSnapshot, String> {
        if let Some(handle) = self.handle(job_id) {
            return Ok(self.handle_snapshot(&handle));
        }
        let (manifest, corrupt) = self.find_manifest(job_id)?;
        let job_dir = v2_job_dir(&self.inner.paths.data_dir, manifest.novel_id, job_id);
        let mut snapshot = snapshot_from_manifest(&manifest, plan_chunk_ids_at(&job_dir));
        if corrupt {
            snapshot.message = "corrupt".into();
            snapshot.error_code = Some("corrupt_manifest".into());
        }
        Ok(snapshot)
    }

    /// 内存索引 + 磁盘扫描（重启后仍能看到 interrupted / corrupt 任务）。
    pub async fn jobs(&self) -> JobList {
        let mut by_id: BTreeMap<JobId, TranslationJobSnapshot> = BTreeMap::new();
        if let Ok(handles) = self.inner.jobs.lock() {
            for (job_id, handle) in handles.iter() {
                by_id.insert(job_id.clone(), self.handle_snapshot(handle));
            }
        }
        let v2 = self.inner.paths.data_dir.join("translations").join("v2");
        if let Ok(novels) = std::fs::read_dir(&v2) {
            for novel in novels.flatten() {
                let Ok(meta) = novel.metadata() else { continue };
                if !meta.is_dir() {
                    continue;
                }
                let Ok(jobs) = std::fs::read_dir(novel.path()) else {
                    continue;
                };
                for job in jobs.flatten() {
                    let Ok(meta) = job.metadata() else { continue };
                    if !meta.is_dir() || job.path().join(".deleting").exists() {
                        continue;
                    }
                    let job_id = job.file_name().to_string_lossy().into_owned();
                    if by_id.contains_key(&job_id) {
                        continue;
                    }
                    let novel_id = novel
                        .file_name()
                        .to_string_lossy()
                        .parse::<i64>()
                        .unwrap_or_default();
                    match read_manifest_at(&job.path()) {
                        Some(manifest) => {
                            by_id.insert(
                                job_id,
                                snapshot_from_manifest(&manifest, plan_chunk_ids_at(&job.path())),
                            );
                        }
                        None => {
                            let mut snapshot = snapshot_from_manifest(
                                &crate::agent::Manifest::new(&JobScope {
                                    novel_id,
                                    job_id: job_id.clone(),
                                    generation: self.inner.generation.get(),
                                }),
                                Vec::new(),
                            );
                            snapshot.state = JobState::Interrupted;
                            snapshot.error_code = Some("corrupt_manifest".into());
                            snapshot.message = "corrupt".into();
                            by_id.insert(job_id, snapshot);
                        }
                    }
                }
            }
        }
        let seq = by_id.values().map(|job| job.seq).max().unwrap_or(0);
        JobList {
            seq,
            jobs: by_id.into_values().collect(),
        }
    }

    /// 阅读器结果：优先最新已完成 v2 Job，其次 v1 只读缓存（标记 partial）。
    pub async fn result(&self, novel_id: i64) -> Result<Option<TranslationResult>, String> {
        if let Ok(jobs) = list_jobs(&self.inner.paths, novel_id) {
            for summary in jobs {
                if summary.corrupt || summary.pending_delete {
                    continue;
                }
                let Some(manifest) = read_manifest_at(&summary.path) else {
                    continue;
                };
                if manifest.state != JobState::Completed {
                    continue;
                }
                if let Some(result) = read_job_result(&self.inner.paths, &manifest)
                    && result.status == PublishStatus::Complete
                {
                    return Ok(Some(result));
                }
            }
        }
        let settings = self.settings_snapshot();
        let Ok((target, _)) = legacy::resolve_target_language(&settings) else {
            return Ok(None);
        };
        let book =
            crate::translation::legacy::read_latest_book(&self.inner.paths.data_dir, novel_id)?;
        Ok(book.and_then(|book| {
            crate::translation::legacy::to_partial_result(novel_id, target, &book)
        }))
    }

    /// 浮层预览：原文 / 译文对照。
    pub async fn preview(&self, job_id: &str, chunk_id: &str) -> Result<ChunkPreview, String> {
        if let Some(handle) = self.handle(job_id) {
            return preview_impl(&handle.ctx, chunk_id);
        }
        let (manifest, corrupt) = self.find_manifest(job_id)?;
        if corrupt {
            return Err("任务清单损坏，无法预览".into());
        }
        let scope = JobScope {
            novel_id: manifest.novel_id,
            job_id: manifest.job_id.clone(),
            generation: manifest.generation,
        };
        let store = JobStore::open(&self.inner.paths, &scope).map_err(|error| error.to_string())?;
        let io = StoreIo::new(Arc::new(store));
        preview_from_io(&io, chunk_id)
    }

    /// 约束浮层只读查询：读 `preparation/constraints.json` 与 manifest 状态。
    ///
    /// 与运行中的 Job 并发安全：只读文件，不写 manifest、不发事件、不阻塞派发。
    pub async fn constraints(&self, job_id: &str) -> Result<ConstraintsView, String> {
        if let Some(handle) = self.handle(job_id) {
            let awaiting = handle
                .runtime
                .lock()
                .map(|runtime| runtime.state == JobState::AwaitingConfirmation)
                .unwrap_or(false);
            return handle
                .ctx
                .with_artifacts(|artifacts| constraints_view(artifacts.io(), awaiting))?
                .ok_or_else(|| format!("约束尚未生成：{job_id}"));
        }
        let (manifest, corrupt) = self.find_manifest(job_id)?;
        if corrupt {
            return Err("任务清单损坏，无法读取约束".into());
        }
        let io = job_store(&self.inner.paths, &manifest)
            .ok_or_else(|| format!("任务目录不可用：{job_id}"))?;
        constraints_view(&io, manifest.state == JobState::AwaitingConfirmation)?
            .ok_or_else(|| format!("约束尚未生成：{job_id}"))
    }

    /// 审查问题浮层只读查询：汇总 `reviews/*.json`，按严重度与块顺序稳定排序。
    ///
    /// 只返回问题定位与计数，不含原文、译文；与运行中的 Job 并发安全。
    pub async fn issues(&self, job_id: &str) -> Result<IssuesView, String> {
        if let Some(handle) = self.handle(job_id) {
            let plan_ids = plan_ids_from_store(&handle.ctx.store);
            return handle
                .ctx
                .with_artifacts(|artifacts| issues_view(artifacts.io(), &plan_ids));
        }
        let (manifest, corrupt) = self.find_manifest(job_id)?;
        if corrupt {
            return Err("任务清单损坏，无法读取审查问题".into());
        }
        let scope = JobScope {
            novel_id: manifest.novel_id,
            job_id: manifest.job_id.clone(),
            generation: manifest.generation,
        };
        let store = JobStore::open(&self.inner.paths, &scope)
            .map_err(|error| format!("任务目录不可用：{error}"))?;
        let plan_ids = plan_ids_from_store(&store);
        let io = StoreIo::new(Arc::new(store));
        issues_view(&io, &plan_ids)
    }

    fn handle_snapshot(&self, handle: &Arc<JobHandle>) -> TranslationJobSnapshot {
        let runtime = handle
            .runtime
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let budget = handle.ctx.budget.snapshot();
        // 约束版本以 manifest 为准（工具冻结 / 用户确认后只有它被原子更新）。
        let constraint_version = handle
            .ctx
            .read_manifest()
            .map(|manifest| manifest.constraint_version)
            .unwrap_or(runtime.constraint_version);
        TranslationJobSnapshot {
            job_id: handle.ctx.scope.job_id.clone(),
            novel_id: handle.ctx.scope.novel_id,
            state: runtime.state,
            phase: runtime.phase,
            counts: runtime.counts,
            current: runtime.current.clone(),
            usage: runtime.usage,
            elapsed_ms: handle.started.elapsed().as_millis() as u64,
            preview: runtime.preview.clone(),
            message: runtime.message.clone(),
            error_code: runtime.error_code.clone(),
            seq: handle.ctx.emitter.seq(),
            constraint_version,
            budget: BudgetSnapshot {
                used: budget.used,
                limit: budget.limit,
                remaining: budget.remaining,
                manual_extra: budget.manual_extra,
            },
            already_target_language: false,
            target_language: handle.ctx.target_language.clone(),
            chunk_ids: tools::plan_chunks(&handle.ctx)
                .unwrap_or_default()
                .into_iter()
                .map(|chunk| chunk.id)
                .collect(),
        }
    }

    fn find_manifest(&self, job_id: &str) -> Result<(crate::agent::Manifest, bool), String> {
        if let Some(handle) = self.handle(job_id) {
            let manifest = handle
                .ctx
                .store
                .read_manifest()
                .map_err(|error| error.to_string())?;
            return Ok((manifest, false));
        }
        let v2 = self.inner.paths.data_dir.join("translations").join("v2");
        let Ok(novels) = std::fs::read_dir(&v2) else {
            return Err(format!("翻译任务不存在：{job_id}"));
        };
        for novel in novels.flatten() {
            let path = novel.path().join(job_id);
            if !path.is_dir() {
                continue;
            }
            let novel_id = novel
                .file_name()
                .to_string_lossy()
                .parse::<i64>()
                .unwrap_or_default();
            match read_manifest_at(&path) {
                Some(manifest) => return Ok((manifest, false)),
                None => {
                    let mut manifest = crate::agent::Manifest::new(&JobScope {
                        novel_id,
                        job_id: job_id.to_string(),
                        generation: self.inner.generation.get(),
                    });
                    manifest.state = JobState::Interrupted;
                    return Ok((manifest, true));
                }
            }
        }
        Err(format!("翻译任务不存在：{job_id}"))
    }

    // ------------------------------------------------------------------
    // 控制
    // ------------------------------------------------------------------

    /// 确认术语与风格：写回约束（版本 +1），用户值优先，随后从检查点继续。
    pub async fn confirm(&self, job_id: &str, edits: ConstraintEdit) -> Result<(), String> {
        let handle = self
            .handle(job_id)
            .ok_or_else(|| format!("翻译任务不存在：{job_id}"))?;
        let manifest = handle
            .ctx
            .store
            .read_manifest()
            .map_err(|error| error.to_string())?;
        if manifest.state == JobState::Deleted {
            return Err("翻译任务已删除".into());
        }
        let constraints = handle.ctx.with_artifacts(|artifacts| {
            let current = artifacts.read_constraints()?.unwrap_or(Constraints {
                version: 0,
                terms: Vec::new(),
                style: String::new(),
                conflicts: Vec::new(),
                unresolved: Vec::new(),
                frozen: false,
            });
            let mut next = current.clone();
            next.version = current.version + 1;
            next.frozen = true;
            next.conflicts.clear();
            next.unresolved.clear();
            for edit in &edits.terms {
                let source = edit.source_term.trim();
                if source.is_empty() {
                    continue;
                }
                let locked = edit.locked_translation.trim();
                if locked.is_empty() {
                    next.unresolved.push(format!("{source}: 用户未提供译名"));
                    continue;
                }
                let term = crate::translation::artifacts::AgentTerm {
                    source_term: source.to_string(),
                    locked_translation: locked.to_string(),
                    term_type: if edit.term_type.trim().is_empty() {
                        "other".into()
                    } else {
                        edit.term_type.trim().to_string()
                    },
                    aliases: edit
                        .aliases
                        .iter()
                        .map(|alias| alias.trim().to_string())
                        .filter(|alias| !alias.is_empty())
                        .collect(),
                    notes: edit.notes.chars().take(1_000).collect(),
                };
                if let Some(existing) = next
                    .terms
                    .iter_mut()
                    .find(|existing| existing.source_term == source)
                {
                    *existing = term;
                } else {
                    next.terms.push(term);
                }
            }
            if !edits.style.trim().is_empty() {
                next.style = edits.style.trim().to_string();
            }
            artifacts.write_constraints(&next)?;
            Ok::<_, String>(next)
        })?;
        handle.ctx.control.mark_confirmed();
        handle.ctx.control.set_awaiting_confirmation(false);
        handle.ctx.control.resume();
        {
            let mut runtime = handle
                .runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            runtime.constraint_version = constraints.version;
            runtime.state = JobState::Queued;
            runtime.message = "confirmed".into();
            runtime.error_code = None;
        }
        let _ = handle
            .ctx
            .update_manifest(|manifest| manifest.constraint_version = constraints.version);
        self.spawn_supervisor(handle);
        Ok(())
    }

    /// 暂停：停止派发，停在最近检查点。
    pub async fn pause(&self, job_id: &str) -> Result<(), String> {
        let handle = self
            .handle(job_id)
            .ok_or_else(|| format!("翻译任务不存在：{job_id}"))?;
        let state = handle
            .runtime
            .lock()
            .map(|runtime| runtime.state)
            .unwrap_or(JobState::Interrupted);
        if state == JobState::AwaitingConfirmation {
            return Err("任务正在等待确认，请先确认或取消".into());
        }
        if !matches!(
            state,
            JobState::Running | JobState::Queued | JobState::Pausing
        ) {
            return Err("当前状态不能暂停".into());
        }
        {
            let mut runtime = handle
                .runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            runtime.state = JobState::Pausing;
            runtime.message = "pausing".into();
        }
        handle.ctx.control.pause();
        // 等 supervisor 收尾，保证调用方看到稳定状态。
        for _ in 0..600 {
            if !handle.running.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        self.transition(&handle, JobState::Paused, "paused", None);
        Ok(())
    }

    /// 恢复：从检查点继续；已完成且有效的块不重复请求。
    /// 恢复：从检查点继续；已完成且有效的块不重复请求。
    ///
    /// 进程内没有 JobHandle 时（应用重启后）从 manifest + 产物重建上下文，
    /// 因此恢复不依赖进程内状态：预算用量、约束版本、阶段与块计划全部来自磁盘。
    pub async fn resume(&self, job_id: &str) -> Result<(), String> {
        let handle = match self.handle(job_id) {
            Some(handle) => handle,
            None => self.rehydrate(job_id)?,
        };
        let state = handle
            .runtime
            .lock()
            .map(|runtime| runtime.state)
            .unwrap_or(JobState::Interrupted);
        if state == JobState::AwaitingConfirmation {
            return Err("任务正在等待确认，请先确认术语与风格".into());
        }
        if !matches!(
            state,
            JobState::Paused | JobState::NeedsAttention | JobState::Interrupted | JobState::Queued
        ) {
            return Err("当前状态不能恢复".into());
        }
        if handle.running.load(Ordering::SeqCst) {
            return Err("任务正在运行".into());
        }
        // 世代与资源版本校验：缓存清理或资源变化后不可静默续跑。
        let manifest = handle
            .ctx
            .store
            .read_manifest()
            .map_err(|error| error.to_string())?;
        if manifest.generation != self.inner.generation.get() {
            return Err("翻译缓存已清理，任务不可恢复".into());
        }
        if manifest.resources != ResourceVersions::current() {
            return Err("内置资源版本已变化，不能静默续跑旧任务；请重新启动翻译".into());
        }
        // 恢复前必须能读到原文记录；缺失即视为缓存不完整，不做半截续跑。
        if read_input(&handle.ctx.store, handle.ctx.scope.novel_id).is_none() {
            return Err("任务缺少原文记录，无法从检查点恢复；请重新启动翻译".into());
        }
        handle.ctx.control.resume();
        // 用户显式恢复 = 接受重新请求上次结果未知的块。
        handle.ctx.clear_unknown_blocks();
        self.transition(&handle, JobState::Queued, "resumed", None);
        self.spawn_supervisor(handle);
        Ok(())
    }

    /// 从磁盘重建一个 Job 的内存上下文（重启后的恢复路径）。
    ///
    /// 预算：manifest 的 `attempts.model_calls / manual_extra` 与 `usage` 一并还原，
    /// 不因重启清零。阶段与块计划：`prepare_source` 不再重新切块（计划仍在磁盘上），
    /// 主 Agent 从 `job_status` 得到待办块，已完成的块直接复用。
    fn rehydrate(&self, job_id: &str) -> Result<Arc<JobHandle>, String> {
        // 1. 定位 manifest 与世代。
        let (manifest, corrupt) = self.find_manifest(job_id)?;
        if corrupt {
            return Err("任务清单损坏，无法恢复".into());
        }
        if manifest.generation != self.inner.generation.get() {
            return Err("翻译缓存已清理，任务不可恢复".into());
        }
        if manifest.resources != ResourceVersions::current() {
            return Err("内置资源版本已变化，不能静默续跑旧任务；请重新启动翻译".into());
        }
        if matches!(
            manifest.state,
            JobState::Deleted | JobState::Completed | JobState::Cancelled
        ) {
            return Err("任务已结束，不能恢复".into());
        }
        // 2. 打开存储并读回原文与设置快照。
        let scope = JobScope {
            novel_id: manifest.novel_id,
            job_id: manifest.job_id.clone(),
            generation: manifest.generation,
        };
        let store = Arc::new(
            JobStore::open_guarded(&self.inner.paths, &scope, self.inner.generation.clone())
                .map_err(|error| error.to_string())?,
        );
        let input = read_input(&store, manifest.novel_id)
            .ok_or_else(|| "任务缺少原文记录，无法恢复".to_string())?;
        let settings = self.settings_snapshot();
        let snapshot = store
            .read("settings_snapshot.json")
            .ok()
            .flatten()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or_default();
        let model_snapshot = snapshot.get("model").cloned().unwrap_or_default();
        let key = self
            .inner
            .credentials
            .read()?
            .filter(|key| !key.trim().is_empty())
            .ok_or("请先配置翻译 API Key")?;
        let model = ModelConfig {
            format: model_snapshot
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or(&settings.translation_api_format)
                .to_string(),
            base_url: settings.translation_api_url.clone(),
            model: model_snapshot
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or(&settings.translation_model)
                .to_string(),
            key,
            extra_json: settings.translation_extra.clone(),
            timeout_seconds: model_snapshot
                .get("timeout_seconds")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| {
                    legacy::translation_timeout_seconds(settings.translation_timeout_seconds)
                }),
            session_seed: format!("pixiv-tool-novel-{}", manifest.novel_id),
        };
        // 3. 预算：累计用量与手动追加都从 manifest 还原，不因重启清零。
        let limit = manifest.attempts.limit;
        let budget = Arc::new(BudgetTracker::new(limit));
        for _ in 0..manifest.attempts.model_calls {
            budget.try_consume_call();
        }
        if manifest.attempts.manual_extra > 0 {
            budget.add_manual(manifest.attempts.manual_extra, "restart_resume");
        }
        let emitter = Arc::new(JobEmitter::new(self.inner.sink.clone(), scope.clone()));
        let dispatch_gate = Arc::new(Semaphore::new(
            settings.translation_agent_novel_concurrency.clamp(1, 16) as usize,
        ));
        let usage = manifest.usage;
        let ctx = Arc::new(JobContext {
            scope: scope.clone(),
            store,
            paths: self.inner.paths.clone(),
            settings: settings.clone(),
            target_language: manifest.target_language.clone(),
            target_language_name: language_name(&manifest.target_language),
            source_language: manifest.source_language.clone(),
            budget,
            budget_config: BudgetConfig::from_settings(&settings),
            scheduler: self.inner.scheduler.clone(),
            model,
            emitter,
            control: Control::default(),
            confirm_before_start: settings.translation_confirm_before_start
                && manifest.constraint_version == 0,
            manifest_lock: StdMutex::new(()),
            journal_lock: StdMutex::new(()),
            dispatch_gate,
            usage: StdMutex::new(usage),
            unknown_blocked: StdMutex::new(BTreeSet::new()),
            io: StoreIo::new(Arc::new(
                JobStore::open_guarded(&self.inner.paths, &scope, self.inner.generation.clone())
                    .map_err(|error| error.to_string())?,
            )),
        });
        // 原文记录只用于 `prepare_source`（恢复时计划已存在，不会重新切块）。
        let _ = &input;
        let handle = Arc::new(JobHandle {
            ctx,
            started: Instant::now(),
            runtime: StdMutex::new(JobRuntime {
                state: manifest.state,
                phase: manifest.phase,
                counts: manifest.counts,
                current: None,
                usage,
                preview: Preview::default(),
                message: "resumed".into(),
                error_code: manifest.last_error.as_ref().map(|error| error.code.clone()),
                constraint_version: manifest.constraint_version,
            }),
            running: AtomicBool::new(false),
        });
        if let Ok(mut jobs) = self.inner.jobs.lock() {
            jobs.insert(job_id.to_string(), handle.clone());
        }
        log::info!(
            "novel={} job={} event=rehydrated phase={} chunks={} translated={}",
            manifest.novel_id,
            job_id,
            manifest.phase.as_str(),
            manifest.counts.chunks,
            manifest.counts.translated
        );
        Ok(handle)
    }

    /// 取消：立即传播取消信号，保留检查点与已提交产物。
    pub async fn cancel(&self, job_id: &str) -> Result<(), String> {
        let Some(handle) = self.handle(job_id) else {
            // 进程重启后任务不在内存：没有在途运行可中断，按磁盘状态直接标记取消。
            let (manifest, corrupt) = self.find_manifest(job_id)?;
            if corrupt {
                return Err("任务清单损坏，无法安全取消；请清理翻译缓存".into());
            }
            if matches!(
                manifest.state,
                JobState::Cancelled | JobState::Completed | JobState::Deleted
            ) {
                return Ok(());
            }
            let scope = JobScope {
                novel_id: manifest.novel_id,
                job_id: manifest.job_id.clone(),
                generation: manifest.generation,
            };
            let store =
                JobStore::open(&self.inner.paths, &scope).map_err(|error| error.to_string())?;
            // 只写 manifest 不广播：seq 由每任务的 emitter 维护，磁盘路径没有该状态；
            // 前端在取消成功后主动刷新任务快照。
            store.update_manifest(|manifest| {
                manifest.state = JobState::Cancelled;
                manifest.last_error = Some(SafeError {
                    code: "cancelled".into(),
                    message: "cancelled".into(),
                    step: manifest.phase.as_str().into(),
                });
            }).map_err(|error| error.to_string())?;
            return Ok(());
        };
        let state = handle
            .runtime
            .lock()
            .map(|runtime| runtime.state)
            .unwrap_or(JobState::Interrupted);
        if state == JobState::Cancelled {
            return Ok(());
        }
        handle.ctx.control.cancel();
        if handle.running.load(Ordering::SeqCst) {
            {
                let mut runtime = handle
                    .runtime
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                runtime.state = JobState::Cancelling;
                runtime.message = "cancelling".into();
            }
            for _ in 0..200 {
                if !handle.running.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        }
        self.transition(&handle, JobState::Cancelled, "cancelled", Some("cancelled"));
        Ok(())
    }

    /// 删除：Job 根目录及全部后代。
    pub async fn delete(&self, job_id: &str) -> Result<(), String> {
        if let Some(handle) = self.handle(job_id) {
            if handle.running.load(Ordering::SeqCst) {
                return Err("任务正在运行，请先取消或暂停".into());
            }
            handle.ctx.control.cancel();
            handle
                .ctx
                .store
                .remove_tree()
                .map_err(|error| error.to_string())?;
            if let Ok(mut jobs) = self.inner.jobs.lock() {
                jobs.remove(job_id);
            }
            return Ok(());
        }
        let (manifest, corrupt) = self.find_manifest(job_id)?;
        if corrupt {
            return Err("任务清单损坏，无法安全删除；请清理翻译缓存".into());
        }
        let scope = JobScope {
            novel_id: manifest.novel_id,
            job_id: job_id.to_string(),
            generation: manifest.generation,
        };
        let store = JobStore::open(&self.inner.paths, &scope).map_err(|error| error.to_string())?;
        store.remove_tree().map_err(|error| error.to_string())
    }

    // ------------------------------------------------------------------
    // 恢复与缓存失效
    // ------------------------------------------------------------------

    /// 启动恢复：未结束状态 → interrupted；不自动请求模型。
    ///
    /// 顺带收尾上次中断的删除（存在 `.deleting` 的目录）。
    pub async fn recover_interrupted(&self) -> Result<Vec<JobId>, String> {
        let v2 = self.inner.paths.data_dir.join("translations").join("v2");
        let mut recovered = Vec::new();
        let Ok(novels) = std::fs::read_dir(&v2) else {
            return Ok(recovered);
        };
        for novel in novels.flatten() {
            let Ok(jobs) = std::fs::read_dir(novel.path()) else {
                continue;
            };
            for job in jobs.flatten() {
                let path = job.path();
                if !path.is_dir() {
                    continue;
                }
                if path.join(".deleting").exists() {
                    let _ = std::fs::remove_dir_all(&path);
                    continue;
                }
                let Some(mut manifest) = read_manifest_at(&path) else {
                    continue;
                };
                if matches!(
                    manifest.state,
                    JobState::Queued | JobState::Running | JobState::Pausing | JobState::Cancelling
                ) {
                    manifest.state = JobState::Interrupted;
                    manifest.updated_at = Utc::now().to_rfc3339();
                    manifest.last_error = Some(SafeError {
                        code: "interrupted".into(),
                        message: "应用退出时任务未结束".into(),
                        step: manifest.phase.as_str().into(),
                    });
                    if let Ok(bytes) = serde_json::to_vec_pretty(&manifest) {
                        let _ = std::fs::write(path.join("manifest.json"), bytes);
                    }
                    recovered.push(manifest.job_id);
                }
            }
        }
        Ok(recovered)
    }

    /// 缓存清理后调用：递增世代、取消在途任务、清空索引。
    ///
    /// 先递增世代：任何迟到回调携带旧世代都会被写入层拒绝，也不会重建 Job 目录。
    pub fn invalidate_cache(&self) {
        let generation = self.inner.generation.bump();
        log::info!("翻译缓存清理：世代递增至 {generation}");
        let Ok(mut jobs) = self.inner.jobs.lock() else {
            return;
        };
        for handle in jobs.values() {
            handle.ctx.control.cancel();
            let mut runtime = handle
                .runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            runtime.state = JobState::Interrupted;
            runtime.message = "cache_cleared".into();
        }
        jobs.clear();
    }

    // ------------------------------------------------------------------
    // 运行
    // ------------------------------------------------------------------

    fn spawn_supervisor(&self, handle: Arc<JobHandle>) {
        if handle.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let inner = self.inner.clone();
        tokio::spawn(async move {
            TranslationService { inner }.supervise(handle).await;
        });
    }

    async fn supervise(&self, handle: Arc<JobHandle>) {
        // 同一小说串行：先取小说锁，再占活跃小说槽位（排队中的小说不占活跃名额）。
        let novel_lock = self.novel_lock(handle.ctx.scope.novel_id);
        let _novel_guard = novel_lock.lock().await;
        let _novel_permit = self
            .inner
            .scheduler
            .enter_novel(handle.ctx.scope.novel_id)
            .await;
        let result = self.drive(&handle).await;
        handle.running.store(false, Ordering::SeqCst);
        if let Err(error) = result {
            log::warn!(
                "novel={} job={} event=job_failed error={}",
                handle.ctx.scope.novel_id,
                handle.ctx.scope.job_id,
                crate::agent::events::safe_text(&error, 160)
            );
        }
    }

    async fn drive(&self, handle: &Arc<JobHandle>) -> Result<(), String> {
        let ctx = &handle.ctx;
        if ctx.control.cancelled() {
            self.transition(handle, JobState::Cancelled, "cancelled", Some("cancelled"));
            return Ok(());
        }
        self.transition(handle, JobState::Running, "running", None);
        let phase = ctx
            .read_manifest()
            .map(|manifest| manifest.phase)
            .unwrap_or(Phase::PrepareSource);
        let limits = ctx.run_limits();
        let (max_tokens, params) = ctx
            .model
            .request_params(ctx.settings.translation_agent_chunk_max_chars.max(1) as usize)?;
        let system = format!(
            "{}\n\n{}\n\n{}",
            system_prompt(),
            role_prompt(Role::Main),
            tools::COORDINATOR_TOOL_CONTRACT
        );
        let prompt = main_prompt(ctx);

        let stop = ctx.control.stop_signal();
        // 续跑循环：一次运行正常结束但完成校验未通过时，带着上次运行的压缩后
        // 历史投影与最新检查点状态重新拉起；连续无进展达到上限才转 needs_attention。
        // 单次运行的轮次上限（max_turns）只约束一轮，任务级的推进由本循环收敛。
        // 无进展续跑上限：每轮都带整段上下文，两轮仍未推进就交给用户判断，
        // 避免模型拒绝或反复空转时持续消耗额度。
        const MAX_IDLE_CONTINUATIONS: u32 = 2;
        const MAX_EMPTY_RUNS: u32 = 2;
        let mut carried: Vec<Arc<rig_core::message::Message>> = Vec::new();
        let mut last_progress = self.progress_snapshot(ctx);
        let mut idle_runs: u32 = 0;
        let mut empty_runs: u32 = 0;
        // 最后一轮正常结束时的模型回答：零工具调用收敛时作为用户可见原因。
        let mut last_stop_text = String::new();
        let mut run_index: u32 = 0;
        // 限流 / 暂时性服务错误的重试计数：与「无进展续跑」分开，按设置的
        // 重试次数与退避策略在任务级重跑整轮，而不是立刻转 needs_attention。
        let retry_policy = tools::retry_policy(ctx);
        let mut provider_retries: u32 = 0;
        let outcome: Result<(), RunError> = loop {
            run_index += 1;
            let request = RunRequest::new(
                Role::Main,
                system.clone(),
                prompt.clone(),
                ctx.model.model()?,
                tools::coordinator_tools(ctx),
                limits,
                ctx.run_sink(),
                ctx.scope.clone(),
            )
            .with_phase(phase)
            .with_budget(ctx.budget.clone())
            .with_budget_config(ctx.budget_config.clone())
            .with_compaction({
                let journal_ctx = handle.ctx.clone();
                crate::agent::CompactionConfig::new(ctx.budget_config.clone(), ctx.scope.clone())
                    .with_scheduler(ctx.scheduler.clone())
                    .with_tracker(ctx.budget.clone())
                    .with_journal(Arc::new(move |record: &crate::agent::CompactionRecord| {
                        // 压缩记录只写计数、耗时、保留范围与结果码；摘要正文不入账。
                        let phase = journal_ctx
                            .read_manifest()
                            .map(|manifest| manifest.phase)
                            .unwrap_or(Phase::Translate);
                        journal_ctx.append_session(&json!({
                            "kind": record.event.as_str(),
                            "agent": "main",
                            "phase": phase.as_str(),
                            "before_est": record.before_tokens,
                            "after_est": record.after_tokens,
                            "keep_from": record.keep_from,
                            "retained": record.retained,
                            "elapsed_ms": record.elapsed_ms,
                            "code": record.code,
                        }));
                    }))
            })
            .with_scheduler(ctx.scheduler.clone())
            .with_request_params(max_tokens, params.clone())
            .with_history(carried.clone())
            .with_result_estimator(Arc::new(tools::ChunkResultEstimator::new(ctx.clone())));
            let result = {
                let run = runtime::run(request, ctx.control.cancel.child());
                tokio::pin!(run);
                tokio::select! {
                    biased;
                    result = &mut run => Some(result),
                    () = stop.cancelled() => None,
                }
            };
            if ctx.control.cancelled() {
                self.transition(handle, JobState::Cancelled, "cancelled", Some("cancelled"));
                return Ok(());
            }
            if ctx.control.awaiting_confirmation() {
                self.transition(
                    handle,
                    JobState::AwaitingConfirmation,
                    "awaiting_confirmation",
                    None,
                );
                return Ok(());
            }
            if ctx.control.pause_requested() {
                self.transition(handle, JobState::Paused, "paused", None);
                return Ok(());
            }
            let Some(result) = result else {
                self.transition(handle, JobState::Paused, "paused", None);
                return Ok(());
            };
            match result {
                Ok(run_outcome) => {
                    ctx.append_session(&json!({
                        "kind": "main_run",
                        "run": run_index,
                        "phase": phase.as_str(),
                        "elapsed_ms": handle.started.elapsed().as_millis() as u64,
                        "tool_calls": run_outcome.tool_calls,
                        "usage": {
                            "input": run_outcome.usage.input,
                            "output": run_outcome.usage.output,
                            "total": run_outcome.usage.total,
                        },
                        "budget_used": ctx.budget.snapshot().used,
                    }));
                    self.record_usage(handle, run_outcome.usage);
                    if !run_outcome.text.trim().is_empty() {
                        last_stop_text = run_outcome.text.clone();
                    }
                    if self.verify_complete(ctx) {
                        break Ok(());
                    }
                    let progress = self.progress_snapshot(ctx);
                    if progress > last_progress {
                        last_progress = progress;
                        idle_runs = 0;
                    } else {
                        idle_runs += 1;
                    }
                    // 完全没有工具调用的空转最烧 token 也最无望：连续两轮就收敛，
                    // 不必等满无进展上限。
                    if run_outcome.tool_calls == 0 {
                        empty_runs += 1;
                    } else {
                        empty_runs = 0;
                    }
                    if idle_runs >= MAX_IDLE_CONTINUATIONS || empty_runs >= MAX_EMPTY_RUNS {
                        log_job(
                            &ctx.scope,
                            "main",
                            phase,
                            None,
                            "continuation_stopped",
                            &[
                                ("runs", &run_index.to_string()),
                                ("empty", &empty_runs.to_string()),
                            ],
                        );
                        break Ok(());
                    }
                    log_job(
                        &ctx.scope,
                        "main",
                        phase,
                        None,
                        "run_continue",
                        &[
                            ("runs", &run_index.to_string()),
                            ("idle", &idle_runs.to_string()),
                            ("empty", &empty_runs.to_string()),
                        ],
                    );
                    // 续跑提示直接给出磁盘产物推导出的下一步动作。单纯重放主提示
                    // 会让模型重复「只读状态、不写产物」的空转模式。
                    carried = run_outcome.history;
                    carried.push(Arc::new(rig_core::message::Message::user(
                        continuation_nudge(
                            ctx,
                            "上一轮回复没有产生任何新的磁盘产物",
                            run_outcome.tool_calls == 0,
                        ),
                    )));
                }
                Err(error) => {
                    let retryable =
                        matches!(&error, RunError::Provider { class, .. } if class.is_retryable());
                    if !retryable || !retry_policy.allows(provider_retries) {
                        break Err(error);
                    }
                    let attempt = provider_retries;
                    provider_retries += 1;
                    log_job(
                        &ctx.scope,
                        "main",
                        phase,
                        None,
                        "run_retry",
                        &[
                            ("attempt", &(attempt + 1).to_string()),
                            ("code", error.code()),
                        ],
                    );
                    if backoff(&retry_policy, attempt, &stop).await.is_none() {
                        if ctx.control.cancelled() {
                            self.transition(
                                handle,
                                JobState::Cancelled,
                                "cancelled",
                                Some("cancelled"),
                            );
                        } else {
                            self.transition(handle, JobState::Paused, "paused", None);
                        }
                        return Ok(());
                    }
                    // 失败轮没有可带回的历史；保留上一轮 carried，补一条带动作的提示。
                    carried.push(Arc::new(rig_core::message::Message::user(
                        continuation_nudge(
                            ctx,
                            "上一轮模型请求被服务端暂时拒绝，已按退避重试",
                            false,
                        ),
                    )));
                }
            }
        };
        match outcome {
            Ok(()) => {
                if self.verify_complete(ctx) {
                    self.transition(handle, JobState::Completed, "completed", None);
                } else {
                    // 模型最后一轮的说明（常见于内容审核拒绝）直接附到用户可见原因：
                    // 比笼统的 incomplete 更能解释「为什么不动」。
                    let excerpt = crate::agent::events::safe_text(&last_stop_text, 200);
                    let (code, base) = Self::incomplete_stage(ctx)
                        .map(|(stage, message)| (format!("incomplete_{stage}"), message))
                        .unwrap_or_else(|| {
                            (
                                "incomplete".into(),
                                "模型结束运行但完成条件未满足".into(),
                            )
                        });
                    let (code, message) = if empty_runs >= MAX_EMPTY_RUNS && !excerpt.is_empty() {
                        ("model_declined".to_string(), format!("模型未调用工具即结束：{excerpt}"))
                    } else if excerpt.is_empty() {
                        (code, base)
                    } else {
                        (code, format!("{base}；模型说明：{excerpt}"))
                    };
                    let _ = ctx.update_manifest(|manifest| {
                        manifest.last_error = Some(SafeError {
                            code: code.clone(),
                            message: message.clone(),
                            step: phase.as_str().into(),
                        });
                    });
                    self.transition(handle, JobState::NeedsAttention, &message, Some(&code));
                }
            }
            Err(error) => {
                let code = error.code().to_string();
                let message = crate::agent::events::safe_text(&error.to_string(), 200);
                let step = ctx
                    .read_manifest()
                    .map(|manifest| manifest.phase.as_str())
                    .unwrap_or("run")
                    .to_string();
                let _ = ctx.update_manifest(|manifest| {
                    manifest.last_error = Some(SafeError {
                        code: code.clone(),
                        message: message.clone(),
                        step,
                    });
                });
                self.transition(handle, JobState::NeedsAttention, &message, Some(&code));
            }
        }
        Ok(())
    }

    /// 完成校验未通过时，按磁盘产物定位卡住的阶段与用户可读原因。
    /// 返回 `None` 表示定位失败（保持笼统的 incomplete）。
    fn incomplete_stage(ctx: &JobContext) -> Option<(&'static str, String)> {
        let chunks = tools::plan_chunks(ctx).ok()?;
        if chunks.is_empty() {
            return Some(("prepare", "原文尚未完成分块".into()));
        }
        let scanned = chunks
            .iter()
            .filter(|chunk| tools::read_scan_for_progress(ctx, &chunk.id).is_some())
            .count();
        if scanned < chunks.len() {
            return Some((
                "scan",
                format!("还有 {} 个分块未完成扫描", chunks.len() - scanned),
            ));
        }
        let frozen = ctx.with_artifacts(|artifacts| {
            artifacts
                .read_constraints()
                .ok()
                .flatten()
                .is_some_and(|constraints| constraints.frozen)
        });
        if !frozen {
            return Some(("constraints", "术语与风格尚未冻结".into()));
        }
        let drafted = chunks
            .iter()
            .filter(|chunk| {
                tools::read_draft_for_verify(ctx, &chunk.id)
                    .ok()
                    .flatten()
                    .is_some()
            })
            .count();
        if drafted < chunks.len() {
            return Some((
                "translate",
                format!("还有 {} 个分块未翻译", chunks.len() - drafted),
            ));
        }
        let reviewed = chunks
            .iter()
            .filter(|chunk| {
                tools::read_review_for_verify(ctx, &chunk.id)
                    .ok()
                    .flatten()
                    .is_some()
            })
            .count();
        if reviewed < chunks.len() {
            return Some((
                "review",
                format!("还有 {} 个分块未审查通过", chunks.len() - reviewed),
            ));
        }
        let global_passed = ctx.with_artifacts(|artifacts| {
            artifacts
                .read_global_review()
                .ok()
                .flatten()
                .is_some_and(|global| global.passed)
        });
        if !global_passed {
            return Some(("global_review", "整体复核未完成或未通过".into()));
        }
        Some(("publish", "合并或发布未完成".into()))
    }

    /// 任务推进度的粗粒度快照：直接数磁盘产物（草稿、审查、冻结、发布），
    /// 只用于判断「续跑是否带来进展」，不是精确度量。
    fn progress_snapshot(&self, ctx: &JobContext) -> u64 {
        let chunks = match tools::plan_chunks(ctx) {
            Ok(chunks) => chunks,
            Err(_) => return 0,
        };
        let mut progress = 0u64;
        for chunk in &chunks {
            // 扫描是 Pass 1 的主要工作量，计入进展避免扫描中途的续跑被误判无进展。
            if tools::read_scan_for_progress(ctx, &chunk.id).is_some() {
                progress += 1;
            }
            if tools::read_draft_for_verify(ctx, &chunk.id)
                .ok()
                .flatten()
                .is_some()
            {
                progress += 4;
            }
            if tools::read_review_for_verify(ctx, &chunk.id)
                .ok()
                .flatten()
                .is_some()
            {
                progress += 2;
            }
        }
        let frozen =
            ctx.with_artifacts(|artifacts| artifacts.read_constraints().ok().flatten().is_some());
        let published =
            ctx.with_artifacts(|artifacts| artifacts.read_result().ok().flatten().is_some());
        progress + u64::from(frozen) * 8 + u64::from(published) * 16
    }

    fn record_usage(&self, handle: &Arc<JobHandle>, usage: TokenUsage) {
        {
            let mut runtime = handle
                .runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            runtime.usage.add(usage);
        }
        let _ = handle
            .ctx
            .update_manifest(|manifest| manifest.usage.add(usage));
    }

    /// 完成判据：全部块草稿结构校验通过 + 审查通过 + 整体复核通过 + 合并五项通过。
    fn verify_complete(&self, ctx: &JobContext) -> bool {
        let Ok(chunks) = tools::plan_chunks(ctx) else {
            return false;
        };
        if chunks.is_empty() {
            return false;
        }
        let mut drafts = Vec::new();
        let mut reviews = Vec::new();
        for chunk in &chunks {
            let Ok(Some(draft)) = tools::read_draft_for_verify(ctx, &chunk.id) else {
                return false;
            };
            if validate_draft_with_language(chunk, &draft, Some(&ctx.target_language)).is_err() {
                return false;
            }
            let Ok(Some(review)) = tools::read_review_for_verify(ctx, &chunk.id) else {
                return false;
            };
            if review.draft_version != draft.version || review.verdict != ReviewVerdict::Pass {
                return false;
            }
            drafts.push(draft);
            reviews.push(review);
        }
        ctx.with_artifacts(|artifacts| {
            let Ok(Some(global)) = artifacts.read_global_review() else {
                return false;
            };
            if !global.passed {
                return false;
            }
            let pages = merge_pages(&chunks, &drafts);
            let report =
                validate_merge_with_language(&chunks, &drafts, &pages, Some(&ctx.target_language));
            if !merge_report_ok(&report) {
                return false;
            }
            if publish_status_with_language(
                &chunks,
                &drafts,
                &reviews,
                Some(&global),
                Some(&ctx.target_language),
            ) != PublishStatus::Complete
            {
                return false;
            }
            matches!(
                artifacts.read_result(),
                Ok(Some(result)) if result.status == PublishStatus::Complete
            )
        })
    }

    fn transition(
        &self,
        handle: &Arc<JobHandle>,
        state: JobState,
        message: &str,
        error_code: Option<&str>,
    ) {
        // 迟到的收尾回调：缓存已清理（世代变化）时直接丢弃，不写回也不再广播。
        if handle.ctx.scope.generation != self.inner.generation.get() {
            return;
        }
        let phase = handle
            .ctx
            .read_manifest()
            .map(|manifest| manifest.phase)
            .unwrap_or(Phase::PrepareSource);
        let (counts, usage, constraint_version) = {
            let runtime = handle
                .runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            (runtime.counts, runtime.usage, runtime.constraint_version)
        };
        {
            let mut runtime = handle
                .runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            runtime.state = state;
            runtime.phase = phase;
            runtime.message = message.to_string();
            runtime.error_code = error_code.map(str::to_string);
        }
        let _ = handle.ctx.update_manifest(|manifest| {
            manifest.state = state;
            manifest.phase = phase;
            manifest.counts = counts;
            manifest.constraint_version = constraint_version.max(manifest.constraint_version);
            if let Some(code) = error_code {
                manifest.last_error = Some(SafeError {
                    code: code.to_string(),
                    message: message.to_string(),
                    step: phase.as_str().into(),
                });
            }
        });
        let mut event = work_event(
            &handle.ctx.scope,
            state,
            phase,
            counts,
            None,
            usage,
            message,
        );
        event.error_code = error_code.map(str::to_string);
        handle.ctx.emitter.emit_now(event);
        handle.ctx.emitter.flush();
    }
}

fn merge_report_ok(report: &MergeReport) -> bool {
    report.coverage_ok
        && report.order_ok
        && report.duplicate_ids.is_empty()
        && report.missing_ids.is_empty()
        && report.markup_issues.is_empty()
}

fn preview_impl(ctx: &JobContext, chunk_id: &str) -> Result<ChunkPreview, String> {
    preview_from_io(&ctx.io, chunk_id)
}

fn preview_from_io(io: &StoreIo, chunk_id: &str) -> Result<ChunkPreview, String> {
    crate::translation::artifacts::validate_chunk_id(chunk_id)?;
    let artifacts = JobArtifacts::new(io);
    let chunk = artifacts
        .read_chunk(chunk_id)?
        .ok_or_else(|| format!("分块不存在：{chunk_id}"))?;
    let draft = artifacts.read_draft(chunk_id)?;
    let review = artifacts.read_review(chunk_id)?;
    let translated = draft
        .as_ref()
        .map(|draft| {
            draft
                .segments
                .iter()
                .map(|segment| PreviewTranslation {
                    id: segment.id.clone(),
                    text: segment.text.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let segments = chunk
        .segments
        .iter()
        .map(|segment| {
            let (page, line, _) = parse_segment_id(&segment.id).unwrap_or((0, 0, 0));
            PreviewSegment {
                id: segment.id.clone(),
                page,
                line,
                text: segment.text.clone(),
                kind: match segment.kind {
                    crate::translation::source::SegmentKind::Text => "text",
                    crate::translation::source::SegmentKind::Chapter => "chapter",
                    crate::translation::source::SegmentKind::Ruby => "ruby",
                    crate::translation::source::SegmentKind::Markup => "markup",
                }
                .into(),
            }
        })
        .collect();
    Ok(ChunkPreview {
        chunk_id: chunk.id.clone(),
        order: chunk.order,
        segments,
        translated,
        context_prev: chunk.prev_context.clone(),
        context_next: chunk.next_context.clone(),
        draft_version: draft.as_ref().map(|draft| draft.version).unwrap_or(0),
        reviewed: draft.as_ref().map(|draft| draft.reviewed).unwrap_or(false),
        needs_repair: draft
            .as_ref()
            .map(|draft| draft.needs_repair)
            .unwrap_or(false),
        review: review.map(|review| PreviewReview {
            verdict: review.verdict,
            notes: review.notes,
            issues: review.issues,
        }),
    })
}

/// 约束视图：只读 `preparation/constraints.json`；不存在返回 `Ok(None)`。
fn constraints_view(
    io: &dyn ArtifactIo,
    awaiting_confirmation: bool,
) -> Result<Option<ConstraintsView>, String> {
    let artifacts = JobArtifacts::new(io);
    let Some(constraints) = artifacts.read_constraints()? else {
        return Ok(None);
    };
    let mut conflicts: Vec<ConflictGroupView> = Vec::new();
    for conflict in &constraints.conflicts {
        let term_type = constraints
            .terms
            .iter()
            .find(|term| term.source_term == conflict.source_term)
            .map(|term| term.term_type.clone())
            .filter(|term_type| !term_type.trim().is_empty())
            .unwrap_or_else(|| "other".to_string());
        let index = match conflicts.iter().position(|group| {
            group.source_term == conflict.source_term && group.term_type == term_type
        }) {
            Some(index) => index,
            None => {
                conflicts.push(ConflictGroupView {
                    source_term: conflict.source_term.clone(),
                    term_type,
                    values: Vec::new(),
                });
                conflicts.len() - 1
            }
        };
        let group = &mut conflicts[index];
        // 既有锁定译名没有来源块；候选值保留提出它的块。
        for value in [
            ConflictValueView {
                value: conflict.existing_translation.clone(),
                chunk_id: String::new(),
            },
            ConflictValueView {
                value: conflict.candidate_translation.clone(),
                chunk_id: conflict.chunk_id.clone(),
            },
        ] {
            if !group.values.contains(&value) {
                group.values.push(value);
            }
        }
    }
    Ok(Some(ConstraintsView {
        version: constraints.version,
        terms: constraints.terms,
        style: constraints.style,
        conflicts,
        unresolved: constraints.unresolved,
        frozen: constraints.frozen,
        awaiting_confirmation,
    }))
}

/// 问题视图：只读全部单块审查记录，按严重度 + 块顺序稳定排序。
fn issues_view(io: &dyn ArtifactIo, plan_ids: &[String]) -> Result<IssuesView, String> {
    let artifacts = JobArtifacts::new(io);
    let ids: Vec<String> = if plan_ids.is_empty() {
        artifacts.list_chunks()?
    } else {
        plan_ids.to_vec()
    };
    let mut issues = Vec::new();
    let mut affected = BTreeSet::new();
    let mut unresolved = BTreeSet::new();
    let mut reviewed = 0u32;
    for chunk_id in &ids {
        let Some(review) = artifacts.read_review(chunk_id)? else {
            continue;
        };
        reviewed = reviewed.saturating_add(1);
        if review.verdict != ReviewVerdict::Pass || !review.issues.is_empty() {
            unresolved.insert(chunk_id.clone());
        }
        for issue in &review.issues {
            affected.insert(chunk_id.clone());
            issues.push(IssueView {
                chunk_id: chunk_id.clone(),
                segment_id: issue.segment_id.clone(),
                kind: issue.kind.clone(),
                severity: issue.severity,
                // 只保留问题定位说明；限长且压平控制字符，不含原文 / 译文。
                detail: crate::agent::events::safe_text(&issue.detail, 300),
            });
        }
    }
    issues.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then_with(|| chunk_rank(&a.chunk_id).cmp(&chunk_rank(&b.chunk_id)))
            .then_with(|| a.segment_id.cmp(&b.segment_id))
            .then_with(|| a.kind.cmp(&b.kind))
    });
    let mut affected_chunks: Vec<String> = affected.into_iter().collect();
    affected_chunks.sort_by_key(|chunk_id| chunk_rank(chunk_id));
    let mut unresolved_chunks: Vec<String> = unresolved.into_iter().collect();
    unresolved_chunks.sort_by_key(|chunk_id| chunk_rank(chunk_id));
    Ok(IssuesView {
        issues,
        affected_chunks,
        unresolved_chunks,
        reviewed,
        total: ids.len() as u32,
    })
}

/// 计划内块 ID；计划缺失时返回空（调用方退化为已落盘块）。
fn plan_ids_from_store(store: &JobStore) -> Vec<String> {
    let Some(bytes) = store.read("preparation/chunk-plan.json").ok().flatten() else {
        return Vec::new();
    };
    serde_json::from_slice::<crate::translation::tools::ChunkPlan>(&bytes)
        .map(|plan| plan.entries.into_iter().map(|entry| entry.id).collect())
        .unwrap_or_default()
}

/// `c0001` → 1；非法 ID 排到最后。
fn chunk_rank(chunk_id: &str) -> u32 {
    chunk_id
        .strip_prefix('c')
        .and_then(|digits| digits.parse::<u32>().ok())
        .unwrap_or(u32::MAX)
}

/// 读取 Job 内保存的原文（`prepare_source` 与阶段重置共用）。
pub fn read_input(store: &JobStore, novel_id: i64) -> Option<legacy::NovelInput> {
    let bytes = store.read("source/input.json").ok()??;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    Some(legacy::NovelInput {
        novel_id,
        title: value.get("title")?.as_str()?.to_string(),
        tags: value
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        content: value.get("content")?.as_str()?.to_string(),
    })
}

/// 目标语言 code → 提示词语言名（恢复时从 manifest 还原）。
fn language_name(code: &str) -> String {
    legacy::TARGET_LANGUAGES
        .iter()
        .find(|(candidate, _)| *candidate == code)
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| code.to_string())
}

fn language_code(language: legacy::SourceLanguage) -> &'static str {
    use legacy::SourceLanguage as L;
    match language {
        L::Japanese => "ja",
        L::Korean => "ko",
        L::Chinese { .. } => "zh",
        L::Russian => "ru",
        L::Latin => "latin",
        L::Unknown => "unknown",
    }
}

/// 续跑提示：一轮正常结束但完成校验未通过、或可重试的模型请求失败后注入。
/// 运行层直接给出磁盘产物推导出的下一步动作，避免模型重复「只读状态、不写产物」。
/// `empty_turn` 表示上一轮完全没有发起工具调用，需要更强的格式要求与参数示例。
fn continuation_nudge(ctx: &JobContext, reason: &str, empty_turn: bool) -> String {
    let pending = tools::pending_state(ctx);
    let phase = ctx
        .read_manifest()
        .map(|manifest| manifest.phase.as_str().to_string())
        .unwrap_or_else(|_| "prepare_source".into());
    let mut lines = vec![
        format!("续跑指令（运行层生成，优先于此前所有说明）：任务停在「{phase}」阶段，{reason}。"),
        "只输出计划、总结或扫描/译文正文都不算完成，本轮回复必须包含工具调用；\
         下面的待办清单直接取自磁盘产物，不要再调用 job_status。"
            .to_string(),
    ];
    if empty_turn {
        lines.push(
            "上一轮没有发起任何工具调用。本次回复只允许是工具调用本身：\
             不要写解释、计划或总结，也不要复述扫描结果——写在回复里不会落盘。"
                .into(),
        );
    }
    let brief = |ids: &[String]| {
        const LIMIT: usize = 24;
        if ids.len() > LIMIT {
            format!("{} 等 {} 个块", ids[..LIMIT].join("、"), ids.len())
        } else {
            format!("{}（共 {} 个）", ids.join("、"), ids.len())
        }
    };
    if pending.chunk_ids.is_empty() {
        lines.push(
            "下一步：调用 prepare_source() 建立分块计划；返回数量为 0 时上报需要人工处理。"
                .into(),
        );
    } else if !pending.pending_scan.is_empty() {
        lines.push(format!(
            "下一步：对未扫描块 {} 逐个调用 read_chunk(chunk_id, true)，单轮最多 2 个；\
             每读完一块立刻调用 submit_scan(...)。",
            brief(&pending.pending_scan)
        ));
        lines.push(
            r#"submit_scan 参数示例（字段名固定）：{"chunk_id":"c0001","terms":[{"source_term":"勇者","locked_translation":"勇者","term_type":"角色"}],"style":{"formality":"轻小说口语","narration":"第一人称"},"notes":"","confidence":"high"}。没有专有名词时 terms 传 []；style 不能省略。"#
                .into(),
        );
    } else if !pending.constraints_frozen {
        lines.push("下一步：全部块已扫描，立即调用 freeze_constraints() 生成冻结版本。".into());
    } else if !pending.pending_translate.is_empty() {
        lines.push(format!(
            "下一步：调用 dispatch_translation([...]) 派发未译块 {}，按并发与预算分批。",
            brief(&pending.pending_translate)
        ));
    } else if !pending.pending_review.is_empty() {
        lines.push(format!(
            "下一步：调用 dispatch_review([...]) 派发未审块 {}。",
            brief(&pending.pending_review)
        ));
    } else if !pending.published {
        lines.push(
            "下一步：调用 merge_drafts() 核对覆盖与顺序，然后 record_global_review(...) 与 publish()。"
                .into(),
        );
    } else {
        lines.push("全部产物已落盘，等待运行层校验；如仍有未决项，按错误码处理对应阶段。".into());
    }
    lines.join("\n")
}

/// 主 Agent 的用户提示：只含 Job 摘要（页 / 块计数、语言、阶段、产物摘要）。
fn main_prompt(ctx: &JobContext) -> String {
    let manifest = ctx
        .read_manifest()
        .unwrap_or_else(|_| crate::agent::Manifest::new(&ctx.scope));
    let (drafts, reviews, chunks, constraints) = ctx.with_artifacts(|artifacts| {
        let chunks = artifacts.list_chunks().unwrap_or_default();
        let drafts = chunks
            .iter()
            .filter(|id| artifacts.read_draft(id).ok().flatten().is_some())
            .count();
        let reviews = chunks
            .iter()
            .filter(|id| artifacts.read_review(id).ok().flatten().is_some())
            .count();
        let constraints = artifacts.read_constraints().ok().flatten();
        (drafts, reviews, chunks, constraints)
    });
    let resumed = matches!(
        manifest.state,
        JobState::Interrupted | JobState::Paused | JobState::NeedsAttention | JobState::Queued
    );
    format!(
        "任务：整篇小说翻译（{} 页 · {} 块）\n\
         目标语言：{}（{}）\n\
         当前阶段：{}\n\
         约束：{}\n\
         检查点：已译 {}/{}，已审 {}/{}；可用分块 {} 个\n\
         {}\n\
         请先调用 job_status 读取当前状态与待办块，再从检查点继续；不要重复已完成的工作，也不要把原文读回上下文。\n\
         推进靠工具调用：在任务完成或需要人工介入之前，每次回复都必须发起工具调用；\
         计划、总结或扫描/译文正文写在普通文本里不会落盘，也不算完成。",
        manifest.counts.pages,
        chunks.len(),
        ctx.target_language,
        ctx.target_language_name,
        manifest.phase.as_str(),
        match &constraints {
            Some(constraints) if constraints.frozen => format!(
                "已冻结版本 {}（{} 条术语）",
                constraints.version,
                constraints.terms.len()
            ),
            _ => "尚未冻结".into(),
        },
        drafts,
        chunks.len(),
        reviews,
        chunks.len(),
        chunks.len(),
        if resumed {
            "本次为恢复运行：已完成且有效的块不会重复请求。"
        } else {
            ""
        }
    )
}

fn settings_snapshot_json(settings: &Settings, target: &str, model: &ModelConfig) -> Value {
    json!({
        "target_language": target,
        "model": {
            "format": model.format,
            "model": model.model,
            "timeout_seconds": model.timeout_seconds,
        },
        "confirm_before_start": settings.translation_confirm_before_start,
        "runtime": {
            "global_concurrency": settings.translation_agent_global_concurrency,
            "novel_concurrency": settings.translation_agent_novel_concurrency,
            "active_novels": settings.translation_agent_active_novels,
            "retry_limit": settings.translation_agent_retry_limit,
            "backoff_base_ms": settings.translation_agent_backoff_base_ms,
            "backoff_max_seconds": settings.translation_agent_backoff_max_seconds,
            "rework_rounds": settings.translation_agent_rework_rounds,
            "split_depth": settings.translation_agent_split_depth,
            "tool_corrections": settings.translation_agent_tool_corrections,
            "max_turns": settings.translation_agent_max_turns,
            "stall_limit": settings.translation_agent_stall_limit,
            "replay_unknown": settings.translation_agent_replay_unknown,
        },
        "budget": {
            "context_window": settings.translation_agent_context_window,
            "output_limit": settings.translation_agent_output_cap,
            "compact_percent": settings.translation_agent_compact_percent,
            "recent_history_tokens": settings.translation_agent_recent_history_tokens,
            "summary_tokens": settings.translation_agent_summary_tokens,
            "call_budget": settings.translation_call_budget,
        },
        "chunking": {
            "target_chars": settings.translation_agent_chunk_target_chars,
            "max_chars": settings.translation_agent_chunk_max_chars,
            "context_chars": settings.translation_agent_context_chars,
        }
    })
}

/// 从磁盘读取 manifest（不要求世代匹配；索引与恢复共用）。
fn read_manifest_at(path: &std::path::Path) -> Option<crate::agent::Manifest> {
    let bytes = std::fs::read(path.join("manifest.json")).ok()?;
    let manifest: crate::agent::Manifest = serde_json::from_slice(&bytes).ok()?;
    if manifest.schema != crate::agent::storage::MANIFEST_SCHEMA {
        return None;
    }
    Some(manifest)
}

/// v2 任务目录：`data/translations/v2/<novel_id>/<job_id>`。
fn v2_job_dir(data_dir: &std::path::Path, novel_id: i64, job_id: &str) -> std::path::PathBuf {
    data_dir
        .join("translations")
        .join("v2")
        .join(novel_id.to_string())
        .join(job_id)
}

/// 从任务目录读取分块计划的块 ID。细分后 ID 不再连续，只能以计划为准。
fn plan_chunk_ids_at(job_dir: &std::path::Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(job_dir.join("preparation").join("chunk-plan.json")) else {
        return Vec::new();
    };
    serde_json::from_slice::<tools::ChunkPlan>(&bytes)
        .map(|plan| plan.entries.into_iter().map(|entry| entry.id).collect())
        .unwrap_or_default()
}

fn snapshot_from_manifest(
    manifest: &crate::agent::Manifest,
    chunk_ids: Vec<String>,
) -> TranslationJobSnapshot {
    let elapsed_ms = chrono::DateTime::parse_from_rfc3339(&manifest.created_at)
        .ok()
        .map(|created| {
            Utc::now()
                .signed_duration_since(created.with_timezone(&Utc))
                .num_milliseconds()
                .max(0) as u64
        })
        .unwrap_or(0);
    TranslationJobSnapshot {
        job_id: manifest.job_id.clone(),
        novel_id: manifest.novel_id,
        state: manifest.state,
        phase: manifest.phase,
        counts: manifest.counts,
        current: None,
        usage: manifest.usage,
        elapsed_ms,
        preview: Preview::default(),
        message: manifest.state.as_str().into(),
        error_code: manifest.last_error.as_ref().map(|error| error.code.clone()),
        seq: 0,
        constraint_version: manifest.constraint_version,
        budget: BudgetSnapshot {
            used: manifest.attempts.model_calls,
            limit: manifest.attempts.limit,
            remaining: manifest
                .attempts
                .limit
                .saturating_sub(manifest.attempts.model_calls),
            manual_extra: manifest.attempts.manual_extra,
        },
        already_target_language: false,
        target_language: manifest.target_language.clone(),
        chunk_ids,
    }
}

// ----------------------------------------------------------------------
// 诊断 / 测试辅助（只读）
// ----------------------------------------------------------------------

fn job_store(paths: &AppPaths, manifest: &crate::agent::Manifest) -> Option<StoreIo> {
    let scope = JobScope {
        novel_id: manifest.novel_id,
        job_id: manifest.job_id.clone(),
        generation: manifest.generation,
    };
    let store = JobStore::open(paths, &scope).ok()?;
    Some(StoreIo::new(Arc::new(store)))
}

/// 读取任意 Job 的阅读器结果。
pub fn read_job_result(
    paths: &AppPaths,
    manifest: &crate::agent::Manifest,
) -> Option<TranslationResult> {
    let io = job_store(paths, manifest)?;
    let artifacts = JobArtifacts::new(&io);
    artifacts.read_result().ok().flatten()
}

/// 读取 Job 的合并报告（诊断用）。
pub fn read_merge_report(
    paths: &AppPaths,
    manifest: &crate::agent::Manifest,
) -> Option<MergeReport> {
    let io = job_store(paths, manifest)?;
    let artifacts = JobArtifacts::new(&io);
    artifacts.read_merge_report().ok().flatten()
}

/// 读取 Job 的块 / 草稿 / 审查（测试断言用）。
pub fn read_job_artifacts(
    paths: &AppPaths,
    manifest: &crate::agent::Manifest,
) -> Option<(Vec<Chunk>, Vec<Draft>, Vec<Review>)> {
    let io = job_store(paths, manifest)?;
    let artifacts = JobArtifacts::new(&io);
    let chunks: Vec<Chunk> = artifacts
        .list_chunks()
        .ok()?
        .iter()
        .filter_map(|id| artifacts.read_chunk(id).ok().flatten())
        .collect();
    let drafts = chunks
        .iter()
        .filter_map(|chunk| artifacts.read_draft(&chunk.id).ok().flatten())
        .collect();
    let reviews = chunks
        .iter()
        .filter_map(|chunk| artifacts.read_review(&chunk.id).ok().flatten())
        .collect();
    Some((chunks, drafts, reviews))
}

/// 渲染合并稿（测试与发布共用）。
pub fn render_job_markdown(source: &SourceDoc, chunks: &[Chunk], drafts: &[Draft]) -> String {
    let pages: BTreeMap<u32, Vec<TranslatedLine>> = merge_pages(chunks, drafts);
    render_markdown(source, &pages)
}

/// 读取最新 v1 缓存的页译文（只读兼容测试用）。
pub fn legacy_pages(
    paths: &AppPaths,
    novel_id: i64,
) -> Result<Option<BTreeMap<usize, Vec<legacy::TranslatedLine>>>, String> {
    Ok(
        crate::translation::legacy::read_latest_book(&paths.data_dir, novel_id)?
            .map(|book| book.pages),
    )
}

/// 判断错误是否为缓存失效（供命令层映射提示）。
pub fn is_cache_invalid(error: &str) -> bool {
    error.contains("缓存世代不一致")
        || error.contains("Job 已被标记删除")
        || error.contains("Job 目录不存在")
        || error.contains("缓存已清理")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(tag: &str) -> (AppPaths, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("pixiv-svc-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = AppPaths {
            data_dir: dir.clone(),
            config_dir: dir.join("config"),
            logs_dir: dir.join("logs"),
        };
        (paths, dir)
    }

    fn settings() -> Settings {
        Settings {
            translation_api_url: "http://127.0.0.1:9/v1".into(),
            translation_model: "mock".into(),
            translation_target_language: "zh-CN".into(),
            ..Settings::default()
        }
    }

    #[test]
    fn control_pause_stop_and_cancel_are_independent_signals() {
        let control = Control::default();
        assert!(!control.pause_requested());
        assert!(!control.cancelled());
        control.pause();
        assert!(control.pause_requested());
        assert!(!control.cancelled(), "暂停不等于取消");
        control.resume();
        assert!(!control.pause_requested());
        control.cancel();
        assert!(control.cancelled());
        control.resume();
        assert!(control.cancelled(), "取消不可逆");
    }

    #[test]
    fn control_stop_signal_is_refreshed_on_resume() {
        let control = Control::default();
        let first = control.stop_signal();
        assert!(!first.is_cancelled());
        control.pause();
        assert!(first.is_cancelled(), "暂停让主运行停在检查点");
        control.resume();
        let second = control.stop_signal();
        assert!(!second.is_cancelled(), "恢复后是新令牌");
        assert!(first.is_cancelled(), "旧令牌保持已取消");
    }

    #[test]
    fn control_rework_counter_and_reset_notes_are_recorded() {
        let control = Control::default();
        assert_eq!(control.rework_rounds(), 0);
        assert_eq!(control.bump_rework(), 1);
        assert_eq!(control.bump_rework(), 2);
        control.note_reset(Phase::Translate, "译文方向整体错误");
        let notes = control.reset_notes();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("translate"));
        assert!(notes[0].contains("译文方向整体错误"));
    }

    #[test]
    fn model_config_never_debugs_the_key() {
        let config = ModelConfig {
            format: "chat_completions".into(),
            base_url: "https://example.com/v1".into(),
            model: "m".into(),
            key: "super-secret-key".into(),
            extra_json: json!({}),
            timeout_seconds: 60,
            session_seed: "pixiv-tool-novel-1".into(),
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("super-secret-key"), "Debug 不得泄露 Key");
        assert!(debug.contains("example.com"));
    }

    #[test]
    fn model_config_request_params_drops_duplicate_chat_output_cap() {
        let config = ModelConfig {
            format: "chat_completions".into(),
            base_url: "https://example.com/v1".into(),
            model: "m".into(),
            key: "k".into(),
            extra_json: json!({}),
            timeout_seconds: 60,
            session_seed: "s".into(),
        };
        let (cap, params) = config.request_params(1_000).unwrap();
        assert!(cap.is_some(), "Chat 也要给出等值上限");
        let params = params.unwrap();
        assert!(
            params.get("max_completion_tokens").is_none(),
            "不得同时下发两个输出上限字段"
        );
        assert_eq!(params["reasoning_effort"], "medium");
    }

    #[tokio::test]
    async fn diagnostic_toggle_is_session_scoped() {
        let (paths, dir) = fixture("diag");
        let service =
            TranslationService::for_tests(paths, settings(), Arc::new(NoopSink), Some("k".into()));
        assert!(!service.diagnostic(), "默认关闭");
        assert!(service.set_diagnostic(true));
        assert!(service.diagnostic());
        assert!(!service.set_diagnostic(false));
        assert!(!service.diagnostic());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn start_rejects_missing_configuration_before_creating_a_job() {
        let (paths, dir) = fixture("noconf");
        let broken = Settings {
            translation_api_url: String::new(),
            ..Settings::default()
        };
        let service = TranslationService::for_tests(
            paths.clone(),
            broken,
            Arc::new(NoopSink),
            Some("k".into()),
        );
        let error = service
            .start(
                legacy::NovelInput {
                    novel_id: 1,
                    title: "t".into(),
                    tags: vec![],
                    description: String::new(),
                    content: "本文".into(),
                },
                false,
            )
            .await
            .unwrap_err();
        assert!(
            error.contains("API URL") || error.contains("配置"),
            "得到 {error}"
        );
        assert!(
            !paths.data_dir.join("translations/v2").exists(),
            "校验失败不得创建 Job 目录"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn invalidate_cache_clears_index_and_bumps_generation_per_call() {
        let (paths, dir) = fixture("gen");
        let service =
            TranslationService::for_tests(paths, settings(), Arc::new(NoopSink), Some("k".into()));
        let start = service.generation().get();
        service.invalidate_cache();
        assert_eq!(service.generation().get(), start + 1);
        service.invalidate_cache();
        assert_eq!(service.generation().get(), start + 2);
        assert!(service.jobs().await.jobs.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn operations_on_unknown_job_return_errors_not_panics() {
        let (paths, dir) = fixture("unknown");
        let service =
            TranslationService::for_tests(paths, settings(), Arc::new(NoopSink), Some("k".into()));
        assert!(service.snapshot("20260101000000-abcdef").await.is_err());
        assert!(service.preview("nope", "c0001").await.is_err());
        assert!(service.resume("nope").await.is_err());
        assert!(service.cancel("nope").await.is_err());
        assert!(service.delete("nope").await.is_err());
        assert!(service.pause("nope").await.is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn is_cache_invalid_matches_generation_and_missing_job() {
        assert!(is_cache_invalid("缓存世代不一致：期望 1，当前 2"));
        assert!(is_cache_invalid("Job 已被标记删除，拒绝写入"));
        assert!(is_cache_invalid("Job 目录不存在或不可用"));
        assert!(is_cache_invalid("翻译缓存已清理，任务不可恢复"));
        assert!(!is_cache_invalid("模型返回 429"));
    }
}
