<script setup lang="ts">
/**
 * 评论表情面板（与 pixiv 网页一致的两栏，2026-10-04 依其前端 bundle 实测）：
 *
 * - 「表情」栏 = 官方文本表情（38 个）：点选触发 `emoji`，由父级把 `(code)` 插入草稿，
 *   面板保持展开（可连点多个，与网页一致）；
 * - 「贴图」栏 = 官方表情贴图（40 个可见 id）：点选**立即**触发 `stamp`（由父级发表情评论）
 *   并收起面板（与网页一致）。
 *
 * 目录内置在应用里（pixiv 同样是前端内置，无接口），图片经 `pixiv-img` 代理显示。
 * 打开/关闭沿用收藏菜单的原生 details + 点击面板外关闭配方。
 */
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  PIXIV_COMMENT_EMOJI,
  PIXIV_COMMENT_STAMPS,
  commentEmojiUrl,
  commentStampUrl,
  pxSrc,
} from "../../api/browse";

const emit = defineEmits<{
  /** 点选文本表情（参数为 code，正文里写作 `(code)`） */
  (e: "emoji", code: string): void;
  /** 点选表情贴图（参数为官方 stampId） */
  (e: "stamp", id: string): void;
}>();

const { t } = useI18n();

const menuEl = ref<HTMLDetailsElement | null>(null);
const tab = ref<"emoji" | "stamp">("emoji");

function close(): void {
  if (menuEl.value) menuEl.value.open = false;
}

/** 点击面板外任意处关闭（pointerdown 捕获当前按下位置，先于 click 生效）。 */
function onDocPointerDown(event: PointerEvent): void {
  if (menuEl.value?.open && !menuEl.value.contains(event.target as Node)) close();
}

onMounted(() => document.addEventListener("pointerdown", onDocPointerDown));
onBeforeUnmount(() => document.removeEventListener("pointerdown", onDocPointerDown));
</script>

<template>
  <details ref="menuEl" class="emoji-menu" @keydown.escape="close">
    <!-- 触发器：纯图标按钮（无文案），与顶栏图标动作同为 20px lucide 线性图标 -->
    <summary
      class="emoji-trigger"
      :aria-label="t('browse.comments.emoji')"
      :title="t('browse.comments.emoji')"
    >
      <svg
        class="emoji-icon"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <!-- lucide smile -->
        <circle cx="12" cy="12" r="10" />
        <path d="M8 14s1.5 2 4 2 4-2 4-2" />
        <line x1="9" x2="9.01" y1="9" y2="9" />
        <line x1="15" x2="15.01" y1="9" y2="9" />
      </svg>
    </summary>
    <div class="emoji-popup" role="dialog" :aria-label="t('browse.comments.emoji')">
      <div class="emoji-tabs" role="tablist">
        <button
          type="button"
          role="tab"
          class="emoji-tab"
          :class="{ current: tab === 'emoji' }"
          :aria-selected="tab === 'emoji'"
          @click="tab = 'emoji'"
        >
          {{ t("browse.comments.emojiTab") }}
        </button>
        <button
          type="button"
          role="tab"
          class="emoji-tab"
          :class="{ current: tab === 'stamp' }"
          :aria-selected="tab === 'stamp'"
          @click="tab = 'stamp'"
        >
          {{ t("browse.comments.stampTab") }}
        </button>
      </div>
      <div
        v-if="tab === 'emoji'"
        class="emoji-grid"
        role="group"
        :aria-label="t('browse.comments.emojiTab')"
      >
        <button
          v-for="emoji in PIXIV_COMMENT_EMOJI"
          :key="emoji.id"
          type="button"
          class="emoji-cell"
          :title="`(${emoji.code})`"
          :aria-label="`(${emoji.code})`"
          @click="emit('emoji', emoji.code)"
        >
          <img :src="pxSrc(commentEmojiUrl(emoji.id))" alt="" loading="lazy" />
        </button>
      </div>
      <div v-else class="stamp-grid" role="group" :aria-label="t('browse.comments.stampTab')">
        <button
          v-for="id in PIXIV_COMMENT_STAMPS"
          :key="id"
          type="button"
          class="stamp-cell"
          :title="t('browse.comments.stampSend')"
          :aria-label="t('browse.comments.stampSend') + ' ' + id"
          @click="close(); emit('stamp', id)"
        >
          <img :src="pxSrc(commentStampUrl(id))" alt="" loading="lazy" />
        </button>
      </div>
    </div>
  </details>
</template>

<style scoped>
.emoji-menu {
  position: relative;
  flex: none;
}

/* 触发器：28px 圆形图标按钮（与评论行内小动作同密度），hover 8% primary 状态层 */
.emoji-trigger {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 999px;
  color: var(--ink-muted);
  cursor: pointer;
  list-style: none;
}

.emoji-trigger::-webkit-details-marker {
  display: none;
}

.emoji-trigger:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
  color: var(--md-sys-color-primary);
}

.emoji-menu[open] .emoji-trigger {
  color: var(--md-sys-color-primary);
}

.emoji-trigger:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.emoji-icon {
  width: 20px;
  height: 20px;
}

/* 面板：贴输入框下方展开（输入框在本行之上，向下展开不遮挡草稿），
   surface-container 底 + 唯一合法轻阴影 */
.emoji-popup {
  position: absolute;
  top: calc(100% + var(--space-xxs));
  left: 0;
  z-index: 2;
  width: 288px;
  padding: var(--space-sm);
  background: var(--md-sys-color-surface-container);
  border-radius: var(--radius-control);
  box-shadow: 0 2px 8px rgb(0 0 0 / 18%);
}

.emoji-tabs {
  display: flex;
  gap: var(--space-xs);
  margin-bottom: var(--space-xs);
}

.emoji-tab {
  flex: 1;
  padding: var(--space-xxs) var(--space-sm);
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: var(--ink-muted);
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}

.emoji-tab.current {
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

.emoji-tab:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.emoji-grid,
.stamp-grid {
  display: grid;
  gap: var(--space-xxs);
  max-height: 184px;
  overflow-y: auto;
}

.emoji-grid {
  grid-template-columns: repeat(8, 1fr);
}

.stamp-grid {
  grid-template-columns: repeat(5, 1fr);
}

.emoji-cell,
.stamp-cell {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  cursor: pointer;
}

.emoji-cell {
  aspect-ratio: 1;
}

.stamp-cell {
  aspect-ratio: 1;
}

.emoji-cell:hover,
.stamp-cell:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.emoji-cell:focus-visible,
.stamp-cell:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: -2px;
}

.emoji-cell img {
  width: 24px;
  height: 24px;
}

.stamp-cell img {
  width: 44px;
  height: 44px;
}
</style>
