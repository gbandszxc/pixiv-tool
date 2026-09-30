<script setup lang="ts">
/** 作品查看器占位页（插画/漫画，F3 替换实现）。 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";

const props = defineProps<{ kind: "illust" | "manga" | "novel"; id: number }>();

const { t } = useI18n();

const title = computed(() => `${t("common.browseWorkTitle")} #${props.id}（${props.kind}）`);
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
