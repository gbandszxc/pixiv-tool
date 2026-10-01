<script setup lang="ts">
/**
 * 系列分集·列表模式：行式目录（surface-container 圆角容器 + 行间 divider +
 * hover 8% primary 状态层，沿用原小说系列目录行 recipe）。
 * - novel 行：两位序号 + 标题 + 字数/日期/R-18 右对齐（保持现状）；
 * - illust 行：48px 小封面 + #N + 标题 + 页数/日期右对齐。
 * 整行 router-link 键盘可达；loading 显示纯色块行骨架（无动画）。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { pxSrc, thumbSrc } from "../../api/browse";
import { useThumbTier } from "../../composables/useThumbTier";
import type { SeriesEpisodeView, SeriesKind } from "./seriesView";

const props = withDefaults(
  defineProps<{
    kind: SeriesKind;
    items: SeriesEpisodeView[];
    /** 首屏加载中（行骨架；切页时父级保留旧内容，不走此态） */
    loading?: boolean;
    /** 骨架行数量 */
    skeletonCount?: number;
  }>(),
  { loading: false, skeletonCount: 8 }
);

const { t } = useI18n();

/** 行内小封面档位（与网格同源）。 */
const gridTier = useThumbTier("thumb_quality_grid");

function rowTarget(ep: SeriesEpisodeView): string {
  return props.kind === "novel"
    ? `/browse/work/novel/${ep.id}`
    : `/browse/work/illust/${ep.id}`;
}

/** novel 行序号保持两位补零（现状）；illust 行用 #N。 */
function orderLabel(order: number): string {
  return props.kind === "novel" ? String(order).padStart(2, "0") : `#${order}`;
}

function thumbSrcOf(ep: SeriesEpisodeView): string {
  // 列表小封面直接走档位改写；失败由 onError 兜底显示占位底色（不整行重试）
  return thumbSrc(ep.cover, gridTier.value);
}

function restrictLabel(ep: SeriesEpisodeView): string {
  if (ep.x_restrict === 1) return t("common.browseR18");
  if (ep.x_restrict === 2) return t("common.browseR18G");
  return "";
}

/** 右对齐元信息：novel = 字数 + 日期；illust = 页数（>1）+ 日期。 */
function metaParts(ep: SeriesEpisodeView): string[] {
  const parts: string[] = [];
  if (props.kind === "novel") {
    if (ep.text_length != null) parts.push(t("browse.series.words", { count: ep.text_length.toLocaleString() }));
  } else if (ep.page_count != null && ep.page_count > 1) {
    parts.push(`${ep.page_count}P`);
  }
  if (ep.update_date) parts.push(ep.update_date);
  return parts;
}

const skeletons = computed(() => Array.from({ length: props.skeletonCount }, (_, i) => i + 1));
</script>

<template>
  <!-- 首屏骨架：纯色块行，无动画 -->
  <div v-if="loading" class="ep-list" aria-hidden="true">
    <div v-for="n in skeletons" :key="n" class="ep-row sk-row">
      <span v-if="kind === 'illust'" class="row-thumb"></span>
      <span class="row-order"></span>
      <span class="row-title"></span>
      <span class="row-meta"></span>
    </div>
  </div>

  <nav v-else class="ep-list" :aria-label="t('browse.series.listLabel')">
    <router-link v-for="ep in items" :key="ep.id" class="ep-row" :to="rowTarget(ep)">
      <img
        v-if="kind === 'illust' && ep.cover"
        class="row-thumb"
        :src="thumbSrcOf(ep)"
        alt=""
        loading="lazy"
        decoding="async"
      />
      <span class="row-order" aria-hidden="true">{{ orderLabel(ep.series_order) }}</span>
      <span class="row-title" :title="ep.title">{{ ep.title }}</span>
      <span class="row-meta">
        <span v-for="part in metaParts(ep)" :key="part">{{ part }}</span>
        <span v-if="restrictLabel(ep)" class="r18-pill">{{ restrictLabel(ep) }}</span>
      </span>
    </router-link>
  </nav>
</template>

<style scoped>
.ep-list {
  border-radius: 16px;
  background: var(--md-sys-color-surface-container);
  overflow: hidden;
}

.ep-row {
  display: flex;
  align-items: center;
  gap: var(--space-md);
  min-height: 48px;
  padding: var(--space-xs) var(--space-lg);
  color: var(--ink);
  text-decoration: none;
  outline: none;
  transition: background-color 0.15s ease;
}

.ep-row + .ep-row {
  border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 30%, transparent);
}

.ep-row:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.ep-row:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: -2px;
}

.row-thumb {
  width: 48px;
  height: 48px;
  border-radius: var(--radius-control);
  object-fit: cover;
  flex-shrink: 0;
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.row-order {
  min-width: 32px;
  color: var(--ink-subtle);
  font-size: 12px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}

.row-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 14px;
}

.row-meta {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--space-md);
  color: var(--ink-subtle);
  font-size: 12px;
  white-space: nowrap;
  flex-shrink: 0;
}

.r18-pill {
  padding: 0 var(--space-sm);
  border-radius: 999px;
  background: var(--ink);
  color: var(--md-sys-color-surface);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.6;
}

/* ===== 骨架（容器底为 surface-container，骨架条用 8% on-surface 状态层色） ===== */

.sk-row .row-thumb {
  background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.sk-row .row-order {
  width: 32px;
  height: 14px;
  min-width: 32px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.sk-row .row-title {
  height: 14px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.sk-row .row-meta {
  width: 30%;
  height: 14px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

@media (max-width: 640px) {
  .ep-row {
    flex-wrap: wrap;
    row-gap: var(--space-xxs);
    padding: var(--space-sm) var(--space-md);
  }

  .row-title {
    /* 序号/封面之后换行：标题独占一行，元信息落到下一行 */
    flex-basis: calc(100% - 92px);
    white-space: normal;
  }

  .row-meta {
    flex-basis: 100%;
    justify-content: flex-start;
  }
}

@media (prefers-reduced-motion: reduce) {
  .ep-row {
    transition: none;
  }
}
</style>
