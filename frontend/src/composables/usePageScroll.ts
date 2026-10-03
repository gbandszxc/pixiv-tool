import { onBeforeUnmount, onMounted, type Ref } from "vue";

let pointer: { x: number; y: number } | undefined;

function parentElement(element: Element): Element | null {
  const root = element.getRootNode();
  return element.parentElement ?? (root instanceof ShadowRoot ? root.host : null);
}

/** 只找实际有纵向滚动内容的容器，隐藏、仅裁剪与只有横向滚动的区域不参与。 */
function scrollContainer(origin: Element | null, scope?: HTMLElement): HTMLElement | null {
  for (let element = origin; element; element = parentElement(element)) {
    if (element instanceof HTMLElement && element.clientHeight > 0 &&
        element.scrollHeight > element.clientHeight &&
        /^(auto|scroll|overlay)$/.test(getComputedStyle(element).overflowY)) return element;
    if (element === scope) break;
  }
  return null;
}

function withinScope(element: Element, scope: HTMLElement): boolean {
  for (let node: Element | null = element; node; node = parentElement(node)) {
    if (node === scope) return true;
  }
  return false;
}

/** 鼠标所在的数据区优先，焦点次之；模态窗口中的滚动不得穿透到背景。 */
export function pageScrollTarget(event: KeyboardEvent, fallback?: HTMLElement): HTMLElement | null {
  const scope = [...document.querySelectorAll<HTMLElement>('dialog[open], [role="dialog"][aria-modal="true"]')]
    .filter(element => element.getClientRects().length).pop();
  const hovered = pointer ? document.elementFromPoint(pointer.x, pointer.y) : null;
  // 鼠标已回到图片区时，不能因焦点还留在胶卷按钮上继续滚动胶卷。
  if (hovered) return !scope || withinScope(hovered, scope) ? scrollContainer(hovered, scope) : null;
  const focused = event.composedPath().find(node => node instanceof Element) as Element | undefined;
  for (const origin of [focused, fallback]) {
    if (origin && (!scope || withinScope(origin, scope))) {
      const target = scrollContainer(origin, scope);
      if (target) return target;
    }
  }
  return scope ? scrollContainer(scope, scope) : null;
}

/** 全局普通滚动：与滚轮一样选中内容区域，PgUp/PgDn 滚动一屏，不切列表数据页。 */
export function usePageScroll(fallback: Ref<HTMLElement | undefined | null>): void {
  function onPointerMove(event: MouseEvent): void {
    pointer = { x: event.clientX, y: event.clientY };
  }
  function clearPointer(): void { pointer = undefined; }
  function onPointerOut(event: PointerEvent): void {
    if (!event.relatedTarget) clearPointer();
  }
  function onKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented || !["PageUp", "PageDown"].includes(event.key) ||
        event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return;
    // composedPath 可识别 Material Web 的 shadow 输入框、下拉和滑杆。
    if (event.composedPath().some(node => node instanceof HTMLElement &&
        (node.matches('input, textarea, select, [role="combobox"], [role="listbox"], [role="slider"]') || node.isContentEditable))) return;
    const target = pageScrollTarget(event, fallback.value ?? undefined);
    if (!target) return;
    event.preventDefault();
    // Chromium/Windows 的按屏滚动保留 1/8 重叠，长图和正文不会失去阅读衔接。
    const step = Math.max(1, Math.floor(target.clientHeight * 0.875));
    target.scrollBy({ top: (event.key === "PageDown" ? 1 : -1) * step, behavior: "auto" });
  }
  onMounted(() => {
    window.addEventListener("pointermove", onPointerMove, { passive: true });
    window.addEventListener("pointerdown", onPointerMove, { passive: true });
    window.addEventListener("wheel", onPointerMove, { passive: true });
    window.addEventListener("pointerout", onPointerOut, { passive: true });
    window.addEventListener("blur", clearPointer);
    // 冒泡阶段：全屏图片的 capture 翻页与控件自身按键优先。
    window.addEventListener("keydown", onKeydown);
  });
  onBeforeUnmount(() => {
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerdown", onPointerMove);
    window.removeEventListener("wheel", onPointerMove);
    window.removeEventListener("pointerout", onPointerOut);
    window.removeEventListener("blur", clearPointer);
    window.removeEventListener("keydown", onKeydown);
    clearPointer();
  });
}
