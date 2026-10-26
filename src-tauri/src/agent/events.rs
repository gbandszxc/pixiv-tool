//! Job 事件与日志辅助。
//!
//! 事件是后端的唯一进度真相源：每 Job `seq` 单调，进度合并节流，阶段切换与
//! 终态立即 flush。Tauri 侧发射 `translation://job`；测试用 [`RecordingSink`]。
//!
//! `seq` 分配与实际下发必须在同一 `state` 临界区内完成：并发线程若能在分配
//! 与下发之间插入更大的 `seq` 先行到达，前端按「`seq` 不大于已见即丢弃」去重
//! 会把迟到的事件永久丢弃（丢的是终态时界面卡在运行中）。[`EventSink`] 契约
//! 要求实现非阻塞且不得回调本发射器，锁内下发不会延长临界区。
//!
//! 节流只作用于传输层，不影响业务状态；调用方在阶段结束、任务终态与界面
//! 关闭前调用 [`JobEmitter::flush`]。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::agent::{
    CurrentWork, JobCounts, JobId, JobScope, JobState, Phase, Preview, Role, TokenUsage,
};

/// Tauri 事件名（前端订阅）。
pub const JOB_EVENT_NAME: &str = "translation://job";

/// 普通进度的最小发送间隔。
pub const PROGRESS_INTERVAL: Duration = Duration::from_secs(5);
/// 生成预览的最小发送间隔。
pub const PREVIEW_INTERVAL: Duration = Duration::from_millis(150);
/// 生成预览累计新增字节达到该值立即发送。
pub const PREVIEW_BYTES: usize = 1024;

/// 一次 Job 状态 / 进度事件（冻结结构，前端快照同构）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JobEvent {
    /// 每 Job 单调递增的序号，从 1 开始。
    pub seq: u64,
    pub job_id: JobId,
    pub novel_id: i64,
    pub state: JobState,
    pub phase: Phase,
    pub counts: JobCounts,
    /// 当前工作位置；空闲时为 `None`。
    pub current: Option<CurrentWork>,
    pub usage: TokenUsage,
    pub elapsed_ms: u64,
    pub preview: Preview,
    /// 仅安全摘要，不含原文 / 译文 / 提示词。
    pub message: String,
    pub error_code: Option<String>,
}

/// 事件出口。实现必须是非阻塞的。
pub trait EventSink: Send + Sync {
    fn emit(&self, event: JobEvent);
}

/// Tauri 事件出口：发射 `translation://job`，payload 与快照结构一致。
#[derive(Clone)]
pub struct TauriEventSink {
    app: AppHandle,
}

impl TauriEventSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl EventSink for TauriEventSink {
    fn emit(&self, event: JobEvent) {
        if let Err(err) = self.app.emit(JOB_EVENT_NAME, &event) {
            // 只报告发射失败，不带事件正文。
            log::warn!(
                "novel={} job={} event=emit_failed error={}",
                event.novel_id,
                event.job_id,
                safe_text(&err.to_string(), 120)
            );
        }
    }
}

/// 测试 / 离线验证用的记录出口。
#[derive(Default)]
pub struct RecordingSink {
    events: Mutex<Vec<JobEvent>>,
}

impl RecordingSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Vec<JobEvent> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    pub fn last(&self) -> Option<JobEvent> {
        self.events
            .lock()
            .ok()
            .and_then(|events| events.last().cloned())
    }

    pub fn len(&self) -> usize {
        self.events.lock().map(|events| events.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        if let Ok(mut events) = self.events.lock() {
            events.clear();
        }
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, event: JobEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }
}

/// 子任务预览槽位：同一角色同一块共用一个槽。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PreviewKey {
    role: Role,
    chunk_id: String,
}

struct SubPreview {
    preview: Preview,
    at: tokio::time::Instant,
}

/// 子任务焦点保持时长：期间只跟随同一个块。
///
/// 并发子任务各自逐帧输出，若按「最后一条事件」直接接管，界面会以 150ms 级
/// 频率在几块之间乱跳。焦点块结束时才让给其他仍在输出的块（ADR 0037）。
const SUB_FOCUS_HOLD: Duration = Duration::from_secs(5);

#[derive(Default)]
struct EmitState {
    seq: u64,
    last_sent: Option<tokio::time::Instant>,
    last_phase: Option<Phase>,
    last_state: Option<JobState>,
    last_counts: JobCounts,
    last_preview_bytes: usize,
    pending: Option<JobEvent>,
    /// 主会话最近一段非空预览。
    main_preview: Preview,
    /// 主会话当前是否在生成（空占位事件会把它置为 false）。
    main_active: bool,
    /// 并发子任务的最近预览，按角色 + 块分槽。
    sub_slots: HashMap<PreviewKey, SubPreview>,
    /// 当前展示的子任务块；在该块生成结束前不切换。
    sub_focus: Option<PreviewKey>,
    /// Job 阶段只由进度 / 状态事件推进，运行层预览不参与。
    job_phase: Option<Phase>,
    /// 最近一次实际下发的预览（无活跃生成时兜底，避免进度事件逐帧清空）。
    display: Preview,
}

/// 事件整理结果：判断按预览节流还是按普通进度节流。
struct Prepared {
    /// 事件本身是否为运行层预览（`message == "preview"`）。
    is_preview: bool,
    /// 是否为「该角色本轮生成结束」的空占位标记。
    marker: bool,
}

impl EmitState {
    fn next_seq(&mut self) -> u64 {
        self.seq = self.seq.saturating_add(1);
        self.seq
    }

    /// 把事件里的预览归入对应槽位，并盖上下发时要展示的预览。
    ///
    /// 运行层事件（含主会话与子任务）都由 [`crate::translation::service::JobProgressSink`]
    /// 归一化为 `message == "preview"`；进度 / 状态事件才会推进 `job_phase`。
    fn prepare(&mut self, event: &mut JobEvent, now: tokio::time::Instant) -> Prepared {
        let is_preview = event.message == "preview";
        let incoming = std::mem::take(&mut event.preview);
        let marker = is_preview && incoming.is_empty();
        if is_preview || !incoming.is_empty() {
            self.route(incoming, now);
        }

        if !is_preview {
            self.job_phase = Some(event.phase);
        }

        let mut display = self.select_display(now);
        if display.is_empty() && !self.display.is_empty() {
            // 所有角色都刚结束一轮：保留上一段文本，直到新的生成接管。
            display = self.display.clone();
        }
        event.preview = display.clone();
        self.display = display;

        if is_preview {
            if let Some(phase) = self.job_phase {
                event.phase = phase;
            }
        }
        if !event.state.is_terminal() {
            // 当前工作位置跟随实际展示的预览；主会话不标块。
            event.current = event
                .preview
                .owner
                .clone()
                .filter(|owner| owner.role != Role::Main);
        }

        Prepared { is_preview, marker }
    }

    fn route(&mut self, incoming: Preview, now: tokio::time::Instant) {
        match incoming.owner.as_ref().map(|owner| owner.role) {
            Some(Role::Main) | None => {
                if incoming.is_empty() {
                    self.main_preview = Preview::default();
                    self.main_active = false;
                } else {
                    self.main_preview = incoming;
                    self.main_active = true;
                }
            }
            Some(role) => {
                let key = PreviewKey {
                    role,
                    chunk_id: incoming
                        .owner
                        .as_ref()
                        .and_then(|owner| owner.chunk_id.clone())
                        .unwrap_or_default(),
                };
                if incoming.is_empty() {
                    self.sub_slots.remove(&key);
                    if self.sub_focus.as_ref() == Some(&key) {
                        self.sub_focus = None;
                    }
                } else {
                    self.sub_slots.insert(
                        key.clone(),
                        SubPreview {
                            preview: incoming,
                            at: now,
                        },
                    );
                    if self.sub_focus.is_none() {
                        self.sub_focus = Some(key);
                    }
                }
            }
        }
    }

    fn select_display(&mut self, now: tokio::time::Instant) -> Preview {
        if self.main_active && !self.main_preview.is_empty() {
            return self.main_preview.clone();
        }
        // 焦点块仍在生成中则保持不动；超过保持窗口视为残留，重新选择。
        if let Some(key) = self.sub_focus.clone() {
            match self.sub_slots.get(&key) {
                Some(slot) if now.saturating_duration_since(slot.at) <= SUB_FOCUS_HOLD => {
                    return slot.preview.clone();
                }
                _ => self.sub_focus = None,
            }
        }
        if let Some((key, slot)) = self.sub_slots.iter().max_by_key(|(_, slot)| slot.at) {
            self.sub_focus = Some(key.clone());
            return slot.preview.clone();
        }
        if !self.main_preview.is_empty() {
            return self.main_preview.clone();
        }
        Preview::default()
    }

    /// 分配 `seq`、补齐 Job 元数据并记录节流基准；返回可直接下发的事件。
    fn seal(&mut self, event: &mut JobEvent) -> u64 {
        let seq = self.next_seq();
        event.seq = seq;
        if event.counts.is_zero() {
            event.counts = self.last_counts;
        }
        self.last_counts = event.counts;
        self.last_state = Some(event.state);
        self.last_phase = Some(event.phase);
        self.last_sent = Some(tokio::time::Instant::now());
        self.last_preview_bytes = event.preview.len_bytes();
        self.pending = None;
        seq
    }
}

/// 每 Job 一个的事件合并器：负责 `seq`、节流与 `flush`。
///
/// 把它作为 [`EventSink`] 传给运行层；业务进度事件与运行层预览共用同一 `seq`
/// 序列。计数全为 0 的事件（运行层只关心预览）沿用上一份已知计数，避免把界面
/// 计数打回 0。
pub struct JobEmitter {
    sink: Arc<dyn EventSink>,
    scope: JobScope,
    started: tokio::time::Instant,
    progress_interval: Duration,
    preview_interval: Duration,
    preview_bytes: usize,
    state: Mutex<EmitState>,
}

impl JobEmitter {
    pub fn new(sink: Arc<dyn EventSink>, scope: JobScope) -> Self {
        Self {
            sink,
            scope,
            started: tokio::time::Instant::now(),
            progress_interval: PROGRESS_INTERVAL,
            preview_interval: PREVIEW_INTERVAL,
            preview_bytes: PREVIEW_BYTES,
            state: Mutex::new(EmitState::default()),
        }
    }

    /// 仅测试使用：缩短节流窗口，避免依赖 tokio 的 test-util 时钟。
    #[cfg(test)]
    fn with_windows(
        mut self,
        progress_interval: Duration,
        preview_interval: Duration,
        preview_bytes: usize,
    ) -> Self {
        self.progress_interval = progress_interval;
        self.preview_interval = preview_interval;
        self.preview_bytes = preview_bytes;
        self
    }

    pub fn seq(&self) -> u64 {
        self.state.lock().map(|state| state.seq).unwrap_or(0)
    }

    /// 最近一次下发的预览（快照 / 刷新回填用）。
    pub fn last_preview(&self) -> Preview {
        self.state
            .lock()
            .map(|state| state.display.clone())
            .unwrap_or_default()
    }

    /// 最近一次下发预览的归属；主会话预览返回 `None`。
    pub fn last_current(&self) -> Option<CurrentWork> {
        self.last_preview()
            .owner
            .filter(|owner| owner.role != Role::Main)
    }

    /// 在持有 `state` 锁的前提下分配 `seq`、补齐 Job 元数据并下发。
    ///
    /// 下发必须在锁内：[`EventSink`] 实现非阻塞且不回调本发射器（Tauri 投递、
    /// 记录追加、空实现），不会与 `state` 形成锁环。
    fn seal_and_emit(&self, state: &mut EmitState, mut event: JobEvent) -> u64 {
        let seq = state.seal(&mut event);
        event.job_id = self.scope.job_id.clone();
        event.novel_id = self.scope.novel_id;
        event.elapsed_ms = self.started.elapsed().as_millis() as u64;
        self.sink.emit(event);
        seq
    }

    /// 立即发送一条事件（跳过节流），返回分配的 `seq`。
    pub fn emit_now(&self, mut event: JobEvent) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        state.prepare(&mut event, tokio::time::Instant::now());
        self.seal_and_emit(&mut state, event)
    }

    /// 发送积压的合并事件（阶段结束、终态、关闭界面之前调用）。
    pub fn flush(&self) {
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(event) = state.pending.take() {
            self.seal_and_emit(&mut state, event);
        }
    }

    /// 判定并处理一条事件：立即发送、合并等待或丢弃。
    fn accept(&self, mut event: JobEvent) {
        let now = tokio::time::Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|err| err.into_inner());
        let prepared = state.prepare(&mut event, now);

        let immediate = state.last_state != Some(event.state)
            || state.last_phase != Some(event.phase)
            || event.state.is_terminal()
            || event.error_code.is_some()
            || prepared.marker;

        if immediate {
            self.seal_and_emit(&mut state, event);
            return;
        }

        let elapsed = state
            .last_sent
            .map(|last| now.saturating_duration_since(last))
            .unwrap_or(self.progress_interval);

        if prepared.is_preview {
            let bytes = event.preview.len_bytes();
            let grew = bytes.saturating_sub(state.last_preview_bytes) >= self.preview_bytes;
            if elapsed >= self.preview_interval || grew {
                self.seal_and_emit(&mut state, event);
            } else {
                // 预览优先：积压的最新预览覆盖普通进度。
                state.pending = Some(event);
            }
            return;
        }

        // 普通进度：≥5s 且计数有变化才发送；窗口内的变化留作下一轮的合并候选。
        if event.counts != state.last_counts {
            if elapsed >= self.progress_interval {
                self.seal_and_emit(&mut state, event);
            } else {
                state.pending = Some(event);
            }
        }
    }
}

impl EventSink for JobEmitter {
    fn emit(&self, event: JobEvent) {
        self.accept(event);
    }
}

/// 日志字段值清洗：压平控制字符并限长；不得传入原文、译文、提示词或凭据。
pub fn safe_text(value: &str, limit: usize) -> String {
    let flattened: String = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    if flattened.chars().count() <= limit {
        return flattened;
    }
    let mut truncated: String = flattened.chars().take(limit).collect();
    truncated.push('…');
    truncated
}

fn log_line(
    scope: &JobScope,
    agent: &str,
    phase: Phase,
    chunk: Option<&str>,
    event: &str,
    fields: &[(&str, &str)],
) -> String {
    let mut line = format!(
        "novel={} job={} agent={} phase={} chunk={} event={}",
        scope.novel_id,
        safe_text(&scope.job_id, 64),
        safe_text(agent, 24),
        phase.as_str(),
        chunk
            .map(|chunk| safe_text(chunk, 32))
            .unwrap_or_else(|| "-".into()),
        safe_text(event, 48)
    );
    for (key, value) in fields {
        line.push(' ');
        line.push_str(&safe_text(key, 24));
        line.push('=');
        line.push_str(&safe_text(value, 160));
    }
    line
}

/// 固定键值日志：`novel= job= agent= phase= chunk= event= k=v`。
///
/// `fields` 的值会经过 [`safe_text`] 清洗；调用方只传计数、耗时、原因码一类
/// 安全摘要，不传正文与凭据。
pub fn log_job(
    scope: &JobScope,
    agent: &str,
    phase: Phase,
    chunk: Option<&str>,
    event: &str,
    fields: &[(&str, &str)],
) {
    let line = log_line(scope, agent, phase, chunk, event, fields);
    log::info!(target: "agent", "{line}");
}

/// 单次工具 / 请求起止的 DEBUG 日志（默认关闭，二级诊断开关控制）。
pub fn debug_job(
    scope: &JobScope,
    agent: &str,
    phase: Phase,
    chunk: Option<&str>,
    event: &str,
    fields: &[(&str, &str)],
) {
    let line = log_line(scope, agent, phase, chunk, event, fields);
    log::debug!(target: "agent", "{line}");
}

/// 事件日志的 WARN 版本（重试、压缩失败、预算耗尽等）。
pub fn warn_job(
    scope: &JobScope,
    agent: &str,
    phase: Phase,
    chunk: Option<&str>,
    event: &str,
    fields: &[(&str, &str)],
) {
    let line = log_line(scope, agent, phase, chunk, event, fields);
    log::warn!(target: "agent", "{line}");
}

/// 构造一条运行层事件（`seq` 与耗时由 [`JobEmitter`] 覆盖）。
pub fn runtime_event(
    scope: &JobScope,
    state: JobState,
    phase: Phase,
    usage: TokenUsage,
    preview: Preview,
    message: &str,
    error_code: Option<&str>,
) -> JobEvent {
    JobEvent {
        seq: 0,
        job_id: scope.job_id.clone(),
        novel_id: scope.novel_id,
        state,
        phase,
        counts: JobCounts::default(),
        current: None,
        usage,
        elapsed_ms: 0,
        preview,
        message: safe_text(message, 120),
        error_code: error_code.map(|code| safe_text(code, 48)),
    }
}

/// 构造带当前工作的进度事件。
pub fn work_event(
    scope: &JobScope,
    state: JobState,
    phase: Phase,
    counts: JobCounts,
    current: Option<CurrentWork>,
    usage: TokenUsage,
    message: &str,
) -> JobEvent {
    JobEvent {
        counts,
        current,
        ..runtime_event(
            scope,
            state,
            phase,
            usage,
            Preview::default(),
            message,
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn scope() -> JobScope {
        JobScope {
            novel_id: 7,
            job_id: "20261024210000-abc123".into(),
            generation: 1,
        }
    }

    fn emitter() -> (JobEmitter, Arc<RecordingSink>) {
        let sink = Arc::new(RecordingSink::new());
        let emitter = JobEmitter::new(sink.clone(), scope());
        (emitter, sink)
    }

    fn progress(counts: JobCounts) -> JobEvent {
        JobEvent {
            counts,
            ..runtime_event(
                &scope(),
                JobState::Running,
                Phase::Translate,
                TokenUsage::default(),
                Preview::default(),
                "progress",
                None,
            )
        }
    }

    #[tokio::test]
    async fn progress_is_throttled_to_the_window_and_count_changes() {
        let (emitter, sink) = emitter();
        let emitter = emitter.with_windows(Duration::from_millis(20), Duration::from_millis(5), 64);
        emitter.emit(progress(JobCounts {
            translated: 1,
            ..JobCounts::default()
        }));
        assert_eq!(sink.len(), 1, "第一条进度立即发送");
        emitter.emit(progress(JobCounts {
            translated: 2,
            ..JobCounts::default()
        }));
        assert_eq!(sink.len(), 1, "窗口内的进度被合并");
        tokio::time::sleep(Duration::from_millis(25)).await;
        emitter.emit(progress(JobCounts {
            translated: 2,
            ..JobCounts::default()
        }));
        assert_eq!(sink.len(), 2, "窗口到点发送最新合并结果");
        emitter.emit(progress(JobCounts {
            translated: 2,
            ..JobCounts::default()
        }));
        assert_eq!(sink.len(), 2, "计数未变化不发送");
        emitter.emit(progress(JobCounts {
            translated: 3,
            ..JobCounts::default()
        }));
        assert_eq!(sink.len(), 2, "窗口内继续合并");
        emitter.flush();
        assert_eq!(sink.len(), 3, "flush 发送积压的合并事件");
        let seqs: Vec<u64> = sink.events().iter().map(|event| event.seq).collect();
        assert_eq!(seqs, vec![1, 2, 3], "seq 必须单调递增");
        assert_eq!(sink.last().unwrap().counts.translated, 3);
    }

    #[tokio::test]
    async fn phase_change_and_terminal_flush_immediately() {
        let (emitter, sink) = emitter();
        emitter.emit(progress(JobCounts {
            translated: 1,
            ..JobCounts::default()
        }));
        let mut review = progress(JobCounts {
            translated: 1,
            ..JobCounts::default()
        });
        review.phase = Phase::Review;
        emitter.emit(review);
        assert_eq!(sink.len(), 2, "阶段切换立即发送");
        let terminal = runtime_event(
            &scope(),
            JobState::Completed,
            Phase::Review,
            TokenUsage::default(),
            Preview::default(),
            "completed",
            None,
        );
        emitter.emit(terminal);
        assert_eq!(sink.len(), 3, "终态立即发送");
        assert_eq!(sink.last().unwrap().state, JobState::Completed);
    }

    #[tokio::test]
    async fn preview_uses_its_own_window_and_supersedes_progress() {
        let (emitter, sink) = emitter();
        let emitter = emitter.with_windows(Duration::from_millis(50), Duration::from_millis(5), 64);
        let preview_event = |text: &str| {
            runtime_event(
                &scope(),
                JobState::Running,
                Phase::Translate,
                TokenUsage::default(),
                Preview {
                    summary: text.to_owned(),
                    reasoning: String::new(),
                    owner: None,
                },
                "preview",
                None,
            )
        };
        emitter.emit(preview_event("first"));
        assert_eq!(sink.len(), 1, "首个预览立即发送");
        emitter.emit(preview_event("second"));
        assert_eq!(sink.len(), 1, "窗口内的小预览被合并");
        assert_eq!(
            sink.last().unwrap().preview.summary,
            "first",
            "被合并的预览尚未发送"
        );
        tokio::time::sleep(Duration::from_millis(8)).await;
        emitter.emit(preview_event("third"));
        assert_eq!(sink.len(), 2);
        assert_eq!(sink.last().unwrap().preview.summary, "third");
        // 累计新增超过字节阈值时即使未满窗口也发送。
        let large = "x".repeat(256);
        emitter.emit(preview_event(&large));
        assert_eq!(sink.len(), 3, "预览字节阈值优先");
    }

    #[tokio::test]
    async fn zero_counts_inherit_last_known_counts() {
        let (emitter, sink) = emitter();
        let emitter = emitter.with_windows(Duration::from_millis(50), Duration::from_millis(1), 32);
        emitter.emit(progress(JobCounts {
            chunks: 10,
            translated: 4,
            ..JobCounts::default()
        }));
        tokio::time::sleep(Duration::from_millis(2)).await;
        let preview = runtime_event(
            &scope(),
            JobState::Running,
            Phase::Translate,
            TokenUsage::default(),
            Preview {
                summary: "stream".into(),
                reasoning: String::new(),
                owner: None,
            },
            "preview",
            None,
        );
        emitter.emit(preview);
        assert_eq!(sink.last().unwrap().counts.translated, 4);
        assert_eq!(sink.last().unwrap().preview.summary, "stream");
    }

    #[test]
    fn safe_text_flattens_controls_and_limits_length() {
        let text = safe_text("line1\nline2\r\tend", 100);
        assert!(!text.contains('\n'));
        assert!(!text.contains('\r'));
        assert!(!text.contains('\t'));
        let long = safe_text(&"a".repeat(300), 10);
        assert_eq!(long.chars().count(), 11, "限长后附加省略号");
    }

    fn owner(role: Role, chunk: &str) -> Option<CurrentWork> {
        Some(CurrentWork {
            page: Some(1),
            chunk_id: Some(chunk.into()),
            role,
        })
    }

    fn owned_preview(role: Role, chunk: &str, text: &str, phase: Phase) -> JobEvent {
        runtime_event(
            &scope(),
            JobState::Running,
            phase,
            TokenUsage::default(),
            Preview {
                summary: text.into(),
                reasoning: String::new(),
                owner: owner(role, chunk),
            },
            "preview",
            None,
        )
    }

    fn empty_owned_preview(role: Role, chunk: &str) -> JobEvent {
        runtime_event(
            &scope(),
            JobState::Running,
            role.default_phase(),
            TokenUsage::default(),
            Preview {
                summary: String::new(),
                reasoning: String::new(),
                owner: owner(role, chunk),
            },
            "preview",
            None,
        )
    }

    #[test]
    fn main_preview_wins_and_subtask_fills_the_gap() {
        let (emitter, sink) = emitter();
        let emitter = emitter.with_windows(Duration::ZERO, Duration::ZERO, 1);
        emitter.emit(progress(JobCounts::default()));
        emitter.emit(owned_preview(Role::Main, "", "主会话", Phase::Translate));
        emitter.emit(owned_preview(
            Role::Translator,
            "c0002",
            "子任务译文",
            Phase::Translate,
        ));
        assert_eq!(
            sink.last().unwrap().preview.summary,
            "主会话",
            "主会话在生成时不被子任务抢走"
        );

        // 主会话本轮结束（空占位）：子任务接管；主会话重新输出时立刻夺回。
        emitter.emit(owned_preview(Role::Main, "", "", Phase::Translate));
        assert_eq!(sink.last().unwrap().preview.summary, "子任务译文");
        assert_eq!(
            sink.last()
                .unwrap()
                .current
                .as_ref()
                .unwrap()
                .chunk_id
                .as_deref(),
            Some("c0002")
        );

        emitter.emit(owned_preview(
            Role::Main,
            "",
            "主会话继续",
            Phase::Translate,
        ));
        assert_eq!(sink.last().unwrap().preview.summary, "主会话继续");
        assert!(sink.last().unwrap().current.is_none(), "主会话预览不标块");
    }

    #[test]
    fn subtask_preview_sticks_to_the_focused_chunk() {
        let (emitter, sink) = emitter();
        let emitter = emitter.with_windows(Duration::ZERO, Duration::ZERO, 1);
        emitter.emit(progress(JobCounts::default()));
        emitter.emit(owned_preview(
            Role::Translator,
            "c0001",
            "第一块",
            Phase::Translate,
        ));
        emitter.emit(owned_preview(
            Role::Translator,
            "c0002",
            "第二块",
            Phase::Translate,
        ));
        assert_eq!(
            sink.last().unwrap().preview.summary,
            "第一块",
            "并发子任务不逐帧抢占同一行"
        );

        // 焦点块结束：让给仍在输出的另一块。
        emitter.emit(empty_owned_preview(Role::Translator, "c0001"));
        assert_eq!(sink.last().unwrap().preview.summary, "第二块");
    }

    #[test]
    fn subtask_preview_does_not_override_job_phase() {
        let (emitter, sink) = emitter();
        let emitter = emitter.with_windows(Duration::ZERO, Duration::ZERO, 1);
        emitter.emit(progress(JobCounts::default()));
        emitter.emit(owned_preview(
            Role::Reviewer,
            "c0005",
            "审查中",
            Phase::Review,
        ));
        assert_eq!(
            sink.last().unwrap().phase,
            Phase::Translate,
            "子任务的运行阶段不能改写 Job 阶段"
        );
    }

    const CONCURRENT_THREADS: usize = 8;
    const CONCURRENT_PER_THREAD: usize = 50;

    /// 断言 RecordingSink 观测到的序列与 `seq` 严格一致且无丢失。
    ///
    /// 这是前端去重契约（`seq <= 已见即丢弃`）的防线：观测顺序一旦与 `seq`
    /// 倒挂，迟到的小 `seq` 会被永久丢弃。
    fn assert_seq_matches_delivery_order(events: &[JobEvent]) {
        assert_eq!(
            events.len(),
            CONCURRENT_THREADS * CONCURRENT_PER_THREAD,
            "并发事件不得丢失"
        );
        for (index, event) in events.iter().enumerate() {
            assert_eq!(
                event.seq,
                index as u64 + 1,
                "seq 必须与实际下发顺序严格一致"
            );
        }
    }

    #[test]
    fn concurrent_emit_now_keeps_seq_matching_delivery_order() {
        let (emitter, sink) = emitter();
        let emitter = Arc::new(emitter);
        let handles: Vec<_> = (0..CONCURRENT_THREADS)
            .map(|_| {
                let emitter = emitter.clone();
                std::thread::spawn(move || {
                    for i in 0..CONCURRENT_PER_THREAD {
                        emitter.emit_now(progress(JobCounts {
                            translated: i as u32 + 1,
                            ..JobCounts::default()
                        }));
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("emit_now 线程不应 panic");
        }
        assert_seq_matches_delivery_order(&sink.events());
    }

    #[test]
    fn concurrent_accept_keeps_seq_matching_delivery_order() {
        // 终态事件每条都命中 accept 的立即发送分支；若 seq 分配与下发不在
        // 同一临界区，多线程下先分配的小 seq 会后到并被前端丢弃。
        let (emitter, sink) = emitter();
        let emitter = Arc::new(emitter);
        let handles: Vec<_> = (0..CONCURRENT_THREADS)
            .map(|_| {
                let emitter = emitter.clone();
                std::thread::spawn(move || {
                    for _ in 0..CONCURRENT_PER_THREAD {
                        emitter.emit(runtime_event(
                            &scope(),
                            JobState::Completed,
                            Phase::Review,
                            TokenUsage::default(),
                            Preview::default(),
                            "completed",
                            None,
                        ));
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("accept 线程不应 panic");
        }
        assert_seq_matches_delivery_order(&sink.events());
    }
}
