import { createApp, createVNode, nextTick, ref } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import Dialog from "../src/components/settings/SettingsDialog.vue";
import { useSettingsStore } from "../src/stores/settings";
import zhCN from "../src/locales/zh-CN";
import enUS from "../src/locales/en-US";
import "../src/styles/main.css";
import "../src/material";

function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const wait = (ms = 40) => new Promise(resolve => setTimeout(resolve, ms));
const settle = async () => { await nextTick(); await wait(); };
const pinia = createPinia();
const settings = useSettingsStore(pinia);
const i18n = createI18n({ legacy: false, locale: "zh-CN", messages: { "zh-CN": zhCN, "en-US": enUS } });
const info = {
  translation: { path: "D:\\很长的项目目录与用户名\\Pixiv Tool\\data\\translations", bytes: 2048, files: 2 },
  images: { path: "D:\\很长的项目目录与用户名\\Pixiv Tool\\data\\cache\\img", bytes: 1048576, files: 4 },
  logs: { path: "D:\\很长的项目目录与用户名\\Pixiv Tool\\data\\logs", bytes: 8192, files: 2 },
};
let log = '[WARN] 翻译服务流内错误：{"code":"1301","message":"内容审核拒绝"}';
let reads = 0, clears = 0, copies = "", fail = false, cacheBusy = false;
window.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string, args: Record<string, any>) => {
    if (command === "settings_get") return settings.settings;
    if (command === "maintenance_info") return structuredClone(info);
    if (command === "read_logs") { reads++; if (fail) throw "模拟日志读取失败"; return log; }
    if (command === "clear_cache") { if (cacheBusy) throw "小说翻译正在进行，请完成后再清理"; clears++; const kind = args.kind as "translation" | "images"; info[kind].bytes = 0; info[kind].files = 0; return {}; }
    if (command === "clear_logs") { clears++; log = ""; info.logs.bytes = 0; return {}; }
    return {};
  }, convertFileSrc: (url: string) => url,
};
Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (path: string) => { copies = path; } } });
const show = ref(false);
createApp({ render: () => createVNode(Dialog, { show: show.value, "onUpdate:show": (value: boolean) => { show.value = value; } }) }).use(pinia).use(i18n).mount("#app");
async function run() {
  await settle(); show.value = true; await settle();
  [...document.querySelectorAll<HTMLElement>(".settings-nav-item")].find(el => el.textContent === "维护")!.click(); await settle();
  const panel = () => document.querySelector<HTMLElement>(".maintenance-storage")!;
  const click = async (root: Element, label: string) => { const button = [...root.querySelectorAll<HTMLElement>("md-text-button,md-filled-button")].find(el => el.textContent?.trim() === label); assert(button, `找到 ${label}`); button.click(); await settle(); };
  assert(panel().textContent?.includes("1 MiB") && panel().textContent?.includes("8 KiB"), "分类及日志大小显示");
  const paths = [...panel().querySelectorAll(".maintenance-path")];
  for (const [index, kind] of (["translation", "images", "logs"] as const).entries()) { await click(paths[index], "复制路径"); assert(copies === info[kind].path, "复制对应路径"); }
  assert(panel().querySelector("pre")?.textContent?.includes("1301"), "原始错误说明呈现");
  log = "[INFO] 新日志自动出现"; await wait(2100); assert(panel().querySelector("pre")?.textContent === log, "自动轮询读取新日志");
  fail = true; document.dispatchEvent(new Event("visibilitychange")); await settle();
  assert(panel().querySelector('[role="alert"]')?.textContent?.includes("模拟日志读取失败") && panel().querySelector("pre")?.textContent === log, "失败保留旧快照");
  fail = false; await click(panel(), "重试"); assert(!panel().querySelector('[role="alert"]'), "手动重试恢复");
  const translation = panel().querySelector(".maintenance-resource")!;
  await click(translation, "清除");
  const confirm = panel().querySelector<HTMLDialogElement>("dialog")!;
  assert(confirm.open && confirm.textContent?.includes("共享设定集"), "清理确认说明后果");
  await click(confirm, "取消"); assert(clears === 0, "取消不清理");
  cacheBusy = true; await click(translation, "清除"); await click(confirm, "清除"); assert(clears === 0 && panel().textContent?.includes("2 KiB"), "进行中拒绝不丢失统计");
  cacheBusy = false; let cacheCleared = false;
  window.addEventListener("pixiv-tool:translation-cache-cleared", () => { cacheCleared = true; }, { once: true });
  await click(translation, "清除"); await click(confirm, "清除"); assert(cacheCleared && info.translation.bytes === 0, "清理成功刷新大小并通知阅读器");
  assert(translation.querySelector("md-text-button")?.hasAttribute("disabled"), "空缓存清理禁用");
  await click(panel(), "清除日志"); await click(confirm, "清除"); assert(panel().querySelector("pre")?.textContent === "暂无日志" && panel().textContent?.includes("总计 0 B"), "清日志后显示空态和零大小");
  show.value = false; await settle(); const stoppedReads = reads; await wait(2100); assert(reads === stoppedReads, "关闭弹窗停止轮询");
  show.value = true; await settle(); i18n.global.locale.value = "en-US"; await settle(); assert(panel().textContent?.includes("Copy path"), "英文文案齐全"); i18n.global.locale.value = "zh-CN";
  Object.assign(window, { __maintenancePreview: (mode: string) => { document.documentElement.classList.toggle("dark", mode === "dark"); }, __maintenanceLoading: () => { show.value = false; } });
  document.querySelector("#maintenance-result")!.textContent = "PASS：大小统计、三处路径复制、原始错误展示、轮询更新与关闭停止、读取失败及重试、取消/任务互斥/清缓存通知、清日志空态、中英文";
}
run().catch(error => { document.querySelector("#maintenance-result")!.textContent = `FAIL: ${error.message}`; console.error(error); });
