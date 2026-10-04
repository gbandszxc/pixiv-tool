/** 真实表单与页面的说明验收；浏览器 mock，不读凭据或请求在线服务。 */
import { createApp, h, nextTick, ref } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter, RouterView } from "vue-router";
import SettingsDialog from "../src/components/settings/SettingsDialog.vue";
import LoginDialog from "../src/components/auth/LoginDialog.vue";
import Search from "../src/views/browse/BrowseSearchView.vue";
import Saucenao from "../src/views/SaucenaoView.vue";
import zh from "../src/locales/zh-CN";
import en from "../src/locales/en-US";
import "../src/styles/main.css";
import "../src/material";

const showSettings = ref(false);
const showLogin = ref(false);
const i18n = createI18n({ legacy: false, locale: "zh-CN", messages: { "zh-CN": zh, "en-US": en } });
const router = createRouter({ history: createMemoryHistory(), routes: [{ path: "/", component: { render: () => h("div") } }, { path: "/browse/search", component: Search }, { path: "/saucenao", component: Saucenao }] });
createApp({ setup: () => () => [
  h(SettingsDialog, { show: showSettings.value, "onUpdate:show": (value: boolean) => { showSettings.value = value; } }),
  h(LoginDialog, { show: showLogin.value, "onUpdate:show": (value: boolean) => { showLogin.value = value; } }),
  h(RouterView),
] }).use(createPinia()).use(i18n).use(router).mount("#app");
function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const settle = async () => { await nextTick(); await new Promise(resolve => setTimeout(resolve, 40)); };

async function checkHelp(container: Element) {
  const buttons = [...container.querySelectorAll<HTMLButtonElement>(".help-trigger")];
  assert(buttons.length, "存在帮助入口");
  for (const button of buttons) {
    const tip = document.getElementById(button.getAttribute("aria-describedby")!)!;
    assert(tip.textContent?.trim() && !tip.matches(":popover-open") && getComputedStyle(tip).display === "none", "说明默认隐藏且具有关联内容");
    button.dispatchEvent(new PointerEvent("pointerenter")); await settle();
    assert(tip.matches(":popover-open"), "悬停显示说明");
    button.dispatchEvent(new PointerEvent("pointerleave"));
    tip.dispatchEvent(new PointerEvent("pointerenter"));
    await new Promise(resolve => setTimeout(resolve, 180));
    assert(tip.matches(":popover-open"), "可移动到说明继续阅读");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true })); await settle();
    assert(!tip.matches(":popover-open"), "Esc 关闭说明");
    button.focus(); await settle();
    assert(tip.matches(":popover-open") && document.activeElement === button, "键盘聚焦显示，焦点留在按钮");
    button.click(); await settle();
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })); await settle();
    assert(!tip.matches(":popover-open"), "点击外部关闭");
    button.blur();
  }
}
async function run() {
  await router.push("/");
  showSettings.value = true; await settle(); await settle();
  const settings = document.querySelector<HTMLDialogElement>(".settings-dialog")!;
  for (const label of ["通用", "外观", "图片与内容", "小说翻译", "高级", "维护"]) {
    [...settings.querySelectorAll<HTMLButtonElement>(".settings-nav-item")].find(button => button.textContent === label)!.click(); await settle();
    await checkHelp(settings);
    assert(settings.open, "关闭 tooltip 不关闭设置对话框");
    assert(!settings.querySelector(".field-hint:not(.credential-error)") || label === "维护", "说明不再堆砌在字段下方");
  }
  showSettings.value = false; await settle();
  showLogin.value = true; await settle();
  const login = document.querySelector<HTMLDialogElement>(".login-dialog")!;
  await checkHelp(login);
  (login.querySelectorAll<HTMLElement>("md-primary-tab")[1]).click(); await settle();
  await checkHelp(login); assert(!login.querySelector(".hint"), "登录说明使用 tooltip");
  showLogin.value = false; await settle();
  for (const path of ["/browse/search", "/saucenao"]) {
    await router.push(path); await settle();
    await checkHelp(document.querySelector(".page-view")!);
    assert(!document.querySelector(".field-help,.page-subtitle,.idle-hint"), "页面介绍收进帮助入口");
  }
  await router.push("/"); showSettings.value = true; await settle();
  [...settings.querySelectorAll<HTMLButtonElement>(".settings-nav-item")].find(button => button.textContent === "小说翻译")!.click(); await settle();
  document.querySelector<HTMLButtonElement>("#settings")!.onclick = () => { showLogin.value = false; showSettings.value = true; };
  document.querySelector<HTMLButtonElement>("#login")!.onclick = () => { showSettings.value = false; showLogin.value = true; };
  for (const path of ["search", "saucenao"]) document.querySelector<HTMLButtonElement>(`#${path}`)!.onclick = () => { showSettings.value = false; showLogin.value = false; void router.push(path === "search" ? "/browse/search" : "/saucenao"); };
  document.querySelector<HTMLButtonElement>("#theme")!.onclick = () => document.documentElement.classList.toggle("dark");
  document.querySelector<HTMLButtonElement>("#language")!.onclick = () => { i18n.global.locale.value = i18n.global.locale.value === "zh-CN" ? "en-US" : "zh-CN"; };
  document.querySelector<HTMLElement>("#previews")!.hidden = false;
  document.querySelector("#result")!.textContent = "PASS：六组设置、两种登录、搜索/以图识图，默认收起、悬停/聚焦/点击、可悬停阅读、Esc/外部关闭";
}
run().catch(error => { document.querySelector("#result")!.textContent = `FAIL: ${error.message}`; console.error(error); });
