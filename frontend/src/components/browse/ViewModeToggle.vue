<script setup lang="ts">
/**
 * 系列分集页·展示模式切换：宫格 / 列表两枚 inline SVG icon toggle
 * （lucide.dev 官方 layout-grid / list 路径，内联且零依赖、不引外部图标库）。
 * 选中态 secondary-container 底 / on-secondary-container，未选中透明底 + ink；
 * 30px、aria-pressed + aria-label、focus-visible primary 2px 外环。
 */
import { useI18n } from "vue-i18n";
import type { SeriesViewMode } from "./seriesView";

const props = defineProps<{ value: SeriesViewMode }>();
const emit = defineEmits<{ (e: "change", mode: SeriesViewMode): void }>();

const { t } = useI18n();

function select(mode: SeriesViewMode): void {
  if (mode === props.value) return;
  emit("change", mode);
}
</script>

<template>
  <div class="view-mode-toggle" role="group" :aria-label="t('browse.series.viewModeLabel')">
    <button
      type="button"
      class="mode-btn"
      :class="{ active: value === 'grid' }"
      :aria-pressed="value === 'grid'"
      :aria-label="t('browse.series.viewGrid')"
      :title="t('browse.series.viewGrid')"
      @click="select('grid')"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <!-- lucide layout-grid -->
        <rect width="7" height="7" x="3" y="3" rx="1" />
        <rect width="7" height="7" x="14" y="3" rx="1" />
        <rect width="7" height="7" x="14" y="14" rx="1" />
        <rect width="7" height="7" x="3" y="14" rx="1" />
      </svg>
    </button>
    <button
      type="button"
      class="mode-btn"
      :class="{ active: value === 'list' }"
      :aria-pressed="value === 'list'"
      :aria-label="t('browse.series.viewList')"
      :title="t('browse.series.viewList')"
      @click="select('list')"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <!-- lucide list -->
        <path d="M3 5h.01" />
        <path d="M3 12h.01" />
        <path d="M3 19h.01" />
        <path d="M8 5h13" />
        <path d="M8 12h13" />
        <path d="M8 19h13" />
      </svg>
    </button>
  </div>
</template>

<style scoped>
.view-mode-toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xxs);
}

.mode-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: var(--ink);
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.mode-btn:hover:not(.active) {
  background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.mode-btn.active {
  background: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

.mode-btn:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.mode-btn svg {
  width: 18px;
  height: 18px;
  stroke-width: 2;
}

@media (prefers-reduced-motion: reduce) {
  .mode-btn {
    transition: none;
  }
}
</style>
