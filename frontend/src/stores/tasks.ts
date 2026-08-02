import { defineStore } from "pinia";
import { ref } from "vue";
import api from "../api";

export interface Task {
  task_id: string;
  source_type: string;
  source_id: string;
  category: string;
  status: string;
  total: number;
  done: number;
  skipped: number;
  failed_ids: string;
  created_at: string;
  updated_at: string;
  error: string | null;
}

export const useTaskStore = defineStore("tasks", () => {
  const tasks = ref<Task[]>([]);

  async function fetchTasks() {
    const resp = await api.get("/api/tasks");
    tasks.value = resp.data.items || [];
  }

  async function createTask(sourceType: string, sourceId: string, formats: string[], category = "novel") {
    const resp = await api.post("/api/tasks", {
      source_type: sourceType,
      source_id: sourceId,
      formats,
      category,
    });
    return resp.data;
  }

  async function pauseTask(taskId: string) {
    await api.post(`/api/tasks/${taskId}/pause`);
    await fetchTasks();
  }

  async function resumeTask(taskId: string) {
    await api.post(`/api/tasks/${taskId}/resume`);
    await fetchTasks();
  }

  async function cancelTask(taskId: string) {
    await api.post(`/api/tasks/${taskId}/cancel`);
    await fetchTasks();
  }

  async function retryFailed(taskId: string) {
    await api.post(`/api/tasks/${taskId}/retry-failed`);
    await fetchTasks();
  }

  async function deleteTask(taskId: string) {
    return (await api.delete(`/api/tasks/${taskId}`)).data as { deleted: number };
  }

  async function deleteTasks(taskIds: string[]) {
    return (await api.delete("/api/tasks", { data: { task_ids: taskIds } })).data as { deleted: number };
  }

  async function deleteCompletedTasks() {
    return (await api.delete("/api/tasks/completed")).data as { deleted: number };
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
