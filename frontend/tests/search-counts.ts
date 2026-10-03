/** /tests/search-counts.html：真实搜索视图 + 确定性 IPC，验证请求数量与排序。 */
import { createApp, nextTick } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter } from "vue-router";
import Search from "../src/views/browse/BrowseSearchView.vue";
import type { BrowseWorkItem } from "../src/api/browse";
import zhCN from "../src/locales/zh-CN";
import "../src/styles/main.css";
import "../src/material";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
const requests: { kind: string; ids: number[] }[] = [];
let missingBookmark = false;
window.__TAURI_INTERNALS__ = {
  invoke: async (command: string, args: Record<string, unknown>) => {
    if (command === "browse_search") {
      const items: BrowseWorkItem[] = [0, 8, 3].map((count, i) => ({
        id: i + (missingBookmark ? 10 : 1), kind: "novel", title: `作品${i}`,
        author_id: 1, author_name: "作者", page_count: 1,
        bookmark_count: missingBookmark && i === 1 ? undefined : count,
      }));
      return { items, total: 3, last_page: 1 };
    }
    if (command === "browse_work_counts") {
      const ids = args.ids as number[];
      requests.push({ kind: args.kind as string, ids });
      return { counts: Object.fromEntries(ids.map((id) => [id, {
        like_count: id, view_count: 100 - id,
        // 详情缺收藏数：保留列表原有值，未知字段垫底。
        ...(missingBookmark ? {} : { bookmark_count: id === 1 ? 0 : id === 2 ? 8 : 3 }),
      }])) };
    }
    return {};
  },
  convertFileSrc: (url: string) => url,
};
const router = createRouter({ history: createMemoryHistory(), routes: [{ path: "/browse/search", component: Search }] });
const app = createApp(Search).use(createPinia()).use(router)
  .use(createI18n({ legacy: false, locale: "zh-CN", messages: { "zh-CN": zhCN } }));
const host = document.createElement("div");
document.body.append(host);
const tick = async () => { await new Promise((resolve) => setTimeout(resolve, 0)); await nextTick(); };
function sort(key: string) {
  const select = host.querySelector<HTMLElement & { value: string }>(".local-sort md-outlined-select");
  assert(select, "排序控件存在");
  select.value = key;
  select.dispatchEvent(new Event("change", { bubbles: true }));
}
const titles = () => [...host.querySelectorAll(".work-card")].map((el) => el.textContent?.match(/作品\d/)?.[0]).join(",");
async function run() {
  await router.push("/browse/search?word=original&kind=novel");
  app.mount(host);
  await tick();
  sort("bookmark");
  await tick();
  assert(requests.length === 0, "列表收藏数完整时不请求详情，零值不算缺失");
  assert(titles() === "作品1,作品2,作品0", "复用收藏数正确降序");
  const directions = host.querySelectorAll<HTMLButtonElement>(".sort-direction-toggle button");
  directions[0].click();
  await tick();
  assert(titles() === "作品0,作品2,作品1" && requests.length === 0, "升序正确且不补取零值");
  directions[1].click();
  await tick();
  sort("like");
  await tick();
  assert(Number(requests.length) === 1 && requests[0].ids.join() === "1,2,3", "切换点赞仅补取当前页缺失字段");
  assert(titles() === "作品2,作品1,作品0", "补取后按点赞重排");
  sort("view");
  await tick();
  assert(Number(requests.length) === 1, "切换浏览数复用详情计数缓存");
  assert(titles() === "作品0,作品1,作品2", "浏览数排序正确");
  sort("default");
  await tick();
  missingBookmark = true;
  await router.push("/browse/search?word=other&kind=novel");
  await tick();
  sort("bookmark");
  await tick();
  assert(Number(requests.length) === 2 && requests[1].ids.join() === "11", "只补取缺失收藏数的作品");
  assert(titles() === "作品2,作品0,作品1", "已有零值仍可排序，缺失项垫底");
}
const output = document.querySelector("#search-result")!;
run().then(() => {
  output.textContent = "PASS：列表计数复用、零值、维度切换、缓存和缺失项补取";
  output.setAttribute("data-status", "passed");
}).catch((error: unknown) => {
  output.textContent = `FAIL：${String(error)}`;
  output.setAttribute("data-status", "failed");
}).finally(() => { app.unmount(); host.remove(); delete window.__TAURI_INTERNALS__; });
