/** 两轮调用在 Rust 测试中验收；这里挂载真实阅读器/设置检查异步回显和安全显示。 */
import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter } from "vue-router";
import Novel from "../src/views/browse/BrowseNovelView.vue";
import Panel from "../src/components/settings/SettingsPanel.vue";
import { useSettingsStore } from "../src/stores/settings";
import type { PageTranslation } from "../src/api/translation";
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
let resolvePage: (result: PageTranslation) => void = () => {};
let rejectPage: (error: string) => void = () => {};
let progressSend: ((raw: string) => void) | null = null;
let calls = 0;
let requestedPage = 0;
let requestedForce = false;
let capturedSettings: Record<string, unknown> = {};
let probeModels: string[] | string = [];
let probeTestOk = true;
let lastProbeUrl = "";
let lastProbeFormat = "";
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
      lastProbeFormat = args.probe?.format ?? "";
      return typeof probeModels === "string" ? Promise.reject(probeModels) : [...probeModels];
    }
    if (command === "translation_test") return probeTestOk ? Promise.resolve() : Promise.reject("模拟检测失败");
    if (command === "novel_translate_page") {
      calls++; requestedPage = args.page; requestedForce = args.force;
      assert(args.novel.tags.includes("同人") && args.novel.description === "星空下的重逢。", "请求包含标签和纯文本简介");
      // 与后端协议同构的 JSON 事件流由用例分步驱动，验证思考期与正文期的预览行切换。
      progressSend = (raw: string) => args.progress.onmessage(raw);
      return new Promise<PageTranslation>((resolve, reject) => { resolvePage = resolve; rejectPage = reject; });
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
  assert(host.querySelector(".translation-trigger-wrap > .translation-indicator.is-done"), "已译页入口显示成功状态点");
  assert(!host.querySelector(".novel-content script") && host.querySelector(".translated-text")?.getAttribute("lang") === "zh-CN", "译文纯文本且语言明确");
  await click("仅译文");
  assert(!host.querySelector("ruby") && host.querySelectorAll(".novel-image").length === 1, "仅译文隐藏原文但图片不重复");
  await click("仅原文"); assert(!host.querySelector(".translated-text") && host.querySelector("ruby"), "仅原文恢复注音");
  await click("双语");
  await selectPage(2); assert(host.querySelector(".translation-status")?.textContent?.includes("尚未翻译"), "未译页不显示旧译文");
  assert(!host.querySelector(".translation-trigger-wrap > .translation-indicator"), "未译页入口无状态点");
  await click("翻译本页");
  assert(requestedPage === 2 && !requestedForce && calls === 1, "只请求当前页且非重译");
  assert(host.querySelector(".translation-popover md-outlined-button")?.hasAttribute("disabled"), "翻译时禁用按钮");
  const busyStatus = host.querySelector(".translation-status")?.textContent ?? "";
  assert(busyStatus.includes("翻译中") && !busyStatus.includes("设定集"), "只回显统一翻译中，不暴露内部阶段");
  assert(host.querySelector(".translation-trigger-wrap > .translation-indicator.is-busy"), "翻译中入口显示进行中状态点");
  // 思考期：预览行显示思考尾窗（与摘要共用一行位置，思考期摘要行不存在）。
  progressSend!(JSON.stringify({ type: "phase", phase: "prepare" }));
  progressSend!(JSON.stringify({ type: "progress", phase: "prepare", received: 0, summary: "", reasoning: "先通读全文，判断文风与叙事视角…" }));
  await settle();
  assert(host.querySelector(".progress-reasoning")?.textContent?.includes("先通读全文"), "思考期预览行显示思考尾窗");
  assert(!host.querySelector(".progress-summary"), "思考期不渲染摘要行（共用一行位置）");
  // 正文到达：摘要流式取代思考的显示位置，思考的最后一句不驻留。
  progressSend!(JSON.stringify({ type: "progress", phase: "translate", received: 2048, summary: "她微笑了。", reasoning: "" }));
  await settle();
  assert(!host.querySelector(".progress-reasoning"), "正文到达后思考行退场，末尾思考不驻留");
  assert(host.querySelector(".progress-summary")?.textContent?.includes("她微笑了。"), "正文摘要取代思考位置流式显示");
  await selectPage(1);
  resolvePage({ status: "translated", lines: [{ line: 0, text: "她微笑了。" }], target_language: "zh-CN", usage: { prompt: 100, completion: 20, total: 120 } }); await settle();
  assert(!host.querySelector(".novel-content")?.textContent?.includes("她微笑了。"), "异步结果不串到另一页");
  assert(!host.querySelector(".translation-progress"), "完成后进度区退场");
  await selectPage(2); assert(host.querySelector(".translated-text")?.textContent === "她微笑了。", "目标页获得译文");
  assert(host.querySelector(".translation-status")?.textContent?.includes("用量 120 tokens"), "完成态回显本次 token 用量");
  await click("重新翻译本页"); assert(requestedForce, "重译显式 force");
  rejectPage("模拟服务失败，请重试"); await settle();
  assert(host.querySelector(".translation-status.is-error") && host.querySelector(".translated-text")?.textContent === "她微笑了。", "失败可见且保留旧译文");
  const failedBadge = host.querySelector<HTMLElement>(".translation-trigger-wrap > .translation-indicator.is-error");
  assert(failedBadge?.title.includes("模拟服务失败"), "失败状态点 hover 可见模型侧报错");
  await click("重新翻译本页"); resolvePage({ status: "translated", lines: [{ line: 0, text: "她露出微笑。" }], target_language: "zh-CN" }); await settle();
  assert(!host.querySelector(".translation-status.is-error") && host.querySelector(".translation-trigger-wrap > .translation-indicator.is-done"), "重试恢复");
  // 原文已是目标语言：只提示、不请求模型；按钮变为「仍然翻译」，再次点击强制走完整流程。
  await click("重新翻译本页");
  resolvePage({ status: "already_target_language", lines: [], target_language: "zh-CN" }); await settle();
  assert(host.querySelector(".translation-status")?.textContent?.includes("无需翻译"), "目标语言一致时提示无需翻译");
  assert(!host.querySelector(".translation-trigger-wrap > .translation-indicator.is-error"), "无需翻译不算失败");
  await click("仍然翻译"); assert(requestedForce, "再次点击强制翻译");
  resolvePage({ status: "translated", lines: [{ line: 0, text: "她再次微笑。" }], target_language: "zh-CN" }); await settle();
  assert(host.querySelector(".translation-status")?.textContent?.includes("已保存") && !host.querySelector(".translation-status")?.textContent?.includes("无需翻译"), "强制翻译后回到已保存状态");

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
  await selectPage(1);
  document.querySelector<HTMLButtonElement>("#preview-reader")!.onclick = () => { host.style.display = ""; panelHost.style.display = "none"; };
  document.querySelector<HTMLButtonElement>("#preview-settings")!.onclick = () => { host.style.display = "none"; panelHost.style.display = ""; };
  document.querySelector<HTMLButtonElement>("#preview-dark")!.onclick = () => document.documentElement.classList.add("dark");
  document.querySelector<HTMLButtonElement>("#preview-light")!.onclick = () => document.documentElement.classList.remove("dark");
  document.querySelector<HTMLElement>("#preview-controls")!.hidden = false;
  document.querySelector("#translation-result")!.textContent = "PASS：底栏 SVG 弹层、焦点/Esc/外部关闭、模式、图片、跨页异步、错误重试、状态点与 hover 报错、统一翻译中状态、目标语言一致提示与强制翻译、设置一级两组分区与凭据状态胶囊、一级不含运行参数与原始 JSON、二级分类入口与返回、JSON 校验定位、跨级草稿保留、越界即时标红与错误计数、恢复本组/全部推荐值、凭据草稿、获取模型原位下拉与绿红检测状态、目标语言选择、接口协议默认值与切换";
}
run().catch(error => { document.querySelector("#translation-result")!.textContent = `FAIL: ${error instanceof Error && error.stack ? error.stack : error.message}`; console.error(error); });
