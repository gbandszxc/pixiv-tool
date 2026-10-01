<script setup lang="ts">
/**
 * 相关推荐：紧凑网格（minmax 120px）+ WorkCard 直接组合。
 * WorkGrid 固定 minmax(160px, 1fr) 且无紧凑变体 prop（复用边界见单元 assumptions），
 * 故此处用本地样式网格承载 WorkCard；自带独立加载骨架 / 小错误态，不阻塞主视图。
 */
import { useI18n } from "vue-i18n";
import WorkCard from "./WorkCard.vue";
import type { BrowseWorkItem } from "../../api/browse";

withDefaults(
  defineProps<{
    items: BrowseWorkItem[];
    loading?: boolean;
    /** 错误文案（无内容时显示小错误态） */
    error?: string;
  }>(),
  { loading: false, error: "" }
);

const emit = defineEmits<{
  (e: "retry"): void;
  (e: "select", item: BrowseWorkItem): void;
}>();

const { t } = useI18n();
</script>

<template>
  <section v-if="loading || items.length || error" class="related">
    <h2 class="related-title">{{ t("browse.work.relatedTitle") }}</h2>

    <!-- 加载骨架：纯色块（与 WorkGrid 同风格，无动画） -->
    <div v-if="loading && !items.length" class="related-grid" aria-hidden="true">
      <div v-for="n in 6" :key="n" class="skeleton-card">
        <div class="skeleton-cover" :class="{ portrait: n % 5 === 0 }"></div>
        <div class="skeleton-line"></div>
      </div>
    </div>

    <div v-else-if="items.length" class="related-grid">
      <WorkCard
        v-for="item in items"
        :key="`${item.kind}:${item.id}`"
        :item="item"
        @click="emit('select', item)"
      />
    </div>

    <!-- 独立小错误态：不阻塞主视图 -->
    <div v-else class="related-state" role="alert">
      <p class="state-text">{{ error }}</p>
      <md-text-button @click="emit('retry')">{{ t("common.retry") }}</md-text-button>
    </div>
  </section>
</template>

<style scoped>
/* 分区标题：16px/600 on-surface-variant（DESIGN.md 浏览模式·分区与 Tab），
 * 避免错误态的 primary 文字按钮比标题更抢眼 */
.related-title {
  margin: 0 0 var(--space-sm);
  color: var(--ink-muted);
  font-size: 16px;
  font-weight: 600;
  line-height: 1.4;
}

.related-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
  gap: var(--space-xs) var(--space-sm);
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

.related-state {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
}

.state-text {
  margin: 0;
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}
</style>
