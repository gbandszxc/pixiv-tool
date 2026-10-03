import { defineStore } from "pinia";
import { computed, ref, shallowRef, watch } from "vue";
import { invoke, isTauri, listen } from "../api/tauri";
import type { UnlistenFn } from "../api/tauri";
import type { TaskMutationResult, TaskRow } from "../api/tauri";

// 保留旧导出名：视图层（TasksView）按 `Task` 引用任务行类型。
export type Task = TaskRow;
export function taskProgress(task: Task): number { return task.total > 0 ? Math.max(0, Math.min(1, task.done / task.total)) : 0; }
export function taskFailedCount(task: Task): number { try { const ids: unknown = JSON.parse(task.failed_ids || "[]"); return Array.isArray(ids) ? ids.length : 0; } catch { return 0; } }

export const useTaskStore = defineStore("tasks", () => {
  // 任务行只通过整批快照替换，避免为每一行建立深层响应式代理。
  const tasks = shallowRef<Task[]>([]);
  const syncFailed = ref(false);
  const activeTasks = computed(() => tasks.value.filter(task => ["running", "pending", "paused"].includes(task.status)));
  const representative = computed(() => {
    const order = ["running", "pending", "paused"];
    return [...activeTasks.value].sort((a, b) => order.indexOf(a.status) - order.indexOf(b.status) || a.created_at.localeCompare(b.created_at))[0];
  });
  let stopMonitor: (() => void) | null = null;
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
            syncFailed.value = false;
            // 请求期间的进度事件或任务变更合并为一次尾随刷新，所有调用者等待最新快照。
          } while (refreshRequested);
        } catch (error) {
          syncFailed.value = true;
          throw error;
        } finally {
          refreshRequested = false;
          refreshPromise = null;
        }
      })();
    }
    return refreshPromise;
  }

  /** 应用级单一订阅；活跃任务或同步失败时保留 2 秒轮询兜底。 */
  function startMonitoring(): () => void {
    if (stopMonitor) return stopMonitor;
    if (!isTauri()) return () => {};
    let disposed = false;
    let timer: ReturnType<typeof setInterval> | undefined;
    const subscriptions: UnlistenFn[] = [];
    const refresh = () => { if (!disposed) void fetchTasks().catch(() => {}); };
    const unwatch = watch(() => activeTasks.value.length > 0 || syncFailed.value, (poll) => {
      if (poll && !timer) timer = setInterval(refresh, 2000);
      if (!poll && timer) { clearInterval(timer); timer = undefined; }
    }, { immediate: true });
    stopMonitor = () => {
      disposed = true; unwatch();
      if (timer) clearInterval(timer);
      subscriptions.forEach(unlisten => unlisten());
      stopMonitor = null;
    };
    for (const event of ["task://progress", "task://done"]) {
      void listen(event, refresh).then(unlisten => { if (disposed) unlisten(); else subscriptions.push(unlisten); }).catch(() => { syncFailed.value = true; });
    }
    refresh();
    return stopMonitor;
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
    activeTasks, representative, syncFailed, startMonitoring,
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
