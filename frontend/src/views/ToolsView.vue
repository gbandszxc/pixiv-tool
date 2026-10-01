<template>
  <div class="page-view tools-view">
    <SectionTabs class="tools-tabs" :tabs="tabs" :value="activeTab" @change="onTabChange" />
    <div class="tools-body"><router-view /></div>
  </div>
</template>

<script setup lang="ts">
/**
 * 工具页：页签壳。顶部 md-secondary-tab（复用 SectionTabs 封装）即子路由导航，
 * 与 /tools/* 子路由双向同步；四个页签分别复用既有 CrawlView / IllustrationView /
 * TasksView / HistoryView，子视图内部逻辑不变。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import SectionTabs, { type SectionTab } from "../components/browse/SectionTabs.vue";

const { t } = useI18n(); const route = useRoute(); const router = useRouter();
const tabs = computed<SectionTab[]>(() => [
  { value: "novel", label: t("nav.crawlNovel") },
  { value: "illustration", label: t("nav.crawlIllustration") },
  { value: "tasks", label: t("nav.tasks") },
  { value: "history", label: t("nav.history") },
]);
/** 当前页签取自路径第二段；未知段回退 novel（与 /tools → /tools/novel 重定向一致）。 */
const activeTab = computed(() => { const seg = route.path.split("/")[2] ?? ""; return tabs.value.some(tab => tab.value === seg) ? seg : "novel"; });
function onTabChange(value: string) { if (value !== activeTab.value) router.push(`/tools/${value}`); }
</script>

<style scoped>
.tools-tabs { margin-bottom: var(--space-lg); }
</style>
