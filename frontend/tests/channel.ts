/** dev.ps1 frontend start 后打开 /tests/channel.html；确定性 IPC，不访问真实 Pixiv。 */
import { createApp, h, nextTick, reactive } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter } from "vue-router";
import Channel from "../src/views/browse/BrowseChannelView.vue";
import WorkGrid from "../src/components/browse/WorkGrid.vue";
import { useSettingsStore } from "../src/stores/settings";
import type { BrowseChannel, BrowseWorkItem } from "../src/api/browse";
import zhCN from "../src/locales/zh-CN";
import "../src/styles/main.css";
import "../src/material";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

const requests: { args: Record<string, unknown>; resolve: (data: BrowseChannel) => void; reject: (error: Error) => void }[] = [];
window.__TAURI_INTERNALS__ = {
  invoke: (_command: string, args: Record<string, unknown>) => new Promise<BrowseChannel>((resolve, reject) => {
    requests.push({ args, resolve, reject });
  }),
  convertFileSrc: (url: string) => url,
};
const pinia = createPinia();
const router = createRouter({ history: createMemoryHistory(), routes: [{ path: "/", component: Channel }, { path: "/browse/ranking", component: { template: "<div/>" } }] });
const i18n = createI18n({ legacy: false, locale: "zh-CN", messages: { "zh-CN": zhCN } });
const props = reactive({ kind: "illustration" as "illustration" | "manga" | "novel" });
const host = document.createElement("div");
document.body.append(host);
const app = createApp({ render: () => h(Channel, props) }).use(pinia).use(router).use(i18n);
const tick = async () => { await new Promise((resolve) => setTimeout(resolve, 0)); await nextTick(); };

function snapshot(label: string, restrict: number): BrowseChannel {
  const item: BrowseWorkItem = { id: restrict + 1, kind: "illust", title: label, author_id: 9, author_name: "测试作者", page_count: 1, x_restrict: restrict };
  const list = { items: [item, { ...item, id: 9, title: "混入普通作品", x_restrict: 0 }, { ...item, id: 10, title: "未知限制作品", x_restrict: undefined }], next_page: null, total: null };
  return { follow: list, recommend: list, ranking: list, new_post: list, ranking_date: "20261001", trending_tags: [], tag_sections: [{ tag: "BanG Dream!", items: [item] }] };
}

function select(label: string): void {
  const button = [...host.querySelectorAll<HTMLButtonElement>(".r18-bar button")].find((el) => el.textContent?.trim() === label);
  assert(button, `找到 ${label} 筛选按钮`);
  button.click();
}

async function run() {
  await router.push("/");
  useSettingsStore(pinia).settings.show_r18 = false;
  app.mount(host);
  await tick();
  assert(Number(requests.length) === 1, "挂载发起普通频道请求");
  select("R-18");
  await tick();
  assert(requests[1]?.args.mode === "r18", "R-18 必须请求服务端 mode=r18，加载中切换不能丢失");
  requests[1].resolve(snapshot("R-18 推荐作品", 1));
  await tick();
  assert(host.querySelectorAll(".work-card").length === 5, "推荐、排行、标签等五分区显示 R-18，不被全局关闭再次过滤");
  assert(!host.textContent?.includes("混入普通作品") && !host.textContent?.includes("未知限制作品"), "R-18 候选中的一般向与未知条目仍被过滤");
  assert(host.textContent?.includes("#BanG Dream!"), "显示官方标签推荐分区");
  requests[0].resolve(snapshot("迟到普通作品", 0));
  await tick();
  assert(!host.textContent?.includes("迟到普通作品"), "迟到响应不能覆盖当前档位");
  const rankingLink = [...host.querySelectorAll<HTMLElement>("md-text-button")].find((el) => el.textContent?.includes("查看完整榜单"));
  assert(rankingLink, "完整榜单入口存在");
  rankingLink.click();
  await tick();
  assert(router.currentRoute.value.query.mode === "daily_r18", "R-18 频道跳到 R-18 日榜");
  select("一般向");
  await tick();
  assert(requests[2]?.args.mode === "all", "一般向请求普通频道");
  assert(!host.textContent?.includes("R-18 推荐作品"), "跨档加载不显示旧卡片");
  requests[2].resolve(snapshot("普通推荐作品", 0));
  await tick();
  select("全部");
  await tick();
  assert(requests.length === 3, "一般向与全部使用同一普通快照，无重复请求");
  select("R-18");
  await tick();
  requests[3].reject(new Error("测试请求失败"));
  await tick();
  assert(host.textContent?.includes("测试请求失败"), "失败显示错误而非静默空列表");
  const retry = [...host.querySelectorAll<HTMLElement>("md-outlined-button, md-text-button")].find((el) => el.textContent?.includes("重试"));
  assert(retry, "错误态提供重试入口");
  retry.click();
  await tick();
  assert(requests[4]?.args.mode === "r18", "重试保留 R-18 服务端模式");
  requests[4].resolve(snapshot("重试成功作品", 1));
  await tick();
  assert(host.textContent?.includes("重试成功作品"), "重试恢复当前分区");
  props.kind = "manga";
  await tick();
  assert(requests[5]?.args.kind === "manga" && requests[5]?.args.mode === "all", "其他频道保持独立筛选");
  requests[5].resolve(snapshot("漫画普通作品", 0));
  await tick();
  assert(host.textContent?.includes("漫画普通作品"), "频道切换成功加载");
  // 共用网格仍默认遵守全局设置，频道手动档位不能外溢。
  const gridHost = document.createElement("div");
  document.body.append(gridHost);
  const grid = createApp(WorkGrid, { items: snapshot("受限作品", 1).recommend.items }).use(pinia).use(i18n);
  grid.mount(gridHost);
  await tick();
  assert(gridHost.querySelectorAll(".work-card").length === 1 && !gridHost.textContent?.includes("受限作品"), "其他网格继续遵守全局 R-18 关闭，仅显示一般向");
  grid.unmount();
  gridHost.remove();
}

const output = document.querySelector("#channel-result")!;
run().then(() => {
  output.textContent = "PASS：频道服务端 R-18 模式、推荐/排行/标签、独立筛选、竞态与错误";
  output.setAttribute("data-status", "passed");
}).catch((error: unknown) => {
  output.textContent = `FAIL：${String(error)}`;
  output.setAttribute("data-status", "failed");
}).finally(() => { app.unmount(); host.remove(); delete window.__TAURI_INTERNALS__; });
