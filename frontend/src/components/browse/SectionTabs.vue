<script setup lang="ts">
/**
 * 分区标签页：md-tabs + md-secondary-tab 封装（M3 pill 选中态由 Material Web 提供）。
 * 受控用法：v-model:value 或监听 change。
 */
import { computed } from "vue";

export interface SectionTab {
  value: string;
  label: string;
}

const props = defineProps<{
  tabs: SectionTab[];
  /** 当前选中的 tab value */
  value?: string;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (e: "update:value", value: string): void;
  (e: "change", value: string): void;
}>();

const activeIndex = computed(() => {
  const index = props.tabs.findIndex((tab) => tab.value === props.value);
  return index >= 0 ? index : 0;
});

function onChange(event: Event): void {
  const index = (event.currentTarget as unknown as { activeTabIndex: number }).activeTabIndex;
  const tab = props.tabs[index];
  if (!tab) return;
  emit("update:value", tab.value);
  emit("change", tab.value);
}
</script>

<template>
  <md-tabs class="section-tabs" :active-tab-index="activeIndex" @change="onChange">
    <md-secondary-tab v-for="tab in tabs" :key="tab.value" :disabled="disabled">{{ tab.label }}</md-secondary-tab>
  </md-tabs>
</template>

<style scoped>
.section-tabs {
  --md-secondary-tab-container-height: 40px;
}
</style>
