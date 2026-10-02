<script setup lang="ts">
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
/**
 * 浏览·搜索页（F2）。
 *
 * - URL 查询参数 `?word=&kind=&order=&mode=&s_mode=` 回显：挂载/前进后退时恢复
 *   状态并自动搜索；执行搜索或切换过滤时 `router.replace` 写回（单一真相源是
 *   路由查询参数，本地 ref 仅作受控回显）。
 * - ID 直达：输入先经 `parseBrowseInput` 解析，命中链接/纯数字直接跳站内路由，
 *   不发起搜索；未命中按关键词搜索。
 * - 端点映射：kind=illust|manga → artworks 检索（type=illust|manga）；
 *   kind=novel → novels 检索（不传 type）。
 * - 结果 WorkGrid + 「约 N 件」total + next_page 无限加载。
 */
import { computed, ref, watch } from "vue";
import { useRoute, useRouter, type LocationQuery } from "vue-router";
import { useI18n } from "vue-i18n";
import { browseSearch, type BrowseWorkItem } from "../../api/browse";
import SectionTabs from "../../components/browse/SectionTabs.vue";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import { useInfiniteList } from "../../composables/useInfiniteList";
import { parseBrowseInput, type ParsedBrowseInput } from "../../utils/parseInput";

const { t } = useI18n();
const route = useRoute();
const router = useRouter();

// ===== 参数取值域 =====

type SearchKind = "illust" | "manga" | "novel";
const ORDERS = ["date_d", "date", "popular_d"] as const;
const MODES = ["all", "safe", "r18"] as const;
const S_MODES = ["s_tag", "s_tag_full", "s_tc"] as const;
type Order = (typeof ORDERS)[number];
type Mode = (typeof MODES)[number];
type SMode = (typeof S_MODES)[number];

/** 值域校验：非法值回落默认（手工改 URL 也不会把页面带崩）。 */
function oneOf<T extends string>(allowed: readonly T[], value: string, fallback: T): T {
  return (allowed as readonly string[]).includes(value) ? (value as T) : fallback;
}

function queryStr(q: LocationQuery, key: string): string {
  const v = q[key];
  const raw = Array.isArray(v) ? v[0] : v;
  return typeof raw === "string" ? raw : "";
}

interface SearchParams {
  word: string;
  kind: SearchKind;
  order: Order;
  mode: Mode;
  sMode: SMode;
}

function paramsFromQuery(q: LocationQuery): SearchParams {
  return {
    word: queryStr(q, "word").trim(),
    kind: oneOf(["illust", "manga", "novel"] as const, queryStr(q, "kind"), "illust"),
    order: oneOf(ORDERS, queryStr(q, "order"), "date_d"),
    mode: oneOf(MODES, queryStr(q, "mode"), "all"),
    sMode: oneOf(S_MODES, queryStr(q, "s_mode"), "s_tag"),
  };
}

// ===== 受控状态（初始值即默认参数，挂载时由 applyQuery 覆盖） =====

const word = ref("");
const kind = ref<SearchKind>("illust");
const order = ref<Order>("date_d");
const mode = ref<Mode>("all");
const sMode = ref<SMode>("s_tag");

function currentParams(): SearchParams {
  return {
    word: word.value.trim(),
    kind: kind.value,
    order: order.value,
    mode: mode.value,
    sMode: sMode.value,
  };
}

function sigOf(p: SearchParams): string {
  return [p.word, p.kind, p.order, p.mode, p.sMode].join("\u0000");
}

// ===== 结果列表 =====

const total = ref<number | null>(null);
const list = useInfiniteList<BrowseWorkItem>(async (page) => {
  const data = await browseSearch(kind.value, word.value.trim(), {
    order: order.value,
    mode: mode.value,
    s_mode: sMode.value,
    // novel 走 novels 端点不带 type；illust/manga 走 artworks 端点并带 type
    type: kind.value === "novel" ? undefined : kind.value,
    page,
  });
  if (page === 1) total.value = data.total ?? null;
  return data;
});
const { items, loading, loadingMore, error, hasMore, loadMore, retry } = list;

/** 最近一次已发起搜索的参数签名：URL 回显到达时据此去重，避免 replace 触发二次请求。 */
let lastFetchedSig = "";

function runSearch(sig: string): void {
  lastFetchedSig = sig;
  total.value = null;
  list.reload();
}

// ===== URL ⇄ 状态 =====

function applyQuery(): void {
  // 离开本页时全局 route.query 也会变化，守卫避免误触发
  if (!route.path.startsWith("/browse/search")) return;
  const params = paramsFromQuery(route.query);
  word.value = params.word;
  kind.value = params.kind;
  order.value = params.order;
  mode.value = params.mode;
  sMode.value = params.sMode;
  if (!params.word) {
    // 无关键词（如从侧栏重新进入）：回到初始空态
    lastFetchedSig = "";
    total.value = null;
    if (items.value.length || loading.value || loadingMore.value || error.value) list.reset();
    return;
  }
  const sig = sigOf(params);
  if (sig !== lastFetchedSig) runSearch(sig);
}

watch(() => route.query, applyQuery, { immediate: true });

function buildQuery(): Record<string, string> {
  const p = currentParams();
  const query: Record<string, string> = {
    kind: p.kind,
    order: p.order,
    mode: p.mode,
    s_mode: p.sMode,
  };
  if (p.word) query.word = p.word;
  return query;
}

// ===== 交互 =====

/** 过滤变化后提交：有关键词且参数有变则重新搜索，并始终把状态写回 URL。 */
function commit(): void {
  if (word.value.trim()) {
    const sig = sigOf(currentParams());
    if (sig !== lastFetchedSig) runSearch(sig);
  }
  void router.replace({ query: buildQuery() });
}

/** 回车 / 搜索按钮：先试 ID 直达，未命中按关键词搜索（同参数重按视为重试）。 */
function doSearch(): void {
  const raw = word.value.trim();
  const target = parseBrowseInput(raw);
  if (target) {
    pushTarget(target);
    return;
  }
  if (!raw) return;
  word.value = raw;
  const sig = sigOf(currentParams());
  if (sig !== lastFetchedSig || error.value) runSearch(sig);
  void router.replace({ query: buildQuery() });
}

function onKindChange(value: string): void {
  kind.value = oneOf(["illust", "manga", "novel"] as const, value, "illust");
  commit();
}

function onOrderChange(event: Event): void {
  order.value = oneOf(ORDERS, (event.target as HTMLSelectElement).value, "date_d");
  commit();
}

function onModeChange(event: Event): void {
  mode.value = oneOf(MODES, (event.target as HTMLSelectElement).value, "all");
  commit();
}

function onSModeChange(event: Event): void {
  sMode.value = oneOf(S_MODES, (event.target as HTMLSelectElement).value, "s_tag");
  commit();
}

// ===== ID 直达与卡片跳转 =====

function pushTarget(target: ParsedBrowseInput): void {
  switch (target.type) {
    case "illust-work":
      void router.push(`/browse/work/illust/${target.id}`);
      break;
    case "novel-work":
      void router.push(`/browse/work/novel/${target.id}`);
      break;
    case "user":
      void router.push(`/browse/user/${target.id}`);
      break;
    case "novel-series":
      void router.push(`/browse/series/novel/${target.id}`);
      break;
    case "illust-series":
      void router.push(`/browse/series/illust/${target.id}`);
      break;
  }
}

/** 结果卡片跳转：novel → 阅读器，其余走插画/漫画查看器（ugoira V1 显示封面帧）。 */
function goWork(item: BrowseWorkItem): void {
  if (item.kind === "novel") void router.push(`/browse/work/novel/${item.id}`);
  else if (item.kind === "manga") void router.push(`/browse/work/manga/${item.id}`);
  else void router.push(`/browse/work/illust/${item.id}`);
}

// ===== 选项文案（computed：语言切换即时生效） =====

const typeTabs = computed(() => [
  { value: "illust", label: t("browse.search.typeIllust") },
  { value: "manga", label: t("browse.search.typeManga") },
  { value: "novel", label: t("browse.search.typeNovel") },
]);
</script>

<template>
  <div class="page-view">
    <div class="browse-list-header">
      <h1 class="page-title">{{ t("nav.browseSearch") }}</h1>
      <ListRefreshButton :busy="loading || loadingMore" :disabled="!word.trim()" @refresh="runSearch(sigOf(currentParams()))" />
    </div>

    <!-- 搜索工具行：大而醒目的输入框 + 搜索按钮 -->
    <div class="tool-row">
      <md-outlined-text-field
        class="search-field"
        :value="word"
        :placeholder="t('browse.search.placeholder')"
        :aria-label="t('nav.browseSearch')"
        @input="word = ($event.target as HTMLInputElement).value"
        @keydown.enter="doSearch"
      />
      <md-filled-button class="search-btn" @click="doSearch">
        {{ t("common.search") }}
      </md-filled-button>
    </div>
    <p class="field-help">{{ t("browse.search.help") }}</p>

    <!-- 过滤行：类型 tabs + 排序 / 对象 / 匹配下拉 -->
    <div class="filter-row">
      <div class="type-tabs-wrap" role="group" :aria-label="t('browse.search.typeLabel')">
        <SectionTabs class="type-tabs" :tabs="typeTabs" :value="kind" @change="onKindChange" />
      </div>
      <label class="select-item">
        <span>{{ t("browse.search.orderLabel") }}</span>
        <md-outlined-select :value="order" :aria-label="t('browse.search.orderLabel')" @change="onOrderChange">
          <md-select-option value="date_d">{{ t("browse.search.orderDateDesc") }}</md-select-option>
          <md-select-option value="date">{{ t("browse.search.orderDateAsc") }}</md-select-option>
          <md-select-option value="popular_d">{{ t("browse.search.orderPopular") }}</md-select-option>
        </md-outlined-select>
      </label>
      <label class="select-item">
        <span>{{ t("browse.search.modeLabel") }}</span>
        <md-outlined-select :value="mode" :aria-label="t('browse.search.modeLabel')" @change="onModeChange">
          <md-select-option value="all">{{ t("browse.search.modeAll") }}</md-select-option>
          <md-select-option value="safe">{{ t("browse.search.modeSafe") }}</md-select-option>
          <md-select-option value="r18">{{ t("browse.search.modeR18") }}</md-select-option>
        </md-outlined-select>
      </label>
      <label class="select-item">
        <span>{{ t("browse.search.sModeLabel") }}</span>
        <md-outlined-select :value="sMode" :aria-label="t('browse.search.sModeLabel')" @change="onSModeChange">
          <md-select-option value="s_tag">{{ t("browse.search.sModeTag") }}</md-select-option>
          <md-select-option value="s_tag_full">{{ t("browse.search.sModeTagFull") }}</md-select-option>
          <md-select-option value="s_tc">{{ t("browse.search.sModeTc") }}</md-select-option>
        </md-outlined-select>
      </label>
    </div>

    <p v-if="total !== null" class="total-line" role="status">
      {{ t("browse.search.total", { count: total }) }}
    </p>

    <WorkGrid
      :items="items"
      :loading="loading"
      :error="error"
      :loading-more="loadingMore"
      :has-more="hasMore"
      @load-more="loadMore"
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
.tool-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-md);
}

.search-field {
  flex: 1;
  min-width: 240px;
}

.search-btn {
  flex: none;
}

.field-help {
  margin: var(--space-xs) 0 0;
  font-size: 12px;
  color: var(--ink-subtle);
}

.filter-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-md);
  margin-top: var(--space-lg);
}

.type-tabs-wrap {
  flex: 0 1 auto;
  min-width: 0;
  overflow: hidden;
}

.select-item {
  display: flex;
  flex: 1;
  align-items: center;
  gap: var(--space-xs);
  min-width: 200px;
}

.select-item > span {
  flex: none;
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-muted);
}

.select-item md-outlined-select {
  flex: 1;
  min-width: 0;
}

.total-line {
  margin: var(--space-md) 0 0;
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-muted);
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

@media (max-width: 640px) {
  /* 工具行换行：输入框与按钮纵向堆叠，输入框占满整行 */
  .tool-row {
    flex-direction: column;
    align-items: stretch;
  }

  .search-field {
    min-width: 0;
  }

  .filter-row {
    flex-direction: column;
    align-items: stretch;
  }

  .select-item {
    min-width: 0;
  }
}
</style>
