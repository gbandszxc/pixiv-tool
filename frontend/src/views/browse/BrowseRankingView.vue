<script setup lang="ts">
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
/**
 * 浏览·排行榜：类型 tab（插画/漫画/动图/小说）+ 周期 select + 日期导航 + 页码制分页（50/页）。
 * ?kind=&mode= query 回显（router.replace 同步）；kind/mode/页码/日期任一变更都回到第 1 页重查。
 * 条目带 rank 徽标（前三名 primary-container 底突出）；R-18 周期用文案后缀标注。
 */
import { computed, onMounted, ref, shallowRef, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import {
  browseRanking,
  errorMessage,
  type BrowseWorkItem,
  type RankingKind,
  type RankingMode,
} from "../../api/browse";
import AppPagination from "../../components/common/AppPagination.vue";
import SectionTabs, { type SectionTab } from "../../components/browse/SectionTabs.vue";
import WorkCard from "../../components/browse/WorkCard.vue";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import { filterByR18, useGlobalR18Filter } from "../../components/browse/r18Filter";

const { t } = useI18n();
const route = useRoute();
const router = useRouter();

/** v2 契约：illust/manga/ugoira 与 novel 各有一套合法 mode。 */
const ART_MODES: RankingMode[] = ["daily", "weekly", "monthly", "rookie", "daily_r18", "weekly_r18"];
const NOVEL_MODES: RankingMode[] = ["daily", "weekly", "monthly", "male", "female", "daily_r18"];
const RANKING_KINDS: RankingKind[] = ["illust", "manga", "ugoira", "novel"];
const PAGE_SIZE = 50;

const kind = ref<RankingKind>("illust");
const mode = ref<RankingMode>("daily");
const page = ref(1);
/** 历史榜单日期（YYYYMMDD）；undefined = 最新一期。 */
const date = ref<string | undefined>(undefined);

const items = shallowRef<BrowseWorkItem[]>([]);
const loading = ref(false);
const error = ref("");
const nextPage = ref<number | null>(null);
const resDate = ref("");
const prevDate = ref<string | null>(null);
const nextDate = ref<string | null>(null);

function normalizeKind(value: unknown): RankingKind {
  return RANKING_KINDS.includes(value as RankingKind) ? (value as RankingKind) : "illust";
}

function modesFor(k: RankingKind): RankingMode[] {
  return k === "novel" ? NOVEL_MODES : ART_MODES;
}

function normalizeMode(value: unknown, k: RankingKind): RankingMode {
  return modesFor(k).includes(value as RankingMode) ? (value as RankingMode) : "daily";
}

/** 请求序号守卫：快速切换条件时丢弃过期响应。 */
let seq = 0;

async function load(): Promise<void> {
  const current = ++seq;
  loading.value = true;
  error.value = "";
  try {
    const data = await browseRanking(kind.value, mode.value, page.value, date.value);
    if (current !== seq) return;
    items.value = data.items;
    nextPage.value = data.next_page ?? null;
    resDate.value = data.date;
    prevDate.value = data.prev_date ?? null;
    nextDate.value = data.next_date ?? null;
  } catch (err) {
    if (current !== seq) return;
    items.value = [];
    error.value = errorMessage(err);
  } finally {
    if (current === seq) loading.value = false;
  }
}

/** 参数变更后的公共收尾：清空列表（页码制整页替换）→ 同步 URL → 重新加载。 */
function reload(): void {
  items.value = [];
  nextPage.value = null;
  syncQuery();
  void load();
}

function changeKind(value: string): void {
  if (value === kind.value) return;
  kind.value = normalizeKind(value);
  mode.value = "daily"; // 两套 mode 集合不同，切换类型一律回到日榜
  page.value = 1;
  date.value = undefined;
  reload();
}

function changeMode(value: string): void {
  if (value === mode.value) return;
  mode.value = normalizeMode(value, kind.value);
  page.value = 1;
  reload();
}

function goPage(value: number): void {
  if (loading.value || value < 1 || value === page.value) return;
  if (value > page.value && nextPage.value === null) return;
  page.value = value;
  reload();
}

function goDate(value: string | null): void {
  if (!value || loading.value) return;
  date.value = value;
  page.value = 1;
  reload();
}

function syncQuery(): void {
  if (route.query.kind === kind.value && route.query.mode === mode.value) return;
  void router.replace({ query: { kind: kind.value, mode: mode.value } });
}

/** 外部导航（浏览器前进/后退、他页跳转）带来的 query 变化 → 回显并重查。 */
watch(
  () => route.query,
  (query) => {
    if (route.path !== "/browse/ranking") return;
    const nextKind = normalizeKind(query.kind);
    const nextMode = normalizeMode(query.mode, nextKind);
    if (nextKind === kind.value && nextMode === mode.value) return;
    kind.value = nextKind;
    mode.value = nextMode;
    page.value = 1;
    date.value = undefined;
    reload();
  }
);

onMounted(() => {
  kind.value = normalizeKind(route.query.kind);
  mode.value = normalizeMode(route.query.mode, kind.value);
  syncQuery();
  void load();
});

const kindTabs = computed<SectionTab[]>(() => [
  { value: "illust", label: t("browse.ranking.kindIllust") },
  { value: "manga", label: t("browse.ranking.kindManga") },
  { value: "ugoira", label: t("browse.ranking.kindUgoira") },
  { value: "novel", label: t("browse.ranking.kindNovel") },
]);

const modeOptions = computed(() =>
  modesFor(kind.value).map((value) => ({ value, label: t(`browse.ranking.mode.${value}`) }))
);

const dateDisplay = computed(() => (resDate.value ? formatYmd(resDate.value) : "—"));

/** 页码制名次区间：末页以实际条数收尾。 */
const rangeLabel = computed(() => {
  const from = (page.value - 1) * PAGE_SIZE + 1;
  const to =
    nextPage.value === null ? (page.value - 1) * PAGE_SIZE + items.value.length : page.value * PAGE_SIZE;
  return t("browse.ranking.pageRange", { from, to: Math.max(to, from) });
});

/**
 * 全局 R-18 过滤：后端 parse_ranking_illust / parse_ranking_novel 已补 x_restrict
 * （illust_content_type.sexual 优先，顶层 x_restrict 兜底），故与其它列表同口径。
 * 过滤只影响渲染，名次与分页仍按服务端口径（rangeLabel / 页码不变）。
 */
const r18Filter = useGlobalR18Filter();
const visibleItems = computed(() => filterByR18(items.value, r18Filter.value));

/** YYYYMMDD → YYYY-MM-DD（非法格式原样返回）。 */
function formatYmd(ymd: string): string {
  if (!/^\d{8}$/.test(ymd)) return ymd;
  return `${ymd.slice(0, 4)}-${ymd.slice(4, 6)}-${ymd.slice(6, 8)}`;
}

/** 卡片跳转：novel → 小说阅读器；其余 → 作品查看器（ugoira 按 illust 详情拉取）。 */
function openWork(item: BrowseWorkItem): void {
  const kindPath = item.kind === "novel" ? "novel" : item.kind === "manga" ? "manga" : "illust";
  void router.push(`/browse/work/${kindPath}/${item.id}`);
}
</script>

<template>
  <div class="page-view browse-ranking">
    <div class="browse-list-header">
      <h1 class="page-title">{{ t("nav.browseRanking") }}</h1>
      <ListRefreshButton :busy="loading" @refresh="load" />
    </div>

    <div class="ranking-toolbar">
      <SectionTabs class="kind-tabs" :tabs="kindTabs" :value="kind" @change="changeKind" />
      <div class="toolbar-controls">
        <md-outlined-select
          class="mode-select"
          :value="mode"
          :aria-label="t('browse.ranking.modeLabel')"
          @change="changeMode(($event.target as HTMLSelectElement).value)"
        >
          <md-select-option v-for="opt in modeOptions" :key="opt.value" :value="opt.value">
            {{ opt.label }}
          </md-select-option>
        </md-outlined-select>
        <div class="date-nav">
          <md-icon-button
            :disabled="!prevDate || loading"
            :aria-label="t('browse.ranking.prevDate')"
            :title="t('browse.ranking.prevDate')"
            @click="goDate(prevDate)"
          >
            <svg class="chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="15 18 9 12 15 6" /></svg>
          </md-icon-button>
          <span class="date-text">{{ dateDisplay }}</span>
          <md-icon-button
            :disabled="!nextDate || loading"
            :aria-label="t('browse.ranking.nextDate')"
            :title="t('browse.ranking.nextDate')"
            @click="goDate(nextDate)"
          >
            <svg class="chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="9 18 15 12 9 6" /></svg>
          </md-icon-button>
        </div>
      </div>
    </div>

    <!-- 骨架 / 错误重试 / 空态交给 WorkGrid；条目分支自绘单元格以叠加 rank 徽标 -->
    <WorkGrid v-if="loading || error || !visibleItems.length" :items="[]" :loading="loading" :error="error" @retry="load" />
    <div v-else class="ranking-grid">
      <div v-for="item in visibleItems" :key="`${item.kind}:${item.id}`" class="rank-cell">
        <span
          v-if="item.rank != null"
          class="rank-badge"
          :class="{ top: (item.rank ?? 0) <= 3 }"
          >#{{ item.rank }}</span
        >
        <WorkCard :item="item" @click="openWork" />
      </div>
    </div>

    <!-- 分页：AppPagination 未知总页数模式（pixiv 只回 next_page 链），名次区间入 #start 插槽；goPage 守卫语义不变 -->
    <AppPagination
      class="pager"
      :current-page="page"
      :has-next="nextPage !== null"
      :disabled="loading"
      @update:currentPage="goPage"
    >
      <template #start>
        <span class="pager-range">{{ rangeLabel }}</span>
      </template>
    </AppPagination>
  </div>
</template>

<style scoped>
.ranking-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-sm) var(--space-md);
  margin-bottom: var(--space-lg);
}

.kind-tabs {
  flex: 1 1 280px;
}

.toolbar-controls {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-sm) var(--space-md);
}

.mode-select {
  width: 168px;
  min-width: 0; /* md-outlined-select 宿主默认 min-width:210px，会压过 width，需显式放开 */
}

.date-nav {
  display: flex;
  align-items: center;
  gap: var(--space-xxs);
}

/* 日期/分页箭头：与系列分集、小说阅读器翻页器同一 recipe（md-icon-button + 20px 线性 chevron） */
.chevron {
  width: 20px;
  height: 20px;
  stroke-width: 1.8;
}

.date-text {
  min-width: 88px;
  color: var(--ink);
  font-size: 12px;
  font-weight: 600;
  text-align: center;
  font-variant-numeric: tabular-nums;
}

/* 与 WorkGrid 同款网格参数（auto-fill 自适应降列） */
.ranking-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
  gap: var(--space-md) var(--space-lg);
}

.rank-cell {
  position: relative;
}

/* rank 徽标：盖在封面右上（左上是页数/R-18 徽标、右下是 novel 角标，互不遮挡） */
.rank-badge {
  position: absolute;
  top: var(--space-xs);
  right: var(--space-xs);
  z-index: 1;
  padding: 1px var(--space-sm);
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
}

/* 前三名：primary-container 底突出（M3 角色，不造新色） */
.rank-badge.top {
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

/* AppPagination 落位：仅负责与网格的间距，行内布局由组件自身承担 */
.pager {
  margin-top: var(--space-lg);
}

.pager-range {
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
}

@media (max-width: 640px) {
  .mode-select {
    width: 100%;
  }
}
</style>
