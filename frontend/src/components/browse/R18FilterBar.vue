<script setup lang="ts">
/**
 * 频道页 R-18 筛选条：三个胶囊（全部 / 一般向 / R-18）+ 隐藏计数提示。
 * 纯受控：选中态由 modelValue 决定，点击只 emit update:modelValue（不请求数据）。
 * 与全局开关的关系见 r18Filter.ts：本条的档位只在频道页生效。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import type { R18Filter } from "./r18Filter";

withDefaults(
  defineProps<{
    modelValue: R18Filter;
    /** 当前档位下被隐藏的 R-18 条目数（0 不显示提示） */
    hiddenCount?: number;
  }>(),
  { hiddenCount: 0 }
);

const emit = defineEmits<{
  (e: "update:modelValue", value: R18Filter): void;
}>();

const { t } = useI18n();

const options = computed(() => [
  { value: "all" as const, label: t("browse.channel.r18All") },
  { value: "safe" as const, label: t("browse.channel.r18Safe") },
  { value: "r18" as const, label: t("browse.channel.r18Only") },
]);
</script>

<template>
  <div class="r18-bar" role="group" :aria-label="t('browse.channel.r18FilterLabel')">
    <button
      v-for="option in options"
      :key="option.value"
      type="button"
      class="chip"
      :class="{ selected: modelValue === option.value }"
      :aria-pressed="modelValue === option.value"
      @click="emit('update:modelValue', option.value)"
    >
      {{ option.label }}
    </button>
    <span v-if="hiddenCount > 0" class="hidden-hint">{{ t("common.browseR18Hidden", { count: hiddenCount }) }}</span>
  </div>
</template>

<style scoped>
.r18-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-sm);
  margin-bottom: var(--space-lg);
}

/* 胶囊：自绘，选中态 secondary-container（与发现页 chips 同一 recipe） */
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

.hidden-hint {
  margin-left: var(--space-sm);
  color: var(--ink-subtle);
  font-size: 12px;
}
</style>
