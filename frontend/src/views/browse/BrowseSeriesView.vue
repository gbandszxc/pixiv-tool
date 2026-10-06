<script setup lang="ts">
import PageBackButton from "../../components/navigation/PageBackButton.vue";
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
/**
 * 系列分集页（/browse/series/:kind/:id，series-episode-ui / F1）。
 *
 * kind = novel（小说系列）| illust（插画/漫画系列，官方接口族不区分两类，
 * watchlist 的 manga 语义在入口处映射为 illust）。系列头（封面 / 标题 / 作者 /
 * 简介 / 话数状态 / 动作）+ 宫格·列表双模式分集列表 + 页码翻页器：
 * - novel：browseNovelSeries(id, (page-1)*30)——游标按页码映射，30 条/页；
 * - illust：browseIllustSeries(id, page)——官方页码制，12 条/页、话数降序。
 * 切页保留旧内容做局部过渡（禁止闪烁）；R-18 全局过滤只作用于分集条目，
 * 头部统计保持服务端口径。模式切换会话内记忆（seriesView.ts，默认 illust→宫格、novel→列表）。
 */
import { computed, ref, shallowRef, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  browseIllustSeries,
  browseNovelSeries,
  errorMessage,
  pxSrc,
  thumbSrc,
  type BrowseIllustSeriesDetail,
  type BrowseSeriesDetail,
} from "../../api/browse";
import AppPagination from "../../components/common/AppPagination.vue";
import SeriesEpisodeGrid from "../../components/browse/SeriesEpisodeGrid.vue";
import SeriesEpisodeList from "../../components/browse/SeriesEpisodeList.vue";
import ViewModeToggle from "../../components/browse/ViewModeToggle.vue";
import { filterByR18, useGlobalR18Filter } from "../../components/browse/r18Filter";
import { useSeriesViewMode, type SeriesEpisodeView, type SeriesKind } from "../../components/browse/seriesView";
import { useThumbTier } from "../../composables/useThumbTier";
import { notify } from "../../ui/notify";
import { fillDownloadForm, openInBrowser } from "../../utils/pixivHooks";
import { pixivIllustSeriesUrl, pixivSeriesUrl } from "../../utils/pixivUrl";

/** 小说系列每页条数（browse_novel_series 每批 30，页码映射 last_order=(page-1)*30）。 */
const NOVEL_PAGE_SIZE = 30;

const props = defineProps<{ kind: SeriesKind; id: number }>();

const { t } = useI18n();

// ===== 数据（页码分页；切页保留旧内容） =====

const novelData = shallowRef<BrowseSeriesDetail | null>(null);
const illustData = shallowRef<BrowseIllustSeriesDetail | null>(null);
const page = ref(1);
/** 首屏加载（无数据可保留，显示整页骨架）。 */
const loading = ref(false);
/** 切页加载（保留旧内容 + 局部过渡）。 */
const switching = ref(false);
const error = ref("");
/** 切页失败文案（内容保留，错误就地显示）。 */
const pageError = ref("");
/** 头图档位改写 404 时为 true → 回落接口原始 URL（随封面源变化重置）。 */
const coverFailed = ref(false);

const hasData = computed(() =>
  props.kind === "illust" ? illustData.value != null : novelData.value != null
);

async function load(target: number, initial: boolean): Promise<void> {
  if (initial) {
    loading.value = true;
    error.value = "";
    novelData.value = null;
    illustData.value = null;
    // 进入/切换系列回到页首（SPA 内路由切换会保留上一页滚动位置）
    window.scrollTo(0, 0);
  } else {
    switching.value = true;
  }
  pageError.value = "";
  try {
    if (props.kind === "illust") {
      illustData.value = await browseIllustSeries(props.id, target);
    } else {
      novelData.value = await browseNovelSeries(props.id, (target - 1) * NOVEL_PAGE_SIZE);
    }
    page.value = target;
  } catch (err) {
    const message = errorMessage(err) || t("common.browseLoadFailed");
    if (initial) error.value = message;
    else pageError.value = message;
  } finally {
    loading.value = false;
    switching.value = false;
  }
}

watch(
  () => [props.kind, props.id] as const,
  () => {
    page.value = 1;
    void load(1, true);
  },
  { immediate: true }
);

function changePage(target: number): void {
  if (target === page.value || loading.value || switching.value) return;
  // 切页回到页首；数据到达前旧内容以局部过渡呈现（不闪烁）
  window.scrollTo(0, 0);
  void load(target, false);
}

// ===== 展示模式（会话内记忆） =====

const { mode, setMode } = useSeriesViewMode(() => props.kind);

// ===== 系列头（两类型统一信息架构） =====

interface SeriesHead {
  title: string;
  userId: number;
  userName: string;
  caption: string;
  /** 接口原始封面（illust 空串时已回退第一话封面） */
  rawCover: string;
  total: number;
  concluded: boolean;
}

const head = computed<SeriesHead | null>(() => {
  if (props.kind === "illust") {
    const d = illustData.value;
    if (!d) return null;
    return {
      title: d.title,
      userId: d.user_id,
      userName: d.user_name,
      caption: d.caption ?? "",
      // 契约：cover 空串 = 未设自定义封面 → 回退第一话封面
      rawCover: d.cover || d.contents[0]?.cover || "",
      total: d.total,
      concluded: d.is_concluded,
    };
  }
  const d = novelData.value;
  if (!d) return null;
  return {
    title: d.title,
    userId: d.user_id,
    userName: d.user_name,
    caption: d.caption ?? "",
    rawCover: d.cover ?? "",
    total: d.total,
    concluded: d.is_concluded ?? false,
  };
});

/** 头部封面档位（thumb_quality_grid，120px 方形展示）。 */
const gridTier = useThumbTier("thumb_quality_grid");

const cover = computed(() => {
  const raw = head.value?.rawCover;
  if (!raw) return "";
  return coverFailed.value ? pxSrc(raw) : thumbSrc(raw, gridTier.value);
});

watch(
  () => head.value?.rawCover,
  () => {
    coverFailed.value = false;
  }
);

function onCoverError(): void {
  coverFailed.value = true;
}

const caption = computed(() => head.value?.caption.replace(/<[^>]*>/g, "").trim() ?? "");

const statusLabel = computed(() =>
  head.value?.concluded ? t("browse.series.concluded") : t("browse.series.ongoing")
);

const statsText = computed(() => {
  if (!head.value) return "";
  return `${t("browse.series.episodes", { count: head.value.total.toLocaleString() })} · ${statusLabel.value}`;
});

// ===== 分集条目（归一 + R-18 过滤） =====

const r18Filter = useGlobalR18Filter();

const contents = computed<SeriesEpisodeView[]>(() => {
  if (props.kind === "illust") {
    return (illustData.value?.contents ?? []).map((ep) => ({
      id: ep.id,
      series_order: ep.series_order,
      title: ep.title,
      cover: ep.cover,
      page_count: ep.page_count,
      x_restrict: ep.x_restrict,
      update_date: datePart(ep.update_date),
    }));
  }
  return (novelData.value?.contents ?? []).map((ep) => ({
    id: ep.id,
    series_order: ep.series_order,
    title: ep.title,
    x_restrict: ep.x_restrict,
    text_length: ep.text_length ?? null,
    update_date: datePart(ep.update_date),
  }));
});

/** ISO 时间戳 → YYYY-MM-DD（缺失/非法为空串）。 */
function datePart(iso?: string): string {
  return iso && /^\d{4}-\d{2}-\d{2}/.test(iso) ? iso.slice(0, 10) : "";
}

/** R-18 全局过滤只作用分集条目（宫格/列表同口径）；头部不受影响。 */
const visibleContents = computed(() => filterByR18(contents.value, r18Filter.value));

// ===== 总页数与翻页 =====

const totalPages = computed(() => {
  if (props.kind === "illust") return illustData.value?.total_pages ?? 1;
  return Math.max(1, Math.ceil((novelData.value?.total ?? 0) / NOVEL_PAGE_SIZE));
});

// ===== 动作 =====

/** 用系统默认浏览器打开 pixiv 系列页（illust 需作者 uid）。 */
function openInPixiv(): void {
  const url =
    props.kind === "illust" && illustData.value
      ? pixivIllustSeriesUrl(illustData.value.user_id, props.id)
      : pixivSeriesUrl(props.id);
  void openInBrowser(url).catch(() => notify(t("browse.hooks.openFailed")));
}
</script>

<template>
  <div class="page-view series-view">
    <!-- 系列头始终保留返回入口，骨架不能遮蔽可操作控件。 -->
    <header class="series-head">
      <div v-if="loading" class="sk sk-cover" aria-hidden="true"></div>
      <img v-else-if="cover" class="cover" :src="cover" alt="" @error="onCoverError" />
      <div class="head-info">
        <!-- 刷新并入标题行右端（与频道页等同一形态）；标题 min-width:0 可省略，窄窗不挤压刷新。 -->
        <div class="page-heading">
          <PageBackButton />
          <div v-if="loading" class="sk sk-title" aria-hidden="true"></div>
          <h1 v-else class="page-title" :title="head?.title">{{ head?.title || t("common.browseSeriesTitle") }}</h1>
          <ListRefreshButton :busy="loading || switching" @refresh="load(page, !hasData)" />
        </div>
        <template v-if="loading">
          <div class="sk sk-line w40" aria-hidden="true"></div>
          <div class="sk sk-line w60" aria-hidden="true"></div>
        </template>
        <template v-else-if="head">
          <router-link class="author-link" :to="`/browse/user/${head.userId}`">
            {{ head.userName }}
          </router-link>
          <p v-if="caption" class="caption">{{ caption }}</p>
          <p class="stats">{{ statsText }}</p>
          <div class="head-actions">
            <md-outlined-button
              v-if="kind === 'novel'"
              @click="fillDownloadForm({ form: 'novel', sourceType: 'series', sourceId: id })"
            >
              {{ t("browse.hooks.fillNovelForm") }}
            </md-outlined-button>
            <md-outlined-button @click="openInPixiv">
              {{ t("browse.hooks.openInBrowser") }}
            </md-outlined-button>
          </div>
        </template>
      </div>
    </header>

    <!-- 首屏骨架：当前模式骨架，无动画 -->
    <div v-if="loading" aria-hidden="true">
      <SeriesEpisodeGrid v-if="mode === 'grid'" :kind="kind" :items="[]" loading />
      <SeriesEpisodeList v-else :kind="kind" :items="[]" loading />
    </div>

    <!-- 首屏错误态：文案 + 重试 -->
    <div v-else-if="error" class="series-state" role="alert">
      <p class="state-text strong">{{ error }}</p>
      <md-outlined-button @click="load(1, true)">{{ t("common.retry") }}</md-outlined-button>
    </div>

    <template v-else-if="head">

      <!-- 工具行：目录标签 + 宫格/列表模式切换（会话内记忆） -->
      <div class="series-toolbar">
        <p class="toolbar-label">{{ t("browse.series.listLabel") }}</p>
        <ViewModeToggle :value="mode" @change="setMode" />
      </div>

      <!-- 切页失败：内容保留，错误就地显示 -->
      <p v-if="pageError" class="page-error" role="alert">{{ pageError }}</p>

      <!-- 分集列表：切页保留旧内容，局部过渡降透明（不闪烁） -->
      <div class="series-body" :class="{ switching, 'is-empty': !visibleContents.length }">
        <SeriesEpisodeGrid v-if="mode === 'grid'" :kind="kind" :items="visibleContents" />
        <SeriesEpisodeList v-else :kind="kind" :items="visibleContents" />
      </div>

      <div v-if="!visibleContents.length" class="series-state">
        <p class="state-text strong">{{ t("browse.series.emptyList") }}</p>
      </div>

      <!-- 页码翻页器（AppPagination reader 变体：sticky 底部吸底居中；受控页码经 update:currentPage 回流 changePage 守卫） -->
      <AppPagination
        variant="reader"
        :current-page="page"
        :total-pages="totalPages"
        @update:currentPage="changePage"
      />
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
  flex: 1;
  min-width: 0;
}

.head-info > .page-heading { margin-bottom: var(--space-lg); }

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

.head-actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-sm);
  margin-top: var(--space-md);
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

/* ===== 工具行 / 切页状态 ===== */

.series-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-md);
  margin-bottom: var(--space-md);
}

.toolbar-label {
  margin: 0;
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
}

.page-error {
  margin: 0 0 var(--space-md);
  color: var(--ink-muted);
  font-size: 13px;
}

/* 末行与翻页器之间的滚动末端余量（承自 .app-content 原底部内边距，见 App.vue .series-page）：
 * 既留出末行呼吸空间，也是全局搜索悬浮按钮（bottom 3 × space-xl、高 2 × space-xl + space-sm）
 * 的驻留带，末行不被它压住。空列表时由 .series-state 自带的间距承担，不叠加。 */
.series-body {
  margin-bottom: calc(var(--space-xl) * 3);
  transition: opacity 0.15s ease;
}

.series-body.is-empty {
  margin-bottom: 0;
}

.series-body.switching {
  opacity: 0.45;
  pointer-events: none;
}

@media (prefers-reduced-motion: reduce) {
  .series-body {
    transition: none;
  }
}

/* ===== 翻页器 ===== */

/* 内容不足一屏时翻页器也要落到底栏：sticky 只在元素将要跑出视口时吸附、不会把它往下拉，
 * 因此页面列至少占满一屏，翻页器以 margin-top: auto 落到列底；内容超一屏时自动余量归 0，
 * 翻页器退回列尾，仍由 reader 变体的 sticky 吸底。 */
.series-view {
  display: flex;
  flex-direction: column;
  min-height: 100%;
}

:deep(.app-pagination.is-reader) {
  margin-top: auto;
}

/* ===== 状态区 ===== */

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
