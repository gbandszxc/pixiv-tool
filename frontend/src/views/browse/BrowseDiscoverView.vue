<script setup lang="ts">
import BrowseNavigation from "../../components/navigation/BrowseNavigation.vue";
import PageBackButton from "../../components/navigation/PageBackButton.vue";
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
/**
 * 浏览·发现页（F2）。
 *
 * - `browseDiscover()` 无翻页参数，每次返回 60 条且与上次几乎不重叠：
 *   无限滚动触发重复调用，按 id 去重后追加；
 *   单批新条目 < 10 时视为推荐池见底，停止并显示「没有更多」。
 * - 顶部过滤 chips（全部/插画/漫画）为纯前端过滤（按 item.kind，ugoira 归入插画）。
 * - 说明位：发现推荐不含小说（小字注释，不做成 tab）。
 */
import { computed, onActivated, onDeactivated, onMounted, ref, shallowRef, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { browseDiscover, errorMessage, type BrowseWorkItem, type WorkKind } from "../../api/browse";
import WorkGrid from "../../components/browse/WorkGrid.vue";

const { t } = useI18n();
const router = useRouter();

// ===== 过滤 chips（纯前端） =====

type DiscoverFilter = "all" | "illust" | "manga";
const filter = ref<DiscoverFilter>("all");
const filterOptions = computed(() => [
  { value: "all" as const, label: t("browse.discover.filterAll") },
  { value: "illust" as const, label: t("browse.discover.filterIllust") },
  { value: "manga" as const, label: t("browse.discover.filterManga") },
]);

/** ugoira 在 pixiv 归属插画分类，勾选「插画」时一并显示。 */
function kindMatches(kind: WorkKind): boolean {
  if (filter.value === "all") return true;
  if (filter.value === "illust") return kind === "illust" || kind === "ugoira";
  return kind === "manga";
}

// ===== 列表状态（重复调用 + 去重追加，非分页，不用 useInfiniteList） =====

/** 单批新条目低于该值视为没有更多（契约 v2：每批 60 条）。 */
const MIN_NEW_PER_BATCH = 10;

const items = shallowRef<BrowseWorkItem[]>([]);
const loading = ref(false);
const loadingMore = ref(false);
/** 首屏失败与追加失败共用：无内容时由 WorkGrid 呈现错误态，有内容时在页内展示重试行 */
const error = ref("");
const hasMore = ref(true);
const active = ref(true);
/** 已出现过的条目键（kind:id），discover 每批几乎不重叠但不去重，由前端保证唯一 */
const seen = new Set<string>();

function keyOf(item: BrowseWorkItem): string {
  return `${item.kind}:${item.id}`;
}

async function fetchBatch(): Promise<void> {
  if (!active.value || loading.value || loadingMore.value || error.value || !hasMore.value) return;
  const initial = items.value.length === 0;
  if (initial) loading.value = true;
  else loadingMore.value = true;
  try {
    const data = await browseDiscover();
    const fresh = data.items.filter((item) => !seen.has(keyOf(item)));
    fresh.forEach((item) => seen.add(keyOf(item)));
    items.value = initial ? fresh : items.value.concat(fresh);
    if (fresh.length < MIN_NEW_PER_BATCH) hasMore.value = false;
  } catch (err) {
    error.value = errorMessage(err) || t("common.browseLoadFailed");
  } finally {
    loading.value = false;
    loadingMore.value = false;
  }
}

function retry(): void {
  if (!error.value) return;
  error.value = "";
  void fetchBatch();
}

function refresh(): void {
  seen.clear();
  items.value = [];
  error.value = "";
  hasMore.value = true;
  void fetchBatch();
}

onMounted(() => {
  void fetchBatch();
});
onActivated(() => { active.value = true; });
onDeactivated(() => { active.value = false; });

const filtered = computed(() => items.value.filter((item) => kindMatches(item.kind)));

/** 极端情况兜底：过滤后为空但推荐池还有余量时，继续拉取直至出现该类条目或见底。 */
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
    void fetchBatch();
  }
});

// ===== 卡片跳转 =====

/** 作品详情路由：novel → 阅读器，ugoira 按插画查看器打开（V1 显示封面帧），manga 单列。 */
function goWork(item: BrowseWorkItem): void {
  if (item.kind === "novel") void router.push(`/browse/work/novel/${item.id}`);
  else if (item.kind === "manga") void router.push(`/browse/work/manga/${item.id}`);
  else void router.push(`/browse/work/illust/${item.id}`);
}
</script>

<template>
  <div class="page-view">
    <div class="browse-list-header">
      <div class="page-heading"><PageBackButton /><h1 class="page-title">{{ t("workspace.discover") }}</h1></div>
      <ListRefreshButton :busy="loading || loadingMore" @refresh="refresh" />
    </div>
    <BrowseNavigation />

    <!-- 过滤 chips：自绘胶囊，选中态 secondary-container（DESIGN.md M3 角色） -->
    <div class="chips-row" role="group" :aria-label="t('nav.browseDiscover')">
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
      <span class="novel-note">{{ t("browse.discover.novelNote") }}</span>
    </div>

    <WorkGrid
      :items="filtered"
      :loading="loading"
      :error="error"
      :loading-more="loadingMore"
      :has-more="hasMore"
      @load-more="fetchBatch"
      @retry="retry"
      @select="goWork"
    />

    <!-- 追加失败的页内重试（WorkGrid 错误态仅在无内容时出现） -->
    <div v-if="error && items.length" class="append-error">
      <span>{{ error }}</span>
      <md-outlined-button @click="retry">{{ t("common.retry") }}</md-outlined-button>
    </div>
  </div>
</template>

<style scoped>
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

.novel-note {
  margin-left: var(--space-sm);
  font-size: 12px;
  color: var(--ink-subtle);
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
