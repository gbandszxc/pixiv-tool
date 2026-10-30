<script setup lang="ts">
import { computed, onActivated, onBeforeUnmount, onDeactivated, onMounted, ref, shallowRef, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import {
  browseDiscover,
  browseHomeFeed,
  errorMessage,
  type BrowseWorkItem,
  type WorkKind,
} from "../../api/browse";
import BrowseNavigation from "../../components/navigation/BrowseNavigation.vue";
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
import PageBackButton from "../../components/navigation/PageBackButton.vue";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import { useAuthStore } from "../../stores/auth";
import { readHomeCache, saveHomeCache } from "../../utils/homeCache";

/**
 * 浏览·推荐页（合并原首页 street 流与 discover 探索画廊流）。
 * - 顶栏五分类平级导航（推荐 / 插画 / 漫画 / 小说 / 排行榜）。
 * - 首屏结合 homeCache 秒开，后台拉取混合流（含小说），条目较少时自动追加 discover 填满首屏。
 * - 触底无限滚动自动续拉，支持纯前端分类过滤 chips（全部/插画/漫画/小说）。
 * - 遵循 KeepAlive active 守卫，离开 Tab 期间停止后台补拉。
 */

const { t } = useI18n();
const router = useRouter();
const auth = useAuthStore();

const cacheUserId = computed(() => (auth.isLoggedIn ? auth.userId : ""));
const items = shallowRef<BrowseWorkItem[]>(readHomeCache(cacheUserId.value));

let disposed = false;
const active = ref(true);

onBeforeUnmount(() => {
  disposed = true;
});
onActivated(() => {
  active.value = true;
});
onDeactivated(() => {
  active.value = false;
});

function persist(): void {
  if (disposed) return;
  const uid = cacheUserId.value;
  if (!uid || auth.userId !== uid) return;
  saveHomeCache(uid, items.value);
}

// ===== 过滤 chips（纯前端） =====

type HomeFilter = "all" | "illust" | "manga" | "novel";
const filter = ref<HomeFilter>("all");
const filterOptions = computed(() => [
  { value: "all" as const, label: t("browse.discover.filterAll") },
  { value: "illust" as const, label: t("browse.discover.filterIllust") },
  { value: "manga" as const, label: t("browse.discover.filterManga") },
  { value: "novel" as const, label: t("nav.browseNovel") },
]);

function kindMatches(kind: WorkKind): boolean {
  if (filter.value === "all") return true;
  if (filter.value === "illust") return kind === "illust" || kind === "ugoira";
  if (filter.value === "manga") return kind === "manga";
  return kind === "novel";
}

const filtered = computed(() => items.value.filter((item) => kindMatches(item.kind)));

// ===== 列表拉取与无限滚动 =====

const loading = ref(false);
const loadingMore = ref(false);
const error = ref("");
const hasMore = ref(true);

function keyOf(item: BrowseWorkItem): string {
  return `${item.kind}:${item.id}`;
}

/** 填充首屏目标数量：大屏下至少达到 20 条，避免首屏稀疏。 */
const INITIAL_FILL_TARGET = 20;
const MIN_NEW_PER_BATCH = 5;

/** 首屏加载（先拉混合 street 流，如果数量偏少自动借 discover 补满首屏）。 */
async function initialLoad(): Promise<void> {
  if (loading.value) return;
  loading.value = true;
  error.value = "";
  hasMore.value = true;

  try {
    const feed = await browseHomeFeed();
    if (disposed) return;

    let list = feed.items;
    const seen = new Set(list.map(keyOf));

    // 如果首屏条目不足以撑满视口，追加一批 discover 画廊插画
    if (list.length < INITIAL_FILL_TARGET) {
      try {
        const discover = await browseDiscover();
        if (!disposed) {
          const extra = discover.items.filter((it) => !seen.has(keyOf(it)));
          extra.forEach((it) => seen.add(keyOf(it)));
          list = list.concat(extra);
        }
      } catch {
        // discover 补齐失败不阻断 street 主流
      }
    }

    if (disposed) return;
    items.value = list;
    persist();
  } catch (err) {
    if (!disposed) {
      error.value = errorMessage(err) || t("common.browseLoadFailed");
    }
  } finally {
    if (!disposed) loading.value = false;
  }
}

/** 触底自动追加下一批。 */
async function fetchMore(): Promise<void> {
  if (!active.value || loading.value || loadingMore.value || !hasMore.value || error.value) {
    return;
  }

  loadingMore.value = true;
  try {
    const seen = new Set(items.value.map(keyOf));
    let fresh: BrowseWorkItem[] = [];

    if (filter.value === "novel") {
      // 筛选小说时，使用 home_feed 提取新小说
      const data = await browseHomeFeed();
      if (disposed) return;
      fresh = data.items.filter((it) => it.kind === "novel" && !seen.has(keyOf(it)));
    } else {
      // 全部、插画、漫画使用 discover 大图推荐流
      const data = await browseDiscover();
      if (disposed) return;
      fresh = data.items.filter((it) => !seen.has(keyOf(it)));
    }

    if (disposed) return;
    if (fresh.length >= MIN_NEW_PER_BATCH) {
      items.value = items.value.concat(fresh);
      persist();
    } else {
      hasMore.value = false;
    }
  } catch (err) {
    if (!disposed) {
      error.value = errorMessage(err) || t("common.browseLoadFailed");
    }
  } finally {
    if (!disposed) loadingMore.value = false;
  }
}

function refresh(): void {
  error.value = "";
  void initialLoad();
}

function retry(): void {
  error.value = "";
  if (!items.value.length) {
    void initialLoad();
  } else {
    void fetchMore();
  }
}

// 极端情况兜底：分类过滤后为空但推荐池尚有余量时，自动补拉直到出现该类条目或见底
watch([filtered, hasMore, loading, loadingMore, error, active], () => {
  if (
    active.value &&
    filter.value !== "all" &&
    !filtered.value.length &&
    items.value.length &&
    hasMore.value &&
    !loading.value &&
    !loadingMore.value &&
    !error.value
  ) {
    void fetchMore();
  }
});

function openWork(item: BrowseWorkItem): void {
  const kindPath = item.kind === "novel" ? "novel" : item.kind === "manga" ? "manga" : "illust";
  void router.push(`/browse/work/${kindPath}/${item.id}`);
}

onMounted(() => {
  void initialLoad();
});
</script>

<template>
  <div class="page-view browse-home">
    <div class="app-page-head">
      <div class="browse-list-header">
        <div class="page-heading">
          <PageBackButton />
          <h1 class="page-title">{{ t("workspace.discover") }}</h1>
        </div>
        <ListRefreshButton :busy="loading || loadingMore" @refresh="refresh" />
      </div>
      <BrowseNavigation />

      <!-- 分类过滤 chips -->
      <div class="chips-row" role="group" :aria-label="t('workspace.discover')">
        <button
          v-for="option in filterOptions"
          :key="option.value"
          type="button"
          class="chip"
          :class="{ selected: filter === option.value }"
          :aria-pressed="filter === option.value"
          @click="filter = option.value"
        >
          {{ option.label }}
        </button>
      </div>
    </div>

    <!-- 首屏失败提示（当有缓存/旧数据时，在顶部以 alert 条呈现，符合 loading.ts 断言） -->
    <p v-if="error && items.length" role="alert" class="cache-error-alert">{{ error }}</p>

    <WorkGrid
      :items="filtered"
      :loading="loading"
      :error="items.length ? '' : error"
      :loading-more="loadingMore"
      :has-more="hasMore"
      @load-more="fetchMore"
      @retry="retry"
      @select="openWork"
    />

    <!-- 追加翻页失败的页内重试 -->
    <div v-if="error && items.length" class="append-error">
      <span>{{ error }}</span>
      <md-outlined-button @click="retry">{{ t("common.retry") }}</md-outlined-button>
    </div>
  </div>
</template>

<style scoped>
.browse-list-header {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-sm) var(--space-md);
  margin-bottom: var(--space-sm);
}

.browse-list-header .page-title {
  margin: 0;
}

.chips-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-sm);
  margin-bottom: var(--space-lg);
}

.chip {
  padding: 6px 16px;
  font: inherit;
  font-size: 13px;
  font-weight: 600;
  color: var(--ink);
  background: transparent;
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 60%, transparent);
  border-radius: 999px;
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.chip:hover {
  background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.chip.selected {
  color: var(--md-sys-color-on-secondary-container);
  background: var(--md-sys-color-secondary-container);
  border-color: transparent;
}

.chip:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.cache-error-alert {
  padding: var(--space-sm) var(--space-md);
  margin-bottom: var(--space-md);
  border-radius: var(--radius-control);
  background: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
  font-size: 13px;
}

.append-error {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: var(--space-md);
  margin-top: var(--space-lg);
  color: var(--ink-muted);
  font-size: 13px;
}
</style>
