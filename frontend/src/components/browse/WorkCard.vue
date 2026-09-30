<script setup lang="ts">
/**
 * 作品卡片：封面 + 标题（两行省略）+ 作者行 + 左上角徽标（页数 / R-18）。
 * 整卡可点击（emit click），键盘可聚焦；无阴影，hover 用 8% primary 状态层（DESIGN.md）。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { pxSrc, type BrowseWorkItem } from "../../api/browse";

const props = defineProps<{
  item: BrowseWorkItem;
  disabled?: boolean;
}>();

const emit = defineEmits<{ (e: "click", item: BrowseWorkItem): void }>();

const { t } = useI18n();

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
</script>

<template>
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
</template>

<style scoped>
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
