/**
 * 评论发表 / 回复的可控 IPC 验收；不访问 Pixiv、不发真实评论。
 *
 * 覆盖：
 * 1. 首屏渲染 roots，草稿为空时「发表」禁用；
 * 2. 根评论发表：invoke 参数（kind/id/authorId/comment、无 parentId）、发送中禁用防重、
 *    成功后清空草稿并重拉首页（新评论出现在顶部）；
 * 3. 失败保留草稿与错误文案；未登录文案同时派发 OPEN_LOGIN_EVENT（弹登录窗）；
 * 4. 回复：入口只挂根评论，parentId = 该评论 id，成功后展开并重拉该评论回复第 1 页；
 * 5. 评论区关闭 / 作者 id 未知时不显示任何发表入口。
 */
import { createApp, nextTick } from "vue";
import { createI18n } from "vue-i18n";
import CommentsSection from "../src/components/browse/CommentsSection.vue";
import { OPEN_LOGIN_EVENT } from "../src/api/browse";
import zh from "../src/locales/zh-CN";
import "../src/styles/main.css";
import "../src/material";

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
const tick = async () => {
  await nextTick();
  await new Promise((r) => setTimeout(r, 0));
};
/** 轮询等待异步 UI 稳定（发表后重拉首页会经历一次列表刷新）。 */
async function waitFor(condition: () => boolean, message: string): Promise<void> {
  for (let i = 0; i < 50; i += 1) {
    await tick();
    if (condition()) return;
  }
  throw new Error(message);
}

type Field = HTMLElement & { shadowRoot: ShadowRoot | null };
type Button = HTMLElement & { disabled: boolean };

interface Row {
  id: string;
  user_id: number;
  user_name: string;
  content: string;
  stamp_url?: string;
  has_replies?: boolean;
}

let roots: Row[] = [];
let rootsOffsets: number[] = [];
let replyCalls: { commentId: unknown; page: unknown }[] = [];
let addCalls: Record<string, unknown>[] = [];
let closed = false;
let loginEvents = 0;
let pending: { resolve: (value: unknown) => void; reject: (error: unknown) => void } | null = null;

function resetState(): void {
  roots = [
    { id: "c1", user_id: 11, user_name: "评论者甲", content: "第一条评论", has_replies: true },
    { id: "c2", user_id: 12, user_name: "评论者乙", content: "第二条评论 (happy) 收尾" },
  ];
  rootsOffsets = [];
  replyCalls = [];
  addCalls = [];
  loginEvents = 0;
  pending = null;
}

window.addEventListener(OPEN_LOGIN_EVENT, () => {
  loginEvents += 1;
});

window.__TAURI_INTERNALS__ = {
  convertFileSrc: (url: string) => url,
  invoke: async (command: string, args: Record<string, unknown> = {}) => {
    if (command === "browse_work_comments") {
      rootsOffsets.push(Number(args.offset ?? 0));
      return closed ? { comments: [], disabled: true } : { comments: roots, next: null };
    }
    if (command === "browse_comment_replies") {
      replyCalls.push({ commentId: args.commentId, page: args.page });
      return {
        comments: [
          { id: `${String(args.commentId)}-r1`, user_id: 21, user_name: "回复者", content: "既有回复" },
        ],
        next: null,
      };
    }
    if (command === "browse_comment_add") {
      addCalls.push(args);
      const { promise, resolve, reject } = Promise.withResolvers<unknown>();
      pending = { resolve, reject };
      return await promise;
    }
    throw new Error(`unexpected command: ${command}`);
  },
};

function mount(authorId: number) {
  const host = document.createElement("div");
  document.body.append(host);
  const app = createApp(CommentsSection, { kind: "illust", id: 9000012, authorId });
  app.use(createI18n({ legacy: false, locale: "zh-CN", messages: { "zh-CN": zh } }));
  app.mount(host);
  return { host, app };
}

function find<T extends HTMLElement = HTMLElement>(host: Element, selector: string): T {
  const el = host.querySelector(selector);
  assert(el, `未找到元素：${selector}`);
  return el as T;
}

function innerTextarea(el: Field): HTMLTextAreaElement {
  const textarea = el.shadowRoot?.querySelector("textarea") ?? el.querySelector("textarea");
  assert(textarea instanceof HTMLTextAreaElement, "文本框内部 textarea 未渲染");
  return textarea;
}

function typeInto(el: Field, value: string): void {
  const textarea = innerTextarea(el);
  textarea.value = value;
  textarea.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
}

/** 按可见正文定位评论项（发表成功后新评论会插到顶部，不能按序号取）。 */
function commentItem(host: Element, text: string): HTMLElement {
  const item = [...host.querySelectorAll<HTMLElement>("li.comment")].find((li) =>
    li.textContent?.includes(text)
  );
  assert(item, `未找到正文含「${text}」的评论项`);
  return item;
}

const ROOT_FIELD = "section.comments > .composer md-outlined-text-field";
const ROOT_SUBMIT = "section.comments > .composer md-filled-button";

async function run(): Promise<void> {
  // ===== ① 首屏：评论列表 + 发表入口初始禁用 =====
  resetState();
  const { host, app } = mount(77);
  await waitFor(() => host.querySelectorAll("li.comment").length === 2, "首屏应渲染 2 条根评论");
  await waitFor(() => Boolean(host.querySelector(ROOT_FIELD)), "首屏加载后应显示发表入口");
  const rootField = find<Field>(host, ROOT_FIELD);
  const rootSubmit = find<Button>(host, ROOT_SUBMIT);
  assert(rootSubmit.disabled, "草稿为空时「发表」应禁用");
  assert(rootsOffsets.join(",") === "0", "首屏应拉 roots offset=0");

  // ===== ② 根评论发表：参数 / 防重 / 成功重拉 =====
  typeInto(rootField, "新评论内容");
  await tick();
  assert(!rootSubmit.disabled, "有草稿后「发表」应可用");
  rootSubmit.click();
  await tick();
  assert(addCalls.length === 1, "点击一次只应调用一次 browse_comment_add");
  const first = addCalls[0];
  assert(first.kind === "illust" && first.id === 9000012, "IPC 应带作品 kind 与 id");
  assert(first.authorId === 77, "IPC 应带作品作者 id（author_user_id）");
  assert(first.comment === "新评论内容", "IPC 应带草稿正文");
  assert(first.parentId === undefined, "根评论不应带 parentId");
  assert(rootSubmit.disabled, "发送中应禁用「发表」");
  rootSubmit.click();
  await tick();
  assert(addCalls.length === 1, "发送中重复点击不应再次提交");
  // 后端成功：新评论进入列表顶部，前端重拉首页
  roots = [{ id: "n1", user_id: 77, user_name: "我", content: "新评论内容" }, ...roots];
  pending!.resolve({ comment_id: "n1", user_id: 77, user_name: "我" });
  await waitFor(() => innerTextarea(rootField).value === "", "发表成功后应清空草稿");
  await waitFor(() => rootsOffsets.join(",") === "0,0", "发表成功后应重拉 roots 首页");
  assert(
    host.querySelectorAll("li.comment")[0]?.querySelector(".user-name")?.textContent === "我",
    "新评论应出现在列表顶部"
  );

  // ===== ③ 失败：保留草稿 + 错误文案；未登录文案联动登录弹窗 =====
  typeInto(rootField, "会失败的评论");
  await tick();
  rootSubmit.click();
  await tick();
  pending!.reject(new Error("评论发布响应缺少评论 ID，请刷新后重试"));
  await waitFor(() => rootField.hasAttribute("error"), "失败后文本框应进入 error 态");
  assert(innerTextarea(rootField).value === "会失败的评论", "失败后应保留草稿");
  assert(loginEvents === 0, "普通失败不应弹登录窗");

  rootSubmit.click();
  await tick();
  pending!.reject(new Error("未登录或登录态已失效，请先登录"));
  await waitFor(() => loginEvents === 1, "未登录文案应派发 OPEN_LOGIN_EVENT");
  assert(rootField.hasAttribute("error"), "未登录失败同样进入 error 态");

  // ===== ④ 回复：入口、parentId、展开并重拉回复首页 =====
  const target = commentItem(host, "第一条评论");
  find<HTMLElement>(target, ".reply-link").click();
  await tick();
  const replyField = find<Field>(target, ".reply-composer md-outlined-text-field");
  assert(
    innerTextarea(replyField).placeholder.includes("@评论者甲"),
    "回复框占位应带被回复者昵称"
  );
  typeInto(replyField, "回复内容");
  await tick();
  const replySubmit = find<Button>(target, ".reply-composer md-filled-button");
  assert(!replySubmit.disabled, "有草稿后回复按钮应可用");
  replySubmit.click();
  await tick();
  assert(addCalls.length === 4, "回复应调用 browse_comment_add");
  assert(addCalls[3].parentId === "c1", "回复应带 parentId = 被回复评论 id");
  assert(replyCalls.length === 0, "回复提交前不应拉取回复");
  pending!.resolve({ comment_id: "c1-r9", user_id: 77, user_name: "我", parent_id: "c1" });
  await waitFor(() => replyCalls.length === 1, "回复后应重拉该评论回复");
  assert(replyCalls[0].commentId === "c1", "回复后应重拉被回复评论的回复");
  assert(replyCalls[0].page === 1, "回复后应重拉回复第 1 页");
  await waitFor(() => !host.querySelector(".reply-composer"), "回复成功后应关闭回复框");
  assert(
    commentItem(host, "第一条评论").querySelector(".replies")?.textContent?.includes("既有回复"),
    "回复区应展开并显示服务端返回的回复"
  );

  // ===== ④b 官方表情：面板目录、插入文本表情、点选贴图即发、正文渲染 =====
  assert(host.querySelectorAll(".emoji-inline").length === 1, "正文里的 (code) 应渲染为行内表情图");
  const pickerMenu = find<HTMLElement>(host, "section.comments > .composer .emoji-menu");
  find<HTMLElement>(pickerMenu, ".emoji-trigger").click();
  await tick();
  assert(pickerMenu.hasAttribute("open"), "点触发器应展开表情面板");
  assert(
    pickerMenu.querySelectorAll(".emoji-cell").length === 38,
    "表情栏应渲染 38 个官方文本表情"
  );
  pickerMenu.querySelector<HTMLElement>(".emoji-cell")!.click();
  await tick();
  assert(
    innerTextarea(rootField).value.endsWith("(normal)"),
    "点选文本表情应把 (code) 追加到草稿"
  );
  const tabs = pickerMenu.querySelectorAll<HTMLElement>(".emoji-tab");
  assert(tabs.length === 2, "面板应有表情 / 贴图两栏");
  tabs[1].click();
  await tick();
  assert(pickerMenu.querySelectorAll(".stamp-cell").length === 40, "贴图栏应渲染 40 个官方贴图");
  pickerMenu.querySelector<HTMLElement>(".stamp-cell")!.click();
  await tick();
  assert(addCalls.length === 5, "点选贴图应调用 browse_comment_add");
  assert(
    addCalls[4].stampId === "301" && addCalls[4].comment === undefined,
    "贴图参数应只带 stampId（不带 comment）"
  );
  roots = [
    {
      id: "n2",
      user_id: 77,
      user_name: "我",
      content: "",
      stamp_url: "https://s.pximg.net/common/images/stamp/generated-stamps/301_s.jpg",
    },
    ...roots,
  ];
  pending!.resolve({ comment_id: "n2", user_id: 77, user_name: "我", stamp_id: "301" });
  await waitFor(() => rootsOffsets.join(",") === "0,0,0", "发表情贴图后应重拉 roots 首页");
  assert(
    host.querySelector("li.comment .stamp") !== null,
    "贴图评论应渲染 stamp 图"
  );
  app.unmount();
  host.remove();

  // ===== ⑤ 评论区关闭 / 作者未知：隐藏发表入口 =====
  resetState();
  closed = true;
  const closedView = mount(77);
  await waitFor(
    () => Boolean(closedView.host.textContent?.includes("作者已关闭评论区")),
    "评论区关闭应显示终态文案"
  );
  assert(!closedView.host.querySelector(".composer"), "评论区关闭时不应显示发表入口");
  closedView.app.unmount();
  closedView.host.remove();

  resetState();
  closed = false;
  const noAuthor = mount(0);
  await waitFor(() => noAuthor.host.querySelectorAll("li.comment").length === 2, "无作者 id 仍应展示评论");
  assert(!noAuthor.host.querySelector(".composer"), "作者 id 未知时不应显示发表入口");
  noAuthor.app.unmount();
  noAuthor.host.remove();

  document.querySelector("#comments-result")!.textContent =
    "PASS：发表参数、防重、成功重拉、失败保留草稿、登录联动、回复 parentId 与展开、官方表情目录/插入/贴图发表、正文表情渲染、关闭/未知作者隐藏入口";
}

run().catch((error) => {
  document.querySelector("#comments-result")!.textContent = `FAIL：${String(error)}`;
  console.error(error);
});
