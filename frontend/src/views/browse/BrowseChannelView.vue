<script setup lang="ts">
/** 浏览·频道页占位（插画/漫画/小说三路由复用，F1 替换实现）。 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

const props = defineProps<{ kind: "illustration" | "manga" | "novel" }>();

const { t } = useI18n();

const title = computed(() => {
  if (props.kind === "illustration") return t("nav.browseIllustration");
  if (props.kind === "manga") return t("nav.browseManga");
  return t("nav.browseNovel");
});
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
