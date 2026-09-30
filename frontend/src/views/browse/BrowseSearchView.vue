<script setup lang="ts">
/** 浏览·搜索占位页（F2 替换实现）。 */
import { useI18n } from "vue-i18n";

const { t } = useI18n();
</script>

<template>
  <div class="page-view">
    <h1 class="page-title">{{ t("nav.browseSearch") }}</h1>
    <p class="placeholder-hint">{{ t("common.browseComingSoon") }}</p>
  </div>
</template>

<style scoped>
.placeholder-hint {
  margin: 0;
  color: var(--ink-muted);
}
</style>
