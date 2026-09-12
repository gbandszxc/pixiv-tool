<template>
  <div class="page-view tasks-view">
    <header class="tasks-header">
      <h1 class="page-title">{{ t("tasks.title") }}</h1>
      <md-text-button :disabled='!completedTaskCount || loading' @click='openConfirm("clear")'>{{ clearingCompleted ? "…" : t("tasks.clearCompleted") }}</md-text-button>
    </header>
    <div v-if="loading" class="m3-loading" role="status">…</div>
    <p v-else-if="!taskStore.tasks.length" class="m3-empty">{{ t("tasks.empty") }}</p>
    <template v-else>
      <div class="tasks-toolbar">
        <div class="m3-radio-group" role="radiogroup" :aria-label='t("tasks.title")'>
          <label v-for="option in categories" :key="option.value" class="m3-choice"><md-radio name="task-category" :value="option.value" :checked='categoryFilter === option.value' @change='categoryFilter = option.value' />{{ option.label }}</label>
        </div>
        <label class="m3-choice"><md-checkbox :checked="allSelected" :indeterminate="someSelected" :disabled='!filteredTasks.length' @change='toggleAll(($event.target as HTMLInputElement).checked)' />{{ t("tasks.selectAllDeletable") }}</label>
        <md-outlined-button :disabled='!selectedTaskIds.length || batchDeleting' @click='openConfirm("batch")'>{{ batchDeleting ? "…" : t("tasks.batchDelete", { count: selectedTaskIds.length }) }}</md-outlined-button>
      </div>
      <section class="task-list" :aria-label='t("tasks.title")'>
        <article v-for="task in filteredTasks" :key="task.task_id" class="task-item">
          <label class="task-select"><md-checkbox :checked='selectedTaskIds.includes(task.task_id)' :aria-label='t("tasks.selectTask", { id: task.task_id })' @change='toggleTask(task.task_id, ($event.target as HTMLInputElement).checked)' /></label>
          <div>
            <div class="task-heading"><div><span class="m3-label">{{ categoryLabel(task) }}</span><span class="task-source">{{ sourceLabel(task) }} — {{ task.source_id }}</span></div><span class="m3-status" :class='`status-${task.status}`'>{{ statusLabel(task.status) }}</span></div>
            <md-linear-progress :value='task.total ? task.done / task.total : 0' :aria-label="progressDetail(task)" />
            <div class="task-progress-meta">{{ progressDetail(task) }}</div>
            <div class="task-actions">
              <md-text-button v-if='task.status === "running"' @click='taskStore.pauseTask(task.task_id)'>{{ t("tasks.pause") }}</md-text-button>
              <md-filled-button v-if='task.status === "paused"' @click='taskStore.resumeTask(task.task_id)'>{{ t("tasks.resume") }}</md-filled-button>
              <md-text-button v-if='["running", "paused"].includes(task.status)' @click='taskStore.cancelTask(task.task_id)'>{{ t("tasks.cancel") }}</md-text-button>
              <md-text-button v-if='task.status === "done" && failedCount(task) > 0' @click='taskStore.retryFailed(task.task_id)'>{{ t("tasks.retryFailed") }}</md-text-button>
              <md-text-button :disabled='deletingTaskIds.has(task.task_id)' @click='openConfirm(task.task_id)'>{{ deletingTaskIds.has(task.task_id) ? "…" : t("common.delete") }}</md-text-button>
            </div>
          </div>
        </article>
      </section>
    </template>
    <dialog ref="confirmDialog" class="m3-dialog local-dialog" @close="pendingAction = null"><p>{{ confirmText }}</p><div class="m3-row"><md-text-button @click='confirmDialog?.close()'>{{ t("common.cancel") }}</md-text-button><md-filled-button @click="runPendingAction">{{ t("common.delete") }}</md-filled-button></div></dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { isTauri, listen } from "../api/tauri";
import type { TaskDoneEvent, TaskProgressEvent, UnlistenFn } from "../api/tauri";
import { useTaskStore } from "../stores/tasks";
import type { Task } from "../stores/tasks";
import { notify } from "../ui/notify";

const ACTIVE = new Set(["pending", "running", "paused"]);
let timer: ReturnType<typeof setInterval> | null = null;
let unlistenFns: UnlistenFn[] = [];
const { t } = useI18n(); const taskStore = useTaskStore();
const loading = ref(false), batchDeleting = ref(false), clearingCompleted = ref(false);
const selectedTaskIds = ref<string[]>([]), deletingTaskIds = ref(new Set<string>());
const categoryFilter = ref<"all" | "novel" | "illustration">("all");
const confirmDialog = ref<HTMLDialogElement>(), pendingAction = ref<string | null>(null);
const categories = computed(() => [{ value: "all" as const, label: t("tasks.category.all") }, { value: "novel" as const, label: t("tasks.category.novel") }, { value: "illustration" as const, label: t("tasks.category.illustration") }]);
const filteredTasks = computed(() => categoryFilter.value === "all" ? taskStore.tasks : taskStore.tasks.filter((task) => (task.category || "novel") === categoryFilter.value));
const completedTaskCount = computed(() => taskStore.tasks.filter((task) => task.status === "done").length);
const allSelected = computed(() => filteredTasks.value.length > 0 && filteredTasks.value.every((task) => selectedTaskIds.value.includes(task.task_id)));
const someSelected = computed(() => !allSelected.value && selectedTaskIds.value.length > 0);
const confirmText = computed(() => { if (pendingAction.value === "clear") return t("tasks.clearCompletedConfirm", { count: completedTaskCount.value }); if (pendingAction.value === "batch") return t("tasks.batchDeleteConfirm", { count: selectedTaskIds.value.length }); const task = taskStore.tasks.find((item) => item.task_id === pendingAction.value); return task && ACTIVE.has(task.status) ? t("tasks.deleteActiveConfirm") : t("tasks.deleteConfirm"); });
function categoryLabel(task: Task) { return task.category === "illustration" ? t("tasks.category.illustration") : t("tasks.category.novel"); }
function sourceLabel(task: Task) { if (task.category === "illustration") return task.source_type === "user" ? t("illust.user") : t("illust.single"); return t(({ single: "crawl.single", series: "crawl.series", user: "crawl.user" } as Record<string, string>)[task.source_type] || "crawl.single"); }
function statusLabel(status: string) { const key = `tasks.status.${status}`; return t(key) === key ? status : t(key); }
function failedCount(task: Task) { try { return JSON.parse(task.failed_ids || "[]").length; } catch { return 0; } }
function progressDetail(task: Task) { return t("tasks.progressDetail", { done: task.done, total: task.total, skipped: task.skipped, failed: failedCount(task) }); }
function toggleTask(id: string, checked: boolean) { selectedTaskIds.value = checked ? [...selectedTaskIds.value, id] : selectedTaskIds.value.filter((item) => item !== id); }
function toggleAll(checked: boolean) { selectedTaskIds.value = checked ? filteredTasks.value.map((task) => task.task_id) : []; }
function ensurePolling() { const active = taskStore.tasks.some((task) => ACTIVE.has(task.status)); if (active && !timer) timer = setInterval(() => refreshTasks().catch(() => {}), 2000); else if (!active && timer) { clearInterval(timer); timer = null; } }
async function refreshTasks() { await taskStore.fetchTasks(); const ids = new Set(filteredTasks.value.map((task) => task.task_id)); selectedTaskIds.value = selectedTaskIds.value.filter((id) => ids.has(id)); ensurePolling(); }
function openConfirm(action: string) { pendingAction.value = action; confirmDialog.value?.showModal(); }
async function deleteOne(id: string) { deletingTaskIds.value = new Set(deletingTaskIds.value).add(id); try { const { deleted } = await taskStore.deleteTask(id); notify(t("tasks.deleted", { count: deleted })); await refreshTasks(); } catch { notify(t("common.deleteFailed")); } finally { const next = new Set(deletingTaskIds.value); next.delete(id); deletingTaskIds.value = next; } }
async function deleteBatch() { batchDeleting.value = true; try { const { deleted } = await taskStore.deleteTasks(selectedTaskIds.value); notify(t("tasks.batchDeleted", { count: deleted })); selectedTaskIds.value = []; await refreshTasks(); } catch { notify(t("common.deleteFailed")); } finally { batchDeleting.value = false; } }
async function clearCompleted() { clearingCompleted.value = true; try { const { deleted } = await taskStore.deleteCompletedTasks(); notify(t("tasks.completedCleared", { count: deleted })); await refreshTasks(); } catch { notify(t("common.deleteFailed")); } finally { clearingCompleted.value = false; } }
async function runPendingAction() { const action = pendingAction.value; confirmDialog.value?.close(); if (action === "clear") await clearCompleted(); else if (action === "batch") await deleteBatch(); else if (action) await deleteOne(action); }
onMounted(async () => { loading.value = true; try { await refreshTasks(); } finally { loading.value = false; } if (isTauri()) unlistenFns.push(await listen<TaskProgressEvent>("task://progress", () => refreshTasks().catch(() => {})), await listen<TaskDoneEvent>("task://done", () => refreshTasks().catch(() => {}))); });
onUnmounted(() => { if (timer) clearInterval(timer); unlistenFns.forEach((unlisten) => unlisten()); });
</script>

<style scoped>
.tasks-header,.tasks-toolbar,.task-heading,.task-actions{display:flex;align-items:center;gap:var(--space-md)}.tasks-header{justify-content:space-between;margin-bottom:var(--space-lg)}.tasks-header .page-title{margin:0}.tasks-toolbar,.m3-radio-group,.task-actions{flex-wrap:wrap}.tasks-toolbar{margin-bottom:var(--space-md)}.m3-radio-group{display:flex;gap:var(--space-sm)}.task-list{overflow:hidden;border-radius:16px;background:var(--md-sys-color-surface-container)}.task-item{display:grid;grid-template-columns:auto minmax(0,1fr);gap:var(--space-sm);padding:var(--space-lg);border-bottom:1px solid color-mix(in srgb,var(--md-sys-color-outline) 30%,transparent)}.task-item:last-child{border-bottom:0}.task-select{padding-top:2px}.task-heading{justify-content:space-between;margin-bottom:var(--space-sm)}.task-source{margin-left:var(--space-sm)}.m3-label,.m3-status{display:inline-flex;padding:2px 8px;border-radius:999px;font-size:12px;font-weight:600;background:var(--md-sys-color-primary-container);color:var(--md-sys-color-on-primary-container)}.m3-status{background:color-mix(in srgb,var(--md-sys-color-outline) 18%,transparent);color:var(--ink-muted)}.status-done{background:#c8f7d0;color:#0d3b18}.status-failed{background:#ffdad6;color:#410002}.status-paused{background:#ffddb2;color:#2a1700}.task-progress-meta{margin:var(--space-xs) 0}.m3-empty,.m3-loading{padding:var(--space-xl);color:var(--ink-muted);text-align:center}.local-dialog p{margin:0 0 var(--space-lg)}.local-dialog .m3-row{justify-content:flex-end}@media (max-width:640px){.task-heading{align-items:flex-start;flex-direction:column}.task-source{display:block;margin:var(--space-xs) 0 0}}
</style>
