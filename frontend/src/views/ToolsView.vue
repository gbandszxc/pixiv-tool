<template>
  <div class="page-view tools-view">
    <div class="app-page-head">
      <div class="page-heading tools-header">
        <PageBackButton />
        <h1 class="page-title">{{ t('workspace.downloads') }}</h1>
        <md-filled-button @click="panel.open()">{{ t('workspace.newDownload') }}</md-filled-button>
      </div>
      <div class="tools-navigation"><SectionTabs class="tools-tabs" :tabs="tabs" :value="route.path" @change="onTabChange" /></div>
    </div>
    <div class="tools-body"><router-view /></div>
  </div>
</template>

<script setup lang="ts">
/** 下载页：任务 / 下载历史沿子路由导航，新建下载打开非模态表单。
 * 页头吸顶；子页面的工具行（分类 / 批量操作）以 `.app-page-head.is-sub` 再吸附在页头下沿，
 * 偏移与页内定位余量由 App.vue 统一实测写入滚动容器的 --app-head-h。 */
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
.tools-header { margin-bottom: var(--space-md); flex-wrap: wrap; }
.tools-header .page-title { flex: 1; }
.tools-navigation { margin-bottom: var(--space-md); }
.tools-tabs { width: max-content; max-width: 100%; }
.tools-view > .tools-body { margin-top: 0; }
</style>
