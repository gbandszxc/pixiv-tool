<template>
  <div class="tasks-view">
    <h1>任务</h1>

    <n-spin :show="loading">
      <n-empty v-if="!taskStore.tasks.length" description="暂无任务" />

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
              <div style="margin-top: 8px">
                <n-progress
                  type="line"
                  :percentage="task.total ? Math.round((task.done / task.total) * 100) : 0"
                  :status="task.status === 'done' ? 'success' : task.status === 'failed' ? 'error' : 'info'"
                />
                <div style="margin-top: 4px; font-size: 12px; color: #999">
                  {{ task.done }}/{{ task.total }} · 跳过 {{ task.skipped }} · 失败 {{ failedCount(task) }}
                </div>
              </div>
            </template>
            <template #footer>
              <n-space>
                <n-button
                  v-if="task.status === 'running'"
                  size="small"
                  @click="taskStore.pauseTask(task.task_id)"
                >暂停</n-button>
                <n-button
                  v-if="task.status === 'paused'"
                  size="small"
                  type="primary"
                  @click="taskStore.resumeTask(task.task_id)"
                >继续</n-button>
                <n-button
                  v-if="['running', 'paused'].includes(task.status)"
                  size="small"
                  type="error"
                  @click="taskStore.cancelTask(task.task_id)"
                >取消</n-button>
                <n-button
                  v-if="task.status === 'done' && failedCount(task) > 0"
                  size="small"
                  @click="taskStore.retryFailed(task.task_id)"
                >重试失败</n-button>
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
import { useTaskStore } from "../stores/tasks";
import type { Task } from "../stores/tasks";

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
  const map: Record<string, string> = {
    pending: "等待中",
    running: "运行中",
    paused: "已暂停",
    done: "完成",
    failed: "失败",
    canceled: "已取消",
  };
  return map[status] || status;
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
