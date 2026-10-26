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
    time::{Duration, Instant},
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
        validation::{merge_pages, validate_merge_with_language},
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
    /// 按块的返工轮次：只统计 `dispatch_translation` 对「待返工块」的派发。
    /// 首次翻译与 `revise_draft` 不计；用户手动恢复任务时清空。
    rework_rounds: StdMutex<BTreeMap<String, u32>>,
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

    /// 单块返工轮次 +1，返回累计值（允许超过上限，便于诊断）。
    pub fn bump_rework(&self, chunk_id: &str) -> u32 {
        let mut rounds = self
            .rework_rounds
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let used = rounds.entry(chunk_id.to_string()).or_insert(0);
        *used = used.saturating_add(1);
        *used
    }

    /// 单块已用返工轮次。
    pub fn rework_rounds(&self, chunk_id: &str) -> u32 {
        self.rework_rounds
            .lock()
            .map(|rounds| rounds.get(chunk_id).copied().unwrap_or(0))
            .unwrap_or(0)
    }

    /// 额度未用尽则记一轮并返回 `true`；已用尽返回 `false`（不记账）。
    ///
    /// 派发工具逐块调用本方法：A 块额度用尽只拒绝 A，不影响同批其他块。
    pub fn try_reserve_rework(&self, chunk_id: &str, limit: u32) -> bool {
        let mut rounds = self
            .rework_rounds
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let used = rounds.entry(chunk_id.to_string()).or_insert(0);
        if *used >= limit {
            return false;
        }
        *used = used.saturating_add(1);
        true
    }

    /// 所有块已用轮次的最大值（兼容旧前端的整体字段）。
    pub fn max_rework_rounds(&self) -> u32 {
        self.rework_map().values().copied().max().unwrap_or(0)
    }

    /// 按块返工计数快照。
    pub fn rework_map(&self) -> BTreeMap<String, u32> {
        self.rework_rounds
            .lock()
            .map(|rounds| rounds.clone())
            .unwrap_or_default()
    }

    /// 清空全部块的返工计数（仅用户手动恢复任务时调用）。
    pub fn clear_rework(&self) {
        if let Ok(mut rounds) = self.rework_rounds.lock() {
            rounds.clear();
        }
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
    /// `sessions/journal.jsonl` 的内存缓冲：append 只进内存并标脏，由间隔或
    /// 显式 flush 点（终态 transition）整文件落盘；单个互斥锁同时承担原
    /// journal_lock 的追加串行化职责（flush 写盘期间 append 阻塞等待）。
    journal: StdMutex<JournalBuffer>,
    /// 单 Job 子任务并发闸门。
    pub dispatch_gate: Arc<Semaphore>,
    /// 分块计划写路径串行化：`split_chunk` 的读计划 → 派生 ID → 写计划是
    /// 多步组合操作，并发派发同时缩块时无锁的后写会覆盖先写（丢 split
    /// 条目）或让两批派生块分到相同 ID（产物相互覆盖）。同步短临界区，
    /// 不得跨 `await` 持有。
    pub(crate) plan_lock: StdMutex<()>,
    /// 分块计划元数据缓存（按计划顺序的块 ID）。`job_status` / 计数 / 待办
    /// 分类等只读路径不再逐块读盘反序列化原文。写路径（`prepare_source` /
    /// `split_chunk`，均持 `plan_lock`）落盘成功即在 `write_plan` 内同步更新；
    /// 读路径未命中时读盘回填，回填持 `plan_lock` 双检，避免旧值覆盖新值。
    /// 值为 `None` 表示尚未从磁盘加载（新建 Job 无计划时保持 `None`）。
    pub(crate) plan_cache: StdMutex<Option<std::sync::Arc<Vec<String>>>>,
    /// 本 Job 累计用量（跨多次运行）。
    usage: StdMutex<TokenUsage>,
    /// 结果未知且按设置不自动重放的块：再次派发直接跳过，避免续跑时反复
    /// 重发同一请求；用户手动恢复时清空。
    unknown_blocked: StdMutex<BTreeSet<String>>,
    /// 产物访问器（持有 `Arc<JobStore>`，与上下文同寿命）。
    io: StoreIo,
}

/// journal 单 Job 保留上限（行）；超出后一次裁掉一半旧行而非逐行，降低
/// 触顶后每次 flush 的重写体积。
const JOURNAL_MAX_LINES: usize = 4_000;
/// 触顶裁剪后保留的行数（最新的一半）。
const JOURNAL_KEEP_LINES: usize = 2_000;
/// journal 缓冲的落盘间隔：append 只进内存，距上次落盘超过该间隔才整文件
/// 重写一次。期间崩溃最多丢这一窗口内的诊断记录（journal 是回放辅助，
/// 允许丢最近少量记录）；终态 transition 强制 flush，不受间隔限制。
const JOURNAL_FLUSH_INTERVAL: Duration = Duration::from_secs(5);
/// `sessions/journal.jsonl` 在 Job 目录内的相对路径。
const JOURNAL_PATH: &str = "sessions/journal.jsonl";

/// `sessions/journal.jsonl` 的内存缓冲：append 追加行并标脏，flush 把全部
/// 内存行一次性原子写盘（替代旧的「每条追加都整文件读改写 + fsync」）。
struct JournalBuffer {
    lines: Vec<String>,
    dirty: bool,
    last_flush: Instant,
}

impl JournalBuffer {
    /// 空缓冲（新建 Job：journal 必然不存在）。
    fn empty() -> Self {
        Self {
            lines: Vec::new(),
            dirty: false,
            last_flush: Instant::now(),
        }
    }

    /// 从磁盘装载既有记录（rehydrate 恢复路径；文件不存在即空缓冲）。
    fn load(store: &JobStore) -> Self {
        let bytes = store.read(JOURNAL_PATH).ok().flatten().unwrap_or_default();
        Self {
            lines: String::from_utf8_lossy(&bytes)
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(str::to_string)
                .collect(),
            dirty: false,
            last_flush: Instant::now(),
        }
    }
}

/// 主 Agent 单轮轮次下限。
///
/// `translation_agent_max_turns` 约束的是单个子 Agent 一轮能走多少步；协调者要
/// 串联扫描、冻结、翻译、审查、返工、整体复核与发布，默认 24 轮不够用。
const COORDINATOR_MIN_TURNS: u32 = 48;

/// 主 Agent 命中单轮轮次上限后的自动续跑次数（与无进展续跑分开计数）。
/// 子 Agent 的 MaxTurns 不续跑——单块在设置轮次内交不出产物就按块失败，
/// 避免单块无限循环。
const MAX_TURN_CONTINUATIONS: u32 = 2;

/// MaxTurns 续跑门禁：`continuations` 为已续跑次数，未达上限时允许再拉起一轮。
fn can_continue_after_max_turns(continuations: u32) -> bool {
    continuations < MAX_TURN_CONTINUATIONS
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

    /// 主 Agent（协调者）的单轮轮次预算。
    ///
    /// 设置值描述单块子 Agent 的一轮步数；协调者要串联扫描、冻结、翻译、审查、
    /// 返工、整体复核与发布，轮次需求远高于单块子 Agent，因此下限抬到
    /// [`COORDINATOR_MIN_TURNS`]，上限仍与设置一致（128）。无进展与工具纠正
    /// 上限不变，避免主 Agent 空转时也被放大。
    pub fn coordinator_run_limits(&self) -> RunLimits {
        let base = self.run_limits();
        RunLimits {
            max_turns: base.max_turns.max(COORDINATOR_MIN_TURNS),
            ..base
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

    /// 运行层（主会话与子任务）的事件出口：统一归一化为预览，不推进 Job 状态机。
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
    /// 记录先进内存缓冲并标脏；距上次落盘 ≥ [`JOURNAL_FLUSH_INTERVAL`] 时才
    /// 整文件落盘一次，终态 transition 强制 flush（见
    /// [`TranslationService::transition`]）。journal 是诊断回放辅助，崩溃时
    /// 丢失最近一个间隔内的记录可接受，换来消除「每条追加都整文件读改写 +
    /// fsync」的写放大。超限一次裁掉一半旧行（保留最近事实）。
    ///
    /// 主运行的上下文压缩经 `CompactionConfig::with_journal` 注入本方法，记录
    /// `compact_start / compact_done / compact_failed`，只含估算 token、耗时、
    /// 保留范围与结果码。
    pub fn append_session(&self, record: &Value) {
        let line = match serde_json::to_string(record) {
            Ok(line) => line,
            Err(_) => return,
        };
        let due = {
            let mut buffer = self
                .journal
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            buffer.lines.push(line);
            if buffer.lines.len() > JOURNAL_MAX_LINES {
                let keep_from = buffer.lines.len() - JOURNAL_KEEP_LINES;
                buffer.lines.drain(..keep_from);
            }
            buffer.dirty = true;
            buffer.last_flush.elapsed() >= JOURNAL_FLUSH_INTERVAL
        };
        if due {
            self.flush_journal(false);
        }
    }

    /// 把内存 journal 缓冲整体落盘。
    ///
    /// `force=false` 时仅在距上次落盘 ≥ [`JOURNAL_FLUSH_INTERVAL`] 且有新记录
    /// 时写；`force=true`（终态 transition、显式收尾、测试断言前）无视间隔。
    /// 写失败保留脏标记，由下一次 flush 重试。单个 journal 互斥锁同时串行化
    /// 追加与写盘：flush 写盘期间并发 append 阻塞等待，不丢行。
    pub fn flush_journal(&self, force: bool) {
        let mut buffer = self
            .journal
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !buffer.dirty || (!force && buffer.last_flush.elapsed() < JOURNAL_FLUSH_INTERVAL) {
            return;
        }
        let mut bytes = Vec::new();
        for line in &buffer.lines {
            bytes.extend_from_slice(line.as_bytes());
            bytes.push(b'\n');
        }
        if self.store.write_atomic(JOURNAL_PATH, &bytes).is_ok() {
            buffer.dirty = false;
            buffer.last_flush = Instant::now();
        }
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

/// 运行层事件归一化：所有运行层事件都只作为「预览」上报。
///
/// Job 的阶段 / 状态 / 错误由 [`TranslationService`] 的状态机统一推进；
/// 子任务与主会话的终态都不能冒充 Job 终态，也不能用运行层自己的阶段覆盖
/// Job 阶段（ADR 0037）。终态事件清空该角色的预览文本，作为「本轮结束」占位，
/// 让事件合并器把界面让给其他仍在生成的角色。
struct JobProgressSink {
    emitter: Arc<JobEmitter>,
}

impl EventSink for JobProgressSink {
    fn emit(&self, mut event: JobEvent) {
        if !matches!(event.state, JobState::Running) {
            event.state = JobState::Running;
            event.preview = event.preview.cleared_text();
            event.message = "preview".into();
            event.error_code = None;
        } else if event.message != "preview" {
            // run_start 等运行层杂项同样只作为空占位事件。
            event.preview = event.preview.cleared_text();
            event.message = "preview".into();
            event.error_code = None;
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
        // 原文解析与语言检测是对最大 8MB 输入的 CPU 密集操作，连同内容副本
        // 挪到阻塞线程执行，避免长时间占住异步 worker。
        let content = novel.content.clone();
        let target_for_parse = target_code.to_string();
        let (source, source_language) = tokio::task::spawn_blocking(
            move || -> Result<(SourceDoc, legacy::SourceLanguage), String> {
                let source = parse_source(&content, &target_for_parse)?;
                let source_language = legacy::detect_language(&content);
                Ok((source, source_language))
            },
        )
        .await
        .map_err(|error| format!("解析小说原文失败：{error}"))??;
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
        let job_id = self.new_job_id(novel.novel_id);
        let scope = JobScope {
            novel_id: novel.novel_id,
            job_id: job_id.clone(),
            generation: self.inner.generation.get(),
        };
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
        // 分块计算（对全量原文的 CPU 密集操作）与首批产物落盘（create_guarded
        // 建目录 + 多次 write_atomic，均含 fsync）同为阻塞操作，连同 novel /
        // source 的所有权一起在阻塞线程完成，异步侧拿回存储句柄与计数。
        let generation = self.inner.generation.clone();
        let paths = self.inner.paths.clone();
        let scope_for_store = scope.clone();
        let call_budget = settings.translation_call_budget;
        let target_for_store = target_code.to_string();
        let source_language_code = language_code(source_language).to_string();
        let (store, counts, call_limit) =
            tokio::task::spawn_blocking(move || -> Result<_, String> {
                let chunks = crate::translation::chunking::chunk_source(&source, &chunk_config);
                let call_limit = if call_budget > 0 {
                    call_budget as u32
                } else {
                    crate::agent::auto_call_budget(chunks.len() as u32)
                };
                let counts = JobCounts {
                    pages: source.page_count(),
                    chunks: chunks.len() as u32,
                    ..JobCounts::default()
                };
                let input_hash = source.input_hash.clone();
                let store = Arc::new(
                    JobStore::create_guarded(&paths, &scope_for_store, generation)
                        .map_err(|error| format!("无法创建翻译任务目录：{error}"))?,
                );
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
                store
                    .update_manifest(|manifest| {
                        manifest.input_hash = input_hash;
                        manifest.target_language = target_for_store;
                        manifest.source_language = source_language_code;
                        manifest.settings_snapshot = settings_json;
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
                Ok((store, counts, call_limit))
            })
            .await
            .map_err(|error| format!("创建翻译任务失败：{error}"))??;
        let budget = Arc::new(BudgetTracker::new(call_limit));

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
            journal: StdMutex::new(JournalBuffer::empty()),
            dispatch_gate,
            plan_lock: StdMutex::new(()),
            plan_cache: StdMutex::new(None),
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
        // 锁内只收集句柄：快照要读磁盘（计划文件、manifest），放在临界区外，
        // 避免任务列表轮询阻塞 start / delete 等同锁路径。
        let handles: Vec<(JobId, Arc<JobHandle>)> = match self.inner.jobs.lock() {
            Ok(jobs) => jobs
                .iter()
                .map(|(job_id, handle)| (job_id.clone(), Arc::clone(handle)))
                .collect(),
            Err(_) => Vec::new(),
        };
        for (job_id, handle) in &handles {
            by_id.insert(job_id.clone(), self.handle_snapshot(handle));
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
        // 目录枚举、manifest 与 MB 级 result.json 读取、v1 缓存回落全是磁盘
        // 阻塞操作，整体挪到阻塞线程，避免长时间占住异步 worker。
        let paths = self.inner.paths.clone();
        let settings = self.settings_snapshot();
        tokio::task::spawn_blocking(move || {
            if let Ok(jobs) = list_jobs(&paths, novel_id) {
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
                    if let Some(result) = read_job_result(&paths, &manifest)
                        && result.status == PublishStatus::Complete
                    {
                        return Ok(Some(result));
                    }
                }
            }
            let Ok((target, _)) = legacy::resolve_target_language(&settings) else {
                return Ok(None);
            };
            let book = crate::translation::legacy::read_latest_book(&paths.data_dir, novel_id)?;
            Ok(book.and_then(|book| {
                crate::translation::legacy::to_partial_result(novel_id, target, &book)
            }))
        })
        .await
        .map_err(|error| format!("读取翻译结果失败：{error}"))?
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
        // 块 ID 只需读一次计划文件（与磁盘态快照 plan_chunk_ids_at 同源）；
        // 全量反序列化每个分块是任务列表轮询下的主要开销，快照不再付出。
        let chunk_ids = plan_ids_from_store(&handle.ctx.store);
        // 事件侧的预览 / 归属 / seq 在锁外读取，避免与运行层发事件形成嵌套等待。
        let preview = handle.ctx.emitter.last_preview();
        let preview_current = handle.ctx.emitter.last_current();
        let seq = handle.ctx.emitter.seq();
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
            current: preview_current.or_else(|| runtime.current.clone()),
            usage: runtime.usage,
            elapsed_ms: handle.started.elapsed().as_millis() as u64,
            preview,
            message: runtime.message.clone(),
            error_code: runtime.error_code.clone(),
            seq,
            constraint_version,
            budget: BudgetSnapshot {
                used: budget.used,
                limit: budget.limit,
                remaining: budget.remaining,
                manual_extra: budget.manual_extra,
            },
            already_target_language: false,
            target_language: handle.ctx.target_language.clone(),
            chunk_ids,
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
        // 超时后仍 transition(Paused) 是有意的保守取舍：在途块随后落盘属于
        // 「停在最近检查点」语义内的可接受窗口（supervisor 收尾时 drive 也会
        // 幂等转 Paused）；不引入 abort 或「收尾中」新状态，避免状态机复杂化。
        // 残留窗口：supervisor 真正退出前 running 仍为 true，delete 仍会被拒。
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
            None => self.rehydrate(job_id).await?,
        };
        self.resume_prepare(&handle)?;
        self.spawn_supervisor(handle);
        Ok(())
    }

    /// 恢复校验与状态重置（不含拉起 supervisor），便于单测覆盖控制位口径。
    fn resume_prepare(&self, handle: &Arc<JobHandle>) -> Result<(), String> {
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
        // 用户显式恢复 = 重置按块的返工额度（自动续跑 / 暂停恢复 / 限流重试
        // 都不清空，避免额度被悄悄刷新）。
        handle.ctx.control.clear_rework();
        self.transition(handle, JobState::Queued, "resumed", None);
        Ok(())
    }

    /// 从磁盘重建一个 Job 的内存上下文（重启后的恢复路径）。
    ///
    /// 预算：manifest 的 `attempts.model_calls / manual_extra` 与 `usage` 一并还原，
    /// 不因重启清零。阶段与块计划：`prepare_source` 不再重新切块（计划仍在磁盘上），
    /// 主 Agent 从 `job_status` 得到待办块，已完成的块直接复用。
    async fn rehydrate(&self, job_id: &str) -> Result<Arc<JobHandle>, String> {
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
        // 最多 8MB 的原文与既有 journal 一起在阻塞线程读回，不占异步 worker。
        let store_for_load = store.clone();
        let rehydrate_novel_id = manifest.novel_id;
        let (input, journal) = tokio::task::spawn_blocking(move || {
            let input = read_input(&store_for_load, rehydrate_novel_id);
            let journal = JournalBuffer::load(&store_for_load);
            (input, journal)
        })
        .await
        .map_err(|error| format!("恢复任务上下文失败：{error}"))?;
        let input = input.ok_or_else(|| "任务缺少原文记录，无法恢复".to_string())?;
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
        //    自动预算旧公式（32 + 16 × 分块数）的任务在恢复时升级到新公式，
        //    避免升级前创建的任务因旧上限过紧而一恢复就耗尽；自定义值不动。
        let chunks = manifest.counts.chunks;
        let legacy_auto = 32u32.saturating_add(16u32.saturating_mul(chunks));
        let limit =
            if settings.translation_call_budget == 0 && manifest.attempts.limit == legacy_auto {
                crate::agent::auto_call_budget(chunks)
            } else {
                manifest.attempts.limit
            };
        if limit != manifest.attempts.limit {
            let _ = store.update_manifest(|manifest| manifest.attempts.limit = limit);
        }
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
            journal: StdMutex::new(journal),
            dispatch_gate,
            plan_lock: StdMutex::new(()),
            plan_cache: StdMutex::new(None),
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
            store
                .update_manifest(|manifest| {
                    manifest.state = JobState::Cancelled;
                    manifest.last_error = Some(SafeError {
                        code: "cancelled".into(),
                        message: "cancelled".into(),
                        step: manifest.phase.as_str().into(),
                    });
                })
                .map_err(|error| error.to_string())?;
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
            // 超时后仍按终态收尾是保守取舍：取消信号已传播到运行层与子任务
            //（drive 的 select、supervise 的排队等待均响应），排队卡死路径已在
            // supervise 中修掉，5 秒超时只剩进程级卡死场景——此时 JoinHandle
            // 已交给 panic 观察者，无 abort 入口，且 abort 在同步段上同样无效。
            // 残留窗口：卡死的 supervisor 可能在 Cancelled 状态下多写少量产物
            //（写层有世代校验兜底），supervisor 退出前 delete 仍会被拒。
            for _ in 0..200 {
                if !handle.running.load(Ordering::SeqCst) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        }
        self.transition(&handle, JobState::Cancelled, "cancelled", Some("cancelled"));
        // 轮询确认 supervisor 已退出才驱逐；超时残留的卡死 supervisor 仍占着
        // 注册表条目，delete 据此继续拒绝（见 evict_terminal_job 的约束说明）。
        self.evict_terminal_job(&handle);
        Ok(())
    }

    /// 删除：Job 根目录及全部后代。
    pub async fn delete(&self, job_id: &str) -> Result<(), String> {
        if let Some(handle) = self.handle(job_id) {
            if handle.running.load(Ordering::SeqCst) {
                return Err("任务正在运行，请先取消或暂停".into());
            }
            handle.ctx.control.cancel();
            let novel_id = handle.ctx.scope.novel_id;
            handle
                .ctx
                .store
                .remove_tree()
                .map_err(|error| error.to_string())?;
            if let Ok(mut jobs) = self.inner.jobs.lock() {
                jobs.remove(job_id);
            }
            self.remove_idle_novel_lock(novel_id);
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
        store.remove_tree().map_err(|error| error.to_string())?;
        self.remove_idle_novel_lock(manifest.novel_id);
        Ok(())
    }

    /// 删除任务后回收该小说的小说锁条目（novel_locks 只增不减会随小说数膨胀）。
    ///
    /// 判据是 `Arc::strong_count`：除注册表自身外还有持有者（该小说仍有排队 /
    /// 运行中的 supervisor 拿着 clone）时保留。清理与 `novel_lock()` 的
    /// `entry()` 在同一把 StdMutex 临界区内互斥，不存在「删掉后旧锁仍被
    /// 并发取走、同小说两把锁失去互斥」的窗口。
    fn remove_idle_novel_lock(&self, novel_id: i64) {
        let mut locks = self
            .inner
            .novel_locks
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(lock) = locks.get(&novel_id)
            && Arc::strong_count(lock) > 1
        {
            return;
        }
        locks.remove(&novel_id);
    }

    /// 终态收尾后把任务移出进程内注册表（只驱逐句柄，不动任务树与小说锁）。
    ///
    /// Completed / Cancelled 且 supervisor 已结束（`running` 复位）时驱逐，
    /// 释放设置快照、模型 Key 明文与存储句柄；任务目录与 manifest 留在磁盘上，
    /// 查询经 `find_manifest` / 目录扫描自然回落磁盘路径。约束：
    ///
    /// - NeedsAttention / Paused / Interrupted 保留内存态：控制位与
    ///   confirm / resume / pause 依赖运行时状态；
    /// - `running` 仍为 true（取消轮询超时的卡死残留）时不驱逐：delete 靠
    ///   注册表条目拒绝「supervisor 未退出先删目录」；
    /// - `Arc::ptr_eq` 保证只驱逐当前注册条目：同 ID 重建（rehydrate）后，
    ///   旧 supervisor 的迟到收尾不会误删新句柄。
    /// - start 生成全新 job_id、rehydrate 拒绝终态，均无「同 ID 覆盖终态条目」
    ///   的路径；`HashMap::insert` 本身按 key 覆盖旧条目，语义一致。
    fn evict_terminal_job(&self, handle: &Arc<JobHandle>) {
        if handle.running.load(Ordering::SeqCst) {
            return;
        }
        let terminal = handle
            .runtime
            .lock()
            .map(|runtime| matches!(runtime.state, JobState::Completed | JobState::Cancelled))
            .unwrap_or(false);
        if !terminal {
            return;
        }
        let job_id = &handle.ctx.scope.job_id;
        if let Ok(mut jobs) = self.inner.jobs.lock()
            && jobs
                .get(job_id)
                .is_some_and(|entry| Arc::ptr_eq(entry, handle))
        {
            jobs.remove(job_id);
        }
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
        let queued = handle.clone();
        let supervisor = tokio::spawn(async move {
            TranslationService { inner }.supervise(queued).await;
        });
        // panic 观察者：supervise panic 时 `running.store(false)` 不会执行，
        // 若无人观察 JoinError，running 位永久为 true——pause / cancel 的轮询
        // 永远超时，resume / delete 永久被拒，只能重启应用。观察者收到
        // JoinError 时复位 running；panic 额外落 interrupted 终态（可恢复）。
        let inner = self.inner.clone();
        tokio::spawn(async move {
            TranslationService { inner }
                .watch_supervisor(supervisor, handle)
                .await;
        });
    }

    /// supervisor 结束观察：正常结束不介入（running 由 supervise 尾部复位）；
    /// JoinError 时复位 running 位，panic 额外落 interrupted 让用户可 resume。
    async fn watch_supervisor(
        &self,
        supervisor: tokio::task::JoinHandle<()>,
        handle: Arc<JobHandle>,
    ) {
        if let Err(error) = supervisor.await {
            handle.running.store(false, Ordering::SeqCst);
            if error.is_panic() {
                log::warn!(
                    "novel={} job={} event=supervisor_panicked state=interrupted",
                    handle.ctx.scope.novel_id,
                    handle.ctx.scope.job_id,
                );
                self.transition(
                    &handle,
                    JobState::Interrupted,
                    "supervisor_panicked",
                    Some("supervisor_panicked"),
                );
            }
        }
    }

    async fn supervise(&self, handle: Arc<JobHandle>) {
        // 同一小说串行：先取小说锁，再占活跃小说槽位（排队中的小说不占活跃名额）。
        // 两段排队等待都必须响应取消：否则排队中的 Job 不受 control.cancel 影响，
        // cancel() 轮询超时强转 Cancelled 后 running 仍为 true，delete / resume
        // 会被「任务正在运行」拒绝，直到前序 Job 释放资源（可能长达数小时）。
        let cancel = handle.ctx.control.cancel.child();
        let novel_lock = self.novel_lock(handle.ctx.scope.novel_id);
        let _novel_guard = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                self.abandon_queue(&handle);
                return;
            }
            guard = novel_lock.lock() => guard,
        };
        let _novel_permit = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                self.abandon_queue(&handle);
                return;
            }
            permit = self
                .inner
                .scheduler
                .enter_novel(handle.ctx.scope.novel_id) => permit,
        };
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
        // supervisor 退出即终态驱逐点：Completed / Cancelled 的句柄在此离开
        // 注册表（transition 已先落盘并 flush 事件，磁盘回落路径自此接管）。
        self.evict_terminal_job(&handle);
    }

    /// 排队期间被取消：退出等待并按终态收尾。
    ///
    /// running 先复位（cancel() 的轮询据此结束），再 transition(Cancelled)
    /// 落盘并广播事件（transition 内部 emit_now + flush）。select 丢弃的是
    /// 尚未取得的锁守卫 / enter_novel 的 future：Scheduler 的 ActiveClaim
    /// 在 Drop 时自行补偿活跃计数，此处无资源泄漏。
    fn abandon_queue(&self, handle: &Arc<JobHandle>) {
        handle.running.store(false, Ordering::SeqCst);
        self.transition(handle, JobState::Cancelled, "cancelled", Some("cancelled"));
        self.evict_terminal_job(handle);
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
        let limits = ctx.coordinator_run_limits();
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
        let mut turn_continuations: u32 = 0;
        // 轮次上限续跑用尽时的实际单轮上限（用于覆盖用户可见原因）。
        let mut turn_limit_gave_up: Option<u32> = None;
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
                    // 主 Agent 是编排者：命中单轮轮次上限时，已完成阶段（扫描 /
                    // 译文 / 审查）都已落盘，整个任务不该在这里失败。带一条
                    // 「定位下一步、不要重复已完成阶段」的指令再拉起，最多
                    // MAX_TURN_CONTINUATIONS 次。子 Agent 不续跑。
                    if let RunError::MaxTurns(max_turns) = &error {
                        // 命中上限前产物可能已经全部就绪（例如最后一步刚发布
                        // 成功），先按磁盘校验，避免为已完成的 Job 再烧一轮。
                        if self.verify_complete(ctx) {
                            break Ok(());
                        }
                        if can_continue_after_max_turns(turn_continuations) {
                            turn_continuations += 1;
                            log_job(
                                &ctx.scope,
                                "main",
                                phase,
                                None,
                                "run_turn_limit",
                                &[
                                    ("runs", &run_index.to_string()),
                                    ("continuation", &turn_continuations.to_string()),
                                    ("max_turns", &max_turns.to_string()),
                                    ("phase", phase.as_str()),
                                ],
                            );
                            // MaxTurns 不携带本轮历史；沿用上一轮的 carried 投影，
                            // 补一条明确下一步的指令，避免模型从头重新扫描或重译。
                            carried.push(Arc::new(rig_core::message::Message::user(
                                continuation_nudge(
                                    ctx,
                                    "上一轮达到单轮轮次上限，已完成产物均已落盘；\
                                     先调用 job_status / read_review_summary 定位下一步，\
                                     不要重新扫描或重译已完成块",
                                    false,
                                ),
                            )));
                            continue;
                        }
                        turn_limit_gave_up = Some(*max_turns);
                        break Err(error);
                    }
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
                    let (stage_code, stage_phase, base) = Self::incomplete_stage(ctx)
                        .unwrap_or_else(|| {
                            (
                                "incomplete",
                                phase,
                                "模型结束运行但完成条件未满足".to_string(),
                            )
                        });
                    // 错误码与 step 都反映真实卡住的阶段（如 rework_exhausted → review）。
                    let (code, message, step_phase) =
                        if empty_runs >= MAX_EMPTY_RUNS && !excerpt.is_empty() {
                            (
                                "model_declined".to_string(),
                                format!("模型未调用工具即结束：{excerpt}"),
                                phase,
                            )
                        } else if excerpt.is_empty() {
                            (stage_code.to_string(), base, stage_phase)
                        } else {
                            (
                                stage_code.to_string(),
                                format!("{base}；模型说明：{excerpt}"),
                                stage_phase,
                            )
                        };
                    let _ = ctx.update_manifest(|manifest| {
                        manifest.phase = step_phase;
                        manifest.last_error = Some(SafeError {
                            code: code.clone(),
                            message: message.clone(),
                            step: step_phase.as_str().into(),
                        });
                    });
                    self.transition(handle, JobState::NeedsAttention, &message, Some(&code));
                }
            }
            Err(error) => {
                let code = error.code().to_string();
                // 续跑用尽后错误码仍是 max_turns，但用户可见原因说明续跑次数；
                // 未触发续跑（如子 Agent 场景不会走到这里）时沿用运行层原文。
                let message = match turn_limit_gave_up {
                    Some(max_turns) => crate::agent::events::safe_text(
                        &turn_limit_exhausted_message(max_turns, MAX_TURN_CONTINUATIONS),
                        200,
                    ),
                    None => crate::agent::events::safe_text(&error.to_string(), 200),
                };
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
    ///
    /// 返回 `(错误码, 真实阶段, 用户可读原因)`；`None` 表示定位失败（保持笼统的
    /// incomplete）。块级判定与 `dispatch_translation` / 续跑提示共用
    /// [`tools::chunk_gate`]，避免「工具认为已完成、收尾却判未完成」。
    fn incomplete_stage(ctx: &JobContext) -> Option<(&'static str, Phase, String)> {
        // 块级定位只需要块 ID：走计划元数据缓存，不逐块读盘反序列化原文。
        let chunk_ids = tools::plan_ids_cached(ctx).ok()?;
        if chunk_ids.is_empty() {
            return Some((
                "incomplete_prepare",
                Phase::PrepareSource,
                "原文尚未完成分块".into(),
            ));
        }
        let versions = tools::phase_versions(ctx);
        let scanned = chunk_ids
            .iter()
            .filter(|chunk_id| {
                tools::read_scan_for_progress_with(ctx, chunk_id, &versions).is_some()
            })
            .count();
        if scanned < chunk_ids.len() {
            return Some((
                "incomplete_scan",
                Phase::PrepareSource,
                format!("还有 {} 个分块未完成扫描", chunk_ids.len() - scanned),
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
            return Some((
                "incomplete_constraints",
                Phase::PrepareConstraints,
                "术语与风格尚未冻结".into(),
            ));
        }
        let mut pending_translate = Vec::new();
        let mut pending_rework = Vec::new();
        let mut pending_review = Vec::new();
        for chunk_id in chunk_ids.iter() {
            let gate = tools::chunk_gate(ctx, chunk_id, &versions);
            if !gate.draft_valid {
                pending_translate.push(chunk_id.clone());
            } else if gate.needs_rework {
                pending_rework.push(chunk_id.clone());
            } else if !gate.review_pass {
                pending_review.push(chunk_id.clone());
            }
        }
        if !pending_translate.is_empty() {
            return Some((
                "incomplete_translate",
                Phase::Translate,
                format!("还有 {} 个分块未翻译", pending_translate.len()),
            ));
        }
        if !pending_rework.is_empty() || !pending_review.is_empty() {
            let limit = tools::rework_limit(ctx);
            let rounds = ctx.control.rework_map();
            let all_exhausted = !pending_rework.is_empty()
                && pending_rework
                    .iter()
                    .all(|id| rounds.get(id).copied().unwrap_or(0) >= limit);
            if all_exhausted {
                return Some((
                    "rework_exhausted",
                    Phase::Review,
                    format!(
                        "仍有 {} 块存在 major/critical 且返工额度已用尽，可手动继续后重试",
                        pending_rework.len()
                    ),
                ));
            }
            return Some((
                "incomplete_review",
                Phase::Review,
                format!(
                    "还有 {} 个分块未通过审查",
                    pending_rework.len() + pending_review.len()
                ),
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
            return Some((
                "incomplete_global_review",
                Phase::GlobalReview,
                "整体复核未完成或未通过".into(),
            ));
        }
        Some((
            "incomplete_publish",
            Phase::Publish,
            "合并或发布未完成".into(),
        ))
    }

    /// 任务推进度的粗粒度快照：直接数磁盘产物（草稿、审查、冻结、发布），
    /// 只用于判断「续跑是否带来进展」，不是精确度量。
    fn progress_snapshot(&self, ctx: &JobContext) -> u64 {
        // 进展快照只需要块 ID：走计划元数据缓存，不逐块读盘反序列化原文。
        let chunk_ids = match tools::plan_ids_cached(ctx) {
            Ok(chunk_ids) => chunk_ids,
            Err(_) => return 0,
        };
        let versions = tools::phase_versions(ctx);
        let mut progress = 0u64;
        for chunk_id in chunk_ids.iter() {
            // 扫描是 Pass 1 的主要工作量，计入进展避免扫描中途的续跑被误判无进展。
            if tools::read_scan_for_progress_with(ctx, chunk_id, &versions).is_some() {
                progress += 1;
            }
            if tools::read_draft_for_verify(ctx, chunk_id)
                .ok()
                .flatten()
                .is_some()
            {
                progress += 4;
            }
            if tools::read_review_for_verify(ctx, chunk_id)
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
            // 草稿结构与语言纯度不在这里单独跑一遍全文校验：紧随其后的
            // validate_merge_with_language 与 publish_status_with_language 内部
            // 都会逐块调用 validate_draft（同一批 chunk/draft、同一目标语言，
            // 纯函数结果一致）。任一块校验失败时，前者以五项不全、后者以
            // 非 Complete 使本判据返回 false——与「先逐块校验再合并」的旧
            // 路径逐位等价，收尾时的全文扫描从三遍降到两遍。
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
        // 先强制 flush journal 缓冲，再让新状态可见（runtime / manifest / 事件
        // 均以新状态为准）：并行观察者一旦看到终态就允许直接读磁盘 journal
        // 回放，缓冲必须在此之前落盘；supervise 尾部的终态驱逐会释放上下文，
        // 同理。flush 在此失败只保留脏标记，不阻断状态机。
        handle.ctx.flush_journal(true);
        let phase = handle
            .ctx
            .read_manifest()
            .map(|manifest| manifest.phase)
            .unwrap_or(Phase::PrepareSource);
        // 收尾计数以磁盘产物为准：内存 counts 只在 refresh_counts 时更新，
        // 子 Agent 直接写产物时容易落后，不能作为 manifest / 事件的真相源。
        let counts = tools::compute_counts(&handle.ctx);
        let (usage, constraint_version) = {
            let runtime = handle
                .runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            (runtime.usage, runtime.constraint_version)
        };
        {
            let mut runtime = handle
                .runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            runtime.state = state;
            runtime.phase = phase;
            runtime.counts = counts;
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
        // 只记录状态 / 阶段 / 计数 / 错误码，不含原文、译文与预览正文。
        let translated = counts.translated.to_string();
        let reviewed = counts.reviewed.to_string();
        let needs_repair = counts.needs_repair.to_string();
        log_job(
            &handle.ctx.scope,
            "service",
            phase,
            None,
            "job_transition",
            &[
                ("state", state.as_str()),
                ("phase", phase.as_str()),
                ("translated", &translated),
                ("reviewed", &reviewed),
                ("needs_repair", &needs_repair),
                ("code", error_code.unwrap_or("")),
            ],
        );
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
        // `reviewed` 只统计结论为 pass 的块；pass 携带的 minor 建议只展示，
        // 不计入未解决。
        if review.verdict == ReviewVerdict::Pass {
            reviewed = reviewed.saturating_add(1);
        }
        if review.verdict == ReviewVerdict::NeedsFix
            || review.issues.iter().any(|issue| issue.is_blocking())
        {
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

/// 主 Agent 达到单轮轮次上限、自动续跑用尽后的用户可见原因。
/// 错误码仍是 `max_turns`，这里只补足「已续跑几次、产物仍可继续」。
fn turn_limit_exhausted_message(max_turns: u32, continuations: u32) -> String {
    format!(
        "已自动续跑 {continuations} 次仍达到单轮轮次上限（{max_turns} 轮）；\
         已完成块的译文与审查均已落盘，可手动继续任务"
    )
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
         下面的待办清单直接取自磁盘产物，与 job_status 的待办字段一致。"
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
            "下一步：调用 prepare_source() 建立分块计划；返回数量为 0 时上报需要人工处理。".into(),
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
    } else if !pending.pending_rework.is_empty() {
        let limit = tools::rework_limit(ctx);
        let rounds = ctx.control.rework_map();
        let exhausted: Vec<String> = pending
            .pending_rework
            .iter()
            .filter(|id| rounds.get(*id).copied().unwrap_or(0) >= limit)
            .cloned()
            .collect();
        lines.push(format!(
            "下一步：块 {} 存在 major/critical 审查问题，需要返工。优先调用 \
             revise_draft(chunk_id, patches=[{{\"segment_id\":\"...\",\"text\":\"...\"}}]) \
             修复审查点名的片段（不消耗返工额度），随后 dispatch_review([chunk_id]) 复检；\
             只有整块结构损坏或大面积漏翻时，才调用 dispatch_translation([chunk_id]) 整块重译。",
            brief(&pending.pending_rework)
        ));
        if !exhausted.is_empty() {
            lines.push(format!(
                "注意：块 {} 的整块重译返工额度已用尽，dispatch_translation 会直接返回 \
                 rework_limit；不要重复调用以免空转。若 revise_draft 无法修复，如实上报\
                 需要用户手动继续任务后重试。",
                brief(&exhausted)
            ));
        }
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

/// 单测用 Job 上下文装配（不启动 supervisor、不触网）。
#[cfg(test)]
pub(crate) fn test_job_context(
    paths: &AppPaths,
    settings: &Settings,
    scope: &JobScope,
    store: Arc<JobStore>,
) -> Arc<JobContext> {
    let scheduler = Arc::new(Scheduler::new(&crate::agent::RuntimeConfig::from_settings(
        settings,
    )));
    let emitter = Arc::new(JobEmitter::new(Arc::new(NoopSink), scope.clone()));
    let model = ModelConfig {
        format: "chat_completions".into(),
        base_url: settings.translation_api_url.clone(),
        model: "mock".into(),
        key: "test-key".into(),
        extra_json: json!({}),
        timeout_seconds: 60,
        session_seed: format!("pixiv-tool-test-{}", scope.job_id),
    };
    Arc::new(JobContext {
        scope: scope.clone(),
        store: store.clone(),
        paths: paths.clone(),
        settings: settings.clone(),
        target_language: "zh-CN".into(),
        target_language_name: "简体中文".into(),
        source_language: "ja".into(),
        budget: Arc::new(BudgetTracker::new(64)),
        budget_config: BudgetConfig::from_settings(settings),
        scheduler,
        model,
        emitter,
        control: Control::default(),
        confirm_before_start: false,
        manifest_lock: StdMutex::new(()),
        journal: StdMutex::new(JournalBuffer::load(&store)),
        dispatch_gate: Arc::new(Semaphore::new(2)),
        plan_lock: StdMutex::new(()),
        plan_cache: StdMutex::new(None),
        usage: StdMutex::new(TokenUsage::default()),
        unknown_blocked: StdMutex::new(BTreeSet::new()),
        io: StoreIo::new(store),
    })
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

    /// 单测 Job 夹具：真实 JobStore + 测试上下文 + 已注册的 handle。
    fn job_fixture(
        tag: &str,
        settings: Settings,
    ) -> (
        TranslationService,
        Arc<JobHandle>,
        JobScope,
        std::path::PathBuf,
    ) {
        let (paths, dir) = fixture(tag);
        let generation = CacheGeneration::new(0);
        let service = TranslationService::for_tests_with_generation(
            paths.clone(),
            settings.clone(),
            Arc::new(NoopSink),
            Some("k".into()),
            generation.clone(),
        );
        let scope = JobScope {
            novel_id: 1,
            job_id: format!("20260101000000-{tag}"),
            generation: generation.get(),
        };
        let store = Arc::new(JobStore::create_guarded(&paths, &scope, generation).unwrap());
        store
            .write_atomic(
                "source/input.json",
                &serde_json::to_vec(&json!({
                    "title": "t",
                    "tags": [],
                    "description": "",
                    "content": "本文",
                }))
                .unwrap(),
            )
            .unwrap();
        let ctx = test_job_context(&paths, &settings, &scope, store);
        let handle = Arc::new(JobHandle {
            ctx,
            started: Instant::now(),
            runtime: StdMutex::new(JobRuntime {
                state: JobState::Paused,
                phase: Phase::Translate,
                counts: JobCounts::default(),
                current: None,
                usage: TokenUsage::default(),
                message: "paused".into(),
                error_code: None,
                constraint_version: 0,
            }),
            running: AtomicBool::new(false),
        });
        if let Ok(mut jobs) = service.inner.jobs.lock() {
            jobs.insert(scope.job_id.clone(), handle.clone());
        }
        (service, handle, scope, dir)
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
    fn control_rework_counter_is_per_chunk_and_clearable() {
        let control = Control::default();
        assert_eq!(control.rework_rounds("c0001"), 0);
        assert_eq!(control.rework_map().len(), 0, "初始无任何块计数");

        assert_eq!(control.bump_rework("c0001"), 1);
        assert_eq!(control.bump_rework("c0001"), 2);
        assert_eq!(control.bump_rework("c0002"), 1);
        assert_eq!(control.rework_rounds("c0001"), 2);
        assert_eq!(control.rework_rounds("c0002"), 1);
        assert_eq!(control.rework_rounds("c0003"), 0, "未出现的块按 0 计");

        assert_eq!(control.max_rework_rounds(), 2, "兼容字段取最大值");
        assert_eq!(
            control.rework_map(),
            BTreeMap::from([("c0001".to_string(), 2), ("c0002".to_string(), 1)])
        );

        control.clear_rework();
        assert_eq!(control.rework_rounds("c0001"), 0);
        assert_eq!(control.max_rework_rounds(), 0);
        assert!(control.rework_map().is_empty());
    }

    #[test]
    fn control_try_reserve_rework_stops_at_limit_per_chunk() {
        let control = Control::default();
        assert!(control.try_reserve_rework("c0001", 2));
        assert!(control.try_reserve_rework("c0001", 2));
        assert!(!control.try_reserve_rework("c0001", 2), "额度用尽");
        assert_eq!(control.rework_rounds("c0001"), 2, "拒绝时不记账");
        // 另一块不受影响：块级隔离。
        assert!(control.try_reserve_rework("c0002", 2));
        assert_eq!(control.rework_rounds("c0002"), 1);
    }

    #[test]
    fn control_rework_counter_and_reset_notes_are_recorded() {
        let control = Control::default();
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

    /// 夹具里的两块：`c0001` needs_fix（待返工），`c0002` 已通过审查。
    fn seed_rework_job(handle: &Arc<JobHandle>) {
        let pass_draft = Draft {
            chunk_id: "c0002".into(),
            version: 1,
            segments: vec![crate::translation::artifacts::TranslatedSegment {
                id: "p002-l000-s0".into(),
                text: "再见".into(),
            }],
            usage: crate::translation::artifacts::TokenUsage::default(),
            reviewed: true,
            needs_repair: false,
        };
        let rework_draft = Draft {
            chunk_id: "c0001".into(),
            version: 1,
            segments: vec![crate::translation::artifacts::TranslatedSegment {
                id: "p001-l000-s0".into(),
                text: "你好".into(),
            }],
            usage: crate::translation::artifacts::TokenUsage::default(),
            reviewed: true,
            needs_repair: false,
        };
        let chunk = |id: &str, order: u32, segment: &str, text: &str| Chunk {
            id: id.into(),
            order,
            segments: vec![crate::translation::source::SourceSegment {
                id: segment.into(),
                text: text.into(),
                kind: crate::translation::source::SegmentKind::Text,
            }],
            prev_context: String::new(),
            next_context: String::new(),
            chars: text.chars().count() as u32,
        };
        let review = |id: &str, verdict, issues| Review {
            chunk_id: id.into(),
            draft_version: 1,
            verdict,
            issues,
            notes: String::new(),
        };
        tools::seed_for_tests(
            &handle.ctx,
            &[
                (
                    chunk("c0001", 1, "p001-l000-s0", "こんにちは"),
                    Some(rework_draft),
                    Some(review(
                        "c0001",
                        ReviewVerdict::NeedsFix,
                        vec![crate::translation::artifacts::Issue {
                            segment_id: "p001-l000-s0".into(),
                            kind: "mistranslation".into(),
                            severity: Severity::Major,
                            detail: "人称错误".into(),
                        }],
                    )),
                ),
                (
                    chunk("c0002", 2, "p002-l000-s0", "さようなら"),
                    Some(pass_draft),
                    Some(review("c0002", ReviewVerdict::Pass, Vec::new())),
                ),
            ],
        );
    }

    #[tokio::test]
    async fn resume_clears_per_chunk_rework_quota() {
        let (service, handle, _scope, dir) = job_fixture("resume-rework", settings());
        handle.ctx.control.bump_rework("c0001");
        handle.ctx.control.bump_rework("c0001");
        handle.ctx.block_unknown("c0002");
        assert_eq!(handle.ctx.control.rework_rounds("c0001"), 2);

        service.resume_prepare(&handle).expect("可恢复");

        assert!(
            handle.ctx.control.rework_map().is_empty(),
            "手动恢复清空按块返工计数"
        );
        assert!(
            !handle.ctx.is_unknown_blocked("c0002"),
            "未知结果标记一并清空"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn transition_writes_fresh_counts_into_manifest() {
        let (service, handle, _scope, dir) = job_fixture("transition-counts", settings());
        seed_rework_job(&handle);
        // 内存 counts 刻意留空，模拟「子 Agent 直接写产物、内存计数落后」。
        assert!(handle.runtime.lock().unwrap().counts.is_zero());

        service.transition(&handle, JobState::Paused, "paused", None);

        let runtime_counts = handle.runtime.lock().unwrap().counts;
        assert_eq!(runtime_counts.chunks, 2, "收尾按磁盘产物重算");
        assert_eq!(runtime_counts.translated, 2, "两块都有有效草稿");
        assert_eq!(runtime_counts.reviewed, 1, "只有 pass 计入已审");
        assert_eq!(runtime_counts.needs_repair, 1, "needs_fix 计入待修");

        let manifest = handle.ctx.read_manifest().expect("读 manifest");
        assert_eq!(manifest.counts, runtime_counts, "manifest 与内存同一口径");
        assert_eq!(manifest.state, JobState::Paused);
        // 块计数不清零：计划存在时按计划数；这里两块都已落盘。
        assert_ne!(manifest.counts.chunks, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// journal 缓冲：append 只进内存（间隔内不落盘），强制 flush 后文件内容
    /// 与内存缓冲逐行一致，且重复 flush 不产生重复行。
    #[test]
    fn journal_append_buffers_in_memory_and_flushes_on_demand() {
        let (_service, handle, _scope, dir) = job_fixture("journal-buffer", settings());
        let ctx = &handle.ctx;
        ctx.append_session(&json!({ "kind": "a", "n": 1 }));
        ctx.append_session(&json!({ "kind": "b", "n": 2 }));
        assert!(
            !ctx.store.exists(JOURNAL_PATH),
            "落盘间隔内不应产生整文件重写"
        );

        ctx.flush_journal(true);
        let read_lines = || {
            let bytes = ctx
                .store
                .read(JOURNAL_PATH)
                .expect("读取 journal")
                .expect("flush 后 journal 应存在");
            String::from_utf8(bytes)
                .expect("journal 是 UTF-8")
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        let flushed = read_lines();
        assert_eq!(flushed.len(), 2, "flush 后文件与内存行数一致");
        assert!(flushed[0].contains("\"kind\":\"a\""));
        assert!(flushed[1].contains("\"kind\":\"b\""));
        let buffered = ctx.journal.lock().unwrap();
        assert_eq!(
            flushed,
            buffered
                .lines
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            "文件内容与内存缓冲逐行一致"
        );
        drop(buffered);

        // 重复强制 flush 幂等：脏标记已清，不重写也不重复。
        ctx.flush_journal(true);
        assert_eq!(read_lines().len(), 2, "重复 flush 不产生重复行");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// journal 超限裁剪：一次裁掉一半旧行，内存与 flush 后的文件都只保留
    /// 最新的 JOURNAL_KEEP_LINES 条。
    #[test]
    fn journal_trims_oldest_half_when_exceeding_limit() {
        let (_service, handle, _scope, dir) = job_fixture("journal-trim", settings());
        let ctx = &handle.ctx;
        let total = JOURNAL_MAX_LINES + 8;
        for index in 0..total {
            ctx.append_session(&json!({ "kind": "tick", "i": index }));
        }

        {
            let buffer = ctx.journal.lock().unwrap();
            assert!(
                buffer.lines.len() >= JOURNAL_KEEP_LINES && buffer.lines.len() <= JOURNAL_MAX_LINES,
                "内存行数应在保留窗口与上限之间（裁半按触发时机滞后收敛）"
            );
            let first = &buffer.lines[0];
            let last = buffer.lines.last().expect("缓冲非空");
            assert!(
                first.contains(&format!("\"i\":{}", JOURNAL_KEEP_LINES + 1)),
                "最旧保留行应是首次裁剪后的第 {} 条：{first}",
                JOURNAL_KEEP_LINES + 1
            );
            assert!(
                last.contains(&format!("\"i\":{}", total - 1)),
                "最新行应保留：{last}"
            );
            let expected = buffer.lines.clone();
            drop(buffer);

            ctx.flush_journal(true);
            let bytes = ctx
                .store
                .read(JOURNAL_PATH)
                .expect("读取 journal")
                .expect("flush 后 journal 应存在");
            let flushed: Vec<String> = String::from_utf8(bytes)
                .expect("UTF-8")
                .lines()
                .map(str::to_string)
                .collect();
            assert_eq!(
                flushed,
                expected.iter().map(String::as_str).collect::<Vec<_>>(),
                "flush 后文件内容与内存缓冲一致"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 终态 transition 强制 flush：间隔内的缓冲记录在转终态后必须已在磁盘上
    /// （e2e 等终态后直接读文件的断言依赖该语义）。
    #[test]
    fn terminal_transition_flushes_buffered_journal() {
        let (service, handle, _scope, dir) = job_fixture("journal-flush", settings());
        handle
            .ctx
            .append_session(&json!({ "kind": "before_terminal", "n": 1 }));
        assert!(!handle.ctx.store.exists(JOURNAL_PATH), "间隔内尚未落盘");

        service.transition(&handle, JobState::Completed, "completed", None);

        let bytes = handle
            .ctx
            .store
            .read(JOURNAL_PATH)
            .expect("读取 journal")
            .expect("终态 transition 必须把缓冲落盘");
        let text = String::from_utf8(bytes).expect("UTF-8");
        assert!(
            text.contains("\"kind\":\"before_terminal\""),
            "缓冲中的记录应已写入文件：{text}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 终态驱逐：Completed 的 Job 在 supervisor 退出序列（transition + evict）后
    /// 离开进程内注册表，但任务列表与快照仍经磁盘路径可见、内容一致。
    #[tokio::test]
    async fn completed_job_leaves_registry_but_stays_queryable() {
        let (service, handle, scope, dir) = job_fixture("evict-completed", settings());
        seed_rework_job(&handle);
        assert!(service.handle(&scope.job_id).is_some(), "夹具默认已注册");

        // 与 supervise 尾部相同的退出序列：transition 先落盘 + flush 事件，再驱逐。
        service.transition(&handle, JobState::Completed, "completed", None);
        service.evict_terminal_job(&handle);

        assert!(
            service.handle(&scope.job_id).is_none(),
            "终态 Job 必须离开注册表"
        );
        // 磁盘回落：快照仍可读，状态与块 ID 来自 manifest / 计划文件。
        let snapshot = service.snapshot(&scope.job_id).await.expect("快照可读");
        assert_eq!(snapshot.state, JobState::Completed);
        assert_eq!(
            snapshot.chunk_ids,
            vec!["c0001".to_string(), "c0002".to_string()]
        );
        let list = service.jobs().await;
        let listed = list
            .jobs
            .iter()
            .find(|job| job.job_id == scope.job_id)
            .expect("任务列表仍应包含已完成 Job");
        assert_eq!(listed.state, JobState::Completed);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 取消同理：驱逐后快照 / 列表 / 再取消都走磁盘路径且语义不变。
    #[tokio::test]
    async fn cancelled_job_leaves_registry_and_falls_back_to_disk() {
        let (service, handle, scope, dir) = job_fixture("evict-cancelled", settings());
        service.transition(&handle, JobState::Cancelled, "cancelled", Some("cancelled"));
        service.evict_terminal_job(&handle);

        assert!(service.handle(&scope.job_id).is_none(), "取消后离开注册表");
        let snapshot = service.snapshot(&scope.job_id).await.expect("快照可读");
        assert_eq!(snapshot.state, JobState::Cancelled);
        assert_eq!(snapshot.error_code.as_deref(), Some("cancelled"));
        // 再取消：磁盘路径对已取消任务幂等返回 Ok，不报「任务不存在」。
        service.cancel(&scope.job_id).await.expect("重复取消幂等");
        let list = service.jobs().await;
        assert!(
            list.jobs.iter().any(|job| job.job_id == scope.job_id),
            "任务列表仍能看到已取消 Job"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 负例：NeedsAttention / Paused / Interrupted 保留内存态，驱逐调用不生效。
    #[tokio::test]
    async fn resumable_states_stay_in_registry() {
        let (service, handle, scope, dir) = job_fixture("evict-negative", settings());
        seed_rework_job(&handle);

        service.transition(&handle, JobState::Paused, "paused", None);
        service.evict_terminal_job(&handle);
        assert!(service.handle(&scope.job_id).is_some(), "Paused 不驱逐");

        service.transition(
            &handle,
            JobState::NeedsAttention,
            "blocked",
            Some("blocked"),
        );
        service.evict_terminal_job(&handle);
        assert!(
            service.handle(&scope.job_id).is_some(),
            "NeedsAttention 不驱逐"
        );

        service.transition(
            &handle,
            JobState::Interrupted,
            "interrupted",
            Some("interrupted"),
        );
        service.evict_terminal_job(&handle);
        assert!(
            service.handle(&scope.job_id).is_some(),
            "Interrupted 不驱逐"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 轻量 chunk_ids 路径：内存态快照与驱逐后的磁盘态快照同源（计划文件），
    /// 顺序与分块计划一致。
    #[tokio::test]
    async fn snapshot_chunk_ids_match_plan_order_across_eviction() {
        let (service, handle, scope, dir) = job_fixture("chunk-ids", settings());
        seed_rework_job(&handle);

        let memory = service.snapshot(&scope.job_id).await.expect("内存快照");
        assert_eq!(
            memory.chunk_ids,
            vec!["c0001".to_string(), "c0002".to_string()],
            "内存态快照按计划顺序给出块 ID"
        );

        service.transition(&handle, JobState::Completed, "completed", None);
        service.evict_terminal_job(&handle);
        let disk = service.snapshot(&scope.job_id).await.expect("磁盘快照");
        assert_eq!(
            disk.chunk_ids, memory.chunk_ids,
            "驱逐前后 chunk_ids 同源一致"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn incomplete_stage_reports_rework_exhausted_when_quota_is_used_up() {
        let mut s = settings();
        s.translation_agent_rework_rounds = 0;
        let (_service, handle, _scope, dir) = job_fixture("incomplete-rw", s);
        seed_rework_job(&handle);
        handle.ctx.with_artifacts(|artifacts| {
            artifacts
                .write_constraints(&Constraints {
                    version: 1,
                    terms: Vec::new(),
                    style: String::new(),
                    conflicts: Vec::new(),
                    unresolved: Vec::new(),
                    frozen: true,
                })
                .unwrap();
        });

        // 额度 0：待返工块的额度天然已用尽。
        let (code, phase, message) =
            TranslationService::incomplete_stage(&handle.ctx).expect("可定位阶段");
        assert_eq!(code, "rework_exhausted");
        assert_eq!(phase, Phase::Review, "step 反映真实阶段");
        assert!(message.contains("返工额度已用尽"), "得到 {message}");

        let _ = std::fs::remove_dir_all(dir);
    }

    /// 真实链路：审查工具写入 `needs_fix` 后，草稿标记 `needs_repair=true`，
    /// 该块必须落在 `pending_rework`（而不是被当成未翻译），额度耗尽时收尾
    /// 判据返回 `rework_exhausted`。
    #[tokio::test]
    async fn needs_fix_from_review_tool_lands_in_pending_rework() {
        let mut s = settings();
        s.translation_agent_rework_rounds = 0;
        let (_service, handle, _scope, dir) = job_fixture("real-rework", s);
        // 审查前的正常状态：草稿已提交、尚未审查。
        tools::seed_for_tests(
            &handle.ctx,
            &[(
                Chunk {
                    id: "c0001".into(),
                    order: 1,
                    segments: vec![crate::translation::source::SourceSegment {
                        id: "p001-l000-s0".into(),
                        text: "こんにちは".into(),
                        kind: crate::translation::source::SegmentKind::Text,
                    }],
                    prev_context: String::new(),
                    next_context: String::new(),
                    chars: 5,
                },
                Some(Draft {
                    chunk_id: "c0001".into(),
                    version: 1,
                    segments: vec![crate::translation::artifacts::TranslatedSegment {
                        id: "p001-l000-s0".into(),
                        text: "你好".into(),
                    }],
                    usage: crate::translation::artifacts::TokenUsage::default(),
                    reviewed: false,
                    needs_repair: false,
                }),
                None,
            )],
        );
        handle.ctx.with_artifacts(|artifacts| {
            artifacts
                .write_constraints(&Constraints {
                    version: 1,
                    terms: Vec::new(),
                    style: String::new(),
                    conflicts: Vec::new(),
                    unresolved: Vec::new(),
                    frozen: true,
                })
                .unwrap();
        });

        // 走审查子 Agent 的工具提交结论：needs_fix + major。
        let submit_review = tools::reviewer_tools(&handle.ctx, "c0001")
            .into_iter()
            .find(|tool| tool.name().as_str() == "submit_review")
            .expect("审查工具存在");
        submit_review
            .run(json!({
                "chunk_id": "c0001",
                "verdict": "needs_fix",
                "issues": [{
                    "segment_id": "p001-l000-s0",
                    "kind": "mistranslation",
                    "severity": "major",
                    "detail": "人称错误",
                }],
                "notes": "",
            }))
            .await
            .expect("提交审查结论");

        let draft = tools::read_draft_for_verify(&handle.ctx, "c0001")
            .expect("读草稿")
            .expect("草稿存在");
        assert!(draft.needs_repair, "审查结论同步到草稿标记");

        let pending = tools::pending_state(&handle.ctx);
        assert_eq!(pending.pending_rework, ["c0001"], "待修块进入返工队列");
        assert!(
            pending.pending_translate.is_empty(),
            "needs_repair 不应让草稿失效：{:?}",
            pending.pending_translate
        );
        assert!(pending.pending_review.is_empty());

        let counts = tools::compute_counts(&handle.ctx);
        assert_eq!(counts.translated, 1, "待修块仍计入已译");
        assert_eq!(counts.needs_repair, 1, "待修单列");
        assert_eq!(counts.reviewed, 0, "needs_fix 不计入已审");

        let (code, phase, message) =
            TranslationService::incomplete_stage(&handle.ctx).expect("可定位阶段");
        assert_eq!(code, "rework_exhausted");
        assert_eq!(phase, Phase::Review, "step 反映真实阶段");
        assert!(message.contains("返工额度已用尽"), "得到 {message}");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn incomplete_stage_prefers_incomplete_review_while_quota_remains() {
        let mut s = settings();
        s.translation_agent_rework_rounds = 2;
        let (_service, handle, _scope, dir) = job_fixture("incomplete-review", s);
        seed_rework_job(&handle);
        handle.ctx.with_artifacts(|artifacts| {
            artifacts
                .write_constraints(&Constraints {
                    version: 1,
                    terms: Vec::new(),
                    style: String::new(),
                    conflicts: Vec::new(),
                    unresolved: Vec::new(),
                    frozen: true,
                })
                .unwrap();
        });

        let (code, phase, message) =
            TranslationService::incomplete_stage(&handle.ctx).expect("可定位阶段");
        assert_eq!(code, "incomplete_review");
        assert_eq!(phase, Phase::Review);
        assert!(message.contains("未通过审查"), "得到 {message}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn continuation_nudge_points_rework_at_revise_draft_first() {
        let mut s = settings();
        s.translation_agent_rework_rounds = 2;
        let (_service, handle, _scope, dir) = job_fixture("nudge-rework", s);
        seed_rework_job(&handle);
        handle.ctx.with_artifacts(|artifacts| {
            artifacts
                .write_constraints(&Constraints {
                    version: 1,
                    terms: Vec::new(),
                    style: String::new(),
                    conflicts: Vec::new(),
                    unresolved: Vec::new(),
                    frozen: true,
                })
                .unwrap();
        });

        let nudge = continuation_nudge(&handle.ctx, "上一轮没有推进", false);
        assert!(nudge.contains("revise_draft"), "得到 {nudge}");
        assert!(nudge.contains("dispatch_review"), "返工后必须复检");
        assert!(nudge.contains("c0001"), "列出待返工块");
        assert!(
            !nudge.contains("额度已用尽"),
            "额度未用尽时不提返工上限：{nudge}"
        );
        assert!(nudge.contains("本轮回复必须包含工具调用"), "既有约束保留");

        // 额度用尽后追加「不要重复调用」的引导。
        handle.ctx.control.bump_rework("c0001");
        handle.ctx.control.bump_rework("c0001");
        let nudge = continuation_nudge(&handle.ctx, "上一轮没有推进", false);
        assert!(nudge.contains("额度已用尽"), "得到 {nudge}");
        assert!(nudge.contains("不要重复调用"), "避免模型空转");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn coordinator_run_limits_raise_max_turns_floor_only_for_main_agent() {
        // 默认设置：子 Agent 仍是 24 轮，主 Agent 至少 48 轮。
        let (_service, handle, _scope, dir) = job_fixture("limits-default", settings());
        let sub = handle.ctx.run_limits();
        assert_eq!(sub.max_turns, 24, "子 Agent 沿用设置值");
        let coordinator = handle.ctx.coordinator_run_limits();
        assert_eq!(coordinator.max_turns, 48, "主 Agent 下限抬到 48");
        assert_eq!(
            coordinator.max_no_progress, sub.max_no_progress,
            "无进展上限不变"
        );
        assert_eq!(
            coordinator.max_tool_corrections, sub.max_tool_corrections,
            "工具纠正上限不变"
        );
        let _ = std::fs::remove_dir_all(dir);

        // 设置值低于下限：子 Agent 仍用设置值（4），主 Agent 抬到 48。
        let mut low = settings();
        low.translation_agent_max_turns = 4;
        let (_service, handle, _scope, dir) = job_fixture("limits-low", low);
        assert_eq!(handle.ctx.run_limits().max_turns, 4);
        assert_eq!(handle.ctx.coordinator_run_limits().max_turns, 48);
        let _ = std::fs::remove_dir_all(dir);

        // 设置值高于下限：两者都遵循设置值，且上限仍 clamp 到 128。
        let mut high = settings();
        high.translation_agent_max_turns = 64;
        let (_service, handle, _scope, dir) = job_fixture("limits-high", high);
        assert_eq!(handle.ctx.run_limits().max_turns, 64);
        assert_eq!(handle.ctx.coordinator_run_limits().max_turns, 64);
        let _ = std::fs::remove_dir_all(dir);

        let mut over = settings();
        over.translation_agent_max_turns = 200;
        let (_service, handle, _scope, dir) = job_fixture("limits-over", over);
        assert_eq!(handle.ctx.run_limits().max_turns, 128, "上限统一 clamp");
        assert_eq!(handle.ctx.coordinator_run_limits().max_turns, 128);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn max_turns_continuation_gate_and_error_mapping() {
        // 门禁：最多续跑 MAX_TURN_CONTINUATIONS 次。
        assert_eq!(MAX_TURN_CONTINUATIONS, 2);
        assert!(can_continue_after_max_turns(0));
        assert!(can_continue_after_max_turns(1));
        assert!(!can_continue_after_max_turns(2), "第 3 次不再续跑");

        // 错误映射：错误码保持 max_turns，消息说明续跑次数与可继续状态。
        let error = RunError::MaxTurns(48);
        assert_eq!(error.code(), "max_turns");
        let message = turn_limit_exhausted_message(48, MAX_TURN_CONTINUATIONS);
        assert!(message.contains("已自动续跑 2 次"), "得到 {message}");
        assert!(message.contains("48 轮"), "带出实际单轮上限：{message}");
        assert!(message.contains("落盘"), "说明产物未丢失：{message}");
        // 用户可见原因经过 safe_text 后关键信息不被截断。
        let safe = crate::agent::events::safe_text(&message, 200);
        assert!(safe.contains("已自动续跑 2 次"), "得到 {safe}");
    }

    #[test]
    fn issues_view_counts_only_pass_as_reviewed_and_ignores_pass_minor() {
        let (service, handle, _scope, dir) = job_fixture("issues-view", settings());
        // c0001: pass + minor（应计入 reviewed，但不计入 unresolved）
        // c0002: needs_fix + major（计入 unresolved）
        let draft = |id: &str, segment: &str, text: &str| Draft {
            chunk_id: id.into(),
            version: 1,
            segments: vec![crate::translation::artifacts::TranslatedSegment {
                id: segment.into(),
                text: text.into(),
            }],
            usage: crate::translation::artifacts::TokenUsage::default(),
            reviewed: true,
            needs_repair: false,
        };
        let chunk = |id: &str, order: u32, segment: &str, text: &str| Chunk {
            id: id.into(),
            order,
            segments: vec![crate::translation::source::SourceSegment {
                id: segment.into(),
                text: text.into(),
                kind: crate::translation::source::SegmentKind::Text,
            }],
            prev_context: String::new(),
            next_context: String::new(),
            chars: text.chars().count() as u32,
        };
        tools::seed_for_tests(
            &handle.ctx,
            &[
                (
                    chunk("c0001", 1, "p001-l000-s0", "おはよう"),
                    Some(draft("c0001", "p001-l000-s0", "早上好")),
                    Some(Review {
                        chunk_id: "c0001".into(),
                        draft_version: 1,
                        verdict: ReviewVerdict::Pass,
                        issues: vec![crate::translation::artifacts::Issue {
                            segment_id: "p001-l000-s0".into(),
                            kind: "style".into(),
                            severity: Severity::Minor,
                            detail: "可更口语".into(),
                        }],
                        notes: String::new(),
                    }),
                ),
                (
                    chunk("c0002", 2, "p002-l000-s0", "おやすみ"),
                    Some(draft("c0002", "p002-l000-s0", "晚安")),
                    Some(Review {
                        chunk_id: "c0002".into(),
                        draft_version: 1,
                        verdict: ReviewVerdict::NeedsFix,
                        issues: vec![crate::translation::artifacts::Issue {
                            segment_id: "p002-l000-s0".into(),
                            kind: "mistranslation".into(),
                            severity: Severity::Major,
                            detail: "语境错误".into(),
                        }],
                        notes: String::new(),
                    }),
                ),
            ],
        );

        let view = service
            .inner
            .jobs
            .lock()
            .unwrap()
            .get("20260101000000-issues-view")
            .map(|handle| {
                let plan_ids = plan_ids_from_store(&handle.ctx.store);
                handle
                    .ctx
                    .with_artifacts(|artifacts| issues_view(artifacts.io(), &plan_ids))
                    .unwrap()
            })
            .expect("夹具已注册");

        assert_eq!(view.reviewed, 1, "只有 pass 计入已审");
        assert_eq!(view.total, 2);
        assert_eq!(
            view.unresolved_chunks,
            vec!["c0002".to_string()],
            "pass + minor 不算未解决"
        );
        assert_eq!(view.issues.len(), 2, "minor 仍返回给前端展示");
        assert_eq!(view.affected_chunks, ["c0001", "c0002"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    // ------------------------------------------------------------------
    // 生命周期防护：排队取消 / panic 复位 / 锁条目回收
    // ------------------------------------------------------------------

    /// 排队中的 Job（挂在同一小说的小说锁上）必须能被取消：
    /// cancel 返回后 running 复位、状态落 Cancelled、立即可 delete。
    /// 修复前 cancel() 轮询 5 秒强转 Cancelled，但 running 仍为 true，
    /// delete / resume 被「任务正在运行」拒绝直到前序 Job 释放锁。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queued_job_cancel_exits_novel_lock_and_allows_delete() {
        let (service, handle, scope, dir) = job_fixture("queued-cancel-lock", settings());
        let novel_lock = service.novel_lock(scope.novel_id);
        let _guard = novel_lock.lock().await; // 模拟前序 Job 占住小说锁
        handle.running.store(true, Ordering::SeqCst);
        {
            let mut runtime = handle.runtime.lock().unwrap();
            runtime.state = JobState::Queued;
            runtime.message = "queued".into();
        }
        let svc = service.clone();
        let queued = handle.clone();
        tokio::spawn(async move {
            svc.supervise(queued).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            handle.running.load(Ordering::SeqCst),
            "排队中的任务 running 应为 true"
        );

        let started = std::time::Instant::now();
        service.cancel(&scope.job_id).await.expect("取消应成功");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "排队取消应秒级返回，实际 {:?}",
            started.elapsed()
        );
        assert!(
            !handle.running.load(Ordering::SeqCst),
            "取消后 running 必须复位"
        );
        assert_eq!(
            handle.runtime.lock().unwrap().state,
            JobState::Cancelled,
            "状态应落 Cancelled 终态"
        );
        assert_eq!(
            handle.ctx.store.read_manifest().unwrap().state,
            JobState::Cancelled,
            "manifest 应落 Cancelled"
        );
        service
            .delete(&scope.job_id)
            .await
            .expect("取消后应立即可删除，不再被「任务正在运行」拒绝");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 排队在活跃小说槽位上的 Job 同样必须能被取消（supervise 的第二段等待）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn queued_job_cancel_exits_active_novel_wait() {
        let mut base = settings();
        base.translation_agent_active_novels = 1;
        let (service, handle, scope, dir) = job_fixture("queued-cancel-slot", base);
        // 占满唯一的活跃小说槽位（用别的小说 id，不碰小说锁）。
        let _permit = service.inner.scheduler.enter_novel(999_999).await;
        handle.running.store(true, Ordering::SeqCst);
        {
            let mut runtime = handle.runtime.lock().unwrap();
            runtime.state = JobState::Queued;
            runtime.message = "queued".into();
        }
        let svc = service.clone();
        let queued = handle.clone();
        tokio::spawn(async move {
            svc.supervise(queued).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            handle.running.load(Ordering::SeqCst),
            "任务应卡在活跃小说槽位等待上"
        );

        service.cancel(&scope.job_id).await.expect("取消应成功");
        assert!(
            !handle.running.load(Ordering::SeqCst),
            "取消后 running 必须复位"
        );
        assert_eq!(handle.runtime.lock().unwrap().state, JobState::Cancelled);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// supervisor panic 后观察者必须复位 running 并落 interrupted：
    /// 否则 pause / cancel 轮询永远超时，resume / delete 永久被拒。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn supervisor_panic_resets_running_and_lands_interrupted() {
        let (service, handle, _scope, dir) = job_fixture("supervisor-panic", settings());
        handle.running.store(true, Ordering::SeqCst);
        // JoinError 观察走的是真实产品路径 watch_supervisor；panic 被
        // JoinHandle 捕获，不会传播到测试进程。
        let supervisor = tokio::spawn(async {
            panic!("模拟 supervisor panic");
        });
        service.watch_supervisor(supervisor, handle.clone()).await;
        assert!(
            !handle.running.load(Ordering::SeqCst),
            "panic 后 running 必须复位"
        );
        assert_eq!(
            handle.runtime.lock().unwrap().state,
            JobState::Interrupted,
            "panic 应落 interrupted（resume 准入状态）"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 正常结束（Join Ok）时观察者不介入：running 由 supervise 尾部复位。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn supervisor_normal_exit_is_not_touched_by_watcher() {
        let (service, handle, _scope, dir) = job_fixture("supervisor-ok", settings());
        handle.running.store(true, Ordering::SeqCst);
        let supervisor = tokio::spawn(async {});
        service.watch_supervisor(supervisor, handle.clone()).await;
        assert!(
            handle.running.load(Ordering::SeqCst),
            "正常结束路径观察者不得提前复位 running"
        );
        assert_eq!(handle.runtime.lock().unwrap().state, JobState::Paused);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 该小说仍有排队 / 运行中的 supervisor 持有锁时，delete 不得清理条目。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn delete_keeps_novel_lock_while_holder_alive() {
        let (service, handle, scope, dir) = job_fixture("lock-keep", settings());
        let holder = service.novel_lock(scope.novel_id);
        let _queued_holder = holder.clone(); // 模拟排队 supervisor 的持有
        service.delete(&scope.job_id).await.expect("删除应成功");
        assert!(
            service
                .inner
                .novel_locks
                .lock()
                .unwrap()
                .contains_key(&scope.novel_id),
            "仍有持有者时不得清理小说锁条目"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 小说锁空闲（无其他持有者）时，delete 应回收条目，避免 map 只增不减。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn delete_removes_idle_novel_lock_entry() {
        let (service, handle, scope, dir) = job_fixture("lock-clean", settings());
        // 创建条目后立即释放返回值：注册表成为唯一持有者。
        let _ = service.novel_lock(scope.novel_id);
        service.delete(&scope.job_id).await.expect("删除应成功");
        assert!(
            !service
                .inner
                .novel_locks
                .lock()
                .unwrap()
                .contains_key(&scope.novel_id),
            "空闲条目应被清理"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
