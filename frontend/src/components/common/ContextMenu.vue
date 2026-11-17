<script setup lang="ts">
/**
 * 通用自定义右键菜单（pointer 定位，项目 M3 菜单 recipe）。
 *
 * - 由调用方以 `open / x / y / items` 受控驱动；条目选中后 emit `select(id)`，
 *   调用方负责关闭（本组件不自行改 `open`）；
 * - 视觉与账号菜单 / 搜索历史下拉同一配方：surface-container 底、`--radius-control`
 *   圆角、唯一合法轻阴影 `0 4px 12px rgb(0 0 0 / 18%)`、40px 菜单行、8% primary
 *   hover，无深色 scrim；
 * - `position: fixed` + Teleport 到 body：不受查看器 / 全屏浮层的层叠上下文与
 *   `container-type` 包含块影响；层级 900（全屏 800 之上、snackbar 1000 之下）；
 * - 视口内定位：默认落在指针处，右 / 下越界时翻到指针左侧 / 上方，四周保留 8px；
 * - 键盘：打开焦点进首个可用条目，↑↓ 循环、Home / End 跳首末，Enter / Space 原生
 *   激活；Esc 关闭并回焦触发前元素，Tab 直接关闭（不把焦点困在菜单里）；
 * - 点击外部、窗口 resize、任意滚动均关闭；调用方切换作品 / 全屏状态时应主动置
 *   `open=false`。
 */
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import type { ContextMenuEntry } from "./contextMenu";

const props = withDefaults(
  defineProps<{
    open: boolean;
    /** 视口坐标（clientX / clientY） */
    x: number;
    y: number;
    items: ContextMenuEntry[];
    /** 菜单可访问名称（如「图片操作」） */
    label?: string;
  }>(),
  { label: "" }
);

const emit = defineEmits<{
  (e: "select", id: string): void;
  (e: "close"): void;
}>();

const menuEl = ref<HTMLElement | null>(null);
const position = ref({ left: 0, top: 0 });
/** 首次测量完成前隐藏，避免越界时先闪一帧错误位置。 */
const placed = ref(false);
/** 打开前的焦点元素：关闭后回焦（菜单内的临时焦点不算）。 */
let restoreFocus: HTMLElement | null = null;

function dismiss(): void {
  emit("close");
}

function detachDismissers(): void {
  window.removeEventListener("resize", dismiss);
  window.removeEventListener("scroll", dismiss, true);
}

async function place(): Promise<void> {
  await nextTick();
  const el = menuEl.value;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  const margin = 8;
  let left = props.x;
  let top = props.y;
  if (left + rect.width > window.innerWidth - margin) {
    left = Math.max(margin, props.x - rect.width);
  }
  if (top + rect.height > window.innerHeight - margin) {
    top = Math.max(margin, props.y - rect.height);
  }
  position.value = { left, top };
  placed.value = true;
  const first = el.querySelector<HTMLButtonElement>('[role="menuitem"]:not(:disabled)');
  (first ?? el).focus({ preventScroll: true });
}

watch(
  () => props.open,
  async (open) => {
    if (open) {
      restoreFocus =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
      placed.value = false;
      position.value = { left: props.x, top: props.y };
      window.addEventListener("resize", dismiss);
      window.addEventListener("scroll", dismiss, true);
      await place();
    } else {
      detachDismissers();
      placed.value = false;
      const target = restoreFocus;
      restoreFocus = null;
      if (target && (!document.activeElement || document.activeElement === document.body || menuEl.value?.contains(document.activeElement))) {
        target.focus({ preventScroll: true });
      }
    }
  }
);

watch(
  () => [props.x, props.y] as const,
  () => {
    if (!props.open) return;
    // 已显示时只跟随新锚点重新定位，不重置 placed（避免菜单闪回透明态）
    position.value = { left: props.x, top: props.y };
    void place();
  }
);

onBeforeUnmount(() => {
  detachDismissers();
});

/** 菜单键盘形态：↑↓ / Home / End 在可用条目间移动，Esc / Tab 关闭。 */
function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape" || event.key === "Tab") {
    event.preventDefault();
    event.stopPropagation();
    dismiss();
    return;
  }
  if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
  event.preventDefault();
  event.stopPropagation();
  const items = Array.from(
    menuEl.value?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ?? []
  );
  if (!items.length) return;
  const current = items.indexOf(document.activeElement as HTMLButtonElement);
  let next = current;
  if (event.key === "ArrowDown") next = current + 1;
  else if (event.key === "ArrowUp") next = current - 1;
  else if (event.key === "Home") next = 0;
  else next = items.length - 1;
  if (next < 0) next = items.length - 1;
  if (next >= items.length) next = 0;
  items[next]?.focus();
}

function choose(item: ContextMenuEntry): void {
  if (item.disabled) return;
  emit("select", item.id);
}
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="ctx-layer">
      <!-- 透明遮罩只负责「点击外部关闭」，不做变暗 scrim（账号菜单同 recipe） -->
      <div
        class="ctx-backdrop"
        @pointerdown.prevent="dismiss"
        @contextmenu.prevent="dismiss"
      ></div>
      <div
        ref="menuEl"
        class="ctx-menu"
        :class="{ 'is-placed': placed }"
        :style="{ left: `${position.left}px`, top: `${position.top}px` }"
        role="menu"
        :aria-label="label || undefined"
        tabindex="-1"
        @keydown="onKeydown"
        @contextmenu.prevent
      >
        <button
          v-for="item in items"
          :key="item.id"
          type="button"
          role="menuitem"
          class="ctx-item"
          :disabled="item.disabled"
          @click="choose(item)"
        >
          <svg
            v-if="item.icon === 'save'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <!-- lucide download -->
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <path d="m7 10 5 5 5-5" />
            <path d="M12 15V3" />
          </svg>
          <svg
            v-else-if="item.icon === 'copy'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <!-- lucide copy -->
            <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
            <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
          </svg>
          <svg
            v-else-if="item.icon === 'fullscreen'"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <!-- lucide maximize -->
            <path d="M8 3H5a2 2 0 0 0-2 2v3" />
            <path d="M21 8V5a2 2 0 0 0-2-2h-3" />
            <path d="M3 16v3a2 2 0 0 0 2 2h3" />
            <path d="M16 21h3a2 2 0 0 0 2-2v-3" />
          </svg>
          <span class="ctx-label">{{ item.label }}</span>
        </button>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.ctx-layer {
  position: fixed;
  inset: 0;
  z-index: 900;
  pointer-events: none;
}

.ctx-backdrop {
  position: fixed;
  inset: 0;
  pointer-events: auto;
  background: transparent;
}

.ctx-menu {
  position: fixed;
  min-width: 200px;
  max-width: min(280px, calc(100vw - 16px));
  padding: var(--space-xxs);
  background: var(--md-sys-color-surface-container);
  border-radius: var(--radius-control);
  box-shadow: 0 4px 12px rgb(0 0 0 / 18%);
  pointer-events: auto;
  opacity: 0;
  transform: scale(0.98);
  transform-origin: top left;
  transition: opacity 0.15s ease, transform 0.15s ease;
}

.ctx-menu.is-placed {
  opacity: 1;
  transform: none;
}

.ctx-item {
  display: flex;
  align-items: center;
  gap: var(--space-md);
  width: 100%;
  min-height: 40px;
  padding: 0 var(--space-md);
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--ink);
  font: inherit;
  font-size: 14px;
  line-height: 1.4;
  text-align: left;
  white-space: nowrap;
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.ctx-item:hover:not(:disabled),
.ctx-item:focus {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.ctx-item:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: -2px;
}

.ctx-item:disabled {
  color: var(--ink-subtle);
  cursor: default;
}

.ctx-item svg {
  flex: none;
  width: 18px;
  height: 18px;
  color: var(--ink-muted);
}

.ctx-item:disabled svg {
  color: inherit;
}

.ctx-label {
  overflow: hidden;
  text-overflow: ellipsis;
}

@media (prefers-reduced-motion: reduce) {
  .ctx-menu,
  .ctx-item {
    transition: none;
  }
}
</style>
