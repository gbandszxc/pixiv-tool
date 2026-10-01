import { defineStore } from "pinia";
import { shallowRef } from "vue";
import { invoke } from "../api/tauri";
import type { TaskMutationResult, TaskRow } from "../api/tauri";

// 保留旧导出名：视图层（TasksView）按 `Task` 引用任务行类型。
export type Task = TaskRow;

export const useTaskStore = defineStore("tasks", () => {
  // 任务行只通过整批快照替换，避免为每一行建立深层响应式代理。
  const tasks = shallowRef<Task[]>([]);
  let refreshPromise: Promise<void> | null = null;
  let refreshRequested = false;

  function fetchTasks(): Promise<void> {
    refreshRequested = true;
    if (!refreshPromise) {
      refreshPromise = (async () => {
        try {
          do {
            refreshRequested = false;
            const data = await invoke<{ items: TaskRow[] }>("tasks_list");
            tasks.value = data.items || [];
            // 请求期间的进度事件或任务变更合并为一次尾随刷新，所有调用者等待最新快照。
          } while (refreshRequested);
        } finally {
          refreshRequested = false;
          refreshPromise = null;
        }
      })();
    }
    return refreshPromise;
  }

  async function createTask(sourceType: string, sourceId: string, formats: string[], category = "novel") {
    // 业务失败时返回 { error }（旧 200+error 风格），由调用方判断。
    return invoke<TaskMutationResult>("task_create", {
      sourceType,
      sourceId,
      formats,
      category,
    });
  }

  async function pauseTask(taskId: string) {
    await invoke("task_pause", { taskId });
    await fetchTasks();
  }

  async function resumeTask(taskId: string) {
    await invoke("task_resume", { taskId });
    await fetchTasks();
  }

  async function cancelTask(taskId: string) {
    await invoke("task_cancel", { taskId });
    await fetchTasks();
  }

  async function retryFailed(taskId: string) {
    await invoke("task_retry_failed", { taskId });
    await fetchTasks();
  }

  async function deleteTask(taskId: string) {
    // 任务不存在时后端 reject '任务不存在'
    return invoke<{ deleted: number }>("task_delete", { taskId });
  }

  async function deleteTasks(taskIds: string[]) {
    return invoke<{ deleted: number }>("tasks_delete", { taskIds });
  }

  async function deleteCompletedTasks() {
    return invoke<{ deleted: number }>("tasks_delete_completed");
  }

  return {
    tasks,
    fetchTasks,
    createTask,
    pauseTask,
    resumeTask,
    cancelTask,
    retryFailed,
    deleteTask,
    deleteTasks,
    deleteCompletedTasks,
  };
});
