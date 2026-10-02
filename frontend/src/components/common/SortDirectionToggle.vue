<script setup lang="ts">
/**
 * 排序方向切换：升 / 降两枚 inline SVG icon toggle，供「本页排序」等本地排序复用
 * （lucide.dev 官方 arrow-up-narrow-wide / arrow-down-wide-narrow 路径，内联且零依赖）。
 * 选中态 secondary-container 底 / on-secondary-container，未选中透明底 + ink；
 * 30px、aria-pressed + aria-label、focus-visible primary 2px 外环；disabled 时整体禁用。
 */
import { useI18n } from "vue-i18n";

type SortDirection = "asc" | "desc";

const props = defineProps<{
  value: SortDirection;
  /** 禁用态（如尚未选择排序维度时） */
  disabled?: boolean;
}>();
const emit = defineEmits<{ (e: "change", value: SortDirection): void }>();

const { t } = useI18n();

function select(dir: SortDirection): void {
  if (props.disabled || dir === props.value) return;
  emit("change", dir);
}
</script>

<template>
  <div class="sort-direction-toggle" role="group" :aria-label="t('common.sortDirectionLabel')">
    <button
      type="button"
      class="dir-btn"
      :class="{ active: value === 'asc' }"
      :aria-pressed="value === 'asc'"
      :disabled="disabled"
      :aria-label="t('common.sortAsc')"
      :title="t('common.sortAsc')"
      @click="select('asc')"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <!-- lucide arrow-up-narrow-wide -->
        <path d="m3 8 4-4 4 4" />
        <path d="M7 4v16" />
        <path d="M11 12h4" />
        <path d="M11 16h7" />
        <path d="M11 20h10" />
      </svg>
    </button>
    <button
      type="button"
      class="dir-btn"
      :class="{ active: value === 'desc' }"
      :aria-pressed="value === 'desc'"
      :disabled="disabled"
      :aria-label="t('common.sortDesc')"
      :title="t('common.sortDesc')"
      @click="select('desc')"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <!-- lucide arrow-down-wide-narrow -->
        <path d="m3 16 4 4 4-4" />
        <path d="M7 20V4" />
        <path d="M11 4h10" />
        <path d="M11 8h7" />
        <path d="M11 12h4" />
      </svg>
    </button>
  </div>
</template>

<style scoped>
.sort-direction-toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xxs);
}

.dir-btn {
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

.dir-btn:hover:not(.active):not(:disabled) {
  background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.dir-btn.active {
  background: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

.dir-btn:disabled {
  color: var(--ink-subtle);
  cursor: default;
}

.dir-btn:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.dir-btn svg {
  width: 18px;
  height: 18px;
  stroke-width: 2;
}

@media (prefers-reduced-motion: reduce) {
  .dir-btn {
    transition: none;
  }
}
</style>
