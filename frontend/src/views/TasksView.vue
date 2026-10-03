<template>
  <div class="page-view tasks-view">
    <div v-if="loading" class="m3-loading" role="status">…</div>
    <p v-else-if="!taskStore.tasks.length" class="m3-empty">{{ t("tasks.empty") }}</p>
    <template v-else>
      <div class="tasks-toolbar">
        <div class="m3-radio-group" role="radiogroup" :aria-label='t("tasks.title")'>
          <label v-for="option in categories" :key="option.value" class="m3-choice"><md-radio name="task-category" :value="option.value" :checked='categoryFilter === option.value' @change='changeCategory(option.value)' />{{ option.label }}</label>
        </div>
        <div class="task-management">
        <label class="m3-choice"><md-checkbox :checked="allSelected" :indeterminate="someSelected" :disabled='!filteredTasks.length' @change='toggleAll(($event.target as HTMLInputElement).checked)' />{{ t("tasks.selectAllDeletable") }}</label>
        <md-outlined-button :disabled='!selectedTaskIds.length || batchDeleting' @click='openConfirm("batch")'>{{ batchDeleting ? "…" : t("tasks.batchDelete", { count: selectedTaskIds.length }) }}</md-outlined-button>
        <md-text-button :disabled='!completedTaskCount || loading' @click='openConfirm("clear")'>{{ clearingCompleted ? "…" : t("tasks.clearCompleted") }}</md-text-button>
        </div>
      </div>
      <section class="task-list" :aria-label='t("tasks.title")'>
        <article v-for="task in pagedTasks" :key="task.task_id" class="task-item">
          <label class="task-select"><md-checkbox :checked='selectedTaskIds.includes(task.task_id)' :aria-label='t("tasks.selectTask", { id: task.task_id })' @change='toggleTask(task.task_id, ($event.target as HTMLInputElement).checked)' /></label>
          <div>
            <div class="task-heading"><div><span class="m3-label">{{ categoryLabel(task) }}</span><span class="task-source">{{ sourceLabel(task) }} — {{ task.source_id }}</span></div><span class="m3-status" :class='`status-${task.status}`'>{{ statusLabel(task.status) }}</span></div>
            <md-linear-progress :value='taskProgress(task)' :aria-label="progressDetail(task)" />
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
      <AppPagination class="pagination" v-model:current-page="page" :total="filteredTasks.length" :page-size="pageSize" :page-size-options="pageSizes" @change="onPagerChange" />
    </template>
    <dialog ref="confirmDialog" class="m3-dialog" @close="pendingAction = null"><p>{{ confirmText }}</p><div class="m3-row dialog-actions"><md-text-button @click='confirmDialog?.close()'>{{ t("common.cancel") }}</md-text-button><md-filled-button @click="runPendingAction">{{ t("common.delete") }}</md-filled-button></div></dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import AppPagination from "../components/common/AppPagination.vue";
import { isTauri } from "../api/tauri";


import { taskProgress, useTaskStore } from "../stores/tasks";
import type { Task } from "../stores/tasks";
import { notify } from "../ui/notify";

const ACTIVE = new Set(["pending", "running", "paused"]);

let disposed = false;
const { t } = useI18n(); const taskStore = useTaskStore();
const loading = ref(false), batchDeleting = ref(false), clearingCompleted = ref(false);
const selectedTaskIds = ref<string[]>([]), deletingTaskIds = ref(new Set<string>());
const categoryFilter = ref<"all" | "novel" | "illustration">("all"), page = ref(1), pageSize = ref(20);
const confirmDialog = ref<HTMLDialogElement>(), pendingAction = ref<string | null>(null);
const categories = computed(() => [{ value: "all" as const, label: t("tasks.category.all") }, { value: "novel" as const, label: t("tasks.category.novel") }, { value: "illustration" as const, label: t("tasks.category.illustration") }]);
const filteredTasks = computed(() => categoryFilter.value === "all" ? taskStore.tasks : taskStore.tasks.filter((task) => (task.category || "novel") === categoryFilter.value));
const pageSizes = [20, 50, 100], pageCount = computed(() => Math.max(1, Math.ceil(filteredTasks.value.length / pageSize.value))), pagedTasks = computed(() => filteredTasks.value.slice((page.value - 1) * pageSize.value, page.value * pageSize.value));
const completedTaskCount = computed(() => taskStore.tasks.filter((task) => task.status === "done").length);
const allSelected = computed(() => filteredTasks.value.length > 0 && filteredTasks.value.every((task) => selectedTaskIds.value.includes(task.task_id)));
const someSelected = computed(() => !allSelected.value && selectedTaskIds.value.length > 0);
const confirmText = computed(() => { if (pendingAction.value === "clear") return t("tasks.clearCompletedConfirm", { count: completedTaskCount.value }); if (pendingAction.value === "batch") return t("tasks.batchDeleteConfirm", { count: selectedTaskIds.value.length }); const task = taskStore.tasks.find((item) => item.task_id === pendingAction.value); return task && ACTIVE.has(task.status) ? t("tasks.deleteActiveConfirm") : t("tasks.deleteConfirm"); });
function categoryLabel(task: Task) { return task.category === "illustration" ? t("tasks.category.illustration") : t("tasks.category.novel"); }
function sourceLabel(task: Task) { if (task.category === "illustration") return task.source_type === "user" ? t("illust.user") : t("illust.single"); return t(({ single: "crawl.single", series: "crawl.series", user: "crawl.user" } as Record<string, string>)[task.source_type] || "crawl.single"); }
function statusLabel(status: string) { const key = `tasks.status.${status}`; return t(key) === key ? status : t(key); }
function failedCount(task: Task) { try { return JSON.parse(task.failed_ids || "[]").length; } catch { return 0; } }
function progressDetail(task: Task) { return t("tasks.progressDetail", { done: task.done, total: task.total, skipped: task.skipped, failed: failedCount(task) }); }
function changeCategory(value: "all" | "novel" | "illustration") { categoryFilter.value = value; page.value = 1; } function toggleTask(id: string, checked: boolean) { selectedTaskIds.value = checked ? [...selectedTaskIds.value, id] : selectedTaskIds.value.filter((item) => item !== id); }
/** 分页回调：页码经 v-model:currentPage 已回写；容量切换由父级回第 1 页（纯前端切片，无需重查）。 */
function onPagerChange({ pageSize: nextSize }: { page: number; pageSize: number | undefined }) { if (nextSize !== undefined && nextSize !== pageSize.value) { pageSize.value = nextSize; page.value = 1; } }
function toggleAll(checked: boolean) { selectedTaskIds.value = checked ? filteredTasks.value.map((task) => task.task_id) : []; }
async function refreshTasks() {
  if (disposed) return;
  await taskStore.fetchTasks();
  if (disposed) return;
  const ids = new Set(filteredTasks.value.map((task) => task.task_id));
  selectedTaskIds.value = selectedTaskIds.value.filter((id) => ids.has(id));
  page.value = Math.min(page.value, pageCount.value);

}
function openConfirm(action: string) { pendingAction.value = action; confirmDialog.value?.showModal(); }
async function deleteOne(id: string) { deletingTaskIds.value = new Set(deletingTaskIds.value).add(id); try { const { deleted } = await taskStore.deleteTask(id); notify(t("tasks.deleted", { count: deleted })); await refreshTasks(); } catch { notify(t("common.deleteFailed")); } finally { const next = new Set(deletingTaskIds.value); next.delete(id); deletingTaskIds.value = next; } }
async function deleteBatch() { batchDeleting.value = true; try { const { deleted } = await taskStore.deleteTasks(selectedTaskIds.value); notify(t("tasks.batchDeleted", { count: deleted })); selectedTaskIds.value = []; await refreshTasks(); } catch { notify(t("common.deleteFailed")); } finally { batchDeleting.value = false; } }
async function clearCompleted() { clearingCompleted.value = true; try { const { deleted } = await taskStore.deleteCompletedTasks(); notify(t("tasks.completedCleared", { count: deleted })); await refreshTasks(); } catch { notify(t("common.deleteFailed")); } finally { clearingCompleted.value = false; } }
async function runPendingAction() { const action = pendingAction.value; confirmDialog.value?.close(); if (action === "clear") await clearCompleted(); else if (action === "batch") await deleteBatch(); else if (action) await deleteOne(action); }
watch(() => taskStore.tasks, () => {
  const ids = new Set(filteredTasks.value.map(task => task.task_id));
  selectedTaskIds.value = selectedTaskIds.value.filter(id => ids.has(id));
  page.value = Math.min(page.value, pageCount.value);
});
onMounted(async () => {
  if (!isTauri()) return;
  loading.value = !taskStore.tasks.length;
  try { await refreshTasks(); } catch { notify(t("workspace.syncFailed")); }
  finally { if (!disposed) loading.value = false; }
});
onUnmounted(() => { disposed = true; });
</script>

<style scoped>
.task-management { display:flex; flex-wrap:wrap; align-items:center; gap:var(--space-md); margin-left:auto; }
.tasks-toolbar { display:flex; align-items:center; justify-content:space-between; gap:var(--space-lg) var(--space-xl); padding-bottom:var(--space-sm); }
.tasks-toolbar .m3-radio-group { gap:var(--space-md); }
@media (max-width:640px) { .task-management { margin-left:0; } }
.task-heading,.task-actions,.pagination{display:flex;align-items:center;gap:var(--space-md)}.tasks-toolbar,.m3-radio-group,.task-actions{flex-wrap:wrap}.tasks-toolbar{margin-bottom:var(--space-md)}.m3-radio-group{display:flex;gap:var(--space-sm)}.task-list{overflow:hidden;border-radius:16px;background:var(--md-sys-color-surface-container)}.task-item{display:grid;grid-template-columns:auto minmax(0,1fr);gap:var(--space-sm);padding:var(--space-lg);border-bottom:1px solid color-mix(in srgb,var(--md-sys-color-outline) 30%,transparent)}.task-item:last-child{border-bottom:0}.task-select{padding-top:2px}.task-heading{justify-content:space-between;margin-bottom:var(--space-sm)}.task-source{margin-left:var(--space-sm)}.m3-label,.m3-status{display:inline-flex;padding:2px 8px;border-radius:999px;font-size:12px;font-weight:600;background:var(--md-sys-color-primary-container);color:var(--md-sys-color-on-primary-container)}.m3-status{background:color-mix(in srgb,var(--md-sys-color-outline) 18%,transparent);color:var(--ink-muted)}.status-done{background:#c8f7d0;color:#0d3b18}.status-failed{background:#ffdad6;color:#410002}.status-paused{background:#ffddb2;color:#2a1700}.task-progress-meta{margin:var(--space-xs) 0}.pagination{margin-top:var(--space-md)}.m3-empty,.m3-loading{padding:var(--space-xl);color:var(--ink-muted);text-align:center}@media (max-width:640px){.task-heading{align-items:flex-start;flex-direction:column}.task-source{display:block;margin:var(--space-xs) 0 0}}
</style>
