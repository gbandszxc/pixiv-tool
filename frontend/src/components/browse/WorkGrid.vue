<script setup lang="ts">
/**
 * 作品网格：auto-fill 自适应列 + 骨架屏（纯色块、无动画）+ 空态 / 错误态 + 无限滚动。
 * 滚动接近底部（IntersectionObserver）时 emit load-more；错误由父级 retry。
 */
import { computed, nextTick, onActivated, onBeforeUnmount, onDeactivated, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import WorkCard from "./WorkCard.vue";
import { filterByR18, useGlobalR18Filter, type R18Filter } from "./r18Filter";
import type { BrowseWorkItem } from "../../api/browse";

const props = withDefaults(
  defineProps<{
    items: BrowseWorkItem[];
    /** 首屏加载中（显示骨架屏） */
    loading?: boolean;
    /** 错误文案（非空且无内容时显示错误态） */
    error?: string;
    /** 追加下一页中（底部追加骨架） */
    loadingMore?: boolean;
    /** 是否还有下一页（false 且有内容时显示「没有更多」） */
    hasMore?: boolean;
    /** 首屏骨架块数量 */
    skeletonCount?: number;
    /** 卡片快捷动作（打开原页 / 返填表单）：仅浏览频道页显式开启 */
    hooks?: boolean;
    /** 卡片显示「取消收藏」动作（收藏页）；无 bookmarkId 的条目自动隐藏 */
    removable?: boolean;
    /** 隐藏网格内的 R-18 计数提示（频道页自带的筛选条已在页级显示同一口径计数） */
    hideR18Hint?: boolean;
    /** 频道页显式覆盖；其他列表默认遵守全局开关。 */
    r18Filter?: R18Filter;
    /**
     * 页码分页模式：关闭无限滚动（不注册 IntersectionObserver / 不 emit load-more /
     * 不显示「没有更多」），翻页完全由父级 AppPagination 驱动。
     */
    paginated?: boolean;
    /**
     * 页码分页每页条数（列数档位取其因数保证每行排满）：
     * 20 → 2 / 4 / 5 / 10 列；30（小说搜索）→ 2 / 3 / 5 / 6 / 10 列。
     * 非分页的定长网格也可传该值并配合 tiered 启用档位（频道页 12 条板块）。
     */
    pageSize?: number;
    /**
     * 非分页定长网格显式启用列数档位（styles/tiered-grid.css，容量 = pageSize）；
     * paginated 模式自动启用。默认关闭，无限滚动网格保持 auto-fill 不受影响。
     */
    tiered?: boolean;
  }>(),
  {
    loading: false,
    error: "",
    loadingMore: false,
    hasMore: true,
    skeletonCount: 12,
    hooks: false,
    removable: false,
    hideR18Hint: false,
    paginated: false,
    pageSize: 20,
    tiered: false,
  }
);

const emit = defineEmits<{
  (e: "load-more"): void;
  (e: "retry"): void;
  (e: "select", item: BrowseWorkItem): void;
  (e: "remove-bookmark", item: BrowseWorkItem): void;
}>();

const { t } = useI18n();

/**
 * 列数档位 class（styles/tiered-grid.css）：paginated 自动启用，容量 = pageSize
 * （搜索页 20 / 30）；非分页定长网格（频道页 12 条板块）经 tiered 显式启用。
 * 档位由宿主 container-type 查询容器宽度驱动，无容器时退回基础 auto-fill。
 */
const gridTierClass = computed(() =>
  props.paginated || props.tiered ? ["tiered-grid", `cap-${props.pageSize}`] : []
);

/** 频道页覆盖只影响本实例，避免全局开关再次隐藏手动选中的作品。 */
const r18Filter = useGlobalR18Filter();
const visibleItems = computed(() => filterByR18(props.items, props.r18Filter ?? r18Filter.value));
const hiddenCount = computed(() => props.items.length - visibleItems.value.length);

/**
 * 无限滚动的读屏播报（视觉隐藏 aria-live 行）：加载完成报「已加载 N 件」，到底复用
 * 「没有更多」既有文案；首屏 / 追加加载中与空列表不播报，避免骨架期刷屏。
 * paginated 模式有自己的页码 aria，此行不渲染。
 */
const liveText = computed(() => {
  if (props.paginated || props.loading || props.loadingMore || props.error) return "";
  if (!visibleItems.value.length) return "";
  if (!props.hasMore) return t("common.browseNoMore");
  return t("browse.work.loadedCount", { count: visibleItems.value.length });
});

const sentinel = ref<HTMLElement | null>(null);
let observer: IntersectionObserver | null = null;
let active = true;

/** 仍按未过滤的 props.items 判断：R-18 全量过滤时保持翻页（可能短暂出现空态继续拉取）。 */
function maybeLoadMore(): void {
  if (props.paginated || !active || !props.hasMore || props.loading || props.loadingMore || props.error || !props.items.length) return;
  emit("load-more");
}

function onIntersect(entries: IntersectionObserverEntry[]): void {
  if (entries.some((entry) => entry.isIntersecting)) maybeLoadMore();
}

/** 首页数据到位后哨兵可能仍在视口内（不再触发新的 intersection），主动补查一次。 */
function checkNearViewport(): void {
  const el = sentinel.value;
  if (!active || !el?.isConnected) return;
  if (el.getBoundingClientRect().top < window.innerHeight + 480) maybeLoadMore();
}

onMounted(() => {
  if (props.paginated) return;
  observer = new IntersectionObserver(onIntersect, { rootMargin: "480px 0px" });
  if (sentinel.value) observer.observe(sentinel.value);
});

onActivated(() => {
  active = true;
  if (!props.paginated && sentinel.value) observer?.observe(sentinel.value);
});
onDeactivated(() => {
  active = false;
  observer?.disconnect();
});

watch(
  () => [props.items.length, props.hasMore, props.loading, props.loadingMore, props.error],
  async () => {
    await nextTick();
    if (!props.paginated && active && sentinel.value && observer) observer.observe(sentinel.value);
    checkNearViewport();
  }
);

onBeforeUnmount(() => {
  observer?.disconnect();
  observer = null;
});
</script>

<template>
  <div class="work-grid-wrap">
    <!-- 首屏骨架：纯 surface-container 色块，不做闪烁动画（DESIGN.md 克制动效 / reduced-motion） -->
    <div v-if="loading && !visibleItems.length" class="work-grid" :class="[{ paginated }, gridTierClass]" aria-hidden="true">
      <div v-for="n in skeletonCount" :key="n" class="skeleton-card">
        <div class="skeleton-cover" :class="{ portrait: n % 5 === 0 }"></div>
        <div class="skeleton-line w70"></div>
        <div class="skeleton-line w45"></div>
      </div>
    </div>

    <!-- 错误态：文案 + 重试 -->
    <div v-else-if="error && !visibleItems.length" class="grid-state" role="alert">
      <p class="state-text">{{ error }}</p>
      <md-outlined-button @click="emit('retry')">{{ t("common.retry") }}</md-outlined-button>
    </div>

    <!-- 空态：插画占位 + 引导文案 + 可选 action -->
    <div v-else-if="!visibleItems.length" class="grid-state">
      <svg class="empty-art" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <!-- lucide image -->
        <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
        <circle cx="9" cy="9" r="2" />
        <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />
      </svg>
      <p class="state-text strong">{{ t("common.browseEmpty") }}</p>
      <p class="state-text">{{ t("common.browseEmptyHint") }}</p>
      <p v-if="hiddenCount > 0 && !hideR18Hint" class="state-text">{{ t("common.browseR18Hidden", { count: hiddenCount }) }}</p>
      <div v-if="$slots.action" class="empty-action"><slot name="action" /></div>
    </div>

    <template v-else>
      <div class="work-grid" :class="[{ paginated }, gridTierClass]">
        <WorkCard
          v-for="(item, index) in visibleItems"
          :key="`${item.kind}:${item.id}`"
          :item="item"
          :priority="index < 12"
          :hooks="hooks"
          :removable="removable && !!item.bookmarkId"
          @click="emit('select', item)"
          @remove-bookmark="emit('remove-bookmark', item)"
        />
        <!-- 追加页骨架：与首屏同款色块 -->
        <template v-if="loadingMore">
          <div v-for="n in 6" :key="`more-${n}`" class="skeleton-card" aria-hidden="true">
            <div class="skeleton-cover" :class="{ portrait: n % 5 === 0 }"></div>
            <div class="skeleton-line w70"></div>
            <div class="skeleton-line w45"></div>
          </div>
        </template>
      </div>
      <p v-if="!paginated && !hasMore" class="grid-end">{{ t("common.browseNoMore") }}</p>
    </template>

    <!-- 读屏播报区：恒挂载保持 aria-live 区域稳定，内容空时不占位不播报 -->
    <p class="grid-live" aria-live="polite">{{ liveText }}</p>

    <div v-if="!paginated" ref="sentinel" class="grid-sentinel" aria-hidden="true"></div>
  </div>
</template>

<style scoped>
.work-grid-wrap {
  position: relative;
  min-height: 240px;
}

/* 读屏专用播报行：视觉隐藏但保留在可访问性树（display:none / visibility:hidden 均不播报）。
 * 项目内无现成视觉隐藏配方，按无障碍惯例内联 clip-path 方案。 */
.grid-live {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
}

.work-grid {
  display: grid;
  /* 基础列（渐进增强兜底）：paginated / tiered 网格的列数档位由全局
   * styles/tiered-grid.css 按容器查询接管（paginated 自动、tiered 显式启用），
   * 无容器宿主时保持本条 auto-fill 不变。 */
  grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
  gap: var(--space-md) var(--space-lg);
}

/* 大屏最小卡宽升级只作用于非档位网格：tiered 网格列数由全局档位表全权接管，
 * 不再叠加 auto-fill 媒体查询（否则会在宽屏压掉档位列数）。 */
@media (min-width: 1440px) {
  .work-grid:not(.paginated):not(.tiered-grid) {
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  }
}

@media (min-width: 1920px) {
  .work-grid:not(.paginated):not(.tiered-grid) {
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  }
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

.empty-action {
  margin-top: var(--space-sm);
}

.grid-end {
  margin: var(--space-lg) 0 0;
  color: var(--ink-subtle);
  font-size: 12px;
  font-weight: 600;
  text-align: center;
}

.grid-sentinel {
  width: 100%;
  height: 1px;
}
</style>
