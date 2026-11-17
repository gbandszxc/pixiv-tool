<script setup lang="ts">
/**
 * 用户搜索结果列表：UserCard 行卡 + loading 骨架 / 错误 / 空态。
 * 状态配方与 WorkGrid 同源（骨架纯色块不闪烁、错误可重试、空态占位插画）；
 * 分页由父级 AppPagination 承担，本组件不做加载合并/无限滚动。
 */
import { useI18n } from "vue-i18n";
import UserCard from "./UserCard.vue";
import type { BrowseUserItem } from "../../api/browse";

withDefaults(
  defineProps<{
    items: BrowseUserItem[];
    loading?: boolean;
    error?: string;
    /** 骨架行数（与每页条数一致）。 */
    skeletonCount?: number;
  }>(),
  { loading: false, error: "", skeletonCount: 10 }
);

defineEmits<{
  (e: "select", item: BrowseUserItem): void;
  (e: "retry"): void;
}>();

const { t } = useI18n();
</script>

<template>
  <div class="user-grid">
    <!-- 首屏骨架：纯 surface-container 色块，不做闪烁动画（DESIGN.md 克制动效） -->
    <div v-if="loading && !items.length" class="rows" aria-hidden="true">
      <div v-for="n in skeletonCount" :key="n" class="skeleton-row">
        <div class="skeleton-avatar"></div>
        <div class="skeleton-lines">
          <div class="skeleton-line w40"></div>
          <div class="skeleton-line w60"></div>
        </div>
        <div class="skeleton-works">
          <div v-for="m in 3" :key="m" class="skeleton-avatar"></div>
        </div>
      </div>
    </div>

    <!-- 错误态：文案 + 重试 -->
    <div v-else-if="error && !items.length" class="grid-state" role="alert">
      <p class="state-text">{{ error }}</p>
      <md-outlined-button @click="$emit('retry')">{{ t("common.retry") }}</md-outlined-button>
    </div>

    <!-- 空态：占位插画 + 引导文案 -->
    <div v-else-if="!items.length" class="grid-state">
      <svg class="empty-art" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <!-- lucide user-round-search -->
        <circle cx="10" cy="10" r="4" />
        <path d="m14.5 14.5 4 4" />
        <path d="m21 21-6-6" />
        <circle cx="12" cy="12" r="10" />
      </svg>
      <p class="state-text strong">{{ t("browse.userSearch.empty") }}</p>
      <p class="state-text">{{ t("browse.userSearch.emptyHint") }}</p>
    </div>

    <div v-else class="rows">
      <UserCard
        v-for="(item, index) in items"
        :key="item.id"
        :item="item"
        :priority="index < 4"
        @click="$emit('select', item)"
      />
    </div>
  </div>
</template>

<style scoped>
.user-grid {
  min-height: 200px;
}

.rows {
  display: flex;
  flex-direction: column;
  gap: var(--space-sm);
}

/* 骨架行：头像块 + 两行文本 + 右侧缩略块（纯色块，同 WorkGrid 配方） */
.skeleton-row {
  display: flex;
  align-items: center;
  gap: var(--space-md);
  padding: var(--space-sm) var(--space-md);
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
}

.skeleton-avatar {
  flex: none;
  width: 56px;
  height: 56px;
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container-high);
}

.skeleton-lines {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: var(--space-xs);
}

.skeleton-line {
  height: 12px;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container-high);
}

.w40 {
  width: 40%;
}

.w60 {
  width: 60%;
}

.skeleton-works {
  display: flex;
  flex: none;
  gap: var(--space-xs);
}

/* 状态区（错误 / 空态）：与 WorkGrid 同款居中配方 */
.grid-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-sm);
  min-height: 200px;
  text-align: center;
}

.state-text {
  margin: 0;
  font-size: 12px;
  color: var(--ink-muted);
}

.state-text.strong {
  font-size: 14px;
  font-weight: 600;
  color: var(--ink);
}

.empty-art {
  width: 48px;
  height: 48px;
  color: var(--ink-subtle);
}
</style>
