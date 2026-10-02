<script setup lang="ts">
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
/**
 * 浏览·追更列表页（watchlist-ui-v1，官方 /following/watchlist 同构）：
 * browse_watchlist 一次返回该类型全部订阅系列（后端已按 max_page 聚合，无需翻页），
 * 漫画/小说两个 SectionTabs 切换。条目为「系列行卡」：2:3 封面 + 「系列作品」overline
 * + 标题 + 作者 + 「N 话 · 更新日期」；主行动「读最新话」直达最新话作品（整卡同动作）。
 * 两类卡均附「系列目录」进应用内系列分集页（novel → /browse/series/novel/:id，
 * manga 为 watchlist 语义、映射到 /browse/series/illust/:id）。追更为用户主动订阅，
 * 页面不做 R-18 渲染期过滤。
 */
import { computed, onMounted, ref, shallowRef } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import {
  browseWatchlist,
  errorMessage,
  pxSrc,
  type BrowseWatchlist as WatchlistSnapshot,
  type BrowseWatchlistItem,
  type WatchKind,
} from "../../api/browse";
import SectionTabs from "../../components/browse/SectionTabs.vue";

const { t } = useI18n();
const router = useRouter();

const kind = ref<WatchKind>("manga");
const kindTabs = computed(() => [
  { value: "manga", label: t("browse.watchlist.tabManga") },
  { value: "novel", label: t("browse.watchlist.tabNovel") },
]);

const data = shallowRef<WatchlistSnapshot | null>(null);
const loading = ref(false);
const error = ref("");

async function load(): Promise<void> {
  loading.value = true;
  error.value = "";
  try {
    data.value = await browseWatchlist(kind.value);
  } catch (err) {
    error.value = errorMessage(err);
  } finally {
    loading.value = false;
  }
}

function onKindChange(value: string): void {
  if (value === kind.value) return;
  kind.value = value as WatchKind;
  void load();
  window.scrollTo({ top: 0 });
}

onMounted(load);

const items = computed(() => data.value?.items ?? []);

/** ISO 时间戳 → YYYY-MM-DD（缺失/非法为空串）。 */
function datePart(iso?: string): string {
  return iso && /^\d{4}-\d{2}-\d{2}/.test(iso) ? iso.slice(0, 10) : "";
}

/** 主行动：读最新话（novel → 阅读器，manga → 查看器）；整卡同动作。 */
function openLatest(item: BrowseWatchlistItem): void {
  if (!item.latest_work_id) return;
  void router.push(`/browse/work/${item.kind}/${item.latest_work_id}`);
}

/** 系列入口：统一进应用内系列分集页（manga 是 watchlist 语义，映射 series kind "illust"）。 */
function openSeries(item: BrowseWatchlistItem): void {
  const seriesKind = item.kind === "novel" ? "novel" : "illust";
  void router.push(`/browse/series/${seriesKind}/${item.id}`);
}
</script>

<template>
  <div class="page-view browse-watchlist">
    <div class="browse-list-header">
      <h1 class="page-title">{{ t("nav.browseWatchlist") }}</h1>
      <ListRefreshButton :busy="loading" @refresh="load" />
    </div>

    <div class="control-row" role="group" :aria-label="t('browse.watchlist.tabLabel')">
      <span class="control-label">{{ t("browse.watchlist.tabLabel") }}</span>
      <SectionTabs :tabs="kindTabs" :value="kind" @change="onKindChange" />
    </div>

    <!-- 整页错误（快照尚未到手）→ 文案 + 重试 -->
    <div v-if="error && !data" class="watchlist-state" role="alert">
      <p class="state-text">{{ error }}</p>
      <md-filled-button @click="load">{{ t("common.retry") }}</md-filled-button>
    </div>

    <!-- 骨架：纯 surface-container 色块，不做闪烁动画 -->
    <div v-else-if="loading" class="watchlist-grid" aria-hidden="true">
      <div v-for="i in 4" :key="i" class="watch-card skeleton">
        <div class="sk-cover"></div>
        <div class="sk-lines">
          <div class="sk-line" style="width: 32%"></div>
          <div class="sk-line" style="width: 78%"></div>
          <div class="sk-line" style="width: 46%"></div>
          <div class="sk-line" style="width: 58%"></div>
        </div>
      </div>
    </div>

    <!-- 空态 -->
    <div v-else-if="!items.length" class="watchlist-state">
      <p class="state-text">{{ t("browse.watchlist.empty") }}</p>
      <p class="state-hint">{{ t("browse.watchlist.emptyHint") }}</p>
    </div>

    <ul v-else class="watchlist-grid">
      <li v-for="item in items" :key="`${item.kind}:${item.id}`" class="watch-card">
        <button
          class="card-main"
          type="button"
          :aria-label="item.title"
          @click="openLatest(item)"
        >
          <img
            v-if="item.cover"
            class="card-cover"
            :src="pxSrc(item.cover)"
            alt=""
            loading="lazy"
          />
          <span v-else class="card-cover card-cover-empty" aria-hidden="true"></span>
          <span class="card-body">
            <span class="card-overline">{{ t("browse.watchlist.seriesLabel") }}</span>
            <span class="card-title">{{ item.title }}</span>
            <span class="card-author">
              <img
                v-if="item.user_avatar"
                class="author-avatar"
                :src="pxSrc(item.user_avatar)"
                alt=""
                loading="lazy"
              />
              <span v-else class="author-avatar author-avatar-empty" aria-hidden="true"></span>
              <span class="author-name">{{ item.user_name }}</span>
            </span>
            <span class="card-meta">
              {{ t("browse.watchlist.episodes", { count: item.total }) }}
              <template v-if="datePart(item.update_date)"> · {{ datePart(item.update_date) }}</template>
            </span>
          </span>
        </button>
        <!-- 动作行与整卡可点击元素为兄弟节点；缩进对齐信息列 -->
        <div class="card-actions">
          <md-filled-button
            class="read-latest"
            :disabled="!item.latest_work_id"
            @click="openLatest(item)"
          >
            {{ t("browse.watchlist.readLatest") }}
          </md-filled-button>
          <md-text-button @click="openSeries(item)">
            {{ t("browse.watchlist.openSeries") }}
          </md-text-button>
        </div>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.control-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-md);
  margin-bottom: var(--space-lg);
}

.control-label {
  min-width: 56px;
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-muted);
}

/* 双列行卡网格：窄窗口自动落到单列 */
.watchlist-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
  gap: var(--space-sm);
  margin: 0;
  padding: 0;
  list-style: none;
}

.watch-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-xxs);
  padding: var(--space-sm);
  border-radius: var(--radius-control);
}

.watch-card:not(.skeleton):hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.card-main {
  display: flex;
  gap: var(--space-md);
  padding: 0;
  background: none;
  border: 0;
  font: inherit;
  color: inherit;
  text-align: left;
  cursor: pointer;
}

.card-cover {
  flex: none;
  display: block;
  width: 112px;
  aspect-ratio: 2 / 3;
  object-fit: cover;
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
}

.card-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-xxs);
  min-width: 0;
}

.card-overline {
  font-size: 12px;
  font-weight: 500;
  color: var(--ink-muted);
}

.card-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--ink);
  line-height: 1.45;
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
  overflow-wrap: anywhere;
}

.card-author {
  display: flex;
  align-items: center;
  gap: var(--space-xxs);
  min-width: 0;
}

.author-avatar {
  flex: none;
  width: 20px;
  height: 20px;
  border-radius: 999px;
  object-fit: cover;
  background: var(--md-sys-color-surface-container);
}

.author-name {
  font-size: 12px;
  color: var(--ink-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.card-meta {
  font-size: 12px;
  color: var(--ink-muted);
}

.card-actions {
  display: flex;
  align-items: center;
  gap: var(--space-xs);
  /* 与信息列左缘对齐（封面 112px + 间距） */
  padding-left: calc(112px + var(--space-md));
}

.read-latest {
  --md-filled-button-container-height: 34px;
}

/* 状态区（错误 / 空态）：居中文案层级 */
.watchlist-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-sm);
  padding: var(--space-xl) 0;
  text-align: center;
}

.state-text {
  font-size: 14px;
  color: var(--ink-muted);
}

.state-hint {
  font-size: 12px;
  color: var(--ink-subtle);
}

/* 骨架色块 */
.skeleton .sk-cover {
  flex: none;
  width: 112px;
  aspect-ratio: 2 / 3;
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
}

.skeleton .sk-lines {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--space-sm);
}

.sk-line {
  height: 14px;
  border-radius: 7px;
  background: var(--md-sys-color-surface-container);
}

@media (max-width: 640px) {
  .card-actions {
    padding-left: 0;
  }
}
</style>
