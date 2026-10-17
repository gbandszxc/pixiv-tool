/**
 * 多小说并行翻译与底栏后台指示验收（ADR 0031）：
 * store 的不同小说并发、同小说排队/同页 join，以及底栏单任务原样、多任务计数 + 选择菜单。
 * 只使用 mock IPC，不访问模型服务。
 */
import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter } from "vue-router";
import DownloadStatusBar from "../src/components/download/DownloadStatusBar.vue";
import { useTranslationStore } from "../src/stores/translation";
import type { NovelTranslationInput, PageTranslation } from "../src/api/translation";
import zhCN from "../src/locales/zh-CN";
import enUS from "../src/locales/en-US";
import "../src/styles/main.css";
import "../src/material";

function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const settle = async () => { await nextTick(); await new Promise(resolve => setTimeout(resolve, 30)); };

interface Pending { novelId: number; page: number; resolve: (result: PageTranslation) => void; reject: (error: string) => void; send: (raw: string) => void; }
const pending: Pending[] = [];
let calls = 0;

window.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string, args: Record<string, any>) => {
    if (command !== "novel_translate_page") return {};
    calls++;
    return new Promise<PageTranslation>((resolve, reject) => {
      pending.push({ novelId: args.novel.novel_id, page: args.page, resolve, reject, send: raw => args.progress.onmessage(raw) });
    });
  },
};

const novel = (id: number, title: string): NovelTranslationInput => ({ novel_id: id, title, tags: [], description: "", content: "アリスは微笑んだ。" });
const result = (text: string): PageTranslation => ({ status: "translated", lines: [{ line: 0, text }], target_language: "zh-CN" });

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

  assert(!statusButton(), "空闲时不占位");

  // ===== 不同小说并行 =====
  const a = store.begin(novel(1, "星の帰り道"), "星の帰り道", 1);
  void store.begin(novel(2, "海辺の手紙"), "海辺の手紙", 2);
  await settle();
  assert(calls === 2, "不同小说各自发起请求，不互相排队");
  assert(store.active.length === 2, "两个在途任务并存");
  assert(store.activeFor(1)?.page === 1 && store.activeFor(2)?.page === 2, "按小说取各自的任务快照");
  // 进度事件只落在发起它的快照上，不会串到另一本小说。
  pending[0].send(JSON.stringify({ type: "progress", phase: "translate", received: 4096, summary: "甲小说的摘要", reasoning: "" }));
  await settle();
  assert(store.activeFor(1)?.summary === "甲小说的摘要" && store.activeFor(2)?.summary === "", "进度事件不串任务");
  assert(store.activeFor(1)?.received === 4096 && store.activeFor(2)?.received === 0, "已接收字节按任务隔离");

  // ===== 同小说同页 join =====
  const joined = store.begin(novel(1, "星の帰り道"), "星の帰り道", 1);
  void joined.catch(() => {});
  await settle();
  assert(calls === 2 && store.active.length === 2, "同小说同页重复调用 join 同一请求");

  // ===== 同小说换页排队 =====
  const nextPage = store.begin(novel(1, "星の帰り道"), "星の帰り道", 2);
  await settle();
  assert(calls === 2, "同小说换页排在该小说队尾，不并发发起");
  assert(store.active.length === 2, "排队页不占用进度快照");

  // ===== 底栏：多任务形态 =====
  assert(host.querySelectorAll(".status-translation").length === 1, "多任务只有一个入口");
  assert(statusButton()?.textContent?.includes("后台 2 个翻译任务正在进行"), "多任务显示计数文案");
  // 计数是固定短句：必须完整显示，不能被省略号截断（回归：百分比 max-width 曾把它压成「后台 2 ...」）。
  const countLabel = statusButton()!.querySelector<HTMLElement>(".translation-full")!;
  assert(countLabel.scrollWidth <= countLabel.clientWidth + 1, `计数文案完整显示（scrollWidth ${countLabel.scrollWidth} / clientWidth ${countLabel.clientWidth}）`);
  assert(getComputedStyle(statusButton()!).maxWidth === "none", "计数入口不受百分比 max-width 压缩");
  assert(statusButton()?.getAttribute("aria-haspopup") === "menu" && statusButton()?.getAttribute("aria-expanded") === "false", "计数入口声明菜单语义");
  statusButton()!.click();
  await settle();
  const picker = host.querySelector<HTMLElement>(".translation-picker");
  assert(picker?.getAttribute("role") === "menu", "点击计数入口浮出任务选择菜单");
  assert(document.activeElement === picker, "打开后焦点进入菜单");
  const items = [...host.querySelectorAll<HTMLElement>(".picker-item")];
  assert(items.length === 2, "菜单按在途任务列出条目");
  assert(items[0].textContent?.includes("星の帰り道") && items[0].textContent?.includes("第 1 页"), "条目标题为小说名与页码");
  assert(items[0].textContent?.includes("翻译正文") && items[0].textContent?.includes("已用"), "条目回显阶段与已用时间");
  assert(items[1].textContent?.includes("海辺の手紙"), "两个任务都可选择");
  // Esc 关闭并回到入口焦点。
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  await settle();
  assert(!host.querySelector(".translation-picker") && document.activeElement === statusButton(), "Esc 关闭菜单并把焦点还给入口");
  // 透明遮罩外部点击关闭。
  statusButton()!.click(); await settle();
  host.querySelector<HTMLElement>(".translation-backdrop")!.click(); await settle();
  assert(!host.querySelector(".translation-picker"), "点击外部关闭菜单");
  // 选择条目进入对应小说。
  statusButton()!.click(); await settle();
  host.querySelectorAll<HTMLElement>(".picker-item")[1].click();
  await settle();
  assert(router.currentRoute.value.path === "/browse/work/novel/2", "选择条目进入该小说页");
  assert(!host.querySelector(".translation-picker"), "跳转后菜单关闭");

  // ===== 单任务：长标题不挤坏左侧下载区 =====
  const downloadLink = host.querySelector<HTMLElement>(".status-link")!;
  assert(downloadLink.getBoundingClientRect().height <= 32, "左侧下载区保持单行，不被长标题挤换行");
  const downloadLabels = [...downloadLink.querySelectorAll<HTMLElement>(":scope > span:not(.task-summary)")];
  assert(downloadLabels.length > 0 && downloadLabels.every(el => el.getBoundingClientRect().height <= 20), "下载区固定文案不换行");

  // ===== 完成与失败按任务记账 =====
  pending[1].resolve(result("乙小说译文"));
  await settle();
  assert(store.doneFor(2)?.lines[0].text === "乙小说译文", "乙小说完成快照");
  assert(!store.doneFor(1), "甲小说没有混淆的完成快照");
  assert(store.active.length === 1, "乙小说任务退场，甲小说仍在");
  assert(statusButton()?.textContent?.includes("翻译中：《星の帰り道》第 1 页"), "只剩一个任务时恢复原有单任务样式");
  pending[0].reject("模拟失败");
  await a.catch(() => {});
  await settle();
  assert(store.consumeError(1, 1) === "模拟失败", "失败按小说与页码记录");
  assert(store.consumeError(1, 1) === null, "读取后清除");

  // ===== 队首结束后队列继续，完成快照按小说各自保留 =====
  assert(calls === 3 && pending[2].novelId === 1 && pending[2].page === 2, "队首结束后自动发起排队页");
  assert(store.active.length === 1 && store.activeFor(1)?.page === 2, "排队页接替为在途任务");
  pending[2].resolve(result("甲小说第二页"));
  await nextPage;
  await settle();
  assert(store.active.length === 0, "全部任务结束后无在途快照");
  assert(!statusButton(), "任务结束底栏入口退场");
  assert(store.doneFor(1)?.page === 2 && store.doneFor(2)?.page === 2, "两本小说的完成快照各自保留");
  assert(store.dones.length === 2, "完成快照按任务累积");
  void joined;

  // ===== 窄窗短文案 =====
  store.begin(novel(3, "短"), "短", 1);
  store.begin(novel(4, "短"), "短", 1);
  await settle();
  assert(host.querySelector(".translation-short")?.textContent?.includes("2 个翻译中"), "窄窗提供计数短文案");
  pending[3].resolve(result("三"));
  pending[4].resolve(result("四"));
  await settle();
  assert(store.active.length === 0, "完成任务全部退场");

  // ===== 单任务长标题：宽度受限、左侧下载区不换行 =====
  const longTitle = "『催眠術で家に遊びに来た妹の友人たちの常識を捻じ曲げてパコハメしまくりのドスケベお泊り会をエンジョイする話』";
  store.begin(novel(5, longTitle), longTitle, 1);
  await settle();
  const single = statusButton()!;
  const bar = host.querySelector<HTMLElement>(".download-status")!;
  assert(single.getBoundingClientRect().width <= bar.getBoundingClientRect().width * 0.56, "单任务标题限制在底栏宽度的 55% 内");
  const link = host.querySelector<HTMLElement>(".status-link")!;
  assert(link.getBoundingClientRect().height <= 32, "左侧下载区保持单行，不被长标题挤换行");
  assert([...link.querySelectorAll<HTMLElement>("span")].every(el => el.getBoundingClientRect().height <= 20), "下载区文案不换行");
  pending[5].resolve(result("长标题译文"));
  await settle();
  assert(store.active.length === 0, "长标题任务结束");

  document.querySelector<HTMLOutputElement>("#translation-tasks-result")!.textContent = "PASS：不同小说并行、同小说排队与同页 join、进度按任务隔离、底栏单/多任务形态、长标题不挤坏下载区、选择菜单焦点与跳转、完成与失败按小说记账";
}
run().catch(error => {
  document.querySelector<HTMLOutputElement>("#translation-tasks-result")!.textContent = `FAIL: ${error.message}`;
  console.error(error);
});
