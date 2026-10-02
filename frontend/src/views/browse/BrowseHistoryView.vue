<script setup lang="ts">
/**
 * 浏览·访问历史页（browse-history-ui-v1）—— 宫格回显浏览过的插画 / 漫画 / 小说。
 *
 * - 头部：返回 + 标题 + 右侧「刷新」与 danger「清空历史」；
 * - 宫格：历史行 → WorkCard（自绘网格，页码分页，与无限滚动不兼容的 WorkGrid 不用）；
 * - 状态：骨架 / 错误重试 / 空态（插画占位 + 引导文案）；
 * - 分页：AppPagination 已知总页数模式，默认 20/页（20/50/100），容量切换回第 1 页；
 * - 清空：原生 dialog 二次确认 → 成功后通知 + 回第 1 页 + 重新加载；
 * - 类别筛选：全部 / 插画 / 漫画 / 小说胶囊（默认全部），切换回第 1 页重新加载；
 * - 进入自动刷新：KeepAlive 下 onActivated 每次激活都重新加载（筛选与页码保留）；
 * - 数据来自历史行（非 BrowseWorkItem），仅展示，不做单条删除 / 搜索。
 */
import { computed, onActivated, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import PageBackButton from "../../components/navigation/PageBackButton.vue";
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
import WorkCard from "../../components/browse/WorkCard.vue";
import AppPagination from "../../components/common/AppPagination.vue";
import { filterByR18, useGlobalR18Filter } from "../../components/browse/r18Filter";
import {
  browseHistoryClear,
  browseHistoryList,
  errorMessage,
  type BrowseHistoryItem,
  type BrowseWorkItem,
} from "../../api/browse";
import { notify } from "../../ui/notify";

const { t } = useI18n();
const router = useRouter();

const PAGE_SIZE_OPTIONS = [20, 50, 100];
/** 类别筛选值：all = 全部（不传 kind）。 */
type HistoryKind = "all" | "illust" | "manga" | "novel";
const page = ref(1);
const pageSize = ref(20);
const kind = ref<HistoryKind>("all");
const items = ref<BrowseHistoryItem[]>([]);
const total = ref(0);
const loading = ref(false);
const clearing = ref(false);
const error = ref("");
const confirmDialog = ref<HTMLDialogElement>();

/** 竞态守卫：刷新 / 翻页 / 清空并发时，仅最后一次请求的结果落地。 */
let seq = 0;

/** 全局 R-18 渲染期过滤：不改数据、不改分页 total（同 WorkGrid / 排行榜口径）。 */
const r18Filter = useGlobalR18Filter();
const visibleItems = computed(() => filterByR18(items.value, r18Filter.value));
const hiddenCount = computed(() => items.value.length - visibleItems.value.length);

/** 类别筛选按钮（顺序：全部 / 插画 / 漫画 / 小说）。 */
const kindOptions = computed(() => [
  { value: "all" as const, label: t("browseHistory.filterAll") },
  { value: "illust" as const, label: t("browseHistory.filterIllust") },
  { value: "manga" as const, label: t("browseHistory.filterManga") },
  { value: "novel" as const, label: t("browseHistory.filterNovel") },
]);

/** 切换类别：回第 1 页并重新加载；重复点击当前项不请求。 */
function selectKind(next: HistoryKind): void {
  if (next === kind.value) return;
  kind.value = next;
  page.value = 1;
  void load();
}

/** 历史行 → WorkCard 需要的 BrowseWorkItem 形状（字段子集）。 */
const cards = computed(() =>
  visibleItems.value.map((it) => ({
    key: `${it.kind}:${it.work_id}`,
    item: {
      id: it.work_id,
      kind: it.kind,
      title: it.title,
      author_id: it.author_id,
      author_name: it.author_name,
      cover: it.cover,
      page_count: it.page_count,
      x_restrict: it.x_restrict,
    } satisfies BrowseWorkItem,
  }))
);

async function load(): Promise<void> {
  const token = ++seq;
  loading.value = true;
  error.value = "";
  try {
    const data = await browseHistoryList(
      page.value,
      pageSize.value,
      kind.value === "all" ? undefined : kind.value
    );
    if (token !== seq) return;
    items.value = data.items;
    total.value = data.total;
  } catch (err) {
    if (token !== seq) return;
    items.value = [];
    total.value = 0;
    error.value = errorMessage(err) || t("browseHistory.loadFailed");
  } finally {
    if (token === seq) loading.value = false;
  }
}

/** 容量切换回第 1 页（页码变化已由 v-model:current-page 回写）。 */
function onPagerChange({ pageSize: nextSize }: { page: number; pageSize: number | undefined }): void {
  if (nextSize !== undefined && nextSize !== pageSize.value) {
    pageSize.value = nextSize;
    page.value = 1;
  }
  void load();
}

function openClearConfirm(): void {
  if (total.value === 0) return;
  confirmDialog.value?.showModal();
}

async function doClear(): Promise<void> {
  confirmDialog.value?.close();
  clearing.value = true;
  try {
    const result = await browseHistoryClear();
    notify(t("browseHistory.cleared", { count: result.deleted }));
    page.value = 1;
    await load();
  } catch (err) {
    notify(errorMessage(err) || t("browseHistory.clearFailed"));
  } finally {
    clearing.value = false;
  }
}

/** 详情页跳转：历史存的就是路由 kind；ugoira 归一到 illust 查看器。 */
function openWork(item: BrowseWorkItem): void {
  const kind = item.kind === "novel" || item.kind === "manga" ? item.kind : "illust";
  void router.push(`/browse/work/${kind}/${item.id}`);
}

onActivated(load);
</script>

<template>
  <div class="page-view browse-history">
    <div class="browse-list-header">
      <div class="page-heading">
        <PageBackButton />
        <h1 class="page-title">{{ t("browseHistory.title") }}</h1>
      </div>
      <div class="header-actions">
        <ListRefreshButton :busy="loading" @refresh="load" />
        <md-text-button
          class="clear-btn"
          :disabled="loading || clearing || total === 0"
          @click="openClearConfirm"
        >
          {{ t("browseHistory.clearAll") }}
        </md-text-button>
      </div>
    </div>

    <!-- 类别筛选：全部 / 插画 / 漫画 / 小说（默认全部），切换回第 1 页重新加载 -->
    <div class="kind-filter" role="group" :aria-label="t('browseHistory.filterLabel')">
      <button
        v-for="option in kindOptions"
        :key="option.value"
        type="button"
        class="chip"
        :class="{ selected: kind === option.value }"
        :aria-pressed="kind === option.value"
        @click="selectKind(option.value)"
      >
        {{ option.label }}
      </button>
    </div>

    <!-- 首屏骨架：纯 surface-container 色块，无闪烁动画（对齐 WorkGrid 骨架规范） -->
    <div v-if="loading && !cards.length" class="history-grid" aria-hidden="true">
      <div v-for="n in 12" :key="n" class="skeleton-card">
        <div class="skeleton-cover" :class="{ portrait: n % 5 === 0 }"></div>
        <div class="skeleton-line w70"></div>
        <div class="skeleton-line w45"></div>
      </div>
    </div>

    <!-- 错误态：可读文案 + 重试 -->
    <div v-else-if="error && !cards.length" class="grid-state" role="alert">
      <p class="state-text">{{ error }}</p>
      <md-outlined-button @click="load">{{ t("common.retry") }}</md-outlined-button>
    </div>

    <!-- 空态：插画占位 + 引导文案 -->
    <div v-else-if="!cards.length" class="grid-state">
      <svg class="empty-art" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
        <circle cx="9" cy="9" r="2" />
        <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />
      </svg>
      <p class="state-text strong">{{ t("browseHistory.empty") }}</p>
      <p class="state-text">{{ t("browseHistory.emptyHint") }}</p>
      <p v-if="hiddenCount > 0" class="state-text">{{ t("common.browseR18Hidden", { count: hiddenCount }) }}</p>
    </div>

    <div v-else class="history-grid">
      <WorkCard
        v-for="(card, index) in cards"
        :key="card.key"
        :item="card.item"
        :priority="index < 12"
        @click="openWork(card.item)"
      />
    </div>

    <AppPagination
      class="pager"
      v-model:current-page="page"
      :total="total"
      :page-size="pageSize"
      :page-size-options="PAGE_SIZE_OPTIONS"
      :disabled="loading"
      @change="onPagerChange"
    />

    <!-- 清空二次确认：无标题的单行正文式（对齐历史页删除确认） -->
    <dialog ref="confirmDialog" class="m3-dialog">
      <p>{{ t("browseHistory.clearAllConfirm") }}</p>
      <div class="m3-row dialog-actions">
        <md-text-button @click="confirmDialog?.close()">{{ t("common.cancel") }}</md-text-button>
        <md-filled-button :disabled="clearing" @click="doClear">{{ t("common.confirm") }}</md-filled-button>
      </div>
    </dialog>
  </div>
</template>

<style scoped>
.header-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-sm) var(--space-md);
}

/* danger 文案色沿用既有规范值 #ba1a1a（AccountMenu / SettingsPanel） */
.clear-btn {
  --md-text-button-label-text-color: #ba1a1a;
}

/* 类别筛选胶囊：与频道页「内容筛选」同一 recipe（透明底、outline 60% 描边、选中 secondary-container） */
.kind-filter {
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

.pager {
  margin-top: var(--space-lg);
}

.history-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
  gap: var(--space-md) var(--space-lg);
}

.skeleton-card {
  padding: var(--space-xs);
}

.skeleton-cover {
  aspect-ratio: 1 / 1;
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
}

.skeleton-cover.portrait {
  aspect-ratio: 1 / 1.4;
}

.skeleton-line {
  height: 12px;
  margin-top: var(--space-sm);
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
}

.skeleton-line.w70 {
  width: 70%;
}

.skeleton-line.w45 {
  width: 45%;
}

.grid-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-sm);
  padding: var(--space-xl) var(--space-lg);
  text-align: center;
}

.state-text {
  margin: 0;
  color: var(--ink-muted);
  font-size: 14px;
  line-height: 1.5;
  max-width: 480px;
  overflow-wrap: anywhere;
}

.state-text.strong {
  color: var(--ink);
  font-weight: 600;
}

.empty-art {
  width: 96px;
  height: 96px;
  color: var(--ink-subtle);
  stroke-width: 1.5;
}
</style>
