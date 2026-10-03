<template>
  <div class="page-view tools-view">
    <div class="page-heading tools-header">
      <PageBackButton />
      <h1 class="page-title">{{ t('workspace.downloads') }}</h1>
      <md-filled-button @click="panel.open()">{{ t('workspace.newDownload') }}</md-filled-button>
    </div>
    <div class="tools-navigation"><SectionTabs class="tools-tabs" :tabs="tabs" :value="route.path" @change="onTabChange" /></div>
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
.tools-header { margin-bottom: var(--space-md); flex-wrap:wrap; }
.tools-header .page-title { flex:1; }
.tools-navigation { border-bottom:1px solid color-mix(in srgb,var(--md-sys-color-outline) 30%,transparent); margin-bottom:var(--space-xl); padding-bottom:var(--space-sm); }
.tools-tabs { width:max-content; max-width:100%; }
</style>
