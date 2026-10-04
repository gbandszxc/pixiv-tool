/** 使用合成凭据与可控 IPC，验证迟到响应及监听器生命周期；不访问真实账号。 */
import { createApp, effectScope, h, KeepAlive, nextTick, ref, type Component } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter } from "vue-router";
import App from "../src/App.vue";
import LoginDialog from "../src/components/auth/LoginDialog.vue";
import SettingsPanel from "../src/components/settings/SettingsPanel.vue";
import SaucenaoView from "../src/views/SaucenaoView.vue";
import BrowseDiscoverView from "../src/views/browse/BrowseDiscoverView.vue";
import HistoryView from "../src/views/HistoryView.vue";
import { useInfiniteList, type PagedList } from "../src/composables/useInfiniteList";
import { useSettingsStore } from "../src/stores/settings";
import { useHistoryStore } from "../src/stores/history";
import { openInBrowser } from "../src/utils/pixivHooks";
import zhCN from "../src/locales/zh-CN";
import "../src/material";

function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const settle = async () => { await nextTick(); await new Promise(resolve => setTimeout(resolve, 0)); };
const pinia = createPinia();
const settings = useSettingsStore(pinia);
const i18n = createI18n({ legacy: false, locale: "zh-CN", messages: { "zh-CN": zhCN } });
const router = createRouter({ history: createMemoryHistory(), routes: [{ path: "/", component: { render: () => h("div") } }] });
const subscriptions = new Set<number>();
const delayed: (() => void)[] = [];
let listenerId = 0;
let openerCalls = 0;
let discoverCalls = 0;
let resolveDiscover: (data: unknown) => void = () => {};
window.__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
  transformCallback: () => ++listenerId,
  convertFileSrc: (url: string) => url,
  invoke: async (command: string, args: Record<string, any>) => {
    if (command === "plugin:event|listen") {
      const id = args.handler as number;
      if (args.event === "app://confirm-exit" || args.event === "tauri://drag-enter") {
        return new Promise(resolve => delayed.push(() => { subscriptions.add(id); resolve(id); }));
      }
      subscriptions.add(id);
      return id;
    }
    if (command === "plugin:event|unlisten") { subscriptions.delete(args.eventId); return; }
    if (command === "plugin:opener|open_url") { openerCalls++; return; }
    if (command === "browse_discover") {
      discoverCalls++;
      return new Promise(resolve => { resolveDiscover = resolve; });
    }
    if (command === "settings_get") return { ...settings.settings };
    if (command === "auth_status") return { is_logged_in: false };
    if (command === "auth_accounts_list") return { accounts: [], active: null };
    if (command === "tasks_list") return { items: [] };
    if (command === "history_list") throw new Error("合成历史读取失败");
    return {};
  },
};
(window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };

function mount(component: Component, props = {}) {
  const el = document.createElement("div");
  document.body.append(el);
  const app = createApp(component, props).use(pinia).use(i18n).use(router);
  app.mount(el);
  return { el, dispose() { app.unmount(); el.remove(); } };
}

async function run() {
  const scope = effectScope();
  const pending: { resolve: (data: PagedList<number>) => void; reject: (error: Error) => void; current: () => boolean }[] = [];
  const list = scope.run(() => useInfiniteList<number>((_page, current) => new Promise((resolve, reject) => pending.push({ resolve, reject, current }))))!;
  const old = list.loadMore();
  list.reset();
  const fresh = list.loadMore();
  assert(!pending[0].current() && pending[1].current(), "重置使旧游标副作用失效");
  pending[0].resolve({ items: [1], next_page: 7 });
  await old;
  assert(!list.items.value.length && list.loading.value, "旧响应不得回填或结束新请求 loading");
  pending[1].resolve({ items: [2], next_page: 2 });
  await fresh;
  assert(list.items.value[0] === 2 && list.page.value === 2, "当前请求正常提交内容与分页");
  const staleError = list.loadMore();
  list.reset();
  pending[2].reject(new Error("过期错误"));
  await staleError;
  assert(!list.error.value, "过期失败不得污染当前列表");
  const late = list.loadMore();
  scope.stop();
  pending[3].resolve({ items: [3] });
  await late;
  assert(!list.items.value.length && !pending[3].current(), "卸载后忽略在途列表与游标更新");
  await list.loadMore();
  assert(pending.length === 4, "卸载后不再发起新请求");

  for (const url of ["javascript:alert(1)", "file:///C:/Windows/test.exe", "data:text/html,test", "custom:launch", "https://name:password@example.com/path"]) {
    let rejected = false;
    try { await openInBrowser(url); } catch { rejected = true; }
    assert(rejected, "危险 scheme 与 URL 凭据在 opener 前被拒绝");
  }
  assert(openerCalls === 0, "被拒绝的地址不调用系统 opener");
  await openInBrowser("https://www.pixiv.net/artworks/42");
  await openInBrowser("http://example.com/work");
  assert(openerCalls === 2, "正常 HTTPS/HTTP 外链仍可打开");

  const show = ref(false);
  const login = mount({ setup: () => () => h(LoginDialog, { show: show.value, "onUpdate:show": (value: boolean) => { show.value = value; } }) });
  show.value = true;
  await settle();
  const tabs = login.el.querySelector("md-tabs") as HTMLElement & { activeTabIndex: number };
  tabs.activeTabIndex = 1;
  tabs.dispatchEvent(new Event("change"));
  await settle();
  const input = login.el.querySelector("#login-phpsessid") as HTMLInputElement;
  assert(input.getAttribute("type") === "password", "Cookie 使用密码字段");
  input.value = "synthetic-session-for-regression";
  input.dispatchEvent(new Event("input"));
  await settle();
  show.value = false;
  await settle();
  assert(input.value === "", "取消或关闭登录清除临时 Cookie");
  login.dispose();
  const panel = mount(SettingsPanel, { section: "advanced" });
  await settle();
  assert(panel.el.querySelector("#saucenao-api-key")?.getAttribute("type") === "password", "SauceNAO Key 遮罩显示");
  panel.dispose();

  const history = useHistoryStore(pinia);
  history.items = [{ id: 1, category: "novel", title: "已有历史", author_name: null, pages: null, series_id: null, illust_type: null, captured_at: "2026-10-01T00:00:00Z" }];
  let historyError = "";
  const onNotify = (event: Event) => { historyError = (event as CustomEvent<string>).detail; };
  window.addEventListener("pixiv-tool:notify", onNotify);
  const historyPage = mount(HistoryView);
  await settle();
  assert(history.items[0]?.title === "已有历史" && historyError === "合成历史读取失败", "历史读取失败保留快照并显示可读错误");
  assert(!historyPage.el.querySelector(".m3-loading"), "失败后历史列表退出 loading");
  historyPage.dispose();
  window.removeEventListener("pixiv-tool:notify", onNotify);

  const visible = ref(true);
  const cached = mount({ setup: () => () => h(KeepAlive, null, { default: () => visible.value ? h(BrowseDiscoverView) : h({ render: () => h("div") }) }) });
  await settle();
  (cached.el.querySelectorAll<HTMLElement>(".chip")[2]).click();
  visible.value = false;
  await settle();
  resolveDiscover({ items: Array.from({ length: 10 }, (_, index) => ({ id: index + 1, kind: "illust", title: "合成插画", author_id: 1, author_name: "作者", page_count: 1, x_restrict: 0 })) });
  await settle();
  assert(discoverCalls === 1, "停用的发现页不得为当前筛选后台连续补拉");
  visible.value = true;
  await settle();
  assert(discoverCalls === 2, "恢复激活后继续补拉筛选结果");
  resolveDiscover({ items: [] });
  await settle();
  cached.dispose();

  await router.push("/");
  for (let n = 0; n < 10; n++) {
    for (const component of [App, SaucenaoView]) {
      const mounted = mount(component);
      await settle();
      assert(delayed.length === 1, "异步 IPC 订阅故意延迟");
      mounted.dispose();
      delayed.shift()!();
      await settle();
      assert(subscriptions.size === 0, "卸载后落地的订阅立即撤销，重复挂载不累积");
    }
  }
}

run().then(() => {
  document.querySelector("#security-result")!.textContent = "PASS：列表/游标竞态、敏感输入、外链边界、历史失败快照、停用页面停止补拉、20 次挂载卸载订阅零积累";
}).catch(error => {
  document.querySelector("#security-result")!.textContent = `FAIL：${String(error)}`;
  console.error(error);
}).finally(() => { delete window.__TAURI_INTERNALS__; delete (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__; });
