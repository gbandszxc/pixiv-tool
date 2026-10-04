/** 两轮调用在 Rust 测试中验收；这里挂载真实阅读器/设置检查异步回显和安全显示。 */
import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter } from "vue-router";
import Novel from "../src/views/browse/BrowseNovelView.vue";
import Panel from "../src/components/settings/SettingsPanel.vue";
import { useSettingsStore } from "../src/stores/settings";
import type { TranslatedLine } from "../src/api/translation";
import zhCN from "../src/locales/zh-CN";
import "../src/styles/main.css";
import "../src/material";

function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const settle = async () => { await nextTick(); await new Promise(resolve => setTimeout(resolve, 30)); };
const pinia = createPinia();
const settings = useSettingsStore(pinia);
const i18n = createI18n({ legacy: false, locale: "zh-CN", messages: { "zh-CN": zhCN } });
const router = createRouter({ history: createMemoryHistory(), routes: [{ path: "/", component: { template: "<div/>" } }, { path: "/browse/search", component: { template: "<div/>" } }] });
let resolvePage: (lines: TranslatedLine[]) => void = () => {};
let rejectPage: (error: string) => void = () => {};
let calls = 0;
let requestedPage = 0;
let requestedForce = false;
let capturedSettings: Record<string, unknown> = {};
let probeModels: string[] | string = [];
let probeTestOk = true;
let lastProbeUrl = "";
const book = { bible: { style: "克制", terms: [] }, pages: { "1": [{ line: 0, text: "序章译名" }, { line: 2, text: "爱丽丝抬头。<script>这是纯文本</script>" }] } };
window.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string, args: Record<string, any>) => {
    if (command === "settings_get") return { ...settings.settings, translation_key_configured: true };
    if (command === "settings_save") { capturedSettings = args.settings; return { status: "success" }; }
    if (command === "browse_work_detail") return { detail_kind: "novel", item: { id: 42, kind: "novel", title: "星の帰り道 · 很长的小说标题用于窗口验收", author_id: 1, author_name: "作者", tags: ["同人", "冒险"], description: "<p>星空下的重逢。</p>", page_count: 2, x_restrict: 0 }, content: "[chapter:序章]\n\nアリスが顔を上げた。[rb:星>ほし]\n[uploadedimage:1]\n[newpage]彼女は微笑んだ。", embedded_images: { "1": "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='120' height='60'><rect fill='gray' width='120' height='60'/></svg>" }, bookmarkState: null };
    if (command === "browse_related") return { items: [], next_page: null };
    if (command === "novel_translation_get") return book;
    if (command === "translation_models") {
      lastProbeUrl = args.probe?.api_url ?? "";
      return typeof probeModels === "string" ? Promise.reject(probeModels) : [...probeModels];
    }
    if (command === "translation_test") return probeTestOk ? Promise.resolve() : Promise.reject("模拟检测失败");
    if (command === "novel_translate_page") {
      calls++; requestedPage = args.page; requestedForce = args.force;
      assert(args.novel.tags.includes("同人") && args.novel.description === "星空下的重逢。", "请求包含标签和纯文本简介");
      args.progress.onmessage("prepare");
      return new Promise<TranslatedLine[]>((resolve, reject) => { resolvePage = resolve; rejectPage = reject; });
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
  const click = async (label: string) => { const element = [...host.querySelectorAll<HTMLElement>(".translation-mode,md-outlined-button")].find(el => el.textContent?.trim() === label); assert(element, `找到按钮 ${label}`); element.click(); await settle(); };
  const selectPage = async (page: number) => { const select = host.querySelector<HTMLSelectElement>(".page-select"); assert(select, "分页存在"); select.value = String(page); select.dispatchEvent(new Event("change", { bubbles: true })); await settle(); };
  const trigger = host.querySelector<HTMLElement>(".translation-trigger");
  assert(trigger?.querySelector("svg") && !host.querySelector(".translation-popover"), "底栏 SVG 入口，初始不展开选项");
  trigger.click(); await settle();
  assert(document.activeElement === host.querySelector(".translation-popover"), "打开后焦点进入弹层");
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); await settle();
  assert(!host.querySelector(".translation-popover"), "Esc 关闭弹层");
  trigger.click(); await settle();
  host.querySelector<HTMLElement>(".translation-backdrop")!.click(); await settle();
  assert(!host.querySelector(".translation-popover"), "点击外部关闭弹层");
  trigger.click(); await settle();
  assert(host.querySelector('[aria-pressed="true"]')?.textContent?.trim() === "双语", "默认双语");
  assert(host.querySelectorAll(".translated-text").length === 2, "恢复已存译文，包括标题");
  assert(!host.querySelector(".novel-content script") && host.querySelector(".translated-text")?.getAttribute("lang") === "zh-CN", "译文纯文本且语言明确");
  await click("仅译文");
  assert(!host.querySelector("ruby") && host.querySelectorAll(".novel-image").length === 1, "仅译文隐藏原文但图片不重复");
  await click("仅原文"); assert(!host.querySelector(".translated-text") && host.querySelector("ruby"), "仅原文恢复注音");
  await click("双语");
  await selectPage(2); assert(host.querySelector(".translation-status")?.textContent?.includes("尚未翻译"), "未译页不显示旧译文");
  await click("翻译本页");
  assert(requestedPage === 2 && !requestedForce && calls === 1, "只请求当前页且非重译");
  assert(host.querySelector(".translation-popover md-outlined-button")?.hasAttribute("disabled"), "翻译时禁用按钮");
  await selectPage(1);
  resolvePage([{ line: 0, text: "她微笑了。" }]); await settle();
  assert(!host.querySelector(".novel-content")?.textContent?.includes("她微笑了。"), "异步结果不串到另一页");
  await selectPage(2); assert(host.querySelector(".translated-text")?.textContent === "她微笑了。", "目标页获得译文");
  await click("重新翻译本页"); assert(requestedForce, "重译显式 force");
  rejectPage("模拟服务失败，请重试"); await settle();
  assert(host.querySelector(".translation-status.is-error") && host.querySelector(".translated-text")?.textContent === "她微笑了。", "失败可见且保留旧译文");
  await click("重新翻译本页"); resolvePage([{ line: 0, text: "她露出微笑。" }]); await settle();
  assert(!host.querySelector(".translation-status.is-error"), "重试恢复");

  const panelHost = document.createElement("div"); panelHost.style.cssText = "display:none;padding:24px;max-width:640px"; document.body.append(panelHost);
  const panelApp = createApp(Panel, { section: "translation" }).use(pinia).use(i18n);
  const panel = panelApp.mount(panelHost) as unknown as { save(): Promise<boolean>; reset(): void; hasUnsaved(): boolean };
  await settle();
  const input = async (id: string, value: string) => { const element = panelHost.querySelector<HTMLElement & { value: string }>(`#${id}`); assert(element, `字段 ${id}`); element.value = value; element.dispatchEvent(new Event("input", { bubbles: true })); await settle(); };
  assert(panelHost.querySelector("#translation-key")?.getAttribute("type") === "password", "Key 为密码字段");
  await input("translation-json", "[]"); assert(!await panel.save(), "拒绝非对象 JSON");
  await input("translation-json", '{"reasoning_effort":"high","thinking":{"budget_tokens":2048}}');
  await input("translation-url", "https://example.com/v1"); await input("translation-model", "test-model"); await input("translation-key", "test-only-key");
  assert(panel.hasUnsaved(), "敏感草稿进入脏检查"); assert(await panel.save(), "合法配置保存");
  assert(capturedSettings.translation_api_key === "test-only-key", "Key 仅通过单次写入参数发送");
  assert(!(settings.settings as unknown as Record<string, unknown>).translation_api_key && !(panelHost.querySelector("#translation-key") as any).value, "Store 和保存后字段不留 Key");
  assert(!panel.hasUnsaved(), "保存后清除脏状态");
  await panel.save(); assert(!("translation_api_key" in capturedSettings), "空白字段不覆盖已存 Key");
  [...panelHost.querySelectorAll<HTMLElement>("md-text-button")].find(el => el.textContent?.trim() === "清除已保存的 Key")?.click(); await settle();
  assert(panel.hasUnsaved() && await panel.save() && capturedSettings.translation_api_key === "", "显式清除凭据");
  await input("translation-key", "discard-only"); panel.reset(); await settle(); assert(!panel.hasUnsaved(), "取消清空敏感草稿");

  // 获取模型 / 检测可用：用当前未保存草稿发探测请求，结果只进组件状态、不落 store。
  const findButton = (label: string) => [...panelHost.querySelectorAll<HTMLElement>("md-outlined-button")].find(el => el.textContent?.trim() === label);
  await input("translation-url", "https://api.example.com/v1"); await input("translation-key", "probe-key");
  probeModels = ["glm-model-b", "glm-model-a"];
  findButton("获取模型")!.click(); await settle();
  assert(lastProbeUrl === "https://api.example.com/v1", "模型请求使用未保存的 URL");
  const modelSelect = panelHost.querySelector<HTMLElement & { value: string }>("#translation-model-select");
  assert(modelSelect?.textContent?.includes("glm-model-a"), "下拉列出获取到的模型");
  modelSelect!.value = "glm-model-a"; modelSelect!.dispatchEvent(new Event("change")); await settle();
  assert((panelHost.querySelector("#translation-model") as HTMLElement & { value: string }).value === "glm-model-a", "选定模型回填输入框");
  findButton("检测可用")!.click(); await settle();
  assert(panelHost.querySelector('[role="status"]')?.textContent?.includes("模型可用"), "检测成功提示");
  probeTestOk = false; findButton("检测可用")!.click(); await settle();
  assert(panelHost.querySelector(".credential-error")?.textContent?.includes("模拟检测失败"), "检测失败错误可见");
  await input("translation-url", "");
  assert(findButton("检测可用")?.hasAttribute("disabled") && findButton("获取模型")?.hasAttribute("disabled"), "URL 为空禁用探测");

  // 留真实组件供宽窄窗口、深浅主题与 hover/focus 验收。
  await selectPage(1);
  document.querySelector<HTMLButtonElement>("#preview-reader")!.onclick = () => { host.style.display = ""; panelHost.style.display = "none"; };
  document.querySelector<HTMLButtonElement>("#preview-settings")!.onclick = () => { host.style.display = "none"; panelHost.style.display = ""; };
  document.querySelector<HTMLButtonElement>("#preview-dark")!.onclick = () => document.documentElement.classList.add("dark");
  document.querySelector<HTMLButtonElement>("#preview-light")!.onclick = () => document.documentElement.classList.remove("dark");
  document.querySelector<HTMLElement>("#preview-controls")!.hidden = false;
  document.querySelector("#translation-result")!.textContent = "PASS：底栏 SVG 弹层、焦点/Esc/外部关闭、模式、图片、跨页异步、错误重试、缓存、设置 JSON 与凭据草稿、获取模型下拉与可用性检测";
}
run().catch(error => { document.querySelector("#translation-result")!.textContent = `FAIL: ${error.message}`; console.error(error); });
