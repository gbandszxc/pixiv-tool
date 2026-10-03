/** /tests/detail.html：三类真实视图的计数、描述、标签导航；不访问 Pixiv。 */
import { createApp } from "vue";
import { createPinia } from "pinia";
import { createI18n } from "vue-i18n";
import { createMemoryHistory, createRouter } from "vue-router";
import Work from "../src/views/browse/BrowseWorkView.vue";
import Novel from "../src/views/browse/BrowseNovelView.vue";
import zhCN from "../src/locales/zh-CN";
import "../src/styles/main.css";
import "../src/material";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
const pinia = createPinia();
const i18n = createI18n({ legacy: false, locale: "zh-CN", messages: { "zh-CN": zhCN } });
const router = createRouter({ history: createMemoryHistory(), routes: [{ path: "/", component: { template: "<div/>" } }, { path: "/browse/search", component: { template: "<div/>" } }] });
let currentKind = "novel";
let missing = false;
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
window.__TAURI_INTERNALS__ = {
  invoke: async (command: string) => {
    if (command === "browse_work_detail") return {
      detail_kind: currentKind === "novel" ? "novel" : "illust",
      item: { id: 42, kind: currentKind, title: "测试作品", author_id: 1, author_name: "测试作者", page_count: 1, x_restrict: 0,
        tags: missing ? [] : ["BanG Dream! & 中文", "很长的标签名称".repeat(12)],
        description: missing ? "" : "第一行<br>第二行 &amp; 说明<p>第三行</p><script>不应显示脚本</script>",
        like_count: missing ? undefined : 0, bookmark_count: missing ? undefined : 1234, view_count: missing ? undefined : 56789 },
      pages: [{ original: "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg'/>" }],
      content: "小说正文第一段。\n小说正文第二段。", bookmarkState: null,
    };
    if (command === "browse_related") return { items: [], next_page: null };
    return {};
  },
  convertFileSrc: (url: string) => url,
};

async function run() {
  await router.push("/");
  for (const kind of ["novel", "illust", "manga"]) {
    currentKind = kind;
    const host = document.createElement("div");
    document.body.append(host);
    const app = createApp(kind === "novel" ? Novel : Work, { kind, id: 42 }).use(pinia).use(i18n).use(router);
    app.mount(host);
    await tick();
    await tick();
    try {
      const description = host.querySelector(".description")?.textContent;
      assert(description && description.includes("第一行\n第二行 & 说明"), `${kind} 描述正文存在、解码实体且保留换行`);
      assert(description.includes("第三行") && !description.includes("不应显示脚本"), `${kind} HTML 只转为安全文本`);
      assert(!host.querySelector(".description script"), `${kind} 不插入描述 HTML`);
      const counts = kind === "novel" ? host.querySelector(".work-meta")?.textContent : host.querySelector(".count-row")?.textContent;
      assert(counts?.includes("1,234") && counts.includes("56,789") && /\b0\b/.test(counts), `${kind} 三项计数包含零值并按千位格式化`);
      const tag = host.querySelector<HTMLAnchorElement>("a.tag-chip");
      assert(tag && tag.getAttribute("href")?.includes("browse/search"), `${kind} 标签为原生搜索链接`);
      tag.click();
      await tick();
      assert(router.currentRoute.value.query.word === "BanG Dream! & 中文" && router.currentRoute.value.query.kind === kind, `${kind} 标签保留原文与作品类型进入搜索`);
    } finally { app.unmount(); host.remove(); }
  }
  missing = true;
  currentKind = "novel";
  const host = document.createElement("div");
  document.body.append(host);
  const app = createApp(Novel, { kind: "novel", id: 42 }).use(pinia).use(i18n).use(router);
  app.mount(host);
  await tick();
  assert(!host.querySelector(".description, .tag-row"), "空描述与标签不生成空白分区");
  assert(!host.textContent?.includes("undefined") && !host.textContent?.includes("NaN"), "缺失计数不产生错误文本");
  app.unmount();
  host.remove();
}
const output = document.querySelector("#detail-result")!;
run().then(() => {
  output.textContent = "PASS：插画/漫画/小说计数、描述换行与安全文本、标签搜索、缺失字段";
  output.setAttribute("data-status", "passed");
}).catch((error: unknown) => {
  output.textContent = `FAIL：${String(error)}`;
  output.setAttribute("data-status", "failed");
}).finally(() => { delete window.__TAURI_INTERNALS__; });
