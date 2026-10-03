/** 真实更新弹窗 + 官方 IPC mock；不联网、不写更新包、不启动安装器。 */
import { createApp, h, nextTick, ref } from "vue";
import { createI18n } from "vue-i18n";
import { mockIPC } from "@tauri-apps/api/mocks";
import UpdateDialog from "../src/components/auth/UpdateDialog.vue";
import zh from "../src/locales/zh-CN";
import en from "../src/locales/en-US";
import "../src/styles/main.css";
import "../src/material";
import type { UpdateCheckInfo } from "../src/api/appUpdate";

const info: UpdateCheckInfo = { has_update:true, current_version:"1.2.0", latest_version:"1.3.0", release_url:"https://github.com/gbandszxc/pixiv-tool/releases/tag/v1.3.0", platform:"windows", package_type:"msi" };
let mode = "success", fallback = "installer", cancelDownload: (() => void) | undefined;
let directoryRequests = 0, downloads = 0;
mockIPC(async (command, payload) => {
  if (command === "cancel_app_update") { cancelDownload?.(); return; }
  if (command === "open_update_directory") { directoryRequests++; return; }
  if (command !== "download_app_update") return;
  downloads++;
  const channel = payload!.progress as { onmessage: (value: unknown) => void };
  const file = "Pixiv.Tool_1.3.0_x64_en-US.msi";
  channel.onmessage({ phase:"downloading", file_name:file, downloaded:32_000_000, total:50_000_000, bytes_per_second:2_400_000 });
  if (mode === "hold") return await new Promise((_, reject) => { cancelDownload = () => reject("更新下载已取消"); });
  await new Promise(resolve => setTimeout(resolve, 250));
  if (mode === "error") throw "更新包 SHA256 校验失败，请重新下载";
  channel.onmessage({ phase:"opening", file_name:file, downloaded:50_000_000, total:50_000_000, bytes_per_second:0 });
  await new Promise(resolve => setTimeout(resolve, 100));
  // 模拟 Channel 消息晚于 invoke 返回，完成引导不应被迟到进度覆盖。
  setTimeout(() => channel.onmessage({ phase:"opening", file_name:file, downloaded:50_000_000, total:50_000_000, bytes_per_second:0 }), 50);
  return { path:`C:\\Temp\\pixiv-tool-update\\1.3.0\\${file}`, package_type:info.package_type, installer_opened:fallback === "installer", directory_opened:fallback === "directory" };
});
const dialog = ref<InstanceType<typeof UpdateDialog>>();
const i18n = createI18n({ legacy:false, locale:"zh-CN", messages:{"zh-CN":zh,"en-US":en} });
createApp({ render:() => h(UpdateDialog, { ref:dialog }) }).use(i18n).mount("#app");
function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
const pause = (time = 30) => new Promise(resolve => setTimeout(resolve, time));
async function wait(check: () => boolean) { const deadline = Date.now()+2500; while (!check()) { assert(Date.now()<deadline,"等待状态超时"); await pause(); } await nextTick(); }
function button(text: string) { return [...document.querySelectorAll<HTMLElement>("dialog[open] md-text-button, dialog[open] md-filled-button, dialog[open] md-outlined-button")].find(el => el.textContent?.trim()===text)!; }
async function open() { dialog.value!.open(info); await nextTick(); }
async function close() { button("取消")?.click(); button("完成")?.click(); await nextTick(); }
const text = () => document.querySelector("dialog")!.textContent!;
async function run() {
  await open(); assert(text().includes("1.3.0"),"新版号缺失");
  mode="hold"; button("下载并安装").click(); await wait(() => text().includes("64%"));
  assert(text().includes("32.0 MB / 50.0 MB") && text().includes("2.4 MB/s"),"下载大小与速度缺失");
  assert(downloads===1,"出现重复下载");
  document.querySelector("dialog")!.dispatchEvent(new Event("cancel",{cancelable:true}));
  await wait(() => text().includes("下载已取消")); await close();
  mode="error"; await open(); button("下载并安装").click(); await wait(() => text().includes("校验失败"));
  assert(button("重新下载") && button("打开发布页"),"失败恢复入口缺失");
  mode="success"; button("重新下载").click(); await wait(() => text().includes("已请求系统"));
  await pause(100); assert(text().includes("已请求系统"),"迟到进度覆盖了安装引导");
  assert(document.querySelector("dialog")!.open,"安装引导自动关闭");
  button("打开下载目录").click(); await pause(); assert(directoryRequests===1,"目录未打开"); await close();
  fallback="directory"; info.platform="linux"; info.package_type="deb";
  await open(); button("下载并安装").click(); await wait(() => text().includes("已打开下载目录"));
  assert(text().includes("deb/rpm"),"Linux 安装引导缺失"); await close();
  fallback="none"; info.platform="macos"; info.package_type="dmg";
  await open(); button("下载并安装").click(); await wait(() => text().includes("均无法打开"));
  assert(text().includes("DMG") && text().includes("C:\\Temp"),"手动安装路径或 macOS 引导缺失"); await close();
  document.querySelector("#update-result")!.textContent="PASS: 进度、速度、取消、失败重试、目录回退、三平台引导";
}
// 验收后允许手动预览各状态与长文件名、语言、窄窗口、深色/减少动态效果。
Object.assign(window, { previewUpdate: async (state="hold", language="zh-CN", platform="windows", target="installer") => {
  document.querySelector("dialog")?.close(); mode=state; fallback=target; info.platform=platform;
  i18n.global.locale.value=language as "zh-CN" | "en-US";
  await open(); if (state!=="available") button(language==="zh-CN"?"下载并安装":"Download and install").click();
} });
run().catch(error => { document.querySelector("#update-result")!.textContent=`FAIL: ${error}`; throw error; });
