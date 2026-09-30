<script setup lang="ts">
/**
 * 小说系列目录页（/browse/series/:id，browse-ui-v1 / F4）。
 *
 * 系列头（封面 / 标题 / 作者 / 简介 / 统计）+ 行式目录列表（序号 / 标题 / 右对齐元信息），
 * browseNovelSeries(id, lastOrder) 游标分页，「加载更多」按钮触达下一批（目录场景比
 * IntersectionObserver 更稳）。行 hover 用 8% primary 状态层，整行 router-link 键盘可达。
 */
import { computed, ref, shallowRef, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  browseNovelSeries,
  errorMessage,
  pxSrc,
  type BrowseSeriesDetail,
} from "../../api/browse";

type SeriesEpisode = BrowseSeriesDetail["contents"][number];

const props = defineProps<{ id: number }>();

const { t } = useI18n();

// ===== 数据（游标分页） =====

const seriesInfo = shallowRef<BrowseSeriesDetail | null>(null);
const contents = shallowRef<SeriesEpisode[]>([]);
/** 下一批游标；null = 已到底。 */
const nextLastOrder = ref<number | null>(null);
const loading = ref(false);
const error = ref("");
const loadingMore = ref(false);
const moreError = ref("");

async function load(): Promise<void> {
  loading.value = true;
  error.value = "";
  moreError.value = "";
  seriesInfo.value = null;
  contents.value = [];
  nextLastOrder.value = null;
  // 进入/切换系列回到页首（SPA 内路由切换会保留上一页滚动位置）
  window.scrollTo(0, 0);
  try {
    const data = await browseNovelSeries(props.id, 0);
    seriesInfo.value = data;
    contents.value = data.contents;
    nextLastOrder.value = data.next_last_order ?? null;
  } catch (err) {
    error.value = errorMessage(err) || t("common.browseLoadFailed");
  } finally {
    loading.value = false;
  }
}

async function loadMore(): Promise<void> {
  if (nextLastOrder.value == null || loadingMore.value) return;
  loadingMore.value = true;
  moreError.value = "";
  try {
    const data = await browseNovelSeries(props.id, nextLastOrder.value);
    seriesInfo.value = data;
    contents.value = contents.value.concat(data.contents);
    nextLastOrder.value = data.next_last_order ?? null;
  } catch (err) {
    moreError.value = errorMessage(err) || t("common.browseLoadFailed");
  } finally {
    loadingMore.value = false;
  }
}

watch(() => props.id, load, { immediate: true });

// ===== 头部展示 =====

const cover = computed(() => pxSrc(seriesInfo.value?.cover));

/** caption 剥 HTML 标签为纯文本（简介里的排版标记不渲染）。 */
const caption = computed(() => (seriesInfo.value?.caption ?? "").replace(/<[^>]*>/g, "").trim());

const statusLabel = computed(() =>
  seriesInfo.value?.is_concluded ? t("browse.series.concluded") : t("browse.series.ongoing")
);

/** 总字数：契约无全系列字段，按已加载章节的 text_length 求和（title 提示口径）。 */
const totalWords = computed(() =>
  contents.value.reduce((sum, ep) => sum + (ep.text_length ?? 0), 0)
);

const statsText = computed(() => {
  const info = seriesInfo.value;
  if (!info) return "";
  const parts = [t("browse.series.episodes", { count: info.total.toLocaleString() }), statusLabel.value];
  if (totalWords.value > 0) {
    parts.push(t("browse.series.totalWords", { count: totalWords.value.toLocaleString() }));
  }
  return parts.join(" · ");
});

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

function episodeWords(ep: SeriesEpisode): string {
  return ep.text_length != null ? t("browse.series.words", { count: ep.text_length.toLocaleString() }) : "";
}

function episodeDate(ep: SeriesEpisode): string {
  return ep.update_date ? ep.update_date.slice(0, 10) : "";
}

function episodeRestrict(ep: SeriesEpisode): string {
  if (ep.x_restrict === 1) return t("common.browseR18");
  if (ep.x_restrict === 2) return t("common.browseR18G");
  return "";
}
</script>

<template>
  <div class="page-view">
    <!-- 首屏骨架：头部色块 + 列表行色块，无动画 -->
    <div v-if="loading" aria-hidden="true">
      <div class="series-head">
        <div class="sk sk-cover"></div>
        <div class="head-info">
          <div class="sk sk-title"></div>
          <div class="sk sk-line w40"></div>
          <div class="sk sk-line w90"></div>
          <div class="sk sk-line w60"></div>
        </div>
      </div>
      <div class="series-list">
        <div v-for="n in 8" :key="n" class="series-row sk-row">
          <div class="sk sk-order"></div>
          <div class="sk sk-line"></div>
          <div class="sk sk-line w30"></div>
        </div>
      </div>
    </div>

    <!-- 错误态：文案 + 重试 -->
    <div v-else-if="error" class="series-state" role="alert">
      <p class="state-text strong">{{ error }}</p>
      <md-outlined-button @click="load">{{ t("common.retry") }}</md-outlined-button>
    </div>

    <template v-else-if="seriesInfo">
      <!-- 系列头 -->
      <header class="series-head">
        <img v-if="cover" class="cover" :src="cover" alt="" />
        <div class="head-info">
          <h1 class="page-title" :title="seriesInfo.title">{{ seriesInfo.title }}</h1>
          <router-link class="author-link" :to="`/browse/user/${seriesInfo.user_id}`">
            {{ seriesInfo.user_name }}
          </router-link>
          <p v-if="caption" class="caption">{{ caption }}</p>
          <p class="stats" :title="t('browse.series.totalWordsHint')">{{ statsText }}</p>
        </div>
      </header>

      <!-- 目录列表（行式，整行 router-link） -->
      <nav v-if="contents.length" class="series-list" :aria-label="t('browse.series.listLabel')">
        <router-link
          v-for="ep in contents"
          :key="ep.id"
          class="series-row"
          :to="`/browse/work/novel/${ep.id}`"
        >
          <span class="row-order" aria-hidden="true">{{ pad2(ep.series_order) }}</span>
          <span class="row-title" :title="ep.title">{{ ep.title }}</span>
          <span class="row-meta">
            <span v-if="episodeWords(ep)">{{ episodeWords(ep) }}</span>
            <span v-if="episodeDate(ep)">{{ episodeDate(ep) }}</span>
            <span v-if="episodeRestrict(ep)" class="r18-pill">{{ episodeRestrict(ep) }}</span>
          </span>
        </router-link>
      </nav>
      <div v-else class="series-state">
        <p class="state-text strong">{{ t("browse.series.emptyList") }}</p>
      </div>

      <!-- 游标加载：按钮触发（next_last_order=null 时显示已全部加载） -->
      <div v-if="contents.length" class="series-more">
        <p v-if="moreError" class="state-text" role="alert">{{ moreError }}</p>
        <md-outlined-button
          v-if="nextLastOrder != null"
          :disabled="loadingMore"
          :aria-busy="loadingMore"
          @click="loadMore"
        >
          {{ t("browse.series.loadMore") }}
        </md-outlined-button>
        <p v-else class="no-more">{{ t("common.browseNoMore") }}</p>
      </div>
    </template>
  </div>
</template>

<style scoped>
/* ===== 骨架 ===== */

.sk {
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
}

.sk-cover {
  width: 120px;
  height: 120px;
  border-radius: var(--radius-control);
  flex-shrink: 0;
}

.sk-title {
  width: 55%;
  height: 22px;
}

.sk-line {
  height: 14px;
  margin-top: var(--space-md);
}

.sk-line.w90 { width: 90%; }
.sk-line.w60 { width: 60%; }
.sk-line.w40 { width: 40%; }
.sk-line.w30 { width: 30%; }

.sk-row .sk-line {
  flex: 1;
  margin-top: 0;
}

.sk-order {
  width: 28px;
  height: 14px;
  flex-shrink: 0;
}

/* ===== 系列头 ===== */

.series-head {
  display: flex;
  gap: var(--space-lg);
  margin-bottom: var(--space-xl);
}

.cover {
  width: 120px;
  height: 120px;
  border-radius: var(--radius-control);
  object-fit: cover;
  flex-shrink: 0;
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.head-info {
  min-width: 0;
}

.head-info .page-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.author-link {
  color: var(--ink-muted);
  font-size: 14px;
  text-decoration: none;
}

.author-link:hover {
  color: var(--md-sys-color-primary);
  text-decoration: underline;
}

.caption {
  margin: var(--space-sm) 0 0;
  color: var(--ink-muted);
  font-size: 14px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.stats {
  margin: var(--space-sm) 0 0;
  color: var(--ink-subtle);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
}

@media (max-width: 640px) {
  .series-head {
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  .head-info {
    width: 100%;
  }

  .head-info .page-title {
    white-space: normal;
  }
}

/* ===== 目录列表 ===== */

.series-list {
  border-radius: 16px;
  background: var(--md-sys-color-surface-container);
  overflow: hidden;
}

.series-row {
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

.series-row + .series-row {
  border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 30%, transparent);
}

.series-row:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.series-row:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: -2px;
}

.row-order {
  min-width: 28px;
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

@media (max-width: 640px) {
  .series-row {
    flex-wrap: wrap;
    row-gap: var(--space-xxs);
    padding: var(--space-sm) var(--space-md);
  }

  .row-title {
    /* 序号之后换行：标题独占一行，元信息落到下一行 */
    flex-basis: calc(100% - 40px);
    white-space: normal;
  }

  .row-meta {
    flex-basis: 100%;
    justify-content: flex-start;
  }
}

/* ===== 加载更多 / 状态 ===== */

.series-more {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-sm);
  margin-top: var(--space-lg);
}

.no-more {
  margin: 0;
  color: var(--ink-subtle);
  font-size: 12px;
  font-weight: 600;
}

.series-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-md);
  padding: var(--space-xl) var(--space-lg);
  text-align: center;
}

.state-text {
  margin: 0;
  color: var(--ink-muted);
  font-size: 14px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.state-text.strong {
  color: var(--ink);
  font-weight: 600;
}
</style>
