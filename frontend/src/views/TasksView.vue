<template>
  <div class="page-view tasks-view">
    <div v-if="loading" class="m3-loading" role="status">…</div>
    <p v-else-if="!taskStore.tasks.length" class="m3-empty">{{ t("tasks.empty") }}</p>
    <template v-else>
      <!-- 分类筛选与批量操作吸附在工具壳页头下方（层级与偏移见 main.css 的 .app-page-head.is-sub） -->
      <div class="app-page-head is-sub">
        <div class="tasks-toolbar">
          <div class="m3-radio-group" role="radiogroup" :aria-label='t("tasks.title")'>
            <label v-for="option in categories" :key="option.value" class="m3-choice"><md-radio name="task-category" :value="option.value" :checked='categoryFilter === option.value' @change='changeCategory(option.value)' />{{ option.label }}</label>
          </div>
          <div class="task-management">
          <label class="m3-choice"><md-checkbox :checked="allSelected" :indeterminate="someSelected" :disabled='!filteredTasks.length' @change='toggleAll(($event.target as HTMLInputElement).checked)' />{{ t("tasks.selectAllDeletable") }}</label>
          <md-outlined-button :disabled='!selectedTaskIds.length || batchDeleting' @click='openConfirm("batch")'>{{ batchDeleting ? "…" : t("tasks.batchDelete", { count: selectedTaskIds.length }) }}</md-outlined-button>
          <md-text-button :disabled='!completedTaskCount || loading || clearingCompleted' :aria-busy='clearingCompleted' @click='openConfirm("clear")'>{{ clearingCompleted ? t("tasks.clearingCompleted") : t("tasks.clearCompleted") }}</md-text-button>
          </div>
        </div>
      </div>
      <section class="task-list" :aria-label='t("tasks.title")'>
        <article v-for="task in pagedTasks" :key="task.task_id" class="task-item">
          <label class="task-select"><md-checkbox :checked='selectedTaskIds.includes(task.task_id)' :aria-label='t("tasks.selectTask", { id: task.task_id })' @change='toggleTask(task.task_id, ($event.target as HTMLInputElement).checked)' /></label>
          <div class="task-content">
            <div class="task-heading">
              <div class="task-title-group">
                <span class="m3-label">{{ categoryLabel(task) }}</span>
                <span class="task-source">{{ sourceLabel(task) }} — {{ task.source_id }}</span>
              </div>
              <span class="m3-status" :class='`status-${task.status}`'>{{ statusLabel(task.status) }}</span>
            </div>
            <div class="task-progress-row">
              <div class="task-progress-track">
                <md-linear-progress class="task-progress-bar" :value='taskProgress(task)' :aria-label="progressDetail(task)" />
                <span class="task-progress-meta">{{ progressDetail(task) }}</span>
              </div>
              <div class="task-actions">
                <md-text-button v-if='task.status === "running"' @click='taskStore.pauseTask(task.task_id)'>{{ t("tasks.pause") }}</md-text-button>
                <md-filled-button v-if='task.status === "paused"' @click='taskStore.resumeTask(task.task_id)'>{{ t("tasks.resume") }}</md-filled-button>
                <md-text-button v-if='["running", "paused"].includes(task.status)' @click='taskStore.cancelTask(task.task_id)'>{{ t("tasks.cancel") }}</md-text-button>
                <md-text-button v-if='task.status === "done" && taskFailedCount(task) > 0' @click='taskStore.retryFailed(task.task_id)'>{{ t("tasks.retryFailed") }}</md-text-button>
                <md-icon-button class="task-delete-btn" :disabled='deletingTaskIds.has(task.task_id)' :aria-label='t("common.delete")' @click='openConfirm(task.task_id)'>
                  <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10 11v6" /><path d="M14 11v6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M3 6h18" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></svg>
                </md-icon-button>
              </div>
            </div>
          </div>
        </article>
      </section>
      <AppPagination class="pagination" v-model:current-page="page" :total="filteredTasks.length" :page-size="pageSize" :page-size-options="pageSizes" @change="onPagerChange" />
    </template>
    <dialog ref="confirmDialog" class="m3-dialog" :aria-label="confirmText" @close="pendingAction = null"><p>{{ confirmText }}</p><div class="m3-row dialog-actions"><md-text-button @click='confirmDialog?.close()'>{{ t("common.cancel") }}</md-text-button><md-filled-button @click="runPendingAction">{{ t("common.delete") }}</md-filled-button></div></dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import AppPagination from "../components/common/AppPagination.vue";
import { isTauri } from "../api/tauri";


import { taskProgress, taskFailedCount, useTaskStore } from "../stores/tasks";
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
function progressDetail(task: Task) { return t("tasks.progressDetail", { done: task.done, total: task.total, skipped: task.skipped, failed: taskFailedCount(task) }); }
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
.task-heading,.pagination{display:flex;align-items:center;gap:var(--space-md)}.tasks-toolbar,.m3-radio-group{flex-wrap:wrap}.tasks-toolbar{margin-bottom:0}.m3-radio-group{display:flex;gap:var(--space-sm)}.task-list{overflow:hidden;border-radius:16px;background:var(--md-sys-color-surface-container)}.task-item{display:grid;grid-template-columns:auto minmax(0,1fr);column-gap:var(--space-xl);row-gap:var(--space-sm);padding:var(--space-md) var(--space-lg);border-bottom:1px solid color-mix(in srgb,var(--md-sys-color-outline) 20%,transparent);align-items:center}.task-item:last-child{border-bottom:0}.task-select{display:flex;align-items:center;padding-right:var(--space-xs)}.task-content{display:flex;flex-direction:column;gap:var(--space-sm);min-width:0}.task-heading{justify-content:space-between}.task-title-group{display:flex;align-items:center;min-width:0;flex-wrap:wrap;gap:var(--space-xs)}.task-source{margin-left:var(--space-xs);font-weight:500;color:var(--md-sys-color-on-surface)}.m3-label,.m3-status{display:inline-flex;padding:2px 8px;border-radius:999px;font-size:12px;font-weight:600;background:var(--md-sys-color-primary-container);color:var(--md-sys-color-on-primary-container)}.m3-status{background:color-mix(in srgb,var(--md-sys-color-outline) 18%,transparent);color:var(--ink-muted)}.status-done{background:var(--state-success-container);color:var(--state-success-ink)}.status-failed{background:var(--md-sys-color-error-container);color:var(--md-sys-color-on-error-container)}.status-paused{background:var(--state-warning-container);color:var(--state-warning-ink)}.task-progress-row{display:flex;align-items:center;justify-content:space-between;gap:var(--space-md);flex-wrap:wrap}.task-progress-track{display:flex;align-items:center;gap:var(--space-lg);flex:1;min-width:260px}.task-progress-bar{flex:1;max-width:620px}.task-progress-meta{font-size:12px;color:var(--ink-muted);white-space:nowrap}.task-actions{display:flex;align-items:center;gap:var(--space-xs);margin-left:auto}.task-delete-btn{--md-icon-button-icon-color:var(--md-sys-color-error);color:var(--md-sys-color-error)}.task-delete-btn svg{width:18px;height:18px;fill:none;stroke:currentColor;stroke-linecap:round;stroke-linejoin:round;stroke-width:2}.pagination{margin-top:var(--space-md)}.m3-empty,.m3-loading{padding:var(--space-xl);color:var(--ink-muted);text-align:center}@media (max-width:640px){.task-heading{align-items:flex-start;flex-direction:column}.task-source{display:block;margin:var(--space-xs) 0 0}.task-progress-track{min-width:100%}.task-progress-bar{max-width:100%}.task-actions{margin-left:0;width:100%;justify-content:flex-end}}
</style>
