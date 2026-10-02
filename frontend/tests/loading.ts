/** dev.ps1 frontend start 后打开 /tests/loading.html；无框架、无真实 Pixiv 请求。 */
import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter } from "vue-router";
import WorkCard from "../src/components/browse/WorkCard.vue";
import WorkGrid from "../src/components/browse/WorkGrid.vue";
import BrowseHome from "../src/views/browse/BrowseHomeView.vue";
import { useAuthStore } from "../src/stores/auth";
import { useSettingsStore } from "../src/stores/settings";
import { readHomeCache, saveHomeCache } from "../src/utils/homeCache";
import type { BrowseWorkItem } from "../src/api/browse";
import zhCN from "../src/locales/zh-CN";
import "../src/styles/main.css";
import "../src/material";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

const pinia = createPinia();
const router = createRouter({ history: createMemoryHistory(), routes: [{ path: "/browse/home", component: BrowseHome }] });
const i18n = createI18n({ legacy: false, locale: "zh-CN", messages: { "zh-CN": zhCN } });
const userId = `loading-test-${Date.now()}`;
const cacheKey = "pixiv-tool-home-v1:" + userId;
const item: BrowseWorkItem = { id: 1, kind: "illust", title: "缓存作品", author_id: 2, author_name: "作者", page_count: 1, x_restrict: 0 };
let reply: () => Promise<unknown>;
// 确定性 IPC：控制响应时机，图片事件手动触发，测试不访问真实账号。
window.__TAURI_INTERNALS__ = { invoke: () => reply(), convertFileSrc: (url: string) => url };

function mount(component: typeof WorkCard | typeof WorkGrid | typeof BrowseHome, props = {}) {
  const el = document.createElement("div");
  document.body.append(el);
  const app = createApp(component, props).use(pinia).use(i18n).use(router);
  app.mount(el);
  return { el, dispose() { app.unmount(); el.remove(); } };
}

async function run() {
  await router.push("/browse/home");
  useAuthStore(pinia).userId = userId;
  useAuthStore(pinia).isLoggedIn = true;
  useSettingsStore(pinia).settings.thumb_quality_grid = "medium";
  saveHomeCache(userId, [item]);
  assert(readHomeCache(userId)[0]?.title === item.title, "同账号恢复缓存");
  assert(!readHomeCache(userId + "-other").length && !readHomeCache("").length, "其他账号与未登录不读缓存");
  saveHomeCache(userId, Array.from({ length: 121 }, (_, n) => ({ ...item, id: n + 1 })));
  assert(readHomeCache(userId).length === 120, "快照最多保存 120 张卡片");
  localStorage.setItem(cacheKey, JSON.stringify({ savedAt: Date.now() - 86400001, items: [item] }));
  assert(!readHomeCache(userId).length, "过期缓存不显示");
  localStorage.setItem(cacheKey, JSON.stringify({ savedAt: Date.now(), items: [{ ...item, x_restrict: "invalid" }] }));
  assert(!readHomeCache(userId).length, "损坏数据不进入组件");
  saveHomeCache(userId, [item]);

  let finish!: (value: unknown) => void;
  reply = () => new Promise((resolve) => { finish = resolve; });
  const home = mount(BrowseHome);
  await nextTick();
  assert(home.el.querySelector(".title")?.textContent === item.title, "IPC 未完成前就显示缓存卡片");
  assert(!home.el.querySelector(".skeleton-card"), "后台刷新不闪骨架屏");
  finish({ items: [{ ...item, title: "新作品" }] });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert(home.el.querySelector(".title")?.textContent === "新作品", "成功刷新更新列表");
  home.dispose();
  reply = async () => { throw new Error("测试断网"); };
  const offline = mount(BrowseHome);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert(offline.el.querySelector(".title")?.textContent === "新作品", "刷新失败保留旧卡片");
  assert(offline.el.querySelector('[role="alert"]')?.textContent?.includes("测试断网"), "失败有可读提示");
  offline.dispose();

  reply = () => new Promise((resolve) => { finish = resolve; });
  const stale = mount(BrowseHome);
  stale.dispose();
  finish({ items: [{ ...item, title: "旧会话迟到数据" }] });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert(readHomeCache(userId)[0]?.title === "新作品", "已卸载会话不得覆写缓存");

  const card = mount(WorkCard, { item: { ...item, cover: "https://fixture.pximg.net/c/600x1200_90/img-master/img/test.jpg" }, priority: true });
  const preview = card.el.querySelector("img")!;
  assert(preview.src.includes("250x250_80_a2") && preview.loading === "eager", "首屏优先请求小图");
  assert(!card.el.querySelector(".cover-upgrade"), "小图完成前不竞争高清请求");
  preview.dispatchEvent(new Event("load"));
  await nextTick();
  const high = card.el.querySelector<HTMLImageElement>(".cover-upgrade")!;
  assert(high.src.includes("540x540_70") && high.style.display === "none", "高清准备期间小图保持显示");
  high.dispatchEvent(new Event("error"));
  await nextTick();
  assert(card.el.querySelector("img:not(.cover-upgrade)"), "高清失败保留小图");
  assert(card.el.querySelector<HTMLImageElement>(".cover-upgrade")!.src.includes("600x1200_90"), "升级失败回退接口原始 URL");
  card.el.querySelector(".cover-upgrade")!.dispatchEvent(new Event("load"));
  await nextTick();
  assert(!card.el.querySelector("img:not(.cover-upgrade)"), "高清成功替换小图");
  card.dispose();
  useSettingsStore(pinia).settings.thumb_quality_grid = "small";
  const small = mount(WorkCard, { item: { ...item, cover: "https://fixture.pximg.net/c/540x540_70/img-master/img/test.jpg" } });
  small.el.querySelector("img")!.dispatchEvent(new Event("load"));
  await nextTick();
  assert(!small.el.querySelector(".cover-upgrade"), "small 档不重复下载");
  small.dispose();
  useSettingsStore(pinia).settings.thumb_quality_grid = "medium";
  const fallback = mount(WorkCard, { item: { ...item, cover: "https://fixture.pximg.net/c/600x1200_90/img-master/img/test.jpg" } });
  fallback.el.querySelector("img")!.dispatchEvent(new Event("error"));
  await nextTick();
  assert(fallback.el.querySelector<HTMLImageElement>("img")!.src.includes("540x540_70"), "小图失败尝试设定档位");
  fallback.el.querySelector("img")!.dispatchEvent(new Event("error"));
  await nextTick();
  assert(fallback.el.querySelector<HTMLImageElement>("img")!.src.includes("600x1200_90"), "两档失败尝试原始 URL");
  fallback.dispose();
  const grid = mount(WorkGrid, { items: Array.from({ length: 13 }, (_, n) => ({ ...item, id: n + 1, cover: "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg'/>" })) });
  const images = grid.el.querySelectorAll("img");
  assert(images[0].loading === "eager" && images[12].loading === "lazy", "首屏立即请求，其余继续懒加载");
  grid.dispose();
}

const output = document.querySelector("#loading-result")!;
run().then(() => {
  output.textContent = "PASS：渐进图片、失败保留、首页即时缓存、后台刷新、账号隔离、过期与迟到响应";
  output.setAttribute("data-status", "passed");
}).catch((err: unknown) => {
  output.textContent = `FAIL：${String(err)}`;
  output.setAttribute("data-status", "failed");
  console.error(err);
}).finally(() => { localStorage.removeItem(cacheKey); delete window.__TAURI_INTERNALS__; });
