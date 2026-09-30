<script setup lang="ts">
/**
 * 作品网格：auto-fill 自适应列 + 骨架屏（纯色块、无动画）+ 空态 / 错误态 + 无限滚动。
 * 滚动接近底部（IntersectionObserver）时 emit load-more；错误由父级 retry。
 */
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import WorkCard from "./WorkCard.vue";
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
  }>(),
  { loading: false, error: "", loadingMore: false, hasMore: true, skeletonCount: 12 }
);

const emit = defineEmits<{
  (e: "load-more"): void;
  (e: "retry"): void;
  (e: "select", item: BrowseWorkItem): void;
}>();

const { t } = useI18n();

const sentinel = ref<HTMLElement | null>(null);
let observer: IntersectionObserver | null = null;

function maybeLoadMore(): void {
  if (!props.hasMore || props.loading || props.loadingMore || props.error || !props.items.length) return;
  emit("load-more");
}

function onIntersect(entries: IntersectionObserverEntry[]): void {
  if (entries.some((entry) => entry.isIntersecting)) maybeLoadMore();
}

/** 首页数据到位后哨兵可能仍在视口内（不再触发新的 intersection），主动补查一次。 */
function checkNearViewport(): void {
  const el = sentinel.value;
  if (!el) return;
  if (el.getBoundingClientRect().top < window.innerHeight + 480) maybeLoadMore();
}

onMounted(() => {
  observer = new IntersectionObserver(onIntersect, { rootMargin: "480px 0px" });
  if (sentinel.value) observer.observe(sentinel.value);
});

watch(
  () => [props.items.length, props.hasMore, props.loading, props.loadingMore, props.error],
  async () => {
    await nextTick();
    if (sentinel.value && observer) observer.observe(sentinel.value);
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
    <div v-if="loading && !items.length" class="work-grid" aria-hidden="true">
      <div v-for="n in skeletonCount" :key="n" class="skeleton-card">
        <div class="skeleton-cover" :class="{ portrait: n % 5 === 0 }"></div>
        <div class="skeleton-line w70"></div>
        <div class="skeleton-line w45"></div>
      </div>
    </div>

    <!-- 错误态：文案 + 重试 -->
    <div v-else-if="error && !items.length" class="grid-state" role="alert">
      <p class="state-text">{{ error }}</p>
      <md-outlined-button @click="emit('retry')">{{ t("common.retry") }}</md-outlined-button>
    </div>

    <!-- 空态：插画占位 + 引导文案 + 可选 action -->
    <div v-else-if="!items.length" class="grid-state">
      <svg class="empty-art" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="3.5" y="5" width="17" height="14" rx="2" />
        <circle cx="9" cy="10" r="1.5" />
        <path d="m6 16.5 3.5-3.5 3 3 2.5-2.5 3 3" />
      </svg>
      <p class="state-text strong">{{ t("common.browseEmpty") }}</p>
      <p class="state-text">{{ t("common.browseEmptyHint") }}</p>
      <div v-if="$slots.action" class="empty-action"><slot name="action" /></div>
    </div>

    <template v-else>
      <div class="work-grid">
        <WorkCard
          v-for="item in items"
          :key="`${item.kind}:${item.id}`"
          :item="item"
          @click="emit('select', item)"
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
      <p v-if="!hasMore" class="grid-end">{{ t("common.browseNoMore") }}</p>
    </template>

    <div ref="sentinel" class="grid-sentinel" aria-hidden="true"></div>
  </div>
</template>

<style scoped>
.work-grid-wrap {
  min-height: 240px;
}

.work-grid {
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
