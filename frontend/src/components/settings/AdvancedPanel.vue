<template>
  <div class="advanced-panel">
    <!-- 分组索引：每项整行可点，沿用小说翻译二级入口配方（56px 行、8% primary hover、primary 2px focus 环） -->
    <div class="advanced-index">
      <button type="button" class="advanced-index-item" @click="emit('open-sync')">
        <span class="advanced-index-text">
          <span class="advanced-index-title">{{ t("sync.entryTitle") }}</span>
          <span class="advanced-index-hint">{{ t("sync.entryHint") }}</span>
        </span>
        <svg class="advanced-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 设置「高级」分组索引页：当前只有「同步」一个子项，点击由父级切到二级同步页
 * （同一内容区切换，不叠模态）。
 */
import { useI18n } from "vue-i18n";

const emit = defineEmits<{ (event: "open-sync"): void }>();
const { t } = useI18n();
</script>

<style scoped>
.advanced-panel { display: grid; gap: var(--space-lg); }
.advanced-index { display: grid; border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.advanced-index-item {
  display: flex; align-items: center; gap: var(--space-md); width: 100%; min-height: 56px;
  padding: var(--space-sm) var(--space-xs); color: var(--ink); font: inherit; text-align: left;
  background: transparent; border: 0; border-bottom: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); cursor: pointer;
}
.advanced-index-item:hover { background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }
.advanced-index-item:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 2px; }
.advanced-index-text { display: grid; gap: 2px; flex: 1; min-width: 0; }
.advanced-index-title { font-size: 14px; font-weight: 500; }
.advanced-index-hint { color: var(--ink-muted); font-size: 12px; line-height: 1.5; overflow-wrap: anywhere; }
.advanced-chevron { flex: none; width: 18px; height: 18px; color: var(--ink-muted); }
</style>
