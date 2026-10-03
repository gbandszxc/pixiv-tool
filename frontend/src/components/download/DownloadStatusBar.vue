<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import { taskFailedCount, taskProgress, useTaskStore } from "../../stores/tasks";
import { useDownloadPanelStore } from "../../stores/downloadPanel";
const tasks = useTaskStore(); const panel = useDownloadPanelStore();
const router = useRouter(); const route = useRoute(); const { t } = useI18n();
function openTasks() { if (route.path !== '/tools/tasks') void router.push('/tools/tasks'); }
</script>

<template>
  <footer class="download-status" :aria-label="t('workspace.downloads')">
    <button class="status-link" @click="openTasks">
      <span>{{ t('workspace.downloads') }}</span>
      <span v-if="tasks.syncFailed" role="status">{{ t('workspace.syncFailed') }}</span>
      <template v-else-if="tasks.representative">
        <span>{{ t('workspace.activeCount', { count: tasks.activeTasks.length }) }}</span>
        <md-linear-progress class="mini-progress" :value="taskProgress(tasks.representative)" :indeterminate="tasks.representative.total <= 0 && tasks.representative.status !== 'paused'" :aria-label="t('tasks.progressDetail', { done: tasks.representative.done, total: tasks.representative.total, skipped: tasks.representative.skipped, failed: taskFailedCount(tasks.representative) })" />
        <span class="task-summary">{{ t(`tasks.status.${tasks.representative.status}`) }} · {{ tasks.representative.source_id }}<template v-if="tasks.representative.total > 0"> · {{ Math.round(taskProgress(tasks.representative) * 100) }}%</template></span>
      </template>
      <span v-else>{{ t('workspace.idle') }}</span>
    </button>
    <button class="new-download" @click="panel.open()">{{ t('workspace.newDownload') }}</button>
  </footer>
</template>

<style scoped>
.download-status { display:flex; align-items:center; gap:var(--space-md); height:calc(var(--space-lg) * 2); padding:0 var(--space-lg); background:var(--md-sys-color-surface-container); border-top:1px solid color-mix(in srgb,var(--md-sys-color-outline) 35%,transparent); font-size:12px; }
button { color:var(--ink-muted); font:inherit; border:0; background:transparent; cursor:pointer; border-radius:var(--radius-control); height:100%; }
button:hover { background:color-mix(in srgb,var(--md-sys-color-primary) 8%,transparent); }
button:focus-visible { outline:2px solid var(--md-sys-color-primary); outline-offset:-2px; }
.status-link { flex:1; min-width:0; display:flex; align-items:center; gap:var(--space-md); text-align:left; }
.mini-progress { width:calc(var(--space-xl) * 5); flex:none; --md-linear-progress-track-height:4px; }
.task-summary { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.new-download { flex:none; color:var(--md-sys-color-primary); padding:0 var(--space-sm); }
@media (max-width:640px) { .task-summary { display:none; } .mini-progress { width:calc(var(--space-xl) * 2); } .download-status,.status-link { gap:var(--space-sm); } }
</style>
