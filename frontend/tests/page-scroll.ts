/** /tests/page-scroll.html：真正布局与 DOM 按键路径，验证滚动区域选择及原生输入保护。 */
import { createApp, ref } from "vue";
import { usePageScroll } from "../src/composables/usePageScroll";

const main = document.querySelector<HTMLElement>("#main")!;
const left = document.querySelector<HTMLElement>("#left")!;
const right = document.querySelector<HTMLElement>("#right")!;
const output = document.querySelector<HTMLElement>("#result")!;
const app = createApp({ setup() { usePageScroll(ref(main)); return () => null; } });
app.mount("#app");
function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
function hover(element: HTMLElement): void {
  const rect = element.getBoundingClientRect();
  window.dispatchEvent(new PointerEvent("pointermove", { clientX: rect.x + 20, clientY: rect.y + 20 }));
}
function key(key: string, target: EventTarget = window, options: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, composed: true, cancelable: true, ...options });
  target.dispatchEvent(event);
  return event;
}
function run(): void {
  const step = Math.floor(left.clientHeight * 0.875);
  hover(left);
  assert(key("PageDown").defaultPrevented && left.scrollTop === step, "图片区域按屏滚动，保留 1/8 阅读重叠");
  assert(right.scrollTop === 0 && main.scrollTop === 0, "同屏其他滚动区域不移动");
  key("PageUp");
  assert(left.scrollTop === 0, "PgUp 回到顶部");
  key("PageDown", document.querySelector("#focus")!);
  assert(left.scrollTop === step && right.scrollTop === 0, "鼠标所在数据区优先于另一栏的键盘焦点");
  key("PageUp");
  hover(right);
  key("PageDown");
  assert(right.scrollTop === step && left.scrollTop === 0, "移动鼠标即可切到信息 / 评论区域");
  left.scrollTop = left.scrollHeight;
  hover(left);
  const last = left.scrollTop;
  key("PageDown");
  assert(left.scrollTop === last && main.scrollTop === 0, "子区域到底后不串动其他区域");
  window.dispatchEvent(new Event("blur"));
  right.scrollTop = 0;
  key("PageDown", document.querySelector("#focus")!);
  assert(right.scrollTop === step, "没有鼠标位置时跟随焦点区域");
  window.dispatchEvent(new Event("blur"));
  key("PageDown");
  assert(main.scrollTop === Math.floor(main.clientHeight * 0.875), "没有局部目标时滚动主内容");
  hover(right);
  const oldRight = right.scrollTop;
  assert(!key("PageDown", document.querySelector("#input")!).defaultPrevented && right.scrollTop === oldRight, "输入框按键交给浏览器");
  const shadow = document.querySelector("#shadow")!.attachShadow({ mode: "open" });
  const input = document.createElement("input"); shadow.append(input);
  const beforeShadowKey = right.scrollTop;
  assert(!key("PageDown", input).defaultPrevented && right.scrollTop === beforeShadowKey, "shadow 输入控件不被抢键");
  assert(!key("PageDown", window, { ctrlKey: true }).defaultPrevented, "修饰键保留系统行为");
  const modal = document.querySelector<HTMLDialogElement>("#modal")!;
  modal.showModal();
  window.dispatchEvent(new PointerEvent("pointermove", { clientX: 1, clientY: 1 }));
  const oldMain = main.scrollTop;
  const oldModalRight = right.scrollTop;
  key("PageDown");
  assert(main.scrollTop === oldMain && right.scrollTop === oldModalRight, "模态弹窗不滚动背景");
  const modalScroll = document.querySelector<HTMLElement>("#modal-scroll")!;
  hover(modalScroll);
  key("PageDown");
  assert(modalScroll.scrollTop > 0 && main.scrollTop === oldMain, "弹窗里的数据区支持按屏滚动");
  modal.close();
  app.unmount();
  assert(!key("PageDown").defaultPrevented, "卸载后清理全局监听");
}
try { run(); output.textContent = "PASS：双滚动区、方向与边界、焦点与主内容兜底、原生及 shadow 输入、模态隔离、监听清理"; output.dataset.status = "passed"; }
catch (error) { output.textContent = `FAIL：${error instanceof Error ? error.stack : String(error)}`; output.dataset.status = "failed"; }
