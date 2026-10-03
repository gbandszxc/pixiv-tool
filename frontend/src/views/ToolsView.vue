<template>
  <div class="page-view tools-view">
    <div class="page-heading tools-header">
      <PageBackButton />
      <SectionTabs class="tools-tabs" :tabs="tabs" :value="route.path" @change="onTabChange" />
      <md-filled-button @click="panel.open()">{{ t('workspace.newDownload') }}</md-filled-button>
    </div>
    <div class="tools-body"><router-view /></div>
  </div>
</template>

<script setup lang="ts">
/** 下载页：任务 / 下载历史沿子路由导航，新建下载打开非模态表单。 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import SectionTabs, { type SectionTab } from "../components/browse/SectionTabs.vue";
import PageBackButton from "../components/navigation/PageBackButton.vue";
import { useDownloadPanelStore } from "../stores/downloadPanel";

const { t } = useI18n(); const route = useRoute(); const router = useRouter();
const panel = useDownloadPanelStore();
const tabs = computed<SectionTab[]>(() => [
  { value: "/tools/tasks", label: t("nav.tasks") },
  { value: "/tools/history", label: t("workspace.downloadHistory") },
]);
function onTabChange(value: string) { if (value !== route.path) void router.push(value); }
</script>

<style scoped>
.tools-header { margin-bottom: var(--space-lg); flex-wrap:wrap; }
.tools-tabs { flex: 1; min-width: 0; }
</style>
