/**
 * 小说翻译 IPC（整篇 Job 化，ADR 0034）。
 *
 * 真相源在后端 `translation::service`：`novel_translation_start` 立即返回 Job 快照，
 * 之后状态与进度经 Tauri 事件 `translation://job` 推送；前端 store 用「快照 + seq 追赶」
 * 避免先读快照再订阅时丢事件（见 stores/translation.ts）。
 *
 * 浏览器（非 Tauri）下用确定性定时事件模拟同一契约，便于纯前端视觉验收：
 * 首个 Job 走 queued → prepare → translate → review → completed，第二个 Job 以
 * needs_attention 收尾（演示待处理形态），与真实后端无关。
 */
import { invoke, isTauri, listen } from "./tauri";

export interface NovelTranslationInput {
  novel_id: number;
  title: string;
  tags: string[];
  description: string;
  content: string;
}
export interface TranslatedLine { line: number; text: string }
export type TranslationMode = "original" | "translated" | "bilingual";

/** 目标语言白名单，与后端 translation::TARGET_LANGUAGES 一致（顺序即选择项顺序）。 */
export const TARGET_LANGUAGE_CODES = ["zh-CN", "zh-TW", "en", "ja", "ko", "es", "fr", "de", "ru"] as const;

/** 接口协议白名单，与后端 translation::TRANSLATION_API_FORMATS 一致；默认 Chat Completions。 */
export const API_FORMATS = ["chat_completions", "responses", "anthropic"] as const;
export type ApiFormat = (typeof API_FORMATS)[number];

/** 协议回显：手改 settings.json 写入的未知值按默认 Chat Completions 处理（与后端加载回落一致）。 */
export function canonicalApiFormat(value: string): ApiFormat {
  return (API_FORMATS as readonly string[]).includes(value) ? (value as ApiFormat) : "chat_completions";
}

/** 归一化语言标签（与后端 normalize_language_code 一致），仅用于回显选中项。 */
export function canonicalLanguage(value: string): string {
  const text = value.trim().replace("_", "-").toLowerCase();
  const base = text.split("-")[0];
  if (base === "zh") return text.includes("tw") || text.includes("hk") || text.includes("hant") ? "zh-TW" : "zh-CN";
  return base;
}

/** 单次生成请求的 token 用量（服务流内 usage 事件，能拿到才上报）；total = 输入 + 输出。 */
export interface TokenUsage {
  prompt: number;
  completion: number;
  total: number;
}

// ===== Job 契约（与 backend agent::JobState / Phase / service::TranslationJobSnapshot 对齐）=====

/** 任务状态；阶段与状态分离，等待确认 / 暂停 / 需要处理都可在检查后恢复。 */
export type TranslationJobState =
  | "queued"
  | "running"
  | "awaiting_confirmation"
  | "pausing"
  | "paused"
  | "needs_attention"
  | "cancelling"
  | "cancelled"
  | "interrupted"
  | "completed"
  | "deleted";

/** 任务阶段（后端 Phase 的 snake_case 序列化）。 */
export type TranslationJobPhase =
  | "prepare_source"
  | "prepare_constraints"
  | "translate"
  | "review"
  | "repair"
  | "global_review"
  | "publish";

/** 终态：不会再继续执行，也不再写入产物（interrupted 可恢复，但不再自行推进）。 */
export function isTerminalJobState(state: TranslationJobState): boolean {
  return state === "cancelled" || state === "interrupted" || state === "completed" || state === "deleted";
}

/** 后台仍持有任务（含暂停 / 等待确认 / 需要处理，但不含终态）。 */
export function isActiveJobState(state: TranslationJobState): boolean {
  return !isTerminalJobState(state);
}

/** 任务计数快照。 */
export interface JobCounts {
  pages: number;
  chunks: number;
  translated: number;
  reviewed: number;
  needs_repair: number;
  terms: number;
}

/** 任务累计用量（后端 TokenUsage：input/output/total）。 */
export interface JobUsage {
  input: number;
  output: number;
  total: number;
}

/** 当前工作位置（后端 CurrentWork；字段缺省表示空闲）。 */
export interface JobCurrent {
  page?: number;
  chunk_id?: string;
  role: "main" | "translator" | "reviewer";
}

/** 实时生成预览：summary 优先，reasoning 为思考尾窗。 */
export interface JobPreview {
  summary: string;
  reasoning: string;
  /** 预览归属：主会话 / 翻译子任务 / 审查子任务（ADR 0037）。 */
  owner?: JobCurrent;
}

/** 调用预算快照。 */
export interface JobBudget {
  used: number;
  limit: number;
  remaining: number;
  manual_extra: number;
}

/** 任务快照：前端 store 的单一数据结构（start / status / jobs 三处同构）。 */
export interface TranslationJobSnapshot {
  job_id: string;
  novel_id: number;
  state: TranslationJobState;
  phase: TranslationJobPhase;
  counts: JobCounts;
  current: JobCurrent | null;
  usage: JobUsage;
  elapsed_ms: number;
  preview: JobPreview;
  message: string;
  error_code: string | null;
  /** 每 Job 单调递增的事件序号；0 表示尚无事件（如从磁盘清单恢复）。 */
  seq: number;
  constraint_version: number;
  budget: JobBudget;
  /** 原文已是目标语言、未创建 Job（仅 start 响应可能为 true）。 */
  already_target_language: boolean;
  target_language: string;
  /** 分块计划的块 ID（顺序即合并顺序）；细分后 ID 不连续，不能按序号拼。 */
  chunk_ids?: string[];
}

/** 任务列表（seq = 所有 Job 的最大事件序号）。 */
export interface TranslationJobList {
  seq: number;
  jobs: TranslationJobSnapshot[];
}

/** `novel_translation_start` 返回体。 */
export interface TranslationStartResponse {
  job: TranslationJobSnapshot;
  already_target_language: boolean;
}

/** `translation://job` 事件载荷（后端 JobEvent）。 */
export interface TranslationJobEvent {
  seq: number;
  job_id: string;
  novel_id: number;
  state: TranslationJobState;
  phase: TranslationJobPhase;
  counts: JobCounts;
  current: JobCurrent | null;
  usage: JobUsage;
  elapsed_ms: number;
  preview: JobPreview;
  message: string;
  error_code: string | null;
}

/** 发布状态：complete = 全部块已审查通过；partial = 有未解决块但可读；needs_attention = 需处理。 */
export type PublishStatus = "complete" | "partial" | "needs_attention";

/** 阅读器投影结果（`novel_translation_result`）。pages 的键是页码字符串（1 起）。 */
export interface TranslationResult {
  novel_id: number;
  job_id: string;
  target_language: string;
  pages: Record<string, TranslatedLine[]>;
  status: PublishStatus;
  unresolved: number;
}

export interface PreviewSegment {
  id: string;
  page: number;
  line: number;
  text: string;
  kind: string;
}

export interface PreviewTranslation {
  id: string;
  text: string;
}

export type ReviewVerdict = "pass" | "needs_fix";
export type IssueSeverity = "critical" | "major" | "minor";

export interface PreviewIssue {
  segment_id: string;
  kind: string;
  severity: IssueSeverity;
  detail: string;
}

export interface PreviewReview {
  verdict: ReviewVerdict;
  notes: string;
  issues: PreviewIssue[];
}

/** 浮层预览：单块原文 / 译文对照。 */
export interface ChunkPreview {
  chunk_id: string;
  order: number;
  segments: PreviewSegment[];
  translated: PreviewTranslation[];
  context_prev: string;
  context_next: string;
  draft_version: number;
  reviewed: boolean;
  needs_repair: boolean;
  review: PreviewReview | null;
}

/** 冻结约束里的术语（v2 字段名；与模型工具 schema 一致）。 */
export interface ConstraintTerm {
  source_term: string;
  locked_translation: string;
  term_type: string;
  aliases: string[];
  notes: string;
}

/** 用户确认时对单个术语的修改（后端 ConstraintTermEdit）。 */
export interface ConstraintTermEdit {
  source_term: string;
  locked_translation: string;
  term_type: string;
  aliases: string[];
  notes: string;
}

export interface ConstraintConflict {
  source_term?: string;
  reason?: string;
  candidates?: string[];
  term_type?: string;
  [key: string]: unknown;
}

/** `novel_translation_constraints` 返回体（U4b 契约）。 */
export interface TranslationConstraints {
  version: number;
  terms: ConstraintTerm[];
  style: string;
  conflicts: ConstraintConflict[];
  unresolved: string[];
  frozen: boolean;
  awaiting_confirmation: boolean;
}

/** 问题行（U4b 契约；chunk / segment 两种命名都接受）。 */
export interface TranslationIssueRow {
  severity: IssueSeverity | string;
  chunk?: string;
  chunk_id?: string;
  segment?: string;
  segment_id?: string;
  kind?: string;
  detail: string;
  resolved?: boolean;
}

/** `novel_translation_issues` 返回体（U4b 契约）。 */
export interface TranslationIssues {
  issues: TranslationIssueRow[];
  affected_chunks: string[];
  unresolved_chunks: string[];
  reviewed: number;
  total: number;
}

export interface TranslationProbe {
  api_url: string;
  api_key?: string;
  model: string;
  /** 接口协议（API_FORMATS 之一），空串由后端按默认 Chat Completions 处理。 */
  format: string;
  /** 单请求超时（秒）：「检测可用」按此值计时；获取模型列表固定 30 秒。 */
  timeout_seconds?: number;
  extra: Record<string, unknown>;
}

/** v1 只读兼容：共享设定集与页译文（`novel_translation_get`，阅读器已不再依赖）。 */
export interface TranslationBook {
  bible: { style: string; terms: { source: string; translation: string; kind: string; aliases: string[]; notes: string }[] };
  pages: Record<string, TranslatedLine[]>;
}

// ===== 命令封装 =====

export async function fetchTranslationModels(probe: TranslationProbe): Promise<string[]> {
  if (!isTauri()) return ["demo-model", "demo-model-fast"];
  return invoke("translation_models", { probe });
}

export async function testTranslationService(probe: TranslationProbe): Promise<void> {
  if (!isTauri()) return;
  return invoke("translation_test", { probe });
}

/** v1 只读缓存（保留；阅读器改用 novel_translation_result）。 */
export function getNovelTranslation(novel: NovelTranslationInput): Promise<TranslationBook> {
  if (!isTauri()) return Promise.resolve({ bible: { style: "", terms: [] }, pages: {} });
  return invoke("novel_translation_get", { novel });
}

/** 启动（或加入）整篇翻译。 */
export async function startTranslationJob(novel: NovelTranslationInput, force: boolean): Promise<TranslationStartResponse> {
  if (!isTauri()) return mockStartJob(novel, force);
  return invoke("novel_translation_start", { novel, force });
}

/** 全部任务快照（内存索引 + 磁盘扫描，含重启后的 interrupted）。 */
export async function listTranslationJobs(): Promise<TranslationJobList> {
  if (!isTauri()) return { seq: mockState.seq, jobs: [...mockState.jobs.values()] };
  return invoke("novel_translation_jobs");
}

/** 单个任务快照。 */
export async function getTranslationJob(jobId: string): Promise<TranslationJobSnapshot> {
  if (!isTauri()) {
    const job = mockState.jobs.get(jobId);
    if (!job) throw new Error("任务不存在");
    return job;
  }
  return invoke("novel_translation_status", { jobId });
}

/** 阅读器投影结果；null = 尚无可用译文（v2 完成 Job 与 v1 只读缓存都由后端回退）。 */
export async function getNovelTranslationResult(novelId: number): Promise<TranslationResult | null> {
  if (!isTauri()) return mockResult(novelId);
  return invoke("novel_translation_result", { novelId });
}

/** 确认术语与风格（确认模式）。 */
export async function confirmTranslationJob(jobId: string, terms: ConstraintTermEdit[], style: string): Promise<void> {
  if (!isTauri()) {
    const job = mockState.jobs.get(jobId);
    if (!job) throw new Error("任务不存在");
    mockPatch(jobId, { state: "running", message: "confirmed" });
    return;
  }
  return invoke("novel_translation_confirm", { jobId, terms, style });
}

export async function pauseTranslationJob(jobId: string): Promise<void> {
  if (!isTauri()) { mockPatch(jobId, { state: "paused", message: "paused" }); return; }
  return invoke("novel_translation_pause", { jobId });
}

export async function resumeTranslationJob(jobId: string): Promise<void> {
  if (!isTauri()) { mockPatch(jobId, { state: "running", message: "resumed" }); return; }
  return invoke("novel_translation_resume", { jobId });
}

export async function cancelTranslationJob(jobId: string): Promise<void> {
  if (!isTauri()) { mockPatch(jobId, { state: "cancelled", message: "cancelled" }); return; }
  return invoke("novel_translation_cancel", { jobId });
}

export async function deleteTranslationJob(jobId: string): Promise<void> {
  if (!isTauri()) { mockState.jobs.delete(jobId); return; }
  return invoke("novel_translation_delete", { jobId });
}

/** 浮层预览：原文 / 译文对照。 */
export async function getNovelTranslationPreview(jobId: string, chunkId: string): Promise<ChunkPreview> {
  if (!isTauri()) return mockPreview(jobId, chunkId);
  return invoke("novel_translation_preview", { jobId, chunkId });
}

/**
 * 冻结约束与确认态（U4b）。命令尚未注册时 invoke 会 reject，调用方按「暂无数据」降级。
 */
export async function getNovelTranslationConstraints(jobId: string): Promise<TranslationConstraints> {
  if (!isTauri()) return mockConstraints(jobId);
  return invoke("novel_translation_constraints", { jobId });
}

/** 问题汇总（U4b）；命令尚未注册时同上降级。 */
export async function getNovelTranslationIssues(jobId: string): Promise<TranslationIssues> {
  if (!isTauri()) return mockIssues(jobId);
  return invoke("novel_translation_issues", { jobId });
}

/** 维护页诊断计数（不含正文）。 */
export async function getTranslationDiagnostics(): Promise<Record<string, unknown>> {
  if (!isTauri()) return { generation: 0, active: 0, jobs: mockState.jobs.size, by_state: {} };
  return invoke("novel_translation_diagnostics");
}

/** 会话级详细诊断开关的浏览器 mock 状态：同一页面会话内保持，刷新即复位。 */
let mockDiagnostic = false;

/**
 * 会话级详细诊断开关（仅本次运行有效，重启复位；不写 settings.json）。
 * 返回后端生效后的状态；Tauri 环境命令失败时 reject，由调用方回滚并提示。
 */
export async function setTranslationDiagnostic(enabled: boolean): Promise<boolean> {
  if (!isTauri()) {
    mockDiagnostic = enabled;
    return mockDiagnostic;
  }
  return invoke("translation_set_diagnostic", { enabled });
}

/** 读取当前诊断开关状态（用于设置页打开时回显真实值）。 */
export async function getTranslationDiagnosticStatus(): Promise<boolean> {
  if (!isTauri()) return mockDiagnostic;
  return invoke("translation_diagnostic_status");
}

/** 事件名与后端 agent::events::JOB_EVENT_NAME 一致。 */
export const JOB_EVENT_NAME = "translation://job";

/** 事件载荷最小校验：形状不符直接忽略，不影响其他事件。 */
function normalizeJobEvent(raw: unknown): TranslationJobEvent | null {
  if (typeof raw !== "object" || raw === null) return null;
  const evt = raw as Record<string, unknown>;
  if (typeof evt.job_id !== "string" || !evt.job_id || typeof evt.novel_id !== "number") return null;
  if (typeof evt.state !== "string" || typeof evt.phase !== "string") return null;
  if (typeof evt.seq !== "number") return null;
  return evt as unknown as TranslationJobEvent;
}

/**
 * 订阅 `translation://job`，返回 unlisten。
 * 非 Tauri 环境返回 mock 事件的订阅句柄；Tauri 环境订阅失败时 reject，由 store 降级为
 * 「仅快照」（下次可见性或手动刷新时重新拉快照）。
 */
export async function subscribeTranslationJob(handler: (event: TranslationJobEvent) => void): Promise<() => void> {
  if (!isTauri()) {
    mockState.subscribers.add(handler);
    return () => {
      mockState.subscribers.delete(handler);
    };
  }
  const unlisten = await listen<unknown>(JOB_EVENT_NAME, (event) => {
    const payload = normalizeJobEvent(event.payload);
    if (payload) handler(payload);
  });
  return unlisten;
}

// ===== 浏览器 mock（确定性定时事件，非 Tauri 环境）=====

interface MockState {
  seq: number;
  started: number;
  jobs: Map<string, TranslationJobSnapshot>;
  subscribers: Set<(event: TranslationJobEvent) => void>;
  timers: Map<string, ReturnType<typeof setTimeout>[]>;
}

const mockState: MockState = { seq: 0, started: 0, jobs: new Map(), subscribers: new Set(), timers: new Map() };

const MOCK_COUNTS: JobCounts = { pages: 2, chunks: 6, translated: 0, reviewed: 0, needs_repair: 0, terms: 0 };
const MOCK_STEPS: { at: number; patch: Partial<TranslationJobSnapshot> }[] = [
  { at: 0, patch: { state: "queued", phase: "prepare_source", message: "queued" } },
  { at: 400, patch: { state: "running", phase: "prepare_source", message: "prepare_source" } },
  { at: 900, patch: { state: "running", phase: "prepare_constraints", preview: { summary: "克制、留白的叙事文风", reasoning: "" } } },
  { at: 1500, patch: { state: "running", phase: "translate", current: { page: 1, chunk_id: "c0001", role: "translator" }, counts: { ...MOCK_COUNTS, translated: 1 } } },
  { at: 2200, patch: { state: "running", phase: "translate", current: { page: 1, chunk_id: "c0002", role: "translator" }, counts: { ...MOCK_COUNTS, translated: 3, terms: 5 }, usage: { input: 8200, output: 1900, total: 10100 }, preview: { summary: "星光洒落在回家的路上，她的脚步很轻。", reasoning: "第一句保留原文的句读节奏…", owner: { page: 1, chunk_id: "c0002", role: "translator" } } } },
  { at: 3000, patch: { state: "running", phase: "review", current: { page: 2, chunk_id: "c0005", role: "reviewer" }, counts: { ...MOCK_COUNTS, translated: 6, reviewed: 3, terms: 5 }, usage: { input: 16400, output: 4200, total: 20600 }, preview: { summary: "", reasoning: "核对术语与语气…", owner: { page: 2, chunk_id: "c0005", role: "reviewer" } } } },
  { at: 3800, patch: { state: "running", phase: "publish", counts: { ...MOCK_COUNTS, translated: 6, reviewed: 6, terms: 5 }, usage: { input: 21800, output: 5600, total: 27400 } } },
  { at: 4400, patch: { state: "completed", phase: "publish", message: "completed", preview: { summary: "", reasoning: "" } } },
];

/** mock 终态：第 2 个 Job 演示 needs_attention（偶数编号），其余 completed。 */
function mockTerminalState(index: number): TranslationJobState {
  return index % 2 === 0 ? "needs_attention" : "completed";
}

function mockEmit(job: TranslationJobSnapshot, patch: Partial<TranslationJobSnapshot>): void {
  mockState.seq += 1;
  const next: TranslationJobSnapshot = { ...job, ...patch, seq: mockState.seq, elapsed_ms: patch.elapsed_ms ?? job.elapsed_ms };
  mockState.jobs.set(next.job_id, next);
  const event: TranslationJobEvent = {
    seq: next.seq, job_id: next.job_id, novel_id: next.novel_id, state: next.state, phase: next.phase,
    counts: next.counts, current: next.current, usage: next.usage, elapsed_ms: next.elapsed_ms,
    preview: next.preview, message: next.message, error_code: next.error_code,
  };
  for (const subscriber of mockState.subscribers) subscriber(event);
}

/** 取消未执行的 mock 步骤（暂停 / 取消时停止推进）。 */
function mockClearTimers(jobId: string): void {
  for (const timer of mockState.timers.get(jobId) ?? []) clearTimeout(timer);
  mockState.timers.delete(jobId);
}

/** 从 fromIndex 起按相对延迟排步骤（继续 / 确认后恢复推进）。 */
function mockSchedule(jobId: string, index: number, fromIndex: number): void {
  const base = MOCK_STEPS[fromIndex].at;
  const timers = MOCK_STEPS.slice(fromIndex).map((step) => setTimeout(() => {
    const current = mockState.jobs.get(jobId);
    if (!current) return;
    const aged = { ...step.patch, elapsed_ms: step.at };
    if (step.patch.state === "completed" && mockTerminalState(index) === "needs_attention") {
      mockEmit(current, {
        ...aged, state: "needs_attention", message: "needs_attention", error_code: "review_issues",
        counts: { ...MOCK_COUNTS, translated: 6, reviewed: 6, needs_repair: 2, terms: 5 },
      });
      return;
    }
    mockEmit(current, aged);
  }, step.at - base));
  mockState.timers.set(jobId, timers);
}

async function mockStartJob(novel: NovelTranslationInput, _force: boolean): Promise<TranslationStartResponse> {
  mockState.started += 1;
  const index = mockState.started;
  const job: TranslationJobSnapshot = {
    job_id: `mock-${String(index).padStart(4, "0")}`,
    novel_id: novel.novel_id,
    state: "queued",
    phase: "prepare_source",
    counts: { ...MOCK_COUNTS, pages: Math.max(1, novel.content.split("[newpage]").length) },
    current: null,
    usage: { input: 0, output: 0, total: 0 },
    elapsed_ms: 0,
    preview: { summary: "", reasoning: "" },
    message: "queued",
    error_code: null,
    seq: 0,
    constraint_version: 1,
    budget: { used: 0, limit: 64 + 32 * MOCK_COUNTS.chunks, remaining: 64 + 32 * MOCK_COUNTS.chunks, manual_extra: 0 },
    already_target_language: false,
    target_language: "zh-CN",
  };
  mockState.jobs.set(job.job_id, job);
  mockSchedule(job.job_id, index, 0);
  return { job, already_target_language: false };
}

/** mock 状态改写（暂停 / 继续 / 取消 / 确认）：发一条对应事件，保持 UI 与真实链路同构。 */
function mockPatch(jobId: string, patch: Partial<TranslationJobSnapshot>): void {
  const job = mockState.jobs.get(jobId);
  if (!job) return;
  if (patch.state === "paused" || patch.state === "cancelled") mockClearTimers(jobId);
  if (patch.state === "running" && (job.state === "paused" || job.state === "awaiting_confirmation")) {
    mockSchedule(jobId, Number.parseInt(jobId.replace(/\D/g, ""), 10) || 1, 3);
  }
  mockEmit(job, patch);
}

function mockResult(novelId: number): TranslationResult | null {
  const completed = [...mockState.jobs.values()]
    .filter((job) => job.novel_id === novelId && (job.state === "completed" || job.state === "needs_attention"))
    .sort((a, b) => (a.job_id < b.job_id ? -1 : 1));
  const job = completed[completed.length - 1];
  if (!job) return null;
  return {
    novel_id: novelId,
    job_id: job.job_id,
    target_language: job.target_language || "zh-CN",
    status: job.state === "completed" ? "complete" : "partial",
    unresolved: job.counts.needs_repair,
    pages: {
      "1": [
        { line: 0, text: "序" },
        { line: 1, text: "这是整篇翻译后的第一段译文，用于小说阅读器的视觉验收。" },
        { line: 3, text: "第二段译文：注音与内嵌图标记同样走译文渲染。" },
      ],
      "2": [
        { line: 0, text: "第一章" },
        { line: 2, text: "分页后的第一章译文，随整篇任务一次投影。" },
      ],
    },
  };
}

function mockChunkSegments(order: number): PreviewSegment[] {
  const page = order <= 3 ? 1 : 2;
  return [
    { id: `p00${page}-l000-s0`, page, line: 0, text: order === 1 ? "序" : "第一章", kind: "chapter" },
    { id: `p00${page}-l001-s0`, page, line: 1, text: `第 ${order} 块的原文段落，用于浮层对照预览。`, kind: "text" },
  ];
}

function mockPreview(jobId: string, chunkId: string): ChunkPreview {
  const job = mockState.jobs.get(jobId);
  const order = Math.max(1, Number.parseInt(chunkId.replace(/\D/g, ""), 10) || 1);
  const translatedCount = job?.counts.translated ?? 0;
  const translated = order <= translatedCount;
  return {
    chunk_id: chunkId,
    order,
    segments: mockChunkSegments(order),
    translated: translated ? mockChunkSegments(order).map((seg) => ({ id: seg.id, text: `${seg.text}（译文）` })) : [],
    context_prev: order > 1 ? "上一块的结尾上下文。" : "",
    context_next: "下一块的开头上下文。",
    draft_version: translated ? 1 : 0,
    reviewed: (job?.counts.reviewed ?? 0) >= order,
    needs_repair: (job?.counts.needs_repair ?? 0) > 0 && order === translatedCount,
    review: null,
  };
}

function mockConstraints(jobId: string): TranslationConstraints {
  const job = mockState.jobs.get(jobId);
  const awaiting = job?.state === "awaiting_confirmation";
  return {
    version: job?.constraint_version ?? 1,
    terms: awaiting
      ? [
          { source_term: "アリス", locked_translation: "爱丽丝", term_type: "person", aliases: ["Alice"], notes: "主角" },
          { source_term: "星の帰り道", locked_translation: "星光归途", term_type: "title", aliases: [], notes: "" },
        ]
      : [],
    style: awaiting ? "克制、留白的叙事文风，短句优先。" : "",
    conflicts: [],
    unresolved: [],
    frozen: !awaiting,
    awaiting_confirmation: awaiting,
  };
}

function mockIssues(jobId: string): TranslationIssues {
  const job = mockState.jobs.get(jobId);
  const issues: TranslationIssueRow[] = job?.state === "needs_attention"
    ? [
        { severity: "major", chunk: "c0005", segment: "p002-l003-s0", kind: "terminology", detail: "术语「アリス」未按冻结译名输出。", resolved: false },
        { severity: "minor", chunk: "c0006", segment: "p002-l008-s0", kind: "style", detail: "语气与前后段落不一致。", resolved: true },
      ]
    : [];
  return {
    issues,
    affected_chunks: [...new Set(issues.map((issue) => issue.chunk ?? ""))].filter(Boolean),
    unresolved_chunks: job?.state === "needs_attention" ? ["c0005"] : [],
    reviewed: job?.counts.reviewed ?? 0,
    total: job?.counts.chunks ?? 0,
  };
}
