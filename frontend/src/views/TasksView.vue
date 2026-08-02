<template>
  <div class="page-view">
    <n-space class="tasks-header" justify="space-between" align="center">
      <h1 class="page-title">{{ t('tasks.title') }}</h1>
      <n-popconfirm @positive-click="handleClearCompleted">
        <template #trigger>
          <n-button
            type="error"
            :disabled="!completedTaskCount || loading"
            :loading="clearingCompleted"
          >
            {{ t('tasks.clearCompleted') }}
          </n-button>
        </template>
        {{ t('tasks.clearCompletedConfirm', { count: completedTaskCount }) }}
      </n-popconfirm>
    </n-space>

    <n-spin :show="loading">
      <n-empty v-if="!taskStore.tasks.length" :description="t('tasks.empty')" />

      <template v-else>
        <n-space class="tasks-toolbar" align="center">
          <n-radio-group v-model:value="categoryFilter" size="small">
            <n-radio-button value="all">{{ t('tasks.category.all') }}</n-radio-button>
            <n-radio-button value="novel">{{ t('tasks.category.novel') }}</n-radio-button>
            <n-radio-button value="illustration">{{ t('tasks.category.illustration') }}</n-radio-button>
          </n-radio-group>
          <n-checkbox
            :checked="allDeletableSelected"
            :indeterminate="someDeletableSelected"
            :disabled="!deletableTasks.length"
            @update:checked="toggleAllDeletable"
          >
            {{ t('tasks.selectAllDeletable') }}
          </n-checkbox>
          <n-popconfirm @positive-click="handleBatchDelete">
            <template #trigger>
              <n-button
                type="warning"
                :disabled="!selectedTaskIds.length"
                :loading="batchDeleting"
              >
                {{ t('tasks.batchDelete', { count: selectedTaskIds.length }) }}
              </n-button>
            </template>
            {{ t('tasks.batchDeleteConfirm', { count: selectedTaskIds.length }) }}
          </n-popconfirm>
        </n-space>

        <n-list bordered>
          <n-list-item v-for="task in filteredTasks" :key="task.task_id">
            <template #prefix>
              <n-checkbox
                :checked="selectedTaskIds.includes(task.task_id)"
                :aria-label="t('tasks.selectTask', { id: task.task_id })"
                @update:checked="toggleTaskSelection(task.task_id, $event)"
              />
            </template>
            <n-thing>
              <template #header>
                <n-space align="center" size="small">
                  <n-tag size="small" type="info">{{ categoryLabel(task) }}</n-tag>
                  <span>{{ sourceLabel(task) }} — {{ task.source_id }}</span>
                </n-space>
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
                  <n-popconfirm @positive-click="handleDeleteTask(task.task_id)">
                    <template #trigger>
                      <n-button
                        size="small"
                        type="error"
                        :loading="deletingTaskIds.has(task.task_id)"
                      >{{ t('common.delete') }}</n-button>
                    </template>
                    {{ isActiveTask(task) ? t('tasks.deleteActiveConfirm') : t('tasks.deleteConfirm') }}
                  </n-popconfirm>
                </n-space>
              </template>
            </n-thing>
          </n-list-item>
        </n-list>
      </template>
    </n-spin>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import {
  NButton,
  NCheckbox,
  NEmpty,
  NList,
  NListItem,
  NPopconfirm,
  NProgress,
  NRadioButton,
  NRadioGroup,
  NSpace,
  NSpin,
  NTag,
  NThing,
  useMessage,
} from "naive-ui";
import { useI18n } from "vue-i18n";
import { useTaskStore } from "../stores/tasks";
import type { Task } from "../stores/tasks";

const TERMINAL_STATUSES = new Set(["done", "failed", "canceled"]);
const ACTIVE_STATUSES = new Set(["pending", "running", "paused"]);

let timer: ReturnType<typeof setInterval> | null = null;

function ensurePolling() {
  const hasActive = taskStore.tasks.some((task) => ACTIVE_STATUSES.has(task.status));
  if (hasActive && !timer) {
    timer = setInterval(async () => {
      try {
        await taskStore.fetchTasks();
      } catch {
        // 静默:轮询失败不打扰
      }
      ensurePolling();
    }, 2000);
  } else if (!hasActive && timer) {
    clearInterval(timer);
    timer = null;
  }
}

const { t } = useI18n();
const message = useMessage();
const taskStore = useTaskStore();
const loading = ref(false);
const batchDeleting = ref(false);
const clearingCompleted = ref(false);
const selectedTaskIds = ref<string[]>([]);
const deletingTaskIds = ref(new Set<string>());

const categoryFilter = ref<"all" | "novel" | "illustration">("all");

const filteredTasks = computed(() => {
  if (categoryFilter.value === "all") return taskStore.tasks;
  return taskStore.tasks.filter((task) => (task.category || "novel") === categoryFilter.value);
});
const deletableTasks = computed(() => filteredTasks.value.filter(isDeletable));
const completedTaskCount = computed(() => taskStore.tasks.filter((task) => task.status === "done").length);

function categoryLabel(task: Task) {
  return task.category === "illustration"
    ? t("tasks.category.illustration")
    : t("tasks.category.novel");
}

function sourceLabel(task: Task) {
  if (task.category === "illustration") {
    return task.source_type === "user" ? t("illust.user") : t("illust.single");
  }
  const map: Record<string, string> = {
    single: "crawl.single",
    series: "crawl.series",
    user: "crawl.user",
  };
  return t(map[task.source_type] || "crawl.single");
}
const allDeletableSelected = computed(
  () => deletableTasks.value.length > 0 && deletableTasks.value.every((task) => selectedTaskIds.value.includes(task.task_id)),
);
const someDeletableSelected = computed(
  () => !allDeletableSelected.value && selectedTaskIds.value.length > 0,
);

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

function isActiveTask(task: Task) {
  return ACTIVE_STATUSES.has(task.status);
}

// 所有任务都允许删除：进行中的任务删除 = 先取消再移除记录。
function isDeletable(_task: Task) {
  return true;
}

function toggleTaskSelection(taskId: string, checked: boolean) {
  selectedTaskIds.value = checked
    ? [...selectedTaskIds.value, taskId]
    : selectedTaskIds.value.filter((id) => id !== taskId);
}

function toggleAllDeletable(checked: boolean) {
  selectedTaskIds.value = checked ? deletableTasks.value.map((task) => task.task_id) : [];
}

async function refreshTasks() {
  await taskStore.fetchTasks();
  const validIds = new Set(deletableTasks.value.map((task) => task.task_id));
  selectedTaskIds.value = selectedTaskIds.value.filter((id) => validIds.has(id));
  ensurePolling();
}

async function handleDeleteTask(taskId: string) {
  deletingTaskIds.value = new Set(deletingTaskIds.value).add(taskId);
  try {
    const { deleted } = await taskStore.deleteTask(taskId);
    message.success(t("tasks.deleted", { count: deleted }));
    await refreshTasks();
  } catch {
    message.error(t("common.deleteFailed"));
  } finally {
    const nextDeletingIds = new Set(deletingTaskIds.value);
    nextDeletingIds.delete(taskId);
    deletingTaskIds.value = nextDeletingIds;
  }
}

async function handleBatchDelete() {
  const taskIds = [...selectedTaskIds.value];
  batchDeleting.value = true;
  try {
    const { deleted } = await taskStore.deleteTasks(taskIds);
    message.success(t("tasks.batchDeleted", { count: deleted }));
    selectedTaskIds.value = [];
    await refreshTasks();
  } catch {
    message.error(t("common.deleteFailed"));
  } finally {
    batchDeleting.value = false;
  }
}

async function handleClearCompleted() {
  clearingCompleted.value = true;
  try {
    const { deleted } = await taskStore.deleteCompletedTasks();
    message.success(t("tasks.completedCleared", { count: deleted }));
    await refreshTasks();
  } catch {
    message.error(t("common.deleteFailed"));
  } finally {
    clearingCompleted.value = false;
  }
}

onMounted(async () => {
  loading.value = true;
  try {
    await refreshTasks();
  } finally {
    loading.value = false;
  }
});

onUnmounted(() => {
  if (timer) clearInterval(timer);
});
</script>
