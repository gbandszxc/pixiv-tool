<script setup lang="ts">
/**
 * 作品卡片：封面 + 标题（两行省略）+ 作者行 + 左上角徽标（页数 / R-18）。
 * 整卡可点击（emit click），键盘可聚焦；无阴影，hover 用 8% primary 状态层（DESIGN.md）。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { pxSrc, type BrowseWorkItem } from "../../api/browse";
import { notify } from "../../ui/notify";
import { fillDownloadForm, openInBrowser, workDownloadTarget } from "../../utils/pixivHooks";
import { pixivWorkUrl } from "../../utils/pixivUrl";

const props = defineProps<{
  item: BrowseWorkItem;
  disabled?: boolean;
  /** 封面快捷动作（打开原页 / 返填表单）：仅浏览频道页显式开启 */
  hooks?: boolean;
}>();

const emit = defineEmits<{ (e: "click", item: BrowseWorkItem): void }>();

const { t } = useI18n();

/** 返填按钮文案：小说 → 小说抓取页，其余 → 插画抓取页 */
const fillLabel = computed(() =>
  props.item.kind === "novel" ? t("browse.hooks.fillNovelForm") : t("browse.hooks.fillIllustForm")
);

/** 小说封面为 1:1.4 竖版，其余按 1:1 方形展示。 */
const portrait = computed(() => props.item.kind === "novel");

/** R-18 优先于页数徽标。 */
const badgeText = computed(() => {
  if (props.item.x_restrict === 1) return t("common.browseR18");
  if (props.item.x_restrict === 2) return t("common.browseR18G");
  return props.item.page_count > 1 ? `${props.item.page_count}P` : "";
});

const restricted = computed(() => props.item.x_restrict === 1 || props.item.x_restrict === 2);

function handleClick(): void {
  if (props.disabled) return;
  emit("click", props.item);
}

function handleOpenInBrowser(): void {
  void openInBrowser(pixivWorkUrl(props.item.kind, props.item.id)).catch(() =>
    notify(t("browse.hooks.openFailed"))
  );
}

function handleFillForm(): void {
  fillDownloadForm(workDownloadTarget(props.item));
}
</script>

<template>
  <div class="work-card-wrap">
    <div
      class="work-card"
      role="button"
      :tabindex="disabled ? -1 : 0"
      :aria-disabled="disabled || undefined"
      :class="{ disabled }"
      @click="handleClick"
      @keydown.enter.prevent="handleClick"
      @keydown.space.prevent="handleClick"
    >
      <div class="cover" :class="{ portrait }">
        <img v-if="item.cover" :src="pxSrc(item.cover)" alt="" loading="lazy" />
        <span v-if="badgeText" class="badge" :class="{ restricted }">{{ badgeText }}</span>
        <span v-if="item.kind === 'novel'" class="kind-mark" :title="t('nav.browseNovel')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M5 4.5A1.5 1.5 0 0 1 6.5 3H19v18H6.5A1.5 1.5 0 0 1 5 19.5z" />
            <path d="M5 19.5c0-.83.67-1.5 1.5-1.5H19" />
          </svg>
        </span>
      </div>
      <p class="title" :title="item.title">{{ item.title }}</p>
      <p class="author" :title="item.author_name">{{ item.author_name }}</p>
    </div>
    <!-- 快捷动作与 role="button" 卡片为兄弟节点：嵌套会被 ARIA children-presentational 从无障碍树抹掉 -->
    <div v-if="hooks && !disabled" class="cover-actions">
      <button
        class="cover-action"
        type="button"
        :aria-label="t('browse.hooks.openInBrowser')"
        :title="t('browse.hooks.openInBrowser')"
        @click="handleOpenInBrowser"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
          <polyline points="15 3 21 3 21 9" />
          <line x1="10" y1="14" x2="21" y2="3" />
        </svg>
      </button>
      <button
        class="cover-action"
        type="button"
        :aria-label="fillLabel"
        :title="fillLabel"
        @click="handleFillForm"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M12 3v12" /><path d="m7 10 5 5 5-5" /><path d="M4 21h16" />
        </svg>
      </button>
    </div>
  </div>
</template>

<style scoped>
.work-card-wrap {
  position: relative;
}

/* 快捷动作：默认隐形，hover / 键盘 focus-within 显现（封面右上角，位置 = 卡片 padding + 封面内缩） */
.cover-actions {
  position: absolute;
  top: calc(var(--space-xs) * 2);
  right: calc(var(--space-xs) * 2);
  display: flex;
  gap: var(--space-xs);
  opacity: 0;
  transition: opacity 0.15s ease;
}

.work-card-wrap:hover .cover-actions,
.work-card-wrap:focus-within .cover-actions {
  opacity: 1;
}

.cover-action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: none;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  cursor: pointer;
}

.cover-action:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.cover-action:active {
  background: color-mix(in srgb, var(--md-sys-color-primary) 12%, var(--md-sys-color-surface-container));
}

.cover-action:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.cover-action svg {
  width: 16px;
  height: 16px;
  stroke-width: 1.8;
}

@media (prefers-reduced-motion: reduce) {
  .cover-actions {
    transition: none;
  }
}

.work-card {
  display: block;
  padding: var(--space-xs);
  text-align: left;
  border-radius: var(--radius-control);
  cursor: pointer;
  outline: none;
  transition: background-color 0.15s ease;
}

.work-card:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.work-card:active {
  background: color-mix(in srgb, var(--md-sys-color-primary) 12%, transparent);
}

.work-card:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.work-card.disabled {
  cursor: default;
  opacity: 0.5;
  pointer-events: none;
}

.cover {
  position: relative;
  aspect-ratio: 1 / 1;
  border-radius: var(--radius-control);
  overflow: hidden;
  /* 主色底占位：封面未加载时可见（DESIGN.md token 派生） */
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.cover.portrait {
  aspect-ratio: 1 / 1.4;
}

.cover img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.badge {
  position: absolute;
  top: var(--space-xs);
  left: var(--space-xs);
  max-width: calc(100% - 2 * var(--space-xs));
  padding: 1px var(--space-sm);
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.badge.restricted {
  background: var(--ink);
  color: var(--md-sys-color-surface);
}

.kind-mark {
  position: absolute;
  right: var(--space-xs);
  bottom: var(--space-xs);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
  color: var(--ink-muted);
}

.kind-mark svg {
  width: 13px;
  height: 13px;
  stroke-width: 1.8;
}

.title,
.author {
  margin: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.title {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  margin-top: var(--space-sm);
  font-size: 14px;
  font-weight: 600;
  line-height: 1.4;
  color: var(--ink);
}

.author {
  margin-top: var(--space-xxs);
  font-size: 12px;
  line-height: 1.4;
  color: var(--ink-muted);
  white-space: nowrap;
}
</style>
