/** 真实更新弹窗 + 官方 IPC mock；不联网、不写更新包、不启动安装器。 */
import { createApp, h, nextTick, ref } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { mockIPC } from "@tauri-apps/api/mocks";
import UpdateDialog from "../src/components/auth/UpdateDialog.vue";
import { useTaskStore } from "../src/stores/tasks";
import { useTranslationStore } from "../src/stores/translation";
import zh from "../src/locales/zh-CN";
import en from "../src/locales/en-US";
import ja from "../src/locales/ja-JP";
import "../src/styles/main.css";
import "../src/material";
import type { UpdateCheckInfo } from "../src/api/appUpdate";
import type { TranslationJobSnapshot } from "../src/api/translation";

const info: UpdateCheckInfo = { has_update:true, current_version:"1.2.0", latest_version:"1.3.0", release_url:"https://github.com/gbandszxc/pixiv-tool/releases/tag/v1.3.0", platform:"windows", package_type:"msi" };
let mode = "success", installMode = "error", cancelDownload: (() => void) | undefined;
let directoryRequests = 0, downloads = 0, installs = 0;
let finishInstall: (() => void) | undefined;
let translationMode = "empty";
mockIPC(async (command, payload) => {
  if (command === "novel_translation_jobs") {
    if (translationMode === "error") throw "任务列表暂不可用";
    return { seq:0, jobs:translationMode === "active" ? [{ state:"running" }] : [] };
  }
  if (command === "cancel_app_update") { cancelDownload?.(); return; }
  if (command === "open_update_directory") { directoryRequests++; return; }
  if (command === "install_app_update") {
    installs++;
    if (installMode === "error") throw "无法启动安装程序，应用未退出";
    await new Promise<void>(resolve => { finishInstall = resolve; });
    return;
  }
  if (command !== "download_app_update") return;
  downloads++;
  const channel = payload!.progress as { onmessage: (value: unknown) => void };
  const file = "Pixiv.Tool_1.3.0_x64_en-US.msi";
  channel.onmessage({ phase:"downloading", file_name:file, downloaded:32_000_000, total:50_000_000, bytes_per_second:2_400_000 });
  if (mode === "hold") return await new Promise((_, reject) => { cancelDownload = () => reject("更新下载已取消"); });
  await new Promise(resolve => setTimeout(resolve, 250));
  if (mode === "error") throw "更新包 SHA256 校验失败，请重新下载";
  channel.onmessage({ phase:"downloading", file_name:file, downloaded:50_000_000, total:50_000_000, bytes_per_second:0 });
  setTimeout(() => channel.onmessage({ phase:"downloading", file_name:file, downloaded:40_000_000, total:50_000_000, bytes_per_second:0 }), 50);
  return { path:`C:\\Temp\\pixiv-tool-update\\1.3.0\\${file}`, package_type:info.package_type };
});
const dialog = ref<InstanceType<typeof UpdateDialog>>();
const i18n = createI18n({ legacy:false, locale:"zh-CN", messages:{"zh-CN":zh,"en-US":en,"ja-JP":ja} });
const pinia = createPinia();
createApp({ render:() => h(UpdateDialog, { ref:dialog }) }).use(pinia).use(i18n).mount("#app");
const tasks = useTaskStore(pinia);
const translations = useTranslationStore(pinia);
function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const pause = (time = 30) => new Promise(resolve => setTimeout(resolve, time));
async function wait(check: () => boolean) { const deadline = Date.now()+2500; while (!check()) { assert(Date.now()<deadline,"等待状态超时"); await pause(); } await nextTick(); }
function button(text: string) { return [...document.querySelectorAll<HTMLElement>("dialog[open] md-text-button, dialog[open] md-filled-button, dialog[open] md-outlined-button")].find(el => el.textContent?.trim()===text)!; }
async function open() { dialog.value!.open(info); await nextTick(); }
async function close() { button("取消")?.click(); button("稍后安装")?.click(); await nextTick(); }
const text = () => document.querySelector("dialog")!.textContent!;
async function run() {
  await open(); assert(text().includes("1.3.0"),"新版号缺失");
  mode="hold"; button("下载更新").click(); await wait(() => text().includes("64%"));
  assert(text().includes("32.0 MB / 50.0 MB") && text().includes("2.4 MB/s"),"下载大小与速度缺失");
  document.querySelector("dialog")!.dispatchEvent(new Event("cancel",{cancelable:true}));
  await nextTick(); assert(!document.querySelector("dialog")!.open,"Esc 没有收起后台下载");
  await open(); assert(text().includes("64%") && downloads===1,"后台进度未恢复或重复下载");
  button("取消下载").click(); await wait(() => text().includes("下载已取消")); await close();
  mode="error"; await open(); button("重新下载").click(); await wait(() => text().includes("校验失败"));
  assert(button("重新下载") && button("打开发布页"),"失败恢复入口缺失");
  mode="success"; button("重新下载").click(); await nextTick(); button("后台下载").click();
  await pause(400); assert(!document.querySelector("dialog")!.open,"下载完成强行重开弹窗");
  await open(); await wait(() => text().includes("更新已下载"));
  assert(installs===0,"下载完成自动启动安装");
  const completedDownloads=downloads;
  await close(); await open(); assert(downloads===completedDownloads && button("退出并安装更新"),"稍后安装后丢失完成包");
  await pause(100); assert(text().includes("更新已下载"),"迟到进度覆盖了完成状态");
  button("打开下载目录").click(); await pause(); assert(directoryRequests===1,"目录未打开");
  button("退出并安装更新").click(); await wait(() => text().includes("应用未退出"));
  assert(button("退出并安装更新") && button("重新下载"),"启动失败未保留恢复入口");
  tasks.syncFailed=true;
  button("退出并安装更新").click(); await wait(() => !!button("仍然退出并安装"));
  assert(button("仍然退出并安装") && installs===1,"任务状态未知时未确认或提前安装");
  await close(); tasks.syncFailed=false;
  const job={ job_id:"update-test", novel_id:123, state:"running" } as TranslationJobSnapshot;
  translations.jobs.set(job.job_id,job); translations.activeByNovel.set(job.novel_id,job.job_id);
  await open(); button("退出并安装更新").click(); await wait(() => !!button("仍然退出并安装"));
  assert(button("仍然退出并安装") && installs===1,"翻译任务未触发确认");
  button("仍然退出并安装").click(); await wait(() => installs===2 && !!button("退出并安装更新"));
  translations.jobs.clear(); translations.activeByNovel.clear();
  for (const snapshotMode of ["active", "error"]) {
    await close(); translationMode=snapshotMode; await open();
    button("退出并安装更新").click(); await wait(() => !!button("仍然退出并安装"));
    assert(installs===2,"远端活跃任务或列表失败时提前安装");
  }
  translationMode="empty";
  await close(); info.platform="macos"; info.package_type="dmg"; info.latest_version="1.3.1";
  await open(); button("下载更新").click(); await wait(() => !!button("退出并安装更新"));
  assert(text().includes("DMG") && text().includes("手动拖拽") && text().includes("C:\\Temp"),"macOS 指引或路径缺失");
  await close(); info.platform="linux"; info.package_type="deb"; info.latest_version="1.3.2";
  await open(); button("下载更新").click(); await wait(() => !!button("退出并安装更新"));
  assert(text().includes("deb/rpm"),"Linux 指引缺失");
  installMode="hold"; button("退出并安装更新").click(); await wait(() => installs===3);
  assert(installs===3 && !button("退出并安装更新"),"安装未锁定重复操作");
  document.querySelector("dialog")!.dispatchEvent(new Event("cancel",{cancelable:true}));
  assert(document.querySelector("dialog")!.open,"启动时 Esc 关闭弹窗");
  finishInstall?.();
  document.querySelector("#update-result")!.textContent="PASS: 进度、后台下载、取消、校验失败、稍后安装恢复、启动失败重试、任务确认、三平台引导、防重退出交接";
}
Object.assign(window, { previewUpdate: async (language="zh-CN") => {
  document.querySelector("dialog")?.close(); i18n.global.locale.value=language as "zh-CN" | "en-US" | "ja-JP";
  info.latest_version="1.3.3"; await open();
} });
run().catch(error => { document.querySelector("#update-result")!.textContent=`FAIL: ${error instanceof Error ? error.stack : error}`; throw error; });
