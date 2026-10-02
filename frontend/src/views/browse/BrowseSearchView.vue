<script setup lang="ts">
import PageBackButton from "../../components/navigation/PageBackButton.vue";
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
/**
 * 浏览·搜索页（F2）。
 *
 * - URL 查询参数 `?word=&kind=&order=&mode=&s_mode=` 回显：挂载/前进后退时恢复
 *   状态并自动搜索；执行搜索或切换过滤时 `router.replace` 写回（单一真相源是
 *   路由查询参数，本地 ref 仅作受控回显）。
 * - ID 直达：输入先经 `parseBrowseInput` 解析，命中链接/纯数字直接跳站内路由，
 *   不发起搜索；纯数字按当前类型 tab（插画/漫画/小说）落到对应详情，链接形态
 *   自带类型不受 tab 影响；未命中按关键词搜索。
 * - 端点映射：kind=illust|manga → artworks 检索（type=illust|manga）；
 *   kind=novel → novels 检索（不传 type）。
 * - 结果 WorkGrid + 「约 N 件」total；页码分页（AppPagination）：接口每页 60 条
 *   固定，前端切 3 个显示页（20/页），接口页缓存在内存供跨页复用。
 * - 本页排序：维度（点赞/收藏/浏览）下拉 + 升降方向切换（SortDirectionToggle），
 *   仅对当前显示页 20 条本地排序；
 *   三项计数列表接口不返回（官方页面亦逐项请求详情），按需经 browse_work_counts
 *   分批补取（全局限速），会话内缓存 + 进度提示，缺失项垫底。
 */
import { computed, ref, shallowRef, watch } from "vue";
import { useRoute, useRouter, type LocationQuery } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  browseSearch,
  browseWorkCounts,
  errorMessage,
  type BrowseWorkItem,
  type WorkCounts,
} from "../../api/browse";
import AppPagination from "../../components/common/AppPagination.vue";
import SortDirectionToggle from "../../components/common/SortDirectionToggle.vue";
import SectionTabs from "../../components/browse/SectionTabs.vue";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import { notify } from "../../ui/notify";
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

// ===== 结果列表（页码分页：接口每页 60 条固定，前端切 3 个显示页）=====

/** 显示页条数（AppPagination 每页容量）。 */
const PAGE_SIZE = 20;
/** 一个接口页（60 条）切成几个显示页。 */
const SEGMENTS_PER_API_PAGE = 3;
/** 接口页缓存上限（超出淘汰最旧；被淘汰页再访问时重新请求）。 */
const MAX_API_PAGES = 30;

const page = ref(1);
const total = ref<number | null>(null);
const lastPage = ref<number | null>(null);
const items = shallowRef<BrowseWorkItem[]>([]);
const loading = ref(false);
const error = ref("");

/** 接口页缓存（apiPage → 60 条）；搜索参数变化时清空，跨显示页复用避免重复请求。 */
const apiPages = new Map<number, BrowseWorkItem[]>();
/** 请求序号守卫：快速翻页 / 切条件时丢弃过期响应。 */
let seq = 0;

/** 显示页数：total÷20 与 lastPage×3 取小（lastPage 是 pixiv 端硬上限）。 */
const pageCount = computed<number | null>(() => {
  const byTotal = total.value != null ? Math.ceil(total.value / PAGE_SIZE) : null;
  const byLast = lastPage.value != null ? lastPage.value * SEGMENTS_PER_API_PAGE : null;
  if (byTotal == null) return byLast;
  if (byLast == null) return byTotal;
  return Math.min(byTotal, byLast);
});

/** 加载当前显示页：命中接口页缓存直接切片，否则请求该接口页并缓存。 */
async function load(): Promise<void> {
  const keyword = word.value.trim();
  if (!keyword) return;
  const token = ++seq;
  loading.value = true;
  error.value = "";
  const apiPage = Math.ceil(page.value / SEGMENTS_PER_API_PAGE);
  const offset = ((page.value - 1) % SEGMENTS_PER_API_PAGE) * PAGE_SIZE;
  const cached = apiPages.get(apiPage);
  if (cached) {
    items.value = cached.slice(offset, offset + PAGE_SIZE);
    loading.value = false;
    afterLoad();
    return;
  }
  try {
    const data = await browseSearch(kind.value, keyword, {
      order: order.value,
      mode: mode.value,
      s_mode: sMode.value,
      // novel 走 novels 端点不带 type；illust/manga 走 artworks 端点并带 type
      type: kind.value === "novel" ? undefined : kind.value,
      page: apiPage,
    });
    if (token !== seq) return;
    apiPages.set(apiPage, data.items);
    if (apiPages.size > MAX_API_PAGES) {
      apiPages.delete(apiPages.keys().next().value!);
    }
    total.value = data.total ?? null;
    lastPage.value = data.last_page ?? null;
    items.value = data.items.slice(offset, offset + PAGE_SIZE);
  } catch (err) {
    if (token !== seq) return;
    items.value = [];
    total.value = null;
    lastPage.value = null;
    error.value = errorMessage(err);
  } finally {
    if (token === seq) loading.value = false;
  }
  if (token === seq) afterLoad();
}

/** 整页替换后的收尾：回页首；处于计数排序时为新页补取计数。 */
function afterLoad(): void {
  window.scrollTo({ top: 0 });
  if (sortKey.value) void ensureCounts();
}

// ===== 本页排序（点赞 / 收藏 / 浏览 × 升/降；仅当前显示页本地排序）=====

type SortKey = "like" | "bookmark" | "view";
/** 本页排序维度：空串 = 默认顺序（接口顺序）；方向独立选择、跨维度保留。 */
const sortKey = ref<"" | SortKey>("");
const sortDir = ref<"asc" | "desc">("desc");

/** 三项计数缓存（`kind:id` → counts）；会话级，跨搜索 / 翻页复用。 */
const countsCache = new Map<string, WorkCounts>();
/** 计数缓存上限（超出淘汰最旧；被淘汰条目重选排序时重新补取）。 */
const MAX_COUNT_ENTRIES = 5000;
/** 缓存版本：普通 Map 非响应式，写入后自增以驱动 sortedItems 重算。 */
const countsVersion = ref(0);
const countsLoading = ref(false);
const countsDone = ref(0);
const countsTotal = ref(0);
/** 补取期间又来了新页请求：本轮结束后补跑一次。 */
let countsPending = false;

function countKey(item: BrowseWorkItem): string {
  return `${item.kind}:${item.id}`;
}

/** 当前显示页中尚无计数的条目。 */
function missingCountItems(): BrowseWorkItem[] {
  return items.value.filter((it) => !countsCache.has(countKey(it)));
}

/**
 * 分批补取当前显示页计数（每批 10 个，进度可见）。
 * 单项请求失败由后端跳过 → 该条计数缺失、排序垫底；整批网络失败提示并保留缺失。
 */
async function ensureCounts(): Promise<void> {
  if (countsLoading.value) {
    countsPending = true;
    return;
  }
  const missing = missingCountItems();
  if (!missing.length) return;
  countsLoading.value = true;
  countsTotal.value = missing.length;
  countsDone.value = 0;
  try {
    for (let i = 0; i < missing.length; i += 10) {
      const batch = missing.slice(i, i + 10);
      const data = await browseWorkCounts(kind.value, batch.map((it) => it.id));
      for (const it of batch) {
        const counts = data.counts[String(it.id)];
        if (!counts) continue;
        countsCache.set(countKey(it), counts);
        if (countsCache.size > MAX_COUNT_ENTRIES) {
          countsCache.delete(countsCache.keys().next().value!);
        }
      }
      countsVersion.value += 1;
      countsDone.value = Math.min(i + batch.length, missing.length);
    }
  } catch (err) {
    notify(errorMessage(err) || t("browse.search.countsFailed"));
  } finally {
    countsLoading.value = false;
    if (countsPending) {
      countsPending = false;
      void ensureCounts();
    }
  }
}

function metricOf(item: BrowseWorkItem, key: SortKey): number | null {
  const counts = countsCache.get(countKey(item));
  if (!counts) return null;
  const value =
    key === "like" ? counts.like_count : key === "bookmark" ? counts.bookmark_count : counts.view_count;
  return value ?? null;
}

/** 展示条目：默认 = 接口顺序；计数缺失项恒垫底。 */
const sortedItems = computed<BrowseWorkItem[]>(() => {
  void countsVersion.value; // 建立响应式依赖（countsCache 为普通 Map）
  const key = sortKey.value;
  if (!key) return items.value;
  const sign = sortDir.value === "desc" ? -1 : 1;
  return [...items.value].sort((a, b) => {
    const av = metricOf(a, key);
    const bv = metricOf(b, key);
    if (av === null && bv === null) return 0;
    if (av === null) return 1;
    if (bv === null) return -1;
    return sign * (av - bv);
  });
});

function onSortKeyChange(event: Event): void {
  const value = (event.target as HTMLSelectElement).value;
  sortKey.value = value === "like" || value === "bookmark" || value === "view" ? value : "";
  if (sortKey.value) void ensureCounts();
}

/** 方向切换：计数已就绪时由 sortedItems 立即重排（未选维度时控件禁用）。 */
function onSortDirChange(dir: "asc" | "desc"): void {
  sortDir.value = dir;
}

/** 翻页：整页替换（页码制）；回页首由 afterLoad 处理。 */
function onPagerChange({ page: next }: { page: number; pageSize: number | undefined }): void {
  if (next === page.value || loading.value) return;
  page.value = next;
  void load();
}

/** 最近一次已发起搜索的参数签名：URL 回显到达时据此去重，避免 replace 触发二次请求。 */
let lastFetchedSig = "";

function runSearch(sig: string): void {
  lastFetchedSig = sig;
  page.value = 1;
  total.value = null;
  lastPage.value = null;
  apiPages.clear();
  items.value = [];
  void load();
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
    lastPage.value = null;
    apiPages.clear();
    if (items.value.length || loading.value || error.value) {
      seq += 1; // 丢弃在途响应
      items.value = [];
      loading.value = false;
      error.value = "";
    }
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

/** 回车 / 搜索按钮：先试 ID 直达（纯数字按当前类型 tab 归属），未命中按关键词搜索（同参数重按视为重试）。 */
function doSearch(): void {
  const raw = word.value.trim();
  const target = parseBrowseInput(raw, kind.value);
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
    case "manga-work":
      void router.push(`/browse/work/manga/${target.id}`);
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
      <div class="page-heading"><PageBackButton /><h1 class="page-title">{{ t("nav.browseSearch") }}</h1></div>
      <ListRefreshButton :busy="loading" :disabled="!word.trim()" @refresh="runSearch(sigOf(currentParams()))" />
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
      <!-- 本页排序：维度下拉 + 升降方向切换；仅对当前显示页 20 条本地排序（计数按需补取） -->
      <div class="local-sort">
        <span class="local-sort-label" aria-hidden="true">{{ t("browse.search.localSortLabel") }}</span>
        <md-outlined-select
          :value="sortKey || 'default'"
          :disabled="!items.length"
          :aria-label="t('browse.search.localSortLabel')"
          @change="onSortKeyChange"
        >
          <md-select-option value="default">{{ t("browse.search.localSortDefault") }}</md-select-option>
          <md-select-option value="like">{{ t("browse.search.localSortLike") }}</md-select-option>
          <md-select-option value="bookmark">{{ t("browse.search.localSortBookmark") }}</md-select-option>
          <md-select-option value="view">{{ t("browse.search.localSortView") }}</md-select-option>
        </md-outlined-select>
        <SortDirectionToggle
          :value="sortDir"
          :disabled="!sortKey || !items.length"
          @change="onSortDirChange"
        />
      </div>
    </div>

    <div class="result-bar">
      <p v-if="total !== null" class="total-line" role="status">
        {{ t("browse.search.total", { count: total }) }}
      </p>
      <span v-if="countsLoading" class="counts-progress" role="status">
        {{ t("browse.search.countsLoading", { done: countsDone, total: countsTotal }) }}
      </span>
    </div>

    <!-- 查询容器：供 WorkGrid 的分页列数档位（2/4/5/10 列）按实际内容宽度查询 -->
    <div class="grid-host">
      <WorkGrid
        :items="sortedItems"
        :loading="loading"
        :error="error"
        paginated
        @retry="load()"
        @select="goWork"
      />
    </div>

    <!-- 页码分页（显示页 20 条；接口每页 60 条由本页切成 3 页） -->
    <AppPagination
      v-if="pageCount !== null && pageCount > 1"
      class="pager"
      :current-page="page"
      :total-pages="pageCount"
      :disabled="loading"
      @change="onPagerChange"
    />
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

/* 过滤行：tabs + 排序/对象/匹配 + 行尾本页排序，单行不换行；
   窄窗先把本页排序组换到第二行（见 1040px 断点），640px 以下纵向堆叠 */
.filter-row {
  display: flex;
  flex-wrap: nowrap;
  align-items: center;
  gap: var(--space-md);
  margin-top: var(--space-lg);
  min-width: 0;
}

.type-tabs-wrap {
  flex: none;
  min-width: 0;
  overflow: hidden;
}

.select-item {
  display: flex;
  flex: 0 1 auto;
  align-items: center;
  gap: var(--space-xs);
  min-width: 0;
}

/* 标签 12px 小字；下拉贴合选中项文本宽度（不设 flex: 1，避免等分撑满整行） */
.select-item > span,
.local-sort-label {
  flex: none;
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-muted);
}

/* 下拉按选中项文本定宽、不收缩：宽度不足时 Material 内部标签会换行增高（64px） */
.select-item md-outlined-select {
  flex: none;
  width: auto;
  min-width: 0;
}

/* 分页网格的查询容器：WorkGrid.paginated 按内容宽度切换 2/4/5/10 列（20 条的因数） */
.grid-host {
  container-type: inline-size;
}

/* 结果行：「约 N 件」+ 计数补取进度（本页排序控件与筛选控件同行，见 .local-sort） */
.result-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-xs) var(--space-md);
  margin-top: var(--space-md);
}

.total-line {
  margin: 0;
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-muted);
}

.counts-progress {
  font-size: 12px;
  color: var(--ink-subtle);
}

.local-sort {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: var(--space-xs);
  margin-left: auto;
}

.local-sort md-outlined-select {
  flex: none;
  width: auto;
  min-width: 0;
}

/* 分页行：仅负责与网格的间距（行内布局由 AppPagination 承担） */
.pager {
  margin-top: var(--space-lg);
}

/* 视口 < 1040px 时内容区已放不下完整筛选行（约 740px）：退回换行；
   640px 以下进一步纵向堆叠（见下） */
@media (max-width: 1040px) {
  .filter-row {
    flex-wrap: wrap;
  }
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

  /* 堆叠布局下下拉撑满整行 */
  .select-item md-outlined-select,
  .local-sort md-outlined-select {
    flex: 1;
    min-width: 0;
  }
}
</style>
