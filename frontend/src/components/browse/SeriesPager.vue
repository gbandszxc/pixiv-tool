<script setup lang="ts">
/**
 * 系列分集翻页器：prev/next + 页码下拉直选（复用小说阅读器翻页器 recipe——
 * sticky 底部、surface 底 + 上缘 divider、md-icon-button + page-select）。
 * page 为受控属性：父级 change 后回写，本组件不持有页码状态；
 * 越出范围时 prev/next 原生 disabled（Material 自带降透明禁用态）。
 */
import { useI18n } from "vue-i18n";

const props = defineProps<{ page: number; totalPages: number }>();
const emit = defineEmits<{ (e: "change", page: number): void }>();

const { t } = useI18n();

function prev(): void {
  if (props.page > 1) emit("change", props.page - 1);
}

function next(): void {
  if (props.page < props.totalPages) emit("change", props.page + 1);
}

function onSelect(event: Event): void {
  emit("change", Number((event.target as HTMLSelectElement).value));
}
</script>

<template>
  <div class="series-pager">
    <div class="pager-inner">
      <md-icon-button
        :aria-label="t('browse.novel.prevPage')"
        :title="t('browse.novel.prevPage')"
        :disabled="page <= 1"
        @click="prev"
      >
        <svg class="pager-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="15 18 9 12 15 6" /></svg>
      </md-icon-button>
      <select
        class="page-select"
        :value="page"
        :aria-label="t('browse.novel.pageSelect')"
        @change="onSelect"
      >
        <option v-for="n in totalPages" :key="n" :value="n">
          {{ t("browse.novel.pageInfo", { current: n, total: totalPages }) }}
        </option>
      </select>
      <md-icon-button
        :aria-label="t('browse.novel.nextPage')"
        :title="t('browse.novel.nextPage')"
        :disabled="page >= totalPages"
        @click="next"
      >
        <svg class="pager-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="9 18 15 12 9 6" /></svg>
      </md-icon-button>
    </div>
  </div>
</template>

<style scoped>
/* 与小说阅读器翻页器同 recipe：sticky 底部 + surface 底 + 上缘 divider */
.series-pager {
  position: sticky;
  bottom: 0;
  z-index: 10;
  display: flex;
  justify-content: center;
  padding: var(--space-sm) var(--space-md);
  background: var(--surface);
  border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 30%, transparent);
}

.pager-inner {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
}

.page-select {
  max-width: 200px;
  padding: var(--space-xs) var(--space-lg) var(--space-xs) var(--space-sm);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 45%, transparent);
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  font-size: 13px;
  text-align: center;
  cursor: pointer;
}

.page-select:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.pager-icon {
  width: 20px;
  height: 20px;
  stroke-width: 1.8;
}
</style>
