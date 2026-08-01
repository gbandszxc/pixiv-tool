<template>
  <div class="page-view">
    <h1 class="page-title">{{ t("nav.crawlIllustration") }}</h1>

    <n-card class="form-card">
      <p class="placeholder-text">{{ t("crawl.illustrationComingSoon") }}</p>
    </n-card>
  </div>
</template>

<script setup lang="ts">
import { NCard } from "naive-ui";
import { useI18n } from "vue-i18n";

const { t } = useI18n();
</script>

<style scoped>
.placeholder-text {
  margin: 0;
  color: var(--ink-muted);
}
</style>
