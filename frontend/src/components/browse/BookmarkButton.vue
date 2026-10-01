<script setup lang="ts">
/**
 * 详情页收藏按钮（BrowseWorkView / BrowseNovelView 顶栏共用，bookmark-ui-v1）。
 *
 * - 状态来自详情响应 bookmarkState（父级持有并经 change 回写）：
 *   未收藏 = 空心 + 「收藏」；已收藏 = 实心 + 「已收藏」，私密收藏追加「私密」角标；
 * - 点击弹原生 details 小菜单（沿用侧栏账号菜单曾用的 details 模式）：
 *   未收藏 → [公开收藏][私密收藏]；已收藏 → [取消收藏]；
 * - 请求进行中禁用（aria-busy + 降透明度）；失败 notify 归一文案；
 *   成功经 change 通知父级更新本地态并 notify；
 * - 未登录错误由 api 层 invokeBrowse 统一派发登录弹窗事件，本组件不重复处理。
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  browseBookmarkAdd,
  browseBookmarkRemove,
  errorMessage,
  type BookmarkRestrict,
  type WorkBookmarkState,
} from "../../api/browse";
import { notify } from "../../ui/notify";

const props = defineProps<{
  /** 作品类型；manga/ugoira 归 illust 端点族（契约 kind 仅 illust | novel） */
  kind: "illust" | "manga" | "novel";
  id: number;
  /** 详情响应的收藏态；成功操作后经 change 回写父级 */
  state?: WorkBookmarkState | null;
}>();

const emit = defineEmits<{ (e: "change", state: WorkBookmarkState | null): void }>();

const { t } = useI18n();

const busy = ref(false);
const menuEl = ref<HTMLDetailsElement | null>(null);

const bookmarked = computed(() => !!props.state);
const isPrivate = computed(() => props.state?.restrict === 1);
const label = computed(() =>
  bookmarked.value ? t("browse.bookmark.bookmarked") : t("browse.bookmark.add")
);
/** 契约 kind 仅 illust | novel（illust 端点族覆盖插画/漫画/动图）。 */
const apiKind = computed<"illust" | "novel">(() => (props.kind === "novel" ? "novel" : "illust"));

function closeMenu(): void {
  if (menuEl.value) menuEl.value.open = false;
}

/** 点击菜单外任意处关闭（pointerdown 捕获当前按下位置，先于 click 生效）。 */
function onDocPointerDown(event: PointerEvent): void {
  if (menuEl.value?.open && !menuEl.value.contains(event.target as Node)) closeMenu();
}

onMounted(() => document.addEventListener("pointerdown", onDocPointerDown));
onBeforeUnmount(() => document.removeEventListener("pointerdown", onDocPointerDown));

async function add(restrict: BookmarkRestrict): Promise<void> {
  if (busy.value || bookmarked.value) return;
  busy.value = true;
  closeMenu();
  try {
    const { bookmarkId } = await browseBookmarkAdd(apiKind.value, props.id, restrict);
    emit("change", { bookmarkId, restrict });
    notify(restrict === 1 ? t("browse.bookmark.addedPrivate") : t("browse.bookmark.added"));
  } catch (err) {
    notify(errorMessage(err) || t("browse.bookmark.addFailed"));
  } finally {
    busy.value = false;
  }
}

async function remove(): Promise<void> {
  const current = props.state;
  if (busy.value || !current) return;
  busy.value = true;
  closeMenu();
  try {
    await browseBookmarkRemove(apiKind.value, props.id, current.bookmarkId);
    emit("change", null);
    notify(t("browse.bookmark.removed"));
  } catch (err) {
    notify(errorMessage(err) || t("browse.bookmark.removeFailed"));
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <details ref="menuEl" class="bm-menu" @keydown.escape="closeMenu">
    <!-- 触发器用原生 summary（自带 button 语义与键盘激活），busy 时降透明度并拦截指针 -->
    <summary class="bm-trigger" :class="{ busy, bookmarked }" :aria-busy="busy || undefined">
      <svg
        class="bm-heart"
        :class="{ filled: bookmarked }"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z" />
      </svg>
      <span class="bm-label">{{ label }}</span>
      <span v-if="bookmarked && isPrivate" class="bm-badge">{{ t("browse.bookmark.privateBadge") }}</span>
    </summary>
    <div class="bm-popup" role="menu" :aria-label="t('browse.bookmark.actionMenu')">
      <template v-if="!bookmarked">
        <button class="bm-option" type="button" role="menuitem" :disabled="busy" @click="add(0)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z" />
          </svg>
          {{ t("browse.bookmark.addPublic") }}
        </button>
        <button class="bm-option" type="button" role="menuitem" :disabled="busy" @click="add(1)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="4.5" y="10.5" width="15" height="10" rx="2" />
            <path d="M8 10.5V7a4 4 0 0 1 8 0v3.5" />
          </svg>
          {{ t("browse.bookmark.addPrivate") }}
        </button>
      </template>
      <button v-else class="bm-option" type="button" role="menuitem" :disabled="busy" @click="remove()">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z" />
          <line x1="4" y1="4" x2="20" y2="20" />
        </svg>
        {{ t("browse.bookmark.removeBookmark") }}
      </button>
    </div>
  </details>
</template>

<style scoped>
.bm-menu {
  position: relative;
  flex-shrink: 0;
}

/* 触发器：对齐 md-outlined-button 规格（40px 高、胶囊、primary 文字） */
.bm-trigger {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xs);
  min-height: 40px;
  padding: 0 var(--space-lg);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 45%, transparent);
  border-radius: 999px;
  color: var(--md-sys-color-primary);
  font-size: 14px;
  font-weight: 600;
  line-height: 1.4;
  cursor: pointer;
  list-style: none;
  user-select: none;
  transition: background-color 0.15s ease;
}

.bm-trigger::-webkit-details-marker {
  display: none;
}

.bm-trigger:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.bm-trigger:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

/* 请求进行中：禁用观感（pointer-events 拦截，防止再次触发菜单） */
.bm-trigger.busy {
  opacity: 0.6;
  pointer-events: none;
}

.bm-heart {
  width: 18px;
  height: 18px;
  flex-shrink: 0;
  stroke-width: 1.8;
}

.bm-heart.filled {
  fill: currentColor;
}

.bm-badge {
  padding: 0 var(--space-xs);
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.6;
}

/* 弹出菜单：surface-container + 12px 圆角 + 唯一合法轻阴影（DESIGN.md Overlay-Only Shadow Rule） */
.bm-popup {
  position: absolute;
  top: calc(100% + var(--space-xs));
  right: 0;
  z-index: 20;
  display: grid;
  gap: var(--space-xxs);
  min-width: 160px;
  padding: var(--space-sm);
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
  box-shadow: 0 4px 12px rgb(0 0 0 / 18%);
}

.bm-option {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
  min-height: 40px;
  padding: 0 var(--space-md);
  border: none;
  background: none;
  color: var(--ink);
  font: inherit;
  font-size: 14px;
  text-align: left;
  cursor: pointer;
}

.bm-option:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.bm-option:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: -2px;
}

.bm-option:disabled {
  opacity: 0.6;
  cursor: default;
}

.bm-option svg {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
  stroke-width: 1.8;
  color: var(--ink-muted);
}

@media (prefers-reduced-motion: reduce) {
  .bm-trigger {
    transition: none;
  }
}
</style>
