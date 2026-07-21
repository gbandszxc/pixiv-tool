<template>
  <div class="page-view">
    <h1 class="page-title">{{ t('tasks.title') }}</h1>

    <n-spin :show="loading">
      <n-empty v-if="!taskStore.tasks.length" :description="t('tasks.empty')" />

      <n-list v-else bordered>
        <n-list-item v-for="task in taskStore.tasks" :key="task.task_id">
          <n-thing>
            <template #header>
              <span>{{ task.source_type }} — {{ task.source_id }}</span>
            </template>
            <template #header-extra>
              <n-tag :type="statusType(task.status)" size="small">
                {{ statusLabel(task.status) }}
              </n-tag>
            </template>
            <template #description>
              <div class="task-progress">
                <n-progress
                  type="line"
                  :percentage="task.total ? Math.round((task.done / task.total) * 100) : 0"
                  :status="task.status === 'done' ? 'success' : task.status === 'failed' ? 'error' : 'info'"
                />
                <div class="task-progress-meta">
                  {{ t('tasks.progressDetail', { done: task.done, total: task.total, skipped: task.skipped, failed: failedCount(task) }) }}
                </div>
              </div>
            </template>
            <template #footer>
              <n-space>
                <n-button
                  v-if="task.status === 'running'"
                  size="small"
                  @click="taskStore.pauseTask(task.task_id)"
                >{{ t('tasks.pause') }}</n-button>
                <n-button
                  v-if="task.status === 'paused'"
                  size="small"
                  type="primary"
                  @click="taskStore.resumeTask(task.task_id)"
                >{{ t('tasks.resume') }}</n-button>
                <n-button
                  v-if="['running', 'paused'].includes(task.status)"
                  size="small"
                  type="error"
                  @click="taskStore.cancelTask(task.task_id)"
                >{{ t('tasks.cancel') }}</n-button>
                <n-button
                  v-if="task.status === 'done' && failedCount(task) > 0"
                  size="small"
                  @click="taskStore.retryFailed(task.task_id)"
                >{{ t('tasks.retryFailed') }}</n-button>
              </n-space>
            </template>
          </n-thing>
        </n-list-item>
      </n-list>
    </n-spin>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { NList, NListItem, NThing, NTag, NProgress, NButton, NSpace, NEmpty, NSpin } from "naive-ui";
import { useI18n } from "vue-i18n";
import { useTaskStore } from "../stores/tasks";
import type { Task } from "../stores/tasks";

const { t } = useI18n();
const taskStore = useTaskStore();
const loading = ref(false);

function statusType(status: string) {
  const map: Record<string, "success" | "error" | "info" | "warning"> = {
    done: "success",
    failed: "error",
    running: "info",
    paused: "warning",
    pending: "info",
    canceled: "info",
  };
  return map[status] || "info";
}

function statusLabel(status: string) {
  // 未知状态回退到原始字符串（便于排查后端新增的状态）
  return t(`tasks.status.${status}`) !== `tasks.status.${status}`
    ? t(`tasks.status.${status}`)
    : status;
}

function failedCount(task: Task) {
  try {
    return JSON.parse(task.failed_ids || "[]").length;
  } catch {
    return 0;
  }
}

onMounted(async () => {
  loading.value = true;
  await taskStore.fetchTasks();
  loading.value = false;
});
</script>
