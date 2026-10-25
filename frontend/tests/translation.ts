/** 整篇翻译 Job 的离线验收：真实阅读器 + 真实浮层 + 设置面板；只使用 mock IPC，不访问模型服务。 */
import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter } from "vue-router";
import Novel from "../src/views/browse/BrowseNovelView.vue";
import Panel from "../src/components/settings/SettingsPanel.vue";
import { useSettingsStore } from "../src/stores/settings";
import { useTranslationStore } from "../src/stores/translation";
import type { TranslationConstraints, TranslationJobEvent, TranslationJobSnapshot, TranslationResult } from "../src/api/translation";
import zhCN from "../src/locales/zh-CN";
import "../src/styles/main.css";
import "../src/material";
import enUS from "../src/locales/en-US";

function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const settle = async () => { await nextTick(); await new Promise(resolve => setTimeout(resolve, 30)); };
const pinia = createPinia();
const settings = useSettingsStore(pinia);
const i18n = createI18n({ legacy: false, locale: "zh-CN", messages: { "zh-CN": zhCN, "en-US": enUS } });
const router = createRouter({ history: createMemoryHistory(), routes: [{ path: "/", component: { template: "<div/>" } }, { path: "/browse/search", component: { template: "<div/>" } }] });

// ===== mock IPC：Job 快照 + translation://job 事件 =====

const EMPTY_COUNTS = { pages: 2, chunks: 0, translated: 0, reviewed: 0, needs_repair: 0, terms: 0 };
const EMPTY_USAGE = { input: 0, output: 0, total: 0 };
let jobSeq = 0;
let job: TranslationJobSnapshot | null = null;
let result: TranslationResult | null = null;
let startCalls = 0;
let lastStartForce = false;
let lastStartNovel: Record<string, any> | null = null;
let startMode: "ok" | "already" | "error" = "ok";
let pauseCalls = 0;
let resumeCalls = 0;
let cancelCalls = 0;
let confirmCalls = 0;
let lastConfirmed: { terms: any[]; style: string } | null = null;
let constraintsAwaiting = true;
let missingConstraints = false;
let missingIssues = false;
let probeModels: string[] | string = [];
let probeTestOk = true;
let lastProbeUrl = "";
let lastProbeFormat = "";
let capturedSettings: Record<string, unknown> = {};
// 会话级诊断开关：默认关闭；diagnosticFail 用于验收「写入失败回滚 + 可读错误」。
let diagnosticEnabled = false;
let diagnosticFail = false;
let diagnosticWrites = 0;

const book = { bible: { style: "克制", terms: [] }, pages: {} };
const jobListeners = new Set<(payload: TranslationJobEvent) => void>();
const callbacks = new Map<number, (event: unknown) => void>();
let callbackSeq = 0;

function newJob(): TranslationJobSnapshot {
  return {
    job_id: "20261024000000-abcdef",
    novel_id: 42,
    state: "queued",
    phase: "prepare_source",
    counts: { ...EMPTY_COUNTS, pages: 2 },
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

/** 发一条与后端同构的事件（seq 单调，快照与事件同步更新）。 */
function emitJob(patch: Partial<TranslationJobSnapshot>): void {
  assert(job, "事件前必须先有任务快照");
  jobSeq += 1;
  job = { ...job!, ...patch, seq: jobSeq };
  const payload: TranslationJobEvent = {
    seq: job.seq, job_id: job.job_id, novel_id: job.novel_id, state: job.state, phase: job.phase,
    counts: job.counts, current: job.current, usage: job.usage, elapsed_ms: job.elapsed_ms,
    preview: job.preview, message: job.message, error_code: job.error_code,
  };
  for (const listener of [...jobListeners]) (listener as unknown as (event: unknown) => void)({ event: "translation://job", id: 1, payload });
}

function constraints(): TranslationConstraints {
  return {
    version: 1,
    terms: constraintsAwaiting
      ? [{ source_term: "アリス", locked_translation: "爱丽丝", term_type: "person", aliases: [], notes: "" }]
      : [{ source_term: "アリス", locked_translation: "爱丽丝", term_type: "person", aliases: [], notes: "" }],
    style: "克制、留白的叙事文风",
    conflicts: constraintsAwaiting ? [{ source_term: "星の帰り道", reason: "与既有译名冲突" }] : [],
    unresolved: [],
    frozen: !constraintsAwaiting,
    awaiting_confirmation: constraintsAwaiting,
  };
}

function previewFor(chunkId: string) {
  const order = Number.parseInt(chunkId.slice(1), 10) || 1;
  const translated = order === 1;
  const segmentId = `p001-l00${order}-s0`;
  return {
    chunk_id: chunkId, order,
    segments: [{ id: segmentId, page: 1, line: order, text: `第 ${order} 块原文`, kind: "text" }],
    translated: translated ? [{ id: segmentId, text: `第 ${order} 块译文` }] : [],
    context_prev: "", context_next: "", draft_version: translated ? 1 : 0,
    reviewed: translated, needs_repair: false, review: null,
  };
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
      if (callback) jobListeners.add(callback as unknown as (payload: TranslationJobEvent) => void);
      return ++callbackSeq;
    }
    if (command === "plugin:event|unlisten") return null;
    if (command === "settings_get") return { ...settings.settings, translation_key_configured: true };
    if (command === "settings_save") { capturedSettings = args.settings; return { status: "success" }; }
    if (command === "browse_work_detail") return { detail_kind: "novel", item: { id: 42, kind: "novel", title: "星の帰り道 · 很长的小说标题用于窗口验收", author_id: 1, author_name: "作者", tags: ["同人", "冒险"], description: "<p>星空下的重逢。</p>", page_count: 2, x_restrict: 0 }, content: "[chapter:序章]\n\nアリスが顔を上げた。[rb:星>ほし]\n[uploadedimage:1]\n[newpage]彼女は微笑んだ。", embedded_images: { "1": "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='120' height='60'><rect fill='gray' width='120' height='60'/></svg>" }, bookmarkState: null };
    if (command === "browse_related") return { items: [], next_page: null };
    if (command === "novel_translation_get") return book;
    if (command === "novel_translation_jobs") return { seq: jobSeq, jobs: job ? [job] : [] };
    if (command === "novel_translation_status") {
      if (!job) throw new Error("任务不存在");
      return job;
    }
    if (command === "novel_translation_result") return result;
    if (command === "novel_translation_start") {
      startCalls++;
      lastStartForce = args.force;
      lastStartNovel = args.novel;
      if (startMode === "error") throw new Error("请先配置翻译 API Key");
      if (startMode === "already") {
        const empty = { ...newJob(), job_id: "", already_target_language: true, target_language: "zh-CN" };
        return { job: empty, already_target_language: true };
      }
      if (!job) job = newJob();
      return { job, already_target_language: false };
    }
    if (command === "novel_translation_pause") {
      pauseCalls++;
      assert(job, "暂停前有任务");
      job = { ...job!, state: "paused", message: "paused" };
      return null;
    }
    if (command === "novel_translation_resume") {
      resumeCalls++;
      assert(job, "继续前有任务");
      job = { ...job!, state: "running", message: "resumed" };
      return null;
    }
    if (command === "novel_translation_cancel") {
      cancelCalls++;
      assert(job, "取消前有任务");
      job = { ...job!, state: "cancelled", message: "cancelled" };
      return null;
    }
    if (command === "novel_translation_confirm") {
      confirmCalls++;
      lastConfirmed = { terms: args.terms, style: args.style };
      constraintsAwaiting = false;
      if (job) job = { ...job, state: "running", message: "confirmed" };
      return null;
    }
    if (command === "novel_translation_preview") return previewFor(args.chunkId);
    if (command === "novel_translation_constraints") {
      if (missingConstraints) throw new Error("Command novel_translation_constraints not found");
      return constraints();
    }
    if (command === "novel_translation_issues") {
      if (missingIssues) throw new Error("Command novel_translation_issues not found");
      return {
        issues: [
          { severity: "minor", chunk: "c0001", segment: "p001-l001-s0", kind: "style", detail: "语气与前后段落不一致。" },
          { severity: "major", chunk: "c0002", segment: "p001-l002-s0", kind: "terminology", detail: "术语未按冻结译名输出。" },
        ],
        affected_chunks: ["c0001", "c0002"],
        unresolved_chunks: ["c0002"],
        reviewed: 1,
        total: 3,
      };
    }
    if (command === "translation_models") {
      lastProbeUrl = args.probe?.api_url ?? "";
      lastProbeFormat = args.probe?.format ?? "";
      return typeof probeModels === "string" ? Promise.reject(probeModels) : [...probeModels];
    }
    if (command === "translation_test") return probeTestOk ? Promise.resolve() : Promise.reject("模拟检测失败");
    if (command === "translation_diagnostic_status") return diagnosticEnabled;
    if (command === "translation_set_diagnostic") {
      diagnosticWrites++;
      if (diagnosticFail) return Promise.reject("模拟诊断开关失败");
      diagnosticEnabled = args.enabled === true;
      return diagnosticEnabled;
    }
    return {};
  },
  convertFileSrc: (url: string) => url,
};

async function run() {
  await router.push("/");
  const host = document.createElement("main");
  host.className = "app-content";
  host.style.cssText = "height:calc(100vh - 24px);padding:24px;container-type:size;overflow:hidden";
  document.body.append(host);
  const app = createApp(Novel, { kind: "novel", id: 42 }).use(pinia).use(i18n).use(router);
  app.mount(host); await settle(); await settle();
  const store = useTranslationStore(pinia);

  const popover = () => host.querySelector<HTMLElement>(".translation-popover");
  const status = () => host.querySelector(".translation-status")?.textContent?.trim() ?? "";
  const button = (label: string) => [...host.querySelectorAll<HTMLElement>(".panel-actions md-filled-button, .panel-actions md-outlined-button, .panel-actions md-text-button, .translation-mode, .panel-body md-filled-button, .panel-body md-outlined-button, .panel-body md-text-button")]
    .find(el => el.textContent?.trim() === label);
  const click = async (label: string) => { const el = button(label); assert(el, `找到按钮 ${label}（现有：${[...host.querySelectorAll(".panel-actions button, md-filled-button, md-outlined-button, md-text-button")].map(el => el.textContent?.trim()).join("/")}）`); el.click(); await settle(); };
  const tab = async (label: string) => { const el = [...host.querySelectorAll<HTMLElement>(".panel-tab")].find(node => node.textContent?.trim() === label); assert(el, `找到 tab ${label}`); el.click(); await settle(); };
  const selectPage = async (page: number) => { const select = host.querySelector<HTMLSelectElement>(".page-select"); assert(select, "分页存在"); select.value = String(page); select.dispatchEvent(new Event("change", { bubbles: true })); await settle(); };
  let openDiag = "";
  const openPanel = async () => {
    if (popover()) return;
    const el = host.querySelector<HTMLElement>(".translation-trigger");
    assert(el, "翻译入口存在");
    el.click();
    await settle();
    openDiag = `expanded=${el.getAttribute("aria-expanded")}`;
    assert(popover(), `面板应打开（${openDiag}）`);
  };

  // ===== 入口与浮层骨架 =====
  const trigger = host.querySelector<HTMLElement>(".translation-trigger");
  assert(trigger?.querySelector("svg") && !popover(), "底栏 SVG 入口，初始不展开面板");
  await openPanel();
  assert(document.activeElement === popover(), `打开后焦点进入浮层（active=${document.activeElement?.tagName}.${(document.activeElement as HTMLElement | null)?.className ?? ""}，${openDiag}）`);
  assert(popover()!.textContent?.includes("整篇作品 · 2 页 · 0 块"), "头部显示整篇范围、页数与块数");
  assert(host.querySelectorAll(".panel-tab").length === 4, "浮层提供四个 tab");
  assert([...host.querySelectorAll(".panel-tab")].map(el => el.textContent?.trim()).join("/") === "状态/术语与风格/预览/问题", "tab 文案与顺序");
  assert(status().includes("尚未翻译"), "未译状态可读");
  // tab 键盘：右方向键切换并把焦点移到新 tab。
  const firstTab = host.querySelector<HTMLElement>(".panel-tab")!;
  firstTab.focus();
  firstTab.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true })); await settle();
  assert(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent?.trim() === "术语与风格", "方向键切换 tab");
  assert((document.activeElement as HTMLElement | null)?.textContent?.trim() === "术语与风格", "切换后焦点落到新 tab");
  await tab("状态");
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); await settle();
  assert(!popover(), "Esc 关闭面板且不取消任务");
  assert(document.activeElement === trigger, "关闭后焦点回到入口");
  await openPanel();
  assert(host.querySelectorAll(".translation-popover").length === 1 && host.querySelectorAll(".translation-backdrop").length === 1, "面板与遮罩成对渲染");
  host.querySelector<HTMLElement>(".translation-backdrop")!.click(); await settle();
  assert(!popover(), "点击外部关闭面板");

  // ===== 启动整篇翻译 =====
  await openPanel();
  await click("开始翻译");
  assert(startCalls === 1 && lastStartForce === false, "首次启动不带 force");
  assert(lastStartNovel?.tags?.includes("同人") && lastStartNovel?.description === "星空下的重逢。", "整篇请求包含元信息与纯文本简介");
  assert(String(lastStartNovel?.content).includes("[chapter:序章]") && String(lastStartNovel?.content).includes("[newpage]"), "整篇原文一次提交");
  assert(status().includes("排队等待中"), "启动后显示排队状态");
  assert(host.querySelector(".translation-trigger-wrap > .translation-indicator.is-busy"), "排队/进行中入口显示 primary 状态点");
  assert(popover()!.textContent?.includes("整篇作品 · 2 页 · 0 块"), "块数未知时仍显示页数");

  // 事件：进入 translate 阶段（计数、当前页块、摘要、用量）。
  emitJob({
    state: "running", phase: "translate", elapsed_ms: 72000,
    counts: { pages: 2, chunks: 3, translated: 1, reviewed: 0, needs_repair: 0, terms: 4 },
    current: { page: 2, chunk_id: "c0002", role: "translator" },
    usage: { input: 8213, output: 1764, total: 9977 },
    preview: { summary: "星光洒落在回家的路上，她的脚步很轻。", reasoning: "", owner: { page: 2, chunk_id: "c0002", role: "translator" } },
  });
  await settle();
  assert(status().includes("翻译中"), "运行中状态摘要更新");
  assert(popover()!.textContent?.includes("整篇作品 · 2 页 · 3 块"), "事件到达后头部显示块数");
  assert(popover()!.textContent?.includes("已译 1/3"), "状态 tab 显示已译计数");
  assert(popover()!.textContent?.includes("已审 0/3"), "状态 tab 显示已审计数");
  assert(popover()!.textContent?.includes("当前：第 2 页 · 分块 c0002"), "状态 tab 显示当前页与块");
  assert(popover()!.textContent?.includes("星光洒落在回家的路上"), "状态 tab 显示摘要");
  assert(popover()!.textContent?.includes("用量 9,977 tokens"), "状态 tab 显示 token 用量");
  assert(popover()!.textContent?.includes("已用 1′12″"), "状态 tab 显示已用时间");
  assert(popover()!.querySelector(".progress-summary"), "状态 tab 有流式摘要行");
  assert(popover()!.textContent?.includes("翻译 c0002"), "预览行标注子任务归属与块");
  assert(!popover()!.querySelector(".progress-reasoning"), "有正文摘要时不显示思考行");
  // 尾部窗口：只展示最新生成的 60 字符。
  emitJob({ preview: { summary: `最旧的开头${"中间".repeat(80)}最新结尾`, reasoning: "" } });
  await settle();
  assert(!popover()!.textContent?.includes("最旧的开头"), "预览不再冻结在最早的开头");
  assert(popover()!.textContent?.includes("最新结尾"), "预览展示最新生成的尾部");
  // 思考期：无摘要时显示思考尾窗。
  emitJob({ preview: { summary: "", reasoning: "核对术语与语气…" } });
  await settle();
  assert(popover()!.querySelector(".progress-reasoning")?.textContent?.includes("核对术语与语气"), "无摘要时显示思考尾窗");

  // ===== U4b 命令缺失时的可读降级（不影响其它 tab） =====
  missingConstraints = true;
  missingIssues = true;
  await tab("术语与风格");
  assert(popover()!.textContent?.includes("暂无术语数据"), "约束命令缺失时术语 tab 显示暂无数据");
  await tab("问题");
  assert(popover()!.textContent?.includes("暂无问题数据"), "问题命令缺失时问题 tab 显示暂无数据");
  missingConstraints = false;
  missingIssues = false;
  await tab("状态");

  // ===== 术语与风格 =====
  await tab("术语与风格");
  const termInput = popover()!.querySelector<HTMLInputElement>(".term-input");
  assert(termInput?.value === "爱丽丝", "确认态展示可编辑术语");
  assert(popover()!.textContent?.includes("アリス"), "source_term 只读展示");
  assert(popover()!.textContent?.includes("术语冲突 1 条"), "冲突条数可见");
  termInput!.value = "艾丽丝";
  termInput!.dispatchEvent(new Event("input", { bubbles: true })); await settle();
  const styleInput = popover()!.querySelector<HTMLTextAreaElement>(".style-input");
  assert(styleInput?.value === "克制、留白的叙事文风", "文风文本域回显");
  styleInput!.value = "克制、留白，短句优先。";
  styleInput!.dispatchEvent(new Event("input", { bubbles: true })); await settle();
  await click("确认并继续");
  assert(confirmCalls === 1, "确认并继续调用 confirm");
  assert(lastConfirmed?.terms[0].locked_translation === "艾丽丝" && lastConfirmed?.style.includes("短句优先"), "用户修改优先提交");
  assert(popover()!.textContent?.includes("已冻结 · 版本 1"), "确认后转为冻结只读展示");
  assert(!popover()!.querySelector(".term-input"), "冻结后术语不可编辑");

  // ===== 预览 =====
  await tab("预览");
  await settle(); await settle();
  const chunks = [...popover()!.querySelectorAll<HTMLElement>(".chunk-item")];
  assert(chunks.length === 3, "按分块数生成块列表");
  assert(popover()!.textContent?.includes("已译") && popover()!.textContent?.includes("未译"), "块列表区分已译与未译");
  const second = chunks.find(el => el.textContent?.includes("c0002"));
  assert(second, "存在 c0002");
  second!.click(); await settle();
  assert(popover()!.textContent?.includes("第 2 块原文"), "选中块展示原文");
  assert(popover()!.textContent?.includes("该块尚未产出译文"), "未译块明确标示，不冒充已译");
  assert(popover()!.textContent?.includes("未审查"), "未审查块明确标示");
  const first = chunks.find(el => el.textContent?.includes("c0001"));
  first!.click(); await settle();
  assert(popover()!.textContent?.includes("第 1 块译文") && popover()!.textContent?.includes("已审查"), "已译块展示译文与审查态");

  // ===== 问题 =====
  await tab("问题");
  await settle();
  assert(popover()!.textContent?.includes("未解决块 1 / 共 3 块"), "问题汇总显示未解决块数");
  assert(popover()!.textContent?.includes("未解决分块：c0002"), "未解决块置顶提示");
  const issueRows = [...popover()!.querySelectorAll<HTMLElement>(".issue-row")];
  assert(issueRows.length === 2, "问题逐条列出");
  assert(issueRows[0].textContent?.includes("c0002") && issueRows[0].textContent?.includes("主要"), "未解决问题排在前面且严重度可读");
  assert(issueRows[1].textContent?.includes("c0001"), "已解决/轻微问题在后");

  // ===== 暂停 / 继续 / 取消 =====
  await tab("状态");
  await click("暂停派发");
  assert(pauseCalls === 1 && status().includes("已暂停"), "暂停派发立即生效");
  assert(button("继续"), "暂停后提供继续");
  await click("继续");
  assert(resumeCalls === 1 && status().includes("翻译中"), "继续恢复运行");
  await click("取消任务");
  assert(cancelCalls === 1 && status().includes("已取消"), "取消任务立即生效");
  assert(button("重新翻译"), "取消后可重新发起");
  assert(!host.querySelector(".translation-trigger-wrap > .translation-indicator.is-busy"), "取消后入口不再是进行中");

  // ===== 关闭面板不取消任务 =====
  const cancelsBefore = cancelCalls;
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); await settle();
  assert(!popover() && cancelCalls === cancelsBefore, "关闭浮层不取消任务");
  assert(store.jobFor(42)?.state === "cancelled", "关闭后任务状态保留在 store");

  // ===== 完成：事件驱动刷新投影结果 =====
  startMode = "ok";
  await openPanel();
  emitJob({
    state: "completed", phase: "publish", elapsed_ms: 300000,
    counts: { pages: 2, chunks: 3, translated: 3, reviewed: 3, needs_repair: 0, terms: 4 },
    current: null,
    usage: { input: 20000, output: 5000, total: 25000 },
    preview: { summary: "", reasoning: "" },
  });
  result = {
    novel_id: 42, job_id: "20261024000000-abcdef", target_language: "zh-CN", status: "complete", unresolved: 0,
    pages: { "1": [{ line: 0, text: "序章" }, { line: 2, text: "爱丽丝抬起了头。" }], "2": [{ line: 0, text: "她微笑了。" }] },
  };
  await settle(); await settle();
  assert(status().includes("整篇译文已保存") && status().includes("25,000 tokens"), "完成态回显整篇用量");
  assert(host.querySelector(".translation-trigger-wrap > .translation-indicator.is-done"), "完成态入口显示成功状态点");
  assert(host.querySelectorAll(".translated-text").length === 2, `完成事件触发结果刷新并渲染当前页译文（texts=${host.querySelectorAll(".translated-text").length}，state=${store.jobFor(42)?.state}，version=${store.lastResultVersion}，novel=${store.lastResultNovelId}，content=${host.querySelector(".novel-content")?.textContent?.slice(0, 80)}）`);
  await selectPage(2);
  assert(host.querySelector<HTMLElement>(".translated-text")?.textContent === "她微笑了。", "第二页译文来自同一份投影");
  await selectPage(1);
  assert(host.querySelector(".novel-content script") === null, "译文按纯文本渲染");
  assert(host.querySelector(".translated-text")?.getAttribute("lang") === "zh-CN", "译文声明目标语言");
  // 模式切换保留（仅译文仍显示图片，不重复原文）。
  await click("仅译文");
  assert(!host.querySelector("ruby") && host.querySelectorAll(".novel-image").length === 1, "仅译文隐藏原文但图片保留");
  await click("仅原文");
  assert(!host.querySelector(".translated-text") && host.querySelector("ruby"), "仅原文恢复注音");
  await click("双语");
  assert(host.querySelectorAll(".translated-text").length === 2, "双语同时显示原文与译文");

  // ===== 需要处理：部分完成不冒充通过 =====
  emitJob({ state: "needs_attention", phase: "review", counts: { pages: 2, chunks: 3, translated: 3, reviewed: 2, needs_repair: 1, terms: 4 }, error_code: "review_issues" });
  result = { novel_id: 42, job_id: "20261024000000-abcdef", target_language: "zh-CN", status: "needs_attention", unresolved: 1, pages: result!.pages };
  await settle(); await settle();
  assert(status().includes("需要处理"), "需要处理状态可读");
  assert(host.querySelector(".translation-trigger-wrap > .translation-indicator.is-error"), "待处理入口显示 error 状态点");
  await openPanel();
  assert(popover()!.textContent?.includes("需要处理：1 处分块未解决"), "状态 tab 提示未解决块");
  assert(popover()!.textContent?.includes("错误代码：review_issues"), "安全错误代码可见");
  assert(button("继续"), "需要处理时提供继续");
  assert(!button("开始翻译"), "活跃任务不提供重复启动");
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); await settle();

  // ===== 原文已是目标语言 =====
  // 先结束待处理任务，回到可重新发起的终态。
  await openPanel();
  if (button("取消任务")) await click("取消任务");
  startMode = "already";
  const startsBefore = startCalls;
  await click("重新翻译");
  assert(startCalls === startsBefore + 1, "重新翻译走 store.start");
  assert(status().includes("无需翻译"), "目标语言一致时提示无需翻译");
  assert(button("仍然翻译"), "无需翻译时按钮变为仍然翻译");
  await click("仍然翻译");
  assert(lastStartForce === true, "仍然翻译显式 force");
  startMode = "ok";

  // ===== 启动失败可见，且不破坏已有译文 =====
  startMode = "error";
  await click("仍然翻译");
  assert(status().includes("请先配置翻译 API Key"), "启动失败原因可见");
  assert(host.querySelector(".translation-trigger-wrap > .translation-indicator.is-error"), "失败入口显示 error 状态点");
  startMode = "ok";
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); await settle();

  // ===== 缓存清理：译文与任务状态一起失效 =====
  await openPanel();
  window.dispatchEvent(new Event("pixiv-tool:translation-cache-cleared")); await settle();
  assert(!host.querySelector(".translated-text"), "缓存清理后译文失效");
  assert(status().includes("尚未翻译"), `缓存清理后状态回到未翻译（实际：${status()}）`);
  assert(store.jobs.size === 0 && store.activeJobs.length === 0, "缓存清理后不保留幽灵任务");

  const panelHost = document.createElement("div"); panelHost.style.cssText = "display:none;padding:24px;max-width:640px"; document.body.append(panelHost);
  const panelApp = createApp(Panel, { section: "translation" }).use(pinia).use(i18n);
  const renderErrors: unknown[] = [];
  panelApp.config.errorHandler = error => { renderErrors.push(error); };
  const panel = panelApp.mount(panelHost) as unknown as { save(): Promise<boolean>; reset(): void; hasUnsaved(): boolean };
  await settle();
  // ===== 一级 / 二级共用的导航助手：按实际 DOM 层级显式驱动，避免推断状态 =====
  const advancedPage = () => panelHost.querySelector("#translation-advanced-page");
  const level = () => {
    if (!advancedPage()) return "basic";
    if (panelHost.querySelector(".advanced-index")) return "index";
    return "detail";
  };
  const findOutlinedButton = (label: string) => [...panelHost.querySelectorAll<HTMLElement>("md-outlined-button,md-filled-button")].find(el => el.textContent?.trim() === label);  /** 一级 → 二级分类首页（幂等）。 */
  const toIndex = async () => {
    if (level() === "basic") {
      const entry = panelHost.querySelector<HTMLElement>(".translation-advanced-entry");
      assert(entry, "一级存在高级设置入口");
      entry.click(); await settle();
    }
    if (level() === "detail") {
      panelHost.querySelector<HTMLElement>(".advanced-back-index")!.click(); await settle();
    }
    assert(level() === "index", `二级分类首页（当前 ${level()}）`);
  };
  /** 二级任意层级 → 一级（幂等）。 */
  const toService = async () => {
    await toIndex();
    panelHost.querySelector<HTMLElement>(".advanced-back-service")!.click(); await settle();
    assert(level() === "basic", `返回小说翻译回到一级（当前 ${level()}）`);
  };
  const openCategory = async (title: string) => {
    await toIndex();
    const item = [...panelHost.querySelectorAll<HTMLElement>(".advanced-index-item")].find(el => el.querySelector(".advanced-index-title")?.textContent?.trim() === title);
    assert(item, `二级分类入口 ${title}`);
    item.click(); await settle();
    assert(level() === "detail", `进入分类 ${title}`);
  };
  const inputIn = async (id: string, value: string) => { const element = panelHost.querySelector<HTMLElement & { value: string }>(`#${id}`); assert(element, `字段 ${id}`); element.value = value; element.dispatchEvent(new Event("input", { bubbles: true })); await settle(); };
  const setExtra = async (value: string) => { await openCategory("请求扩展"); await inputIn("translation-agent-extra", value); };
  const advancedSummary = () => panelHost.querySelector(".advanced-category")?.textContent?.trim() ?? "";
  const entryMeta = () => panelHost.querySelector(".translation-advanced-meta")?.textContent?.trim() ?? "";
  const entryErrors = () => panelHost.querySelector(".translation-advanced-error")?.textContent?.trim() ?? "";

  for (const locale of ["zh-CN", "en-US"] as const) {
    i18n.global.locale.value = locale; await settle();
    assert(!renderErrors.length, `小说翻译设置渲染异常（${locale}）：${renderErrors.map(String).join("；")}`);
    for (const id of ["translation-format", "translation-url", "translation-key", "translation-model", "translation-target-language", "translation-confirm-before-start"]) {
      assert(panelHost.querySelector(`#${id}`), `小说翻译一级字段 ${id} 在 ${locale} 下可见`);
    }
    // 一级不得出现运行参数墙或原始 JSON（计划 §8.1 / 验收 8）
    assert(!panelHost.querySelector("#translation-json") && !panelHost.querySelector("#translation-agent-extra"), `一级不显示原始 JSON（${locale}）`);
    assert(!panelHost.querySelector('[id^="translation-agent-"]:not(#translation-agent-replay-unknown)'), `一级不显示运行参数（${locale}）`);
    assert(panelHost.querySelectorAll("fieldset.translation-group").length === 2, `小说翻译一级分为两组（${locale}）`);
    assert(entryMeta().length > 0, `高级入口显示推荐参数摘要（${locale}）`);
    await toIndex();
    assert(panelHost.querySelectorAll(".advanced-index-item").length === 6, `二级分类入口为六个（${locale}）`);
    await openCategory(locale === "zh-CN" ? "请求扩展" : "Request extensions");
    const extraLabel = panelHost.querySelector('[for="translation-agent-extra"]');
    assert(extraLabel?.parentElement?.textContent?.includes('{"reasoning_effort":"none"}'), `JSON 示例在 ${locale} 下完整显示`);
    await toService();
  }
  i18n.global.locale.value = "zh-CN"; await settle();
  assert([...panelHost.querySelectorAll("fieldset.translation-group > .translation-legend")].map(el => el.textContent?.trim()).join("/") === "服务连接/模型与语言", "一级子分区为服务连接 / 模型与语言");
  assert(panelHost.querySelectorAll('fieldset.translation-group[role="group"][aria-labelledby]').length === 2, "分组以 role=group + aria-labelledby 命名");
  const input = async (id: string, value: string) => { const element = panelHost.querySelector<HTMLElement & { value: string }>(`#${id}`); assert(element, `字段 ${id}`); element.value = value; element.dispatchEvent(new Event("input", { bubbles: true })); await settle(); };
  assert(panelHost.querySelector("#translation-key")?.getAttribute("type") === "password", "Key 为密码字段");
  assert(panelHost.querySelector(".translation-key-chip")?.textContent?.trim() === "已保存", "已存凭据在标签行显示状态胶囊");
  await setExtra("[]"); assert(!await panel.save(), "拒绝非对象 JSON");
  assert(advancedPage() && advancedSummary() === "请求扩展", "JSON 校验失败定位到请求扩展分类");
  await setExtra('{"reasoning_effort":"high","thinking":{"budget_tokens":2048}}');
  await toService();
  await input("translation-url", "https://example.com/v1"); await input("translation-model", "test-model"); await input("translation-key", "test-only-key");
  assert(panel.hasUnsaved(), "敏感草稿进入脏检查"); assert(await panel.save(), "合法配置保存");
  assert(!advancedPage() && panelHost.querySelector(".translation-advanced-entry"), "保存成功后回到一级");
  assert(capturedSettings.translation_api_key === "test-only-key", "Key 仅通过单次写入参数发送");
  assert(!(settings.settings as unknown as Record<string, unknown>).translation_api_key && !(panelHost.querySelector("#translation-key") as any).value, "Store 和保存后字段不留 Key");
  assert(!panel.hasUnsaved(), "保存后清除脏状态");
  await panel.save(); assert(!("translation_api_key" in capturedSettings), "空白字段不覆盖已存 Key");
  [...panelHost.querySelectorAll<HTMLElement>("md-text-button")].find(el => el.textContent?.trim() === "清除已保存的 Key")?.click(); await settle();
  assert(panelHost.querySelector(".translation-key-chip")?.textContent?.trim() === "保存时清除 Key", "清除动作在胶囊上回显待清除状态");
  assert(panel.hasUnsaved() && await panel.save() && capturedSettings.translation_api_key === "", "显式清除凭据");
  await input("translation-key", "discard-only"); panel.reset(); await settle(); assert(!panel.hasUnsaved(), "取消清空敏感草稿");

  // ===== 设置分层（验收 8）：草稿跨级保留、分组 / 全部恢复、隐藏项错误定位 =====
  assert(entryMeta() === "已修改 1 项", `已保存的高级 JSON 让一级入口显示已修改项数（实际 ${entryMeta()}）`);
  await openCategory("并发与任务预算");
  await inputIn("translation-agent-global-concurrency", "9");
  assert(panelHost.querySelector(".field-hint")?.textContent?.includes("自动（32 + 16 × 分块数）"), "调用预算为 0 时显示自动公式");
  await toService();
  assert(entryMeta() === "已修改 2 项", `一级摘要随草稿更新（实际 ${entryMeta()}）`);
  await openCategory("并发与任务预算");
  assert((panelHost.querySelector("#translation-agent-global-concurrency") as unknown as { value: string }).value === "9", "一级 / 二级往返保留草稿");
  await inputIn("translation-agent-global-concurrency", "99");
  assert(panelHost.querySelector("#translation-agent-global-concurrency")?.hasAttribute("error"), "越界即时标红");
  await toService();
  assert(entryErrors().includes("1"), "一级入口显示待修正项数");
  // 面板平时 display:none，隐藏元素拿不到焦点；定位检查段临时可见，检查完恢复。
  panelHost.style.display = "block"; await settle();
  assert(!await panel.save(), "隐藏分类的非法值阻止保存");
  await settle();
  assert(advancedPage() && advancedSummary() === "并发与任务预算", "保存失败定位到出错分类");
  const badField = panelHost.querySelector("#translation-agent-global-concurrency");
  await settle();
  // Material Web 的 text field 把焦点放在内部 input 上，因此接受「内部输入框获得焦点」。
  const innerInput = badField?.shadowRoot?.querySelector("input");
  assert(document.activeElement === badField || document.activeElement === innerInput || badField?.matches(":focus-within") || innerInput?.matches(":focus"),
    `保存失败聚焦到出错字段（active=${document.activeElement?.tagName}，字段=${badField?.tagName}）`);
  panelHost.style.display = "none"; await settle();
  await inputIn("translation-agent-global-concurrency", "9");
  const restoreGroup = findOutlinedButton("恢复本组推荐值"); assert(restoreGroup, "分类页有恢复本组推荐值"); restoreGroup.click(); await settle();
  assert((panelHost.querySelector("#translation-agent-global-concurrency") as unknown as { value: string }).value === "4", "恢复本组推荐值");
  assert(advancedSummary() === "并发与任务预算" && entryErrors() === "", "恢复本组后本组无待修正项");
  await toIndex();
  const restoreAllButton = findOutlinedButton("恢复全部推荐值"); assert(restoreAllButton, "分类首页有恢复全部推荐值"); restoreAllButton.click(); await settle();
  const restoreConfirm = findOutlinedButton("确认恢复"); assert(restoreConfirm, "恢复全部有确认按钮"); restoreConfirm.click(); await settle();
  await toService();
  assert(entryMeta() === "推荐参数", "恢复全部后一级摘要回到推荐参数");
  assert(!panelHost.querySelector(".translation-advanced-error"), "恢复全部后无待修正项");
  assert((panelHost.querySelector("#translation-url") as unknown as { value: string }).value === "https://example.com/v1", "恢复全部不改一级连接设置");
  panel.reset(); await settle(); assert(!panel.hasUnsaved(), "分层操作后的草稿可整体取消");

  // ===== 诊断开关：立即写后端、失败回滚并显示可读错误、不计入已修改项 =====
  const metaBeforeDiagnostics = entryMeta();
  await openCategory("诊断");
  const diagnosticsToggle = panelHost.querySelector<HTMLElement & { checked: boolean }>("#translation-agent-detailed-diagnostics");
  assert(diagnosticsToggle, "诊断分类有详细诊断开关");
  assert(diagnosticsToggle!.checked === false, "打开设置时按后端状态回显（默认关闭）");
  diagnosticsToggle!.checked = true; diagnosticsToggle!.dispatchEvent(new Event("change", { bubbles: true })); await settle();
  assert(diagnosticWrites === 1 && diagnosticEnabled, "切换开关立即写后端");
  await toService();
  assert(entryMeta() === metaBeforeDiagnostics, "诊断开关不计入「已修改 N 项」");
  assert(!panel.hasUnsaved(), "诊断开关不进入设置脏检查");
  await openCategory("诊断");
  diagnosticFail = true;
  const toggleAgain = panelHost.querySelector<HTMLElement & { checked: boolean }>("#translation-agent-detailed-diagnostics")!;
  toggleAgain.checked = false; toggleAgain.dispatchEvent(new Event("change", { bubbles: true })); await settle();
  assert(diagnosticWrites === 2 && diagnosticEnabled, "写入失败不改变后端状态");
  assert(panelHost.querySelector("#translation-agent-detailed-diagnostics")?.checked === true, "失败后开关回滚到开启");
  const diagnosticsError = panelHost.querySelector(".diagnostics-error");
  // 后端 reject 的可读消息原样展示；无消息时才用本地兜底文案。
  assert(diagnosticsError?.getAttribute("role") === "alert" && (diagnosticsError.textContent ?? "").trim().length > 0,
    `失败时显示可读错误（错误节点=${diagnosticsError ? diagnosticsError.textContent : "无"}）`);
  diagnosticFail = false;
  await toService();
  await openCategory("诊断");
  const toggleRestore = panelHost.querySelector<HTMLElement & { checked: boolean }>("#translation-agent-detailed-diagnostics")!;
  toggleRestore.checked = false; toggleRestore.dispatchEvent(new Event("change", { bubbles: true })); await settle();
  assert(!diagnosticEnabled && diagnosticWrites === 3, "失败后可重新切换成功");
  await toService();
  panel.reset(); await settle();

  // 获取模型 / 检测可用：用当前未保存草稿发探测请求，结果只进组件状态、不落 store。
  const findButton = (label: string) => [...panelHost.querySelectorAll<HTMLElement>("md-outlined-button,md-text-button")].find(el => el.textContent?.trim() === label);
  await input("translation-url", "https://api.example.com/v1"); await input("translation-key", "probe-key");
  probeModels = ["glm-model-b", "glm-model-a"];
  findButton("获取模型")!.click(); await settle();
  assert(lastProbeUrl === "https://api.example.com/v1", "模型请求使用未保存的 URL");
  const modelControl = panelHost.querySelector<HTMLElement & { value: string }>("#translation-model");
  assert(modelControl?.tagName === "MD-OUTLINED-SELECT" && panelHost.querySelectorAll("#translation-model").length === 1, "获取成功后文本框原位变下拉，不额外新增控件");
  assert(modelControl.textContent?.includes("glm-model-a") && modelControl.textContent?.includes("test-model"), "下拉列出获取到的模型并保留当前值");
  modelControl.value = "glm-model-a"; modelControl.dispatchEvent(new Event("change")); await settle();
  assert(modelControl.value === "glm-model-a", "选定模型即当前模型");
  // 接口协议：默认 Chat Completions；切换后 URL 占位、模型列表与探测草稿同步换协议。
  const formatSelect = panelHost.querySelector<HTMLElement & { value: string }>("#translation-format");
  assert(formatSelect?.tagName === "MD-OUTLINED-SELECT" && formatSelect.value === "chat_completions", "接口协议默认 Chat Completions");
  assert(lastProbeFormat === "chat_completions", "探测草稿带上当前协议");
  formatSelect.value = "anthropic"; formatSelect.dispatchEvent(new Event("change", { bubbles: true })); await settle();
  const urlField = panelHost.querySelector<HTMLElement & { placeholder: string }>("#translation-url");
  assert(urlField?.placeholder === "https://api.anthropic.com/v1", "切换协议后 URL 占位跟随");
  assert(panelHost.querySelector("#translation-model")?.tagName === "MD-OUTLINED-TEXT-FIELD", "切换协议重置已获取的模型列表");
  probeModels = ["claude-model-b", "claude-model-a"];
  findButton("获取模型")!.click(); await settle();
  assert(lastProbeFormat === "anthropic", "切换协议后探测使用新协议");
  formatSelect.value = "chat_completions"; formatSelect.dispatchEvent(new Event("change", { bubbles: true })); await settle();
  probeModels = ["glm-model-b", "glm-model-a"];
  findButton("获取模型")!.click(); await settle();
  findButton("检测可用")!.click(); await settle();
  const okStatus = panelHost.querySelector('[role="status"]');
  assert(okStatus?.textContent?.includes("模型可用") && okStatus.classList.contains("state-success"), "检测成功为绿色状态");
  probeTestOk = false; findButton("检测可用")!.click(); await settle();
  const failStatus = panelHost.querySelector('[role="alert"]');
  assert(failStatus?.textContent?.includes("模拟检测失败") && failStatus.classList.contains("state-error"), "检测失败为红色状态");
  findButton("手动输入模型 ID")!.click(); await settle();
  const manualModel = panelHost.querySelector<HTMLElement & { value: string }>("#translation-model");
  assert(manualModel?.tagName === "MD-OUTLINED-TEXT-FIELD" && manualModel.value === "glm-model-a", "手动输入回到文本框且保留模型");
  await input("translation-url", "");
  assert(findButton("检测可用")?.hasAttribute("disabled") && findButton("获取模型")?.hasAttribute("disabled"), "URL 为空禁用探测");
  // 目标语言：默认跟随界面语言，可手动指定并随保存落盘。
  const targetSelect = panelHost.querySelector<HTMLElement & { value: string }>("#translation-target-language");
  assert(targetSelect?.tagName === "MD-OUTLINED-SELECT" && targetSelect.value === "", "目标语言默认跟随界面语言");
  assert(targetSelect.textContent?.includes("跟随界面语言") && targetSelect.textContent?.includes("简体中文"), "跟随项回显当前界面语言");
  targetSelect.value = "en"; targetSelect.dispatchEvent(new Event("change")); await settle();
  assert(targetSelect.value === "en" && await panel.save(), "可手动指定目标语言并保存");
  assert(capturedSettings.translation_target_language === "en", "目标语言随整表保存写入");

  // 留真实组件供宽窄窗口、深浅主题与 hover/focus 验收；设置页停在「已获取模型 + 检测可用」态。
  probeTestOk = true;
  await input("translation-url", "https://api.example.com/v1");
  findButton("获取模型")!.click(); await settle();
  findButton("检测可用")!.click(); await settle();
  // 预览钩子：视觉验收时切换探测成败，观察绿/红状态与 hover 报错。
  Object.assign(window, { __probePreview: (ok: boolean) => { probeTestOk = ok; findButton("检测可用")?.click(); } });

  // 阅读器停在「已完成 + 双语」态，供人工视觉验收。
  startMode = "ok";
  window.dispatchEvent(new Event("pixiv-tool:translation-cache-cleared")); await settle();
  if (!job) job = newJob();
  job = { ...job, state: "completed", phase: "publish", counts: { pages: 2, chunks: 3, translated: 3, reviewed: 3, needs_repair: 0, terms: 4 }, usage: { input: 20000, output: 5000, total: 25000 } };
  result = {
    novel_id: 42, job_id: job.job_id, target_language: "zh-CN", status: "complete", unresolved: 0,
    pages: { "1": [{ line: 0, text: "序章" }, { line: 2, text: "爱丽丝抬起了头。" }], "2": [{ line: 0, text: "她微笑了。" }] },
  };
  emitJob({});
  await settle(); await settle();
  await selectPage(1);
  document.querySelector<HTMLButtonElement>("#preview-reader")!.onclick = () => { host.style.display = ""; panelHost.style.display = "none"; };
  document.querySelector<HTMLButtonElement>("#preview-settings")!.onclick = () => { host.style.display = "none"; panelHost.style.display = ""; };
  document.querySelector<HTMLButtonElement>("#preview-dark")!.onclick = () => document.documentElement.classList.add("dark");
  document.querySelector<HTMLButtonElement>("#preview-light")!.onclick = () => document.documentElement.classList.remove("dark");
  document.querySelector<HTMLElement>("#preview-controls")!.hidden = false;
  document.querySelector("#translation-result")!.textContent = "PASS：整篇 Job 浮层（四个 tab / 阶段与计数 / 术语确认与冻结 / 块预览未译标示 / 问题排序）、启动与 force、暂停继续取消、关闭不取消、完成与部分完成事件刷新投影、目标语言兜底、失败可见、缓存清理失效、状态点与焦点/Esc/外部关闭、设置一级两组分区与凭据状态胶囊、一级不含运行参数与原始 JSON、二级分类入口与返回、JSON 校验定位、跨级草稿保留、越界即时标红与错误计数、恢复本组/全部推荐值、诊断开关读写与失败回滚、凭据草稿、获取模型原位下拉与绿红检测状态、目标语言选择、接口协议默认值与切换";
}
run().catch(error => { document.querySelector("#translation-result")!.textContent = `FAIL: ${error instanceof Error && error.stack ? error.stack : error.message}`; console.error(error); });
