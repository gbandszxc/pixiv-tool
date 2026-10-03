/** 模拟热更新后的模块路由与应用注入路由分离；使用真实 App 和页面返回控件。 */
import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createRouter, createWebHashHistory } from "vue-router";
import moduleRouter from "../src/router";
import App from "../src/App.vue";
import zh from "../src/locales/zh-CN";
import "../src/styles/main.css";
import "../src/material";

const router = createRouter({ history: createWebHashHistory(), routes: moduleRouter.options.routes });
createApp(App).use(createPinia()).use(router).use(createI18n({ legacy:false, locale:"zh-CN", messages:{"zh-CN":zh} })).mount("#app");
function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
async function waitFor(check: () => boolean, message: string) {
  const end = Date.now() + 3000;
  while (!check()) { assert(Date.now() < end, message); await new Promise(r => setTimeout(r, 20)); }
  await nextTick();
  await new Promise(r => setTimeout(r, 50));
}
async function back(path: string) {
  const selector = router.currentRoute.value.path.startsWith("/browse/work/") ? ".work-topbar md-icon-button, .topbar md-icon-button" : ".page-back";
  await waitFor(() => !!document.querySelector(selector), `返回入口缺失：目标=${path}，当前=${router.currentRoute.value.fullPath}，地址=${location.hash}`);
  (document.querySelector(selector) as HTMLElement).click();
  await waitFor(() => router.currentRoute.value.fullPath === path, `返回没有回到来源 ${path}；页面=${router.currentRoute.value.fullPath}，地址=${location.hash}`);
}
async function push(path: string) {
  await router.push(path);
  await nextTick();
  await new Promise(r => setTimeout(r, 50));
}
async function run() {
  await router.isReady();
  assert(!moduleRouter.options.history.state.back, "模块路由没有历史，实际应用路由必须独立判定");
  await push("/browse/work/illust/9000005");
  await push("/browse/user/100004");
  await back("/browse/work/illust/9000005");
  await back("/browse/home");
  await push("/browse/search?word=source&kind=novel");
  const searchPath = router.currentRoute.value.fullPath;
  await push("/browse/work/novel/9000005");
  await back(searchPath);
  await push("/tools/tasks");
  await back(searchPath);
  await back("/browse/home");
  await push("/browse/work/illust/9000006");
  window.dispatchEvent(new KeyboardEvent("keydown", { key:"Escape", bubbles:true }));
  await waitFor(() => router.currentRoute.value.path === "/browse/home", "图片 Escape 返回实际来源");
  // 错误态也必须保留有效的返回，离线 IPC 不接触真实账号。
  window.__TAURI_INTERNALS__ = { convertFileSrc: url => url, invoke:async () => { throw new Error("模拟加载失败"); } };
  try {
    await push("/browse/work/illust/9000007");
    await waitFor(() => !!document.querySelector(".work-error md-outlined-button"), "错误态返回入口");
    (document.querySelector(".work-error md-outlined-button") as HTMLElement).click();
    await waitFor(() => router.currentRoute.value.path === "/browse/home", "错误态返回实际来源");
  } finally { delete window.__TAURI_INTERNALS__; }
}
run().then(() => { document.querySelector("#navigation-context-result")!.textContent = "PASS：注入路由与模块实例分离时，作者、图片、小说、搜索与下载仍逐层返回来源"; }).catch(error => { document.querySelector("#navigation-context-result")!.textContent = `FAIL：${String(error)}`; console.error(error); });

