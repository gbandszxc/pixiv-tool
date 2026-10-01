<script setup lang="ts">
/**
 * 系列分集·宫格模式：`repeat(auto-fill, minmax(160px,1fr))`（与 WorkGrid 同参）。
 * - illust：封面卡（1:1 封面 + 左上角 #N 序号胶囊（作品卡页数徽标 recipe）+
 *   R-18 右上角徽标（ink 底 surface 字）+ 标题两行截断）→ /browse/work/illust/{id}；
 * - novel：目录瓦片卡（大号 #N + 标题两行 + 字数元信息）→ /browse/work/novel/{id}。
 * 整卡 router-link 键盘可达；loading 显示纯色块骨架（无动画）。
 */
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { pxSrc, thumbSrc } from "../../api/browse";
import { useThumbTier } from "../../composables/useThumbTier";
import type { SeriesEpisodeView, SeriesKind } from "./seriesView";

const props = withDefaults(
  defineProps<{
    kind: SeriesKind;
    items: SeriesEpisodeView[];
    /** 首屏加载中（骨架色块；切页时父级保留旧内容，不走此态） */
    loading?: boolean;
    /** 骨架块数量 */
    skeletonCount?: number;
  }>(),
  { loading: false, skeletonCount: 12 }
);

const { t } = useI18n();

/** 网格封面档位（thumb_quality_grid，与 WorkCard 同源）。 */
const gridTier = useThumbTier("thumb_quality_grid");

/** 改写档位 404 的封面回落原始 URL（按条目 id 记忆，切换数据时随 props 重建）。 */
const failedCovers = ref(new Set<number>());

function coverSrc(ep: SeriesEpisodeView): string {
  return failedCovers.value.has(ep.id)
    ? pxSrc(ep.cover)
    : thumbSrc(ep.cover, gridTier.value);
}

function onCoverError(id: number): void {
  const next = new Set(failedCovers.value);
  next.add(id);
  failedCovers.value = next;
}

function episodeTarget(ep: SeriesEpisodeView): string {
  return props.kind === "novel"
    ? `/browse/work/novel/${ep.id}`
    : `/browse/work/illust/${ep.id}`;
}

function orderLabel(order: number): string {
  return `#${order}`;
}

function restrictLabel(ep: SeriesEpisodeView): string {
  if (ep.x_restrict === 1) return t("common.browseR18");
  if (ep.x_restrict === 2) return t("common.browseR18G");
  return "";
}

/** illust 卡副行：页数（>1 时）+ 日期；novel 瓦片：字数。 */
function metaText(ep: SeriesEpisodeView): string {
  const parts: string[] = [];
  if (props.kind === "novel") {
    if (ep.text_length != null) {
      parts.push(t("browse.series.words", { count: ep.text_length.toLocaleString() }));
    }
  } else if (ep.page_count != null && ep.page_count > 1) {
    parts.push(`${ep.page_count}P`);
  }
  if (ep.update_date) parts.push(ep.update_date);
  return parts.join(" · ");
}

const skeletons = computed(() => Array.from({ length: props.skeletonCount }, (_, i) => i + 1));
</script>

<template>
  <!-- 首屏骨架：纯色块，无动画 -->
  <div v-if="loading" class="ep-grid" aria-hidden="true">
    <div v-for="n in skeletons" :key="n" class="ep-card skeleton">
      <template v-if="kind === 'illust'">
        <div class="ep-cover-box"></div>
        <div class="sk-line w80"></div>
        <div class="sk-line w50"></div>
      </template>
      <template v-else>
        <div class="sk-order"></div>
        <div class="sk-line w90"></div>
        <div class="sk-line w40"></div>
      </template>
    </div>
  </div>

  <div v-else class="ep-grid">
    <!-- illust：封面卡 -->
    <template v-if="kind === 'illust'">
      <router-link
        v-for="ep in items"
        :key="ep.id"
        class="ep-card"
        :to="episodeTarget(ep)"
      >
        <div class="ep-cover-box">
          <img
            v-if="ep.cover"
            :src="coverSrc(ep)"
            alt=""
            loading="lazy"
            decoding="async"
            @error="onCoverError(ep.id)"
          />
          <span class="order-badge" aria-hidden="true">{{ orderLabel(ep.series_order) }}</span>
          <span v-if="restrictLabel(ep)" class="restrict-badge">{{ restrictLabel(ep) }}</span>
        </div>
        <p class="ep-title" :title="ep.title">{{ ep.title }}</p>
        <p v-if="metaText(ep)" class="ep-meta">{{ metaText(ep) }}</p>
      </router-link>
    </template>

    <!-- novel：目录瓦片卡 -->
    <template v-else>
      <router-link
        v-for="ep in items"
        :key="ep.id"
        class="tile-card"
        :to="episodeTarget(ep)"
      >
        <span class="tile-order" aria-hidden="true">{{ orderLabel(ep.series_order) }}</span>
        <span class="tile-title" :title="ep.title">{{ ep.title }}</span>
        <span v-if="metaText(ep)" class="tile-meta">{{ metaText(ep) }}</span>
      </router-link>
    </template>
  </div>
</template>

<style scoped>
.ep-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
  gap: var(--space-md) var(--space-lg);
}

/* ===== illust 封面卡（WorkCard 同 recipe） ===== */

.ep-card {
  display: block;
  padding: var(--space-xs);
  border-radius: var(--radius-control);
  color: inherit;
  text-decoration: none;
  outline: none;
  transition: background-color 0.15s ease;
}

.ep-card:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.ep-card:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.ep-cover-box {
  position: relative;
  aspect-ratio: 1 / 1;
  border-radius: var(--radius-control);
  overflow: hidden;
  contain: content;
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.ep-cover-box img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

/* #N 序号胶囊：与作品卡页数徽标同 recipe（左上角） */
.order-badge {
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
  font-variant-numeric: tabular-nums;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* R-18 徽标：右上角，ink 底 surface 字（同作品卡 restricted 徽标） */
.restrict-badge {
  position: absolute;
  top: var(--space-xs);
  right: var(--space-xs);
  padding: 1px var(--space-sm);
  border-radius: 999px;
  background: var(--ink);
  color: var(--md-sys-color-surface);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
}

.ep-title {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  margin: var(--space-sm) 0 0;
  overflow: hidden;
  font-size: 14px;
  font-weight: 600;
  line-height: 1.4;
  color: var(--ink);
  overflow-wrap: anywhere;
}

.ep-meta {
  margin: var(--space-xxs) 0 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  line-height: 1.4;
  color: var(--ink-muted);
}

/* ===== novel 目录瓦片卡 ===== */

.tile-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-xs);
  min-height: 120px;
  padding: var(--space-md);
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
  color: inherit;
  text-decoration: none;
  outline: none;
  transition: background-color 0.15s ease;
}

.tile-card:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.tile-card:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.tile-order {
  font-size: 20px;
  font-weight: 600;
  line-height: 1.3;
  color: var(--ink);
  font-variant-numeric: tabular-nums;
}

.tile-title {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
  font-size: 14px;
  font-weight: 600;
  line-height: 1.4;
  color: var(--ink);
  overflow-wrap: anywhere;
}

.tile-meta {
  margin-top: auto;
  font-size: 12px;
  color: var(--ink-subtle);
}

/* ===== 骨架 ===== */

.skeleton {
  pointer-events: none;
}

.sk-line {
  height: 12px;
  margin-top: var(--space-sm);
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
}

.sk-line.w80 { width: 80%; }
.sk-line.w90 { width: 90%; }
.sk-line.w50 { width: 50%; }
.sk-line.w40 { width: 40%; }

.sk-order {
  width: 36px;
  height: 22px;
  margin-top: var(--space-xs);
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
}

@media (prefers-reduced-motion: reduce) {
  .ep-card,
  .tile-card {
    transition: none;
  }
}
</style>
