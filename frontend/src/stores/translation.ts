/**
 * 整篇翻译任务状态（ADR 0034）：以 Job 快照为中心，多小说各自一条。
 *
 * - `jobs`：jobId → 快照（终态保留，浮层可回看「已完成 / 需要处理」）；
 * - `activeByNovel`：novelId → 当前 Job（非终态；同一小说只展示最新一条）；
 * - `errors`：启动 / 操作失败记录，跨页面保留，由阅读器按小说消费；
 * - `lastResultVersion`：结果刷新信号（终态或部分完成时 +1，阅读器据此拉一次投影结果）。
 *
 * 事件与快照的合并规则：先拉 `novel_translation_jobs()` 快照，再订阅 `translation://job`；
 * 事件 seq ≤ 本地 seq 丢弃；快照 seq 落后于本地事件时保留事件版本。监听属于 App 级单例，
 * 页面卸载不断开——关闭浮层或离开阅读页都不取消任务。
 */
import { computed, ref, watchEffect } from "vue";
import { defineStore } from "pinia";
import { errorMessage } from "../api/tauri";
import {
  cancelTranslationJob,
  confirmTranslationJob,
  deleteTranslationJob,
  getNovelTranslationConstraints,
  getNovelTranslationIssues,
  getNovelTranslationPreview,
  getNovelTranslationResult,
  getTranslationJob,
  isActiveJobState,
  isTerminalJobState,
  listTranslationJobs,
  pauseTranslationJob,
  resumeTranslationJob,
  startTranslationJob,
  subscribeTranslationJob,
  type ChunkPreview,
  type ConstraintTermEdit,
  type NovelTranslationInput,
  type TranslationConstraints,
  type TranslationIssues,
  type TranslationJobEvent,
  type TranslationJobSnapshot,
  type TranslationResult,
} from "../api/translation";

/** 单一任务的操作失败记录（启动 / 暂停 / 恢复 / 取消 / 确认）。 */
export interface TranslationFailure {
  novelId: number;
  jobId: string;
  message: string;
}

/** `start` 结果：已创建/加入任务，或原文已是目标语言（未建 Job）。 */
export interface TranslationStartOutcome {
  status: "started" | "already_target_language";
  job: TranslationJobSnapshot | null;
  targetLanguage: string;
}

/** 失败记录只保留最近若干条：避免长期占用内存，同时覆盖多小说并行。 */
const MAX_ERRORS = 12;
/** 预览缓存上限：翻块浏览不无限增长。 */
const MAX_PREVIEWS = 120;

/** 构造事件缺失字段的兜底快照（首次收到未知 Job 的事件时使用）。 */
function snapshotFromEvent(event: TranslationJobEvent, existing?: TranslationJobSnapshot): TranslationJobSnapshot {
  return {
    job_id: event.job_id,
    novel_id: event.novel_id,
    state: event.state,
    phase: event.phase,
    counts: event.counts ?? { pages: 0, chunks: 0, translated: 0, reviewed: 0, needs_repair: 0, terms: 0 },
    current: event.current ?? null,
    usage: event.usage ?? { input: 0, output: 0, total: 0 },
    elapsed_ms: event.elapsed_ms ?? 0,
    preview: event.preview ?? { summary: "", reasoning: "" },
    message: event.message ?? "",
    error_code: event.error_code ?? null,
    seq: event.seq,
    constraint_version: existing?.constraint_version ?? 0,
    budget: existing?.budget ?? { used: 0, limit: 0, remaining: 0, manual_extra: 0 },
    already_target_language: false,
    target_language: existing?.target_language ?? "",
  };
}

export const useTranslationStore = defineStore("translation", () => {
  /** jobId → 快照（含终态，供浮层回看）。 */
  const jobs = ref(new Map<string, TranslationJobSnapshot>());
  /** novelId → 当前 Job（非终态，每个小说至多一条）。 */
  const activeByNovel = ref(new Map<number, string>());
  const errors = ref<TranslationFailure[]>([]);
  /** 结果刷新信号：任意 Job 进入 completed / needs_attention 时自增。 */
  const lastResultVersion = ref(0);
  /** 最近一次触发刷新信号的小说，阅读器据此过滤无关事件。 */
  const lastResultNovelId = ref(0);
  /** novelId → 小说标题（状态栏与任务菜单展示用；由阅读器 / start 登记）。 */
  const novelTitles = ref(new Map<number, string>());

  /** 事件时间戳：估算「已用」时在快照 elapsed_ms 上补上两次事件之间的间隔。 */
  const receivedAt = new Map<string, number>();
  /** 本地已应用的最大 seq（快照落后时不回退）。 */
  const seqByJob = new Map<string, number>();
  /** `${jobId}:${chunkId}` → 预览（非响应式，组件自持展示状态）。 */
  const previewCache = new Map<string, ChunkPreview>();
  /** novelId → 已拉取的投影结果及其版本。 */
  const results = ref(new Map<number, { version: number; result: TranslationResult | null }>());
  /** jobId:constraintVersion:state → 约束（版本或状态变化后自动重取）。 */
  const constraintsCache = new Map<string, TranslationConstraints>();

  /** 后台计时器：仅在有活跃 Job 时每秒推进「已用」。 */
  const now = ref(Date.now());
  let ticker: number | null = null;
  let started: Promise<void> | null = null;
  let unlisten: (() => void) | null = null;
  let cacheClearBound = false;
  let visibilityBound = false;

  /** 活跃 Job（每本小说一条，按 Job ID 升序稳定排序）。 */
  const activeJobs = computed<TranslationJobSnapshot[]>(() => {
    const list: TranslationJobSnapshot[] = [];
    for (const jobId of activeByNovel.value.values()) {
      const job = jobs.value.get(jobId);
      if (job && isActiveJobState(job.state)) list.push(job);
    }
    return list.sort((a, b) => (a.job_id < b.job_id ? -1 : a.job_id > b.job_id ? 1 : 0));
  });

  /** 当前小说最新的 Job（含终态）；没有则 null。 */
  function jobFor(novelId: number): TranslationJobSnapshot | null {
    let latest: TranslationJobSnapshot | null = null;
    for (const job of jobs.value.values()) {
      if (job.novel_id !== novelId) continue;
      if (!latest || job.job_id > latest.job_id) latest = job;
    }
    return latest;
  }

  /** 当前小说的活跃 Job（不把暂停 / 待确认伪称进行中，但仍属后台任务）。 */
  function activeFor(novelId: number): TranslationJobSnapshot | null {
    const jobId = activeByNovel.value.get(novelId);
    if (!jobId) return null;
    const job = jobs.value.get(jobId);
    return job && isActiveJobState(job.state) ? job : null;
  }

  /** 该小说最近一次失败记录；不匹配返回 null。 */
  function errorFor(novelId: number): string | null {
    const failure = errors.value.find((entry) => entry.novelId === novelId);
    return failure?.message ?? null;
  }

  /** 读取并清除该小说的失败记录（阅读器展示后消费）。 */
  function consumeError(novelId: number): string | null {
    const index = errors.value.findIndex((entry) => entry.novelId === novelId);
    if (index < 0) return null;
    const [failure] = errors.value.splice(index, 1);
    return failure.message;
  }

  function pushError(novelId: number, jobId: string, error: unknown): void {
    const message = errorMessage(error);
    errors.value = [...errors.value.filter((entry) => !(entry.novelId === novelId && entry.jobId === jobId)), { novelId, jobId, message }].slice(-MAX_ERRORS);
  }

  /**
   * 应用一份快照：更新 jobs、活跃索引、结果刷新信号。
   * `seq` 落后于本地已应用事件的快照不回退（快照与事件存在竞态）。
   */
  function applySnapshot(snapshot: TranslationJobSnapshot): void {
    const localSeq = seqByJob.get(snapshot.job_id) ?? 0;
    if ((snapshot.seq ?? 0) < localSeq) return;
    seqByJob.set(snapshot.job_id, snapshot.seq ?? 0);
    const previous = jobs.value.get(snapshot.job_id);
    jobs.value.set(snapshot.job_id, snapshot);
    receivedAt.set(snapshot.job_id, Date.now());

    const novelId = snapshot.novel_id;
    const currentId = activeByNovel.value.get(novelId);
    const currentJob = currentId ? jobs.value.get(currentId) : undefined;
    if (isTerminalJobState(snapshot.state)) {
      if (currentId === snapshot.job_id) activeByNovel.value.delete(novelId);
    } else if (!currentId || snapshot.job_id > currentId || currentJob === undefined) {
      // 同一小说只展示最新 Job：Job ID 前缀是时间戳，可直接比较先后。
      activeByNovel.value.set(novelId, snapshot.job_id);
    } else if (currentJob && isTerminalJobState(currentJob.state)) {
      activeByNovel.value.set(novelId, snapshot.job_id);
    }

    // 发布态变化都触发一次结果刷新：完成→需要处理（退回已发布块）也要让阅读器重读投影。
    const published = snapshot.state === "completed" || snapshot.state === "needs_attention";
    if (published && previous?.state !== snapshot.state) {
      lastResultVersion.value += 1;
      lastResultNovelId.value = novelId;
    }
    if (previous && previous.constraint_version !== snapshot.constraint_version) {
      constraintsCache.clear();
      previewCache.clear();
    }
    if (snapshot.state === "completed" || snapshot.state === "needs_attention" || snapshot.phase === "publish") {
      // 终态后块内容已定稿：清掉旧草稿预览，下次按需重读。
      for (const key of previewCache.keys()) if (key.startsWith(`${snapshot.job_id}:`)) previewCache.delete(key);
    }
  }

  /** 事件入口（订阅回调与测试注入共用）。 */
  function ingest(event: TranslationJobEvent): void {
    const localSeq = seqByJob.get(event.job_id) ?? 0;
    if (event.seq <= localSeq) return;
    const existing = jobs.value.get(event.job_id);
    applySnapshot(snapshotFromEvent(event, existing));
  }

  /** 拉全量快照（首次订阅前 / 重新可见 / 手动刷新）。 */
  async function refreshJobs(): Promise<void> {
    const list = await listTranslationJobs();
    const seen = new Set<string>();
    for (const job of list.jobs) {
      seen.add(job.job_id);
      applySnapshot(job);
    }
    for (const jobId of [...jobs.value.keys()]) {
      if (!seen.has(jobId)) {
        jobs.value.delete(jobId);
        seqByJob.delete(jobId);
        receivedAt.delete(jobId);
      }
    }
    for (const [novelId, jobId] of [...activeByNovel.value.entries()]) {
      if (!seen.has(jobId)) activeByNovel.value.delete(novelId);
    }
  }

  /** 刷新单个 Job（动作完成后立即回读，不等事件）。 */
  async function refreshJob(jobId: string): Promise<void> {
    const snapshot = await getTranslationJob(jobId);
    applySnapshot(snapshot);
  }

  /** 缓存清理：后端 Job 索引与产物已删除，本地不保留可点击的幽灵条目。 */
  function reset(): void {
    jobs.value.clear();
    activeByNovel.value.clear();
    errors.value = [];
    results.value.clear();
    previewCache.clear();
    constraintsCache.clear();
    seqByJob.clear();
    receivedAt.clear();
    novelTitles.value.clear();
  }

  /** 事件订阅失败时的重连：可见性变化时重新拉快照并尝试重新订阅。 */
  async function reconnect(): Promise<void> {
    if (!unlisten) {
      try {
        unlisten = await subscribeTranslationJob(ingest);
      } catch {
        unlisten = null;
      }
    }
    await refreshJobs().catch(() => {});
  }

  /** 全局单例初始化（幂等）：先快照、再订阅，最后挂缓存清理与重连钩子。 */
  function ensureListening(): Promise<void> {
    if (started) return started;
    started = (async () => {
      await refreshJobs().catch(() => {});
      try {
        unlisten = await subscribeTranslationJob(ingest);
      } catch {
        // 事件不可用（如测试环境缺少 event plugin）：退化为快照 + 手动刷新。
        unlisten = null;
      }
      if (typeof window !== "undefined" && !cacheClearBound) {
        cacheClearBound = true;
        window.addEventListener("pixiv-tool:translation-cache-cleared", reset);
      }
      if (typeof document !== "undefined" && !visibilityBound) {
        visibilityBound = true;
        document.addEventListener("visibilitychange", () => {
          if (document.visibilityState === "visible") void reconnect();
        });
      }
    })();
    return started;
  }

  /** 登记小说标题（状态栏与任务菜单用；只记第一次，空标题忽略）。 */
  function registerTitle(novelId: number, title: string): void {
    const text = title.trim();
    if (!text || novelTitles.value.get(novelId) === text) return;
    novelTitles.value.set(novelId, text);
  }

  /** 读标题：未登记时回退「小说 #id」，由调用方本地化。 */
  function titleFor(novelId: number): string {
    return novelTitles.value.get(novelId) ?? "";
  }

  /** 已用时间：活跃任务的快照 elapsed_ms 补上最近一次事件到现在的间隔。 */
  function elapsedFor(job: TranslationJobSnapshot): number {
    if (!isActiveJobState(job.state)) return job.elapsed_ms;
    const stamp = receivedAt.get(job.job_id);
    return stamp ? job.elapsed_ms + Math.max(0, now.value - stamp) : job.elapsed_ms;
  }

  /** 有活跃任务时每秒推进一次 now，供状态栏「已用」自走。 */
  watchEffect(() => {
    const active = activeJobs.value.length > 0;
    if (active && ticker === null) {
      ticker = window.setInterval(() => { now.value = Date.now(); }, 1000);
    } else if (!active && ticker !== null) {
      window.clearInterval(ticker);
      ticker = null;
    }
  });

  /**
   * 启动整篇翻译。原文已是目标语言时不创建 Job，返回提示；重复启动由后端合并并回传既有
   * Job 快照（force 才会新建）。
   */
  async function start(novel: NovelTranslationInput, force = false): Promise<TranslationStartOutcome> {
    registerTitle(novel.novel_id, novel.title);
    await ensureListening();
    try {
      const response = await startTranslationJob(novel, force);
      if (response.already_target_language || !response.job.job_id) {
        return { status: "already_target_language", job: null, targetLanguage: response.job.target_language };
      }
      applySnapshot(response.job);
      return { status: "started", job: jobs.value.get(response.job.job_id) ?? response.job, targetLanguage: response.job.target_language };
    } catch (error) {
      pushError(novel.novel_id, "", error);
      throw error;
    }
  }

  /** 任务动作统一走这里：成功后立即回读快照，失败记账后原样抛出供界面提示。 */
  async function runAction(jobId: string, action: (id: string) => Promise<void>): Promise<void> {
    const novelId = jobs.value.get(jobId)?.novel_id ?? 0;
    try {
      await action(jobId);
      await refreshJob(jobId);
    } catch (error) {
      pushError(novelId, jobId, error);
      throw error;
    }
  }

  function pause(jobId: string): Promise<void> {
    return runAction(jobId, pauseTranslationJob);
  }

  function resume(jobId: string): Promise<void> {
    return runAction(jobId, resumeTranslationJob);
  }

  function cancel(jobId: string): Promise<void> {
    return runAction(jobId, cancelTranslationJob);
  }

  function remove(jobId: string): Promise<void> {
    return runAction(jobId, deleteTranslationJob);
  }

  async function confirm(jobId: string, terms: ConstraintTermEdit[], style: string): Promise<void> {
    await runAction(jobId, (id) => confirmTranslationJob(id, terms, style));
    constraintsCache.clear();
  }

  /** 浮层预览（按 `${jobId}:${chunkId}` 缓存；force 时重读）。 */
  async function preview(jobId: string, chunkId: string, force = false): Promise<ChunkPreview> {
    const key = `${jobId}:${chunkId}`;
    if (!force) {
      const cached = previewCache.get(key);
      if (cached) return cached;
    }
    const data = await getNovelTranslationPreview(jobId, chunkId);
    if (previewCache.size >= MAX_PREVIEWS) {
      const oldest = previewCache.keys().next().value;
      if (oldest !== undefined) previewCache.delete(oldest);
    }
    previewCache.set(key, data);
    return data;
  }

  /**
   * 冻结约束 / 确认态。U4b 命令未注册时返回 null（界面显示「暂无数据」），
   * 不影响状态、预览与阅读器。
   */
  async function constraints(jobId: string): Promise<TranslationConstraints | null> {
    const job = jobs.value.get(jobId);
    const key = `${jobId}:${job?.constraint_version ?? 0}:${job?.state ?? ""}`;
    const cached = constraintsCache.get(key);
    if (cached) return cached;
    try {
      const data = await getNovelTranslationConstraints(jobId);
      constraintsCache.set(key, data);
      return data;
    } catch {
      return null;
    }
  }

  /** 问题汇总；命令未注册时返回 null（界面显示「暂无数据」）。 */
  async function issues(jobId: string): Promise<TranslationIssues | null> {
    try {
      return await getNovelTranslationIssues(jobId);
    } catch {
      return null;
    }
  }

  /** 阅读器投影结果；版本一致时走缓存，避免每次事件都读盘。 */
  async function result(novelId: number, force = false): Promise<TranslationResult | null> {
    const cached = results.value.get(novelId);
    const version = lastResultVersion.value;
    if (!force && cached && cached.version === version) return cached.result;
    const data = await getNovelTranslationResult(novelId);
    results.value.set(novelId, { version, result: data });
    return data;
  }

  return {
    jobs,
    activeByNovel,
    activeJobs,
    errors,
    lastResultVersion,
    lastResultNovelId,
    novelTitles,
    ensureListening,
    refreshJobs,
    refreshJob,
    reconnect,
    reset,
    ingest,
    jobFor,
    activeFor,
    errorFor,
    consumeError,
    registerTitle,
    titleFor,
    elapsedFor,
    start,
    pause,
    resume,
    cancel,
    remove,
    confirm,
    preview,
    constraints,
    issues,
    result,
  };
});
