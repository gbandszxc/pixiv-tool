<script setup lang="ts">
/** 小说阅读器占位页（/browse/work/novel/:id，F4 替换实现）。 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

const props = defineProps<{ id: number }>();

const { t } = useI18n();

const title = computed(() => `${t("common.browseReaderTitle")} #${props.id}`);
</script>

<template>
  <div class="page-view">
    <h1 class="page-title">{{ title }}</h1>
    <p class="placeholder-hint">{{ t("common.browseComingSoon") }}</p>
  </div>
</template>

<style scoped>
.placeholder-hint {
  margin: 0;
  color: var(--ink-muted);
}
</style>
