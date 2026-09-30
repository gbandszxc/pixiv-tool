<script setup lang="ts">
/** 浏览·排行榜占位页（F1 替换实现）。 */
import { useI18n } from "vue-i18n";

const { t } = useI18n();
</script>

<template>
  <div class="page-view">
    <h1 class="page-title">{{ t("nav.browseRanking") }}</h1>
    <p class="placeholder-hint">{{ t("common.browseComingSoon") }}</p>
  </div>
</template>

<style scoped>
.placeholder-hint {
  margin: 0;
  color: var(--ink-muted);
}
</style>
