/** 启动 dev.ps1 frontend 后，在新标签打开 /tests/navigation.html#/browse/home。 */
import { nextTick } from "vue";
import router from "../src/router";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

async function waitFor(check: () => boolean): Promise<void> {
  const deadline = Date.now() + 5000;
  while (!check()) {
    assert(Date.now() < deadline, "等待页面超时");
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

async function click(selector: string, expectedPath?: string): Promise<void> {
  await waitFor(() => !!document.querySelector(selector));
  (document.querySelector(selector) as HTMLElement).click();
  if (expectedPath) await waitFor(() => router.currentRoute.value.fullPath === expectedPath);
  await nextTick();
}

async function back(expectedPath: string): Promise<void> {
  const selector = router.currentRoute.value.path.startsWith("/browse/work/")
    ? ".work-topbar md-icon-button, .topbar md-icon-button"
    : ".page-back";
  await click(selector, expectedPath);
}

function checkInlineBack(): void {
  const button = document.querySelector(".page-back");
  assert(button?.closest(".page-heading, .author-heading"), "返回必须并入标题或资料行");
  assert(!button.textContent?.trim(), "图标返回不显示常驻文字");
  assert(button.getAttribute("aria-label") === button.getAttribute("title"), "返回保留一致的可访问名称与悬停提示");
}

async function run(): Promise<void> {
  await router.isReady();
  assert(!router.options.history.state.back, "请在新标签运行，保证无上游历史");
  assert(!document.querySelector(".page-back"), "首页无历史时隐藏返回按钮");

  // 深链首开都回首页，replace 不会制造「首页 ↔ 深链」循环。
  for (const path of ["/browse/user/100004", "/browse/work/novel/9000005", "/browse/work/illust/9000005", "/browse/series/novel/700012"]) {
    await router.replace(path);
    await nextTick();
    if (!path.startsWith("/browse/work/")) checkInlineBack();
    await back("/browse/home");
    assert(!router.options.history.state.back, "兜底不得把孤儿页重新压栈");
  }

  // 复现用户链路：真实点击首页卡片和作品作者，再逐层返回。
  await click(".work-card");
  await waitFor(() => router.currentRoute.value.path.startsWith("/browse/work/"));
  const workPath = router.currentRoute.value.fullPath;
  await click(".author-row");
  await waitFor(() => router.currentRoute.value.path.startsWith("/browse/user/"));
  await back(workPath);
  await back("/browse/home");

  // 相关推荐必须保留当前作品，不能覆盖它。
  await router.push("/browse/work/illust/9000005");
  await click(".side-panel .work-card");
  await waitFor(() => router.currentRoute.value.path !== "/browse/work/illust/9000005");
  await back("/browse/work/illust/9000005");
  await back("/browse/home");

  // 跨页面、多层堆栈、工具旧路径重定向与完整 query 恢复。
  const chain = [
    "/browse/search?word=landscape&kind=novel",
    "/browse/work/novel/9000005",
    "/browse/series/novel/700012",
    "/browse/user/100004",
    "/browse/work/manga/9000002",
    "/illustration?sourceType=single&sourceId=9000002",
    "/saucenao",
    "/browse/ranking?kind=novel&mode=daily",
  ];
  const visited = [router.currentRoute.value.fullPath];
  for (const path of chain) {
    await router.push(path);
    await nextTick();
    await new Promise((resolve) => setTimeout(resolve, 50));
    if (!path.startsWith("/browse/work/")) checkInlineBack();
    visited.push(router.currentRoute.value.fullPath);
  }
  for (const path of visited.slice(0, -1).reverse()) await back(path);

  // 回退后另走分支：旧的前进分支被原生历史替换，新分支仍可返回。
  await router.push("/browse/discover");
  await router.push("/browse/feed");
  await back("/browse/discover");
  await router.push("/browse/bookmark");
  await back("/browse/discover");
  await back("/browse/home");

  for (const path of ["/browse/illustration", "/browse/manga", "/browse/novel", "/browse/watchlist"]) {
    await router.push(path);
    await nextTick();
    checkInlineBack();
    await back("/browse/home");
  }

  // 真实 640px iframe 触发媒体查询，检查窄窗和长标题的返回点击区域。
  const frame = document.createElement("iframe");
  frame.width = "640";
  frame.height = "800";
  document.body.append(frame);
  try {
    for (const path of ["/browse/search", "/browse/user/100004", "/browse/series/novel/700012"]) {
      frame.src = `/#${path}`;
      await waitFor(() => frame.contentWindow?.location.hash === `#${path}` && !!frame.contentDocument?.querySelector(".page-back"));
      const doc = frame.contentDocument!;
      const button = doc.querySelector(".page-back")!;
      const box = button.getBoundingClientRect();
      assert(box.width === 40 && box.height === 40, "窄窗返回点击区域保持 40×40");
      const heading = doc.querySelector(".page-heading");
      if (heading) {
        const title = heading.querySelector("h1");
        if (title) title.textContent = "用于验证窄窗口换行的长标题".repeat(10);
        assert(heading.scrollWidth <= heading.clientWidth, "长标题不能挤出返回按钮或造成横向溢出");
      }
    }
  } finally {
    frame.remove();
  }
}

const result = document.querySelector("#navigation-result")!;
run().then(() => {
  result.textContent = "PASS：深链兜底、首页→详情→作者、相关推荐、多层返回、query、重定向与新分支";
  result.setAttribute("data-status", "passed");
}).catch((error: unknown) => {
  result.textContent = `FAIL：${String(error)}`;
  result.setAttribute("data-status", "failed");
  console.error(error);
});
