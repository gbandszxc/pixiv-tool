/** 作者关注流程的可控 IPC 验收；不会访问 Pixiv 或更改真实关注关系。 */
import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createRouter, createMemoryHistory } from "vue-router";
import Author from "../src/views/browse/BrowseAuthorView.vue";
import { useAuthStore } from "../src/stores/auth";
import zh from "../src/locales/zh-CN";
import "../src/styles/main.css";
import "../src/material";

function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const tick = async () => { await nextTick(); await new Promise(r => setTimeout(r, 0)); };
const pinia = createPinia();
const auth = useAuthStore(pinia); auth.userId = "1";
let followed = false, unknown = false, calls = 0;
let resolveWrite: (value: unknown) => void, rejectWrite: (error: Error) => void;
window.__TAURI_INTERNALS__ = {
  convertFileSrc: (url: string) => url,
  invoke: async (command: string, args: Record<string, unknown> = {}) => {
    if (command === "browse_user_profile") return { id:42, name:"测试作者", pixiv_id:"", profile_img:"", following_count:1, mypixiv_count:0, is_followed:unknown ? undefined : followed };
    if (command === "browse_user_works") return { items:[], next_page:null };
    if (command === "browse_user_follow") {
      assert(args.id === 42 && typeof args.followed === "boolean", "IPC 使用目标作者与期望状态");
      calls++;
      return new Promise((resolve, reject) => {
        resolveWrite = value => { followed = Boolean(args.followed); resolve(value); };
        rejectWrite = reject;
      });
    }
    throw new Error(`unexpected command: ${command}`);
  },
};
async function run() {
  const router = createRouter({ history:createMemoryHistory(), routes:[{path:"/browse/author/42",component:{template:"<div/>"}}] });
  await router.push("/browse/author/42");
  const host = document.createElement("div"); document.body.append(host);
  const app = createApp(Author, { id:42 }).use(pinia).use(router).use(createI18n({legacy:false,locale:"zh-CN",messages:{"zh-CN":zh}}));
  app.mount(host); await tick(); await tick();
  const button = () => host.querySelector<HTMLElement & {disabled:boolean}>(".follow-button")!;
  assert(button().textContent === "关注", "未关注状态");
  button().click(); button().click(); await tick();
  assert(calls === 1 && button().disabled, "请求中禁用并防止重复提交");
  resolveWrite!({is_followed:true}); await tick();
  assert(button().textContent === "已关注" && button().getAttribute("aria-pressed") === "true", "成功才回显关注状态");
  button().click(); await tick(); rejectWrite!(new Error("模拟失败")); await tick();
  assert(button().textContent === "已关注" && !button().disabled, "失败保留状态并允许重试");
  button().click(); await tick(); resolveWrite!({is_followed:false}); await tick();
  assert(button().textContent === "关注", "取消关注成功回显");
  button().click(); await tick(); auth.userId = "2"; resolveWrite!({is_followed:true}); await tick();
  assert(button().textContent === "关注", "账号切换后忽略旧请求回显");
  auth.userId = "42"; await tick(); assert(!host.querySelector(".follow-button"), "本人资料不显示关注自己");
  app.unmount(); auth.userId = "1"; unknown = true;
  const unknownApp = createApp(Author,{id:42}).use(pinia).use(router).use(createI18n({legacy:false,locale:"zh-CN",messages:{"zh-CN":zh}}));
  unknownApp.mount(host); await tick(); await tick();
  assert(button().disabled && button().title.includes("刷新"), "未知状态禁用，提示刷新");
  unknownApp.unmount(); host.remove();
  document.querySelector("#follow-result")!.textContent = "PASS：关注、取消、失败恢复、防重、账号隔离、本人及未知状态";
}
run().catch(error => { document.querySelector("#follow-result")!.textContent = `FAIL：${String(error)}`; console.error(error); });
