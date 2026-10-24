/**
 * 多小说并行整篇翻译与底栏后台指示验收（ADR 0031 / 0034）：
 * store 的 Job 快照隔离、seq 追赶、底栏单/多任务形态、暂停与终态的计数语义、任务菜单跳转。
 * 只使用 mock IPC，不访问模型服务。
 */
import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter } from "vue-router";
import DownloadStatusBar from "../src/components/download/DownloadStatusBar.vue";
import { useTranslationStore } from "../src/stores/translation";
import type { NovelTranslationInput, TranslationJobEvent, TranslationJobSnapshot } from "../src/api/translation";
import zhCN from "../src/locales/zh-CN";
import enUS from "../src/locales/en-US";
import "../src/styles/main.css";
import "../src/material";

function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const settle = async () => { await nextTick(); await new Promise(resolve => setTimeout(resolve, 30)); };

const jobs = new Map<string, TranslationJobSnapshot>();
let seq = 0;
let startCalls = 0;
const listeners = new Set<(event: TranslationJobEvent) => void>();
const callbacks = new Map<number, (event: unknown) => void>();
let callbackSeq = 0;

const EMPTY_COUNTS = { pages: 2, chunks: 3, translated: 0, reviewed: 0, needs_repair: 0, terms: 0 };
const EMPTY_USAGE = { input: 0, output: 0, total: 0 };

function newJob(novelId: number): TranslationJobSnapshot {
  return {
    job_id: `2026102400${String(novelId).padStart(4, "0")}-aaaaaa`,
    novel_id: novelId,
    state: "queued",
    phase: "prepare_source",
    counts: { ...EMPTY_COUNTS },
    current: null,
    usage: { ...EMPTY_USAGE },
    elapsed_ms: 0,
    preview: { summary: "", reasoning: "" },
    message: "queued",
    error_code: null,
    seq: 0,
    constraint_version: 1,
    budget: { used: 0, limit: 80, remaining: 80, manual_extra: 0 },
    already_target_language: false,
    target_language: "zh-CN",
  };
}

function emit(jobId: string, patch: Partial<TranslationJobSnapshot>): void {
  const current = jobs.get(jobId);
  assert(current, `任务 ${jobId} 存在`);
  seq += 1;
  const next = { ...current!, ...patch, seq };
  jobs.set(jobId, next);
  const payload: TranslationJobEvent = {
    seq: next.seq, job_id: next.job_id, novel_id: next.novel_id, state: next.state, phase: next.phase,
    counts: next.counts, current: next.current, usage: next.usage, elapsed_ms: next.elapsed_ms,
    preview: next.preview, message: next.message, error_code: next.error_code,
  };
  for (const listener of [...listeners]) (listener as unknown as (event: unknown) => void)({ event: "translation://job", id: 1, payload });
}

window.__TAURI_INTERNALS__ = {
  transformCallback: (callback: (event: unknown) => void) => {
    const id = ++callbackSeq;
    callbacks.set(id, callback);
    return id;
  },
  invoke: async (command: string, args: Record<string, any>) => {
    if (command === "plugin:event|listen") {
      const callback = callbacks.get(args.handler);
      if (callback) listeners.add(callback as unknown as (event: TranslationJobEvent) => void);
      return ++callbackSeq;
    }
    if (command === "plugin:event|unlisten") return null;
    if (command === "novel_translation_jobs") return { seq, jobs: [...jobs.values()] };
    if (command === "novel_translation_status") {
      const found = [...jobs.values()].find(job => job.job_id === args.jobId);
      if (!found) throw new Error("任务不存在");
      return found;
    }
    if (command === "novel_translation_start") {
      startCalls++;
      const novelId = args.novel.novel_id;
      const existing = [...jobs.values()].find(job => job.novel_id === novelId);
      if (existing && !args.force) return { job: existing, already_target_language: false };
      const created = newJob(novelId);
      jobs.set(created.job_id, created);
      return { job: created, already_target_language: false };
    }
    if (command === "novel_translation_result") return null;
    if (command === "novel_translation_pause" || command === "novel_translation_resume" || command === "novel_translation_cancel") return null;
    return {};
  },
};

const novel = (id: number, title: string): NovelTranslationInput => ({ novel_id: id, title, tags: [], description: "", content: "アリスは微笑んだ。" });
const jobId = (novelId: number) => `2026102400${String(novelId).padStart(4, "0")}-aaaaaa`;

const pinia = createPinia();
const i18n = createI18n({ legacy: false, locale: "zh-CN", messages: { "zh-CN": zhCN, "en-US": enUS } });
const router = createRouter({
  history: createMemoryHistory(),
  routes: [
    { path: "/", component: { template: "<div/>" } },
    { path: "/browse/work/novel/:id", component: { template: "<div/>" } },
    { path: "/tools/tasks", component: { template: "<div/>" } },
  ],
});

async function run() {
  await router.push("/");
  const host = document.createElement("div");
  document.body.append(host);
  const app = createApp(DownloadStatusBar).use(pinia).use(i18n).use(router);
  app.mount(host);
  await settle();
  const store = useTranslationStore(pinia);
  const statusButton = () => host.querySelector<HTMLElement>(".status-translation");
  const picker = () => host.querySelector<HTMLElement>(".translation-picker");

  assert(!statusButton(), "空闲时不占位");

  // ===== 不同小说并行（每本一条 Job） =====
  await store.start(novel(1, "星の帰り道"));
  await store.start(novel(2, "海辺の手紙"));
  await settle();
  assert(startCalls === 2, "不同小说各自启动，不互相排队");
  assert(store.activeJobs.length === 2, "两条活跃 Job 并存");
  assert(store.activeFor(1)?.novel_id === 1 && store.activeFor(2)?.novel_id === 2, "按小说取各自的 Job");
  assert(statusButton()?.textContent?.includes("后台 2 个翻译任务正在进行"), "全部在推进时显示计数文案");

  // ===== 进度事件按 Job 隔离，旧 seq 丢弃 =====
  emit(jobId(1), { state: "running", phase: "translate", counts: { ...EMPTY_COUNTS, translated: 2 }, current: { page: 1, chunk_id: "c0002", role: "translator" }, usage: { input: 100, output: 20, total: 120 }, preview: { summary: "甲小说的摘要", reasoning: "" }, elapsed_ms: 5000 });
  await settle();
  assert(store.activeFor(1)?.counts.translated === 2 && store.activeFor(2)?.counts.translated === 0, "进度事件不串任务");
  assert(store.activeFor(1)?.preview.summary === "甲小说的摘要" && store.activeFor(2)?.preview.summary === "", "预览按 Job 隔离");
  const stale = store.activeFor(1)!.seq;
  emit(jobId(1), { state: "running", phase: "review", counts: { ...EMPTY_COUNTS, translated: 1 } });
  // 手工注入一个更旧的 seq：应被丢弃。
  store.ingest({ ...store.activeFor(1)!, seq: stale, phase: "translate", counts: { ...EMPTY_COUNTS, translated: 0 } } as TranslationJobEvent);
  await settle();
  assert(store.activeFor(1)?.counts.translated === 1, "seq 小于本地的事件被丢弃");
  assert(store.activeFor(1)?.phase === "review", "阶段不被旧事件回退");

  // ===== 重复启动同小说：后端合并，前端不重复计数 =====
  await store.start(novel(1, "星の帰り道"));
  await settle();
  assert(store.activeJobs.length === 2, "同小说重复启动不产生第二条活跃 Job");
  assert(store.jobs.size === 2, "同小说重复启动复用既有 Job 快照");

  // ===== 底栏多任务：计数入口 + 选择菜单 =====
  assert(host.querySelectorAll(".status-translation").length === 1, "多任务只有一个入口");
  const countLabel = statusButton()!.querySelector<HTMLElement>(".translation-full")!;
  assert(countLabel.scrollWidth <= countLabel.clientWidth + 1, `计数文案完整显示（scrollWidth ${countLabel.scrollWidth} / clientWidth ${countLabel.clientWidth}）`);
  assert(statusButton()?.getAttribute("aria-haspopup") === "menu" && statusButton()?.getAttribute("aria-expanded") === "false", "计数入口声明菜单语义");
  statusButton()!.click();
  await settle();
  assert(picker()?.getAttribute("role") === "menu", "点击计数入口浮出任务选择菜单");
  assert(document.activeElement === picker(), "打开后焦点进入菜单");
  const items = [...host.querySelectorAll<HTMLElement>(".picker-item")];
  assert(items.length === 2, "菜单按活跃 Job 列出条目");
  assert(items[0].textContent?.includes("星の帰り道") && items[0].textContent?.includes("第 1 页"), "条目标题为小说名与当前页");
  assert(items[0].textContent?.includes("审查译文") && items[0].textContent?.includes("已用"), "条目回显阶段与已用时间");
  assert(items[1].textContent?.includes("海辺の手紙"), "两个任务都可选择");
  // Esc 关闭并回到入口焦点。
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  await settle();
  assert(!picker() && document.activeElement === statusButton(), "Esc 关闭菜单并把焦点还给入口");
  // 透明遮罩外部点击关闭。
  statusButton()!.click(); await settle();
  host.querySelector<HTMLElement>(".translation-backdrop")!.click(); await settle();
  assert(!picker(), "点击外部关闭菜单");
  // 选择条目进入对应小说。
  statusButton()!.click(); await settle();
  host.querySelectorAll<HTMLElement>(".picker-item")[1].click();
  await settle();
  assert(router.currentRoute.value.path === "/browse/work/novel/2", "选择条目进入该小说页");
  assert(!picker(), "跳转后菜单关闭");

  // ===== 暂停不冒充进行中，但仍计入后台 =====
  emit(jobId(2), { state: "paused", message: "paused" });
  await settle();
  assert(store.activeJobs.length === 2, "暂停任务仍计入后台");
  assert(statusButton()?.textContent?.includes("后台 2 个翻译任务") && !statusButton()?.textContent?.includes("正在进行"), "混有暂停任务时不谎称正在进行");
  statusButton()!.click(); await settle();
  const pausedItem = [...host.querySelectorAll<HTMLElement>(".picker-item")].find(el => el.textContent?.includes("海辺の手紙"));
  assert(pausedItem?.textContent?.includes("已暂停"), "菜单条目回显暂停状态");
  assert(pausedItem?.querySelector(".picker-dot.is-paused"), "暂停任务状态点保持中性");
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); await settle();

  // ===== 单任务：长标题不挤坏左侧下载区 =====
  emit(jobId(2), { state: "cancelled", message: "cancelled" });
  await settle();
  assert(store.activeJobs.length === 1, "取消的任务退出后台");
  assert(statusButton()?.textContent?.includes("星の帰り道") && statusButton()?.textContent?.includes("第 1 页") && statusButton()?.textContent?.includes("翻译中"), "只剩一个任务时恢复单任务样式");
  const downloadLink = host.querySelector<HTMLElement>(".status-link")!;
  assert(downloadLink.getBoundingClientRect().height <= 32, "左侧下载区保持单行");
  const downloadLabels = [...downloadLink.querySelectorAll<HTMLElement>(":scope > span:not(.task-summary)")];
  assert(downloadLabels.length > 0 && downloadLabels.every(el => el.getBoundingClientRect().height <= 20), "下载区固定文案不换行");

  // ===== 完成：终态不占位 =====
  emit(jobId(1), { state: "completed", message: "completed", counts: { ...EMPTY_COUNTS, translated: 3, reviewed: 3 } });
  await settle();
  assert(store.activeJobs.length === 0, "完成任务退出后台");
  assert(!statusButton(), "任务结束底栏入口退场");
  assert(store.jobFor(1)?.state === "completed", "终态快照保留供浮层回看");

  // ===== 窄窗短文案 =====
  await store.start(novel(3, "短"));
  await store.start(novel(4, "短"));
  emit(jobId(3), { state: "running", phase: "translate" });
  emit(jobId(4), { state: "running", phase: "translate" });
  await settle();
  assert(host.querySelector(".translation-short")?.textContent?.includes("2 个翻译中"), "窄窗提供计数短文案");
  emit(jobId(3), { state: "completed" });
  emit(jobId(4), { state: "completed" });
  await settle();
  assert(store.activeJobs.length === 0, "完成任务全部退场");

  // ===== 单任务长标题：宽度受限、左侧下载区不换行 =====
  const longTitle = "『催眠術で家に遊びに来た妹の友人たちの常識を捻じ曲げてパコハメしまくりのドスケベお泊り会をエンジョイする話』";
  await store.start(novel(5, longTitle));
  await settle();
  const single = statusButton()!;
  const bar = host.querySelector<HTMLElement>(".download-status")!;
  assert(single.getBoundingClientRect().width <= bar.getBoundingClientRect().width * 0.56, "单任务标题限制在底栏宽度的 55% 内");
  const link = host.querySelector<HTMLElement>(".status-link")!;
  assert(link.getBoundingClientRect().height <= 32, "左侧下载区保持单行，不被长标题挤换行");
  emit(jobId(5), { state: "cancelled" });
  await settle();
  assert(store.activeJobs.length === 0, "长标题任务结束");

  // ===== 多小说任务状态互不覆盖 =====
  await store.start(novel(6, "甲"), false);
  await store.start(novel(7, "乙"), false);
  emit(jobId(6), { state: "needs_attention", counts: { ...EMPTY_COUNTS, translated: 2, needs_repair: 1 } });
  emit(jobId(7), { state: "running", phase: "review", counts: { ...EMPTY_COUNTS, translated: 3, reviewed: 1 } });
  await settle();
  assert(store.activeFor(6)?.state === "needs_attention" && store.activeFor(6)?.counts.needs_repair === 1, "甲小说待处理状态独立");
  assert(store.activeFor(7)?.state === "running" && store.activeFor(7)?.counts.reviewed === 1, "乙小说进度不被甲覆盖");
  assert(statusButton()?.textContent?.includes("后台 2 个翻译任务") && !statusButton()?.textContent?.includes("正在进行"), "含待处理任务时计数不冒充进行中");
  statusButton()!.click(); await settle();
  const needsItem = [...host.querySelectorAll<HTMLElement>(".picker-item")].find(el => el.textContent?.includes("甲"));
  assert(needsItem?.textContent?.includes("需要处理"), "菜单区分需要处理状态");
  assert(statusButton()?.textContent?.includes("后台 2 个翻译任务"), "计数入口保持完整");

  document.querySelector<HTMLOutputElement>("#translation-tasks-result")!.textContent = "PASS：Job 快照按小说隔离、seq 追赶丢弃旧事件、重复启动合并、底栏单/多任务形态、暂停/待处理不冒充进行中、终态退场、长标题不挤坏下载区、选择菜单焦点与跳转";
}
run().catch(error => {
  document.querySelector<HTMLOutputElement>("#translation-tasks-result")!.textContent = `FAIL: ${error.message}`;
  console.error(error);
});
