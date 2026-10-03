/** dev.ps1 frontend 后打开 /tests/startup-viewer.html；固定五页样例，无网络与真实设置写入。 */
import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import ImageViewer from "../src/components/browse/ImageViewer.vue";
import router from "../src/router";
import { groupRoots } from "../src/router/navigation";
import { useSettingsStore } from "../src/stores/settings";
import zhCN from "../src/locales/zh-CN";
import "../src/styles/main.css";
import "../src/material";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
const pages = Array.from({ length: 5 }, (_, i) => ({
  original: `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="100" height="150"><rect width="100" height="150" fill="lightblue"/><text x="40" y="75">${i + 1}</text></svg>`)}`,
  width: 100, height: 150,
}));
const app = createApp(ImageViewer, { pages }).use(createPinia()).use(createI18n({
  legacy: false, locale: "zh-CN", messages: { "zh-CN": zhCN },
}));
const settings = useSettingsStore();
const host = document.querySelector<HTMLElement>("#viewer")!;
const output = document.querySelector<HTMLElement>("#result")!;
const key = async (key: string, options: KeyboardEventInit = {}) => {
  window.dispatchEvent(new KeyboardEvent("keydown", { key, cancelable: true, ...options }));
  await nextTick();
};
const label = () => host.querySelector(".fs-label")?.textContent?.trim();
async function wheel(target: Element, deltaY: number, options: WheelEventInit = {}) {
  const e = new WheelEvent("wheel", { deltaY, bubbles: true, cancelable: true, ...options });
  target.dispatchEvent(e);
  await nextTick();
  return e;
}

async function run() {
  await settings.fetchSettings();
  app.use(router);
  await router.isReady();
  await router.replace("/");
  assert(router.currentRoute.value.path === groupRoots.discover, "默认进入发现推荐首页");
  for (const page of Object.values(groupRoots)) {
    await settings.saveSettings({ startup_page: page });
    await settings.fetchSettings();
    await router.replace("/");
    assert(router.currentRoute.value.path === page, "保存的启动入口生效");
  }
  await router.replace("/browse/work/manga/42");
  assert(router.currentRoute.value.path === "/browse/work/manga/42", "深链不被启动偏好覆盖");
  await router.replace("/?sourceId=123&sourceType=single");
  assert(router.currentRoute.value.path === "/tools/tasks" && router.currentRoute.value.query.sourceId === "123" && router.currentRoute.value.query.downloadForm === "novel", "旧下载预填链接仍有效");
  await settings.saveSettings({ startup_page: groupRoots.discover });

  app.mount(host);
  // 图片 complete 在缓存命中时早于组件 load 回调；以真正可打开的入口状态为准。
  const deadline = Date.now() + 5000;
  while (!host.querySelector('[role="button"][title]')) {
    assert(Date.now() < deadline, "样例图片加载超时");
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  host.querySelector<HTMLElement>('[role="button"][title]')!.click();
  await nextTick();
  assert(host.querySelector(".fs-overlay"), "打开全屏");
  await key("PageUp");
  assert(label() === "第 1 / 5 页", "首页 PgUp 不越界");
  await key("PageDown");
  assert(label() === "第 2 / 5 页", "PgDn 单页前进");
  await key("PageUp");
  const stage = host.querySelector(".fs-stage")!;
  assert((await wheel(stage, 100)).defaultPrevented && label() === "第 2 / 5 页", "滚轮下翻页并阻止默认滚动");
  await wheel(stage, 100);
  assert(label() === "第 2 / 5 页", "密集滚轮限速");
  assert(!(await wheel(host.querySelector(".fs-filmstrip")!, 100)).defaultPrevented && label() === "第 2 / 5 页", "胶卷保持原生滚动");
  assert(!(await wheel(stage, 100, { ctrlKey: true })).defaultPrevented, "Ctrl 滚轮不抢占缩放");
  await new Promise(resolve => setTimeout(resolve, 270));
  await wheel(stage, -100);
  assert(label() === "第 1 / 5 页", "滚轮上后退");
  host.querySelector<HTMLElement>('md-icon-button[title="双图模式"]')!.click();
  await nextTick();
  await key("PageDown");
  assert(label() === "第 3-4 / 5 页", "默认 RTL 双页 PgDn 前进一组");
  await key("PageDown");
  assert(label() === "第 5 / 5 页", "奇数末页单独展示");
  await key("PageDown");
  assert(label() === "第 5 / 5 页", "末页不循环");
  await key("PageUp");
  assert(label() === "第 3-4 / 5 页", "双页 PgUp 后退一组");
  await key("PageDown", { ctrlKey: true });
  assert(label() === "第 3-4 / 5 页", "修饰键不抢占系统快捷键");
  await new Promise(resolve => setTimeout(resolve, 270));
  await wheel(stage, 100);
  assert(label() === "第 5 / 5 页", "双页滚轮前进一组");
  await new Promise(resolve => setTimeout(resolve, 270));
  await wheel(stage, -100);
  assert(label() === "第 3-4 / 5 页", "双页滚轮后退一组");
  const input = document.createElement("input");
  host.querySelector(".fs-overlay")!.append(input);
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "PageDown", bubbles: true, composed: true }));
  await nextTick();
  assert(label() === "第 3-4 / 5 页", "输入框按键不触发翻页");
  input.remove();
  await key("Escape");
  assert(!host.querySelector(".fs-overlay") && document.activeElement === host.querySelector(".stage-scroll"), "退出恢复舞台焦点");
}
run().then(() => { output.textContent = "PASS：默认与可选启动页、深链与预填、滚轮与限速、胶卷滚动、单双页 PgUp/PgDn 与边界、退出焦点"; output.dataset.status = "passed"; })
  .catch(error => { output.textContent = `FAIL：${error instanceof Error ? error.stack : String(error)}`; output.dataset.status = "failed"; });
