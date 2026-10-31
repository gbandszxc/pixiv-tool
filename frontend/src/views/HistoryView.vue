<template>
  <div class="page-view history-view">
    <div class="app-page-head is-sub">
      <div class="history-toolbar">
        <div class="history-filters">
          <div class="m3-radio-group" role="radiogroup" :aria-label='t("history.typeColumn")'>
            <label v-for="option in categories" :key="option.value" class="m3-choice">
              <md-radio name="history-category" :value="option.value" :checked='category === option.value' @change='changeCategory(option.value)' />{{ option.label }}
            </label>
          </div>
          <md-outlined-text-field class="history-search" :placeholder='t("history.searchPlaceholder")' :aria-label='t("history.searchPlaceholder")' :value="keyword" @input='keyword = ($event.target as HTMLInputElement).value' @keydown.enter="search" />
          <md-filled-button @click="search">{{ t("common.search") }}</md-filled-button>
        </div>
        <div class="history-actions">
          <div class="view-switch-group" role="group" :aria-label='t("history.viewMode")'>
            <button type="button" class="view-switch-btn" :class="{ 'is-active': viewMode === 'list' }" :title='t("history.viewList")' @click="setViewMode('list')">
              <svg viewBox="0 0 24 24" aria-hidden="true"><line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/><line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/></svg>
            </button>
            <button type="button" class="view-switch-btn" :class="{ 'is-active': viewMode === 'grid' }" :title='t("history.viewGrid")' @click="setViewMode('grid')">
              <svg viewBox="0 0 24 24" aria-hidden="true"><rect width="7" height="7" x="3" y="3" rx="1"/><rect width="7" height="7" x="14" y="3" rx="1"/><rect width="7" height="7" x="14" y="14" rx="1"/><rect width="7" height="7" x="3" y="14" rx="1"/></svg>
            </button>
          </div>
          <label v-if="viewMode === 'grid'" class="m3-choice grid-select-all">
            <md-checkbox :checked="allRowsSelected" :indeterminate="someRowsSelected" :aria-label='t("tasks.selectAllDeletable")' @change='toggleAll(($event.target as HTMLInputElement).checked)' />
            {{ t("tasks.selectAllDeletable") }}
          </label>
          <md-outlined-button :disabled='!checkedRowKeys.length || batchDeleting' @click='openConfirm("batch")'>{{ batchDeleting ? "…" : t("history.batchDelete", { count: checkedRowKeys.length }) }}</md-outlined-button>
          <md-text-button :disabled='!rows.length || loading || clearing' @click='openConfirm("all")'>{{ clearing ? "…" : t("history.deleteAll") }}</md-text-button>
        </div>
      </div>
    </div>
    <div v-if="loading" class="m3-loading" role="status">…</div>
    <p v-else-if="!rows.length" class="m3-empty">{{ t("history.empty") }}</p>
    <template v-else>
      <!-- 列表模式 -->
      <div v-if="viewMode === 'list'" class="history-table-wrap">
        <table class="m3-table">
          <thead>
            <tr>
              <th><md-checkbox :checked="allRowsSelected" :indeterminate="someRowsSelected" :aria-label='t("tasks.selectAllDeletable")' @change='toggleAll(($event.target as HTMLInputElement).checked)' /></th>
              <th v-if='category === "all"'>{{ t("history.typeColumn") }}</th>
              <th>{{ t("history.titleColumn") }}</th>
              <th>{{ t("history.authorColumn") }}</th>
              <th v-if='category === "novel"'>{{ t("history.seriesColumn") }}</th>
              <th v-if='category !== "novel"'>{{ t("history.pagesColumn") }}</th>
              <th>{{ t("history.capturedAtColumn") }}</th>
              <th>{{ t("history.actions") }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in rows" :key="rowKey(row)">
              <td><md-checkbox :checked='checkedRowKeys.includes(rowKey(row))' :aria-label="row.title" @change='toggleRow(rowKey(row), ($event.target as HTMLInputElement).checked)' /></td>
              <td v-if='category === "all"'><span class="m3-label">{{ row.category === "novel" ? t("history.category.novel") : t("history.category.illustration") }}</span></td>
              <td :title="row.title">{{ row.title }}</td>
              <td :title='row.author_name || ""'>{{ row.author_name || "-" }}</td>
              <td v-if='category === "novel"'>{{ row.series_id ?? "-" }}</td>
              <td v-if='category !== "novel"'>{{ row.pages ?? "-" }}</td>
              <td>{{ formatCapturedAt(row.captured_at) }}</td>
              <td class="row-actions">
                <md-icon-button :aria-label='t("history.viewDetail")' @click='goDetail(row)'><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" /><circle cx="12" cy="12" r="3" /></svg></md-icon-button>
                <md-icon-button :aria-label='row.category === "novel" ? t("history.openFile") : t("history.openFolder")' @click='openFolder(row)'><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2" /></svg></md-icon-button>
                <md-icon-button class="btn-delete" :aria-label='t("common.delete")' @click='openConfirm(row)'><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10 11v6" /><path d="M14 11v6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M3 6h18" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></svg></md-icon-button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <!-- 宫格模式 -->
      <div v-else class="history-grid-wrap">
        <div class="history-grid">
          <article v-for="row in rows" :key="rowKey(row)" class="history-card" :class="{ 'is-selected': checkedRowKeys.includes(rowKey(row)) }">
            <!-- 小说书本封面（方形） -->
            <div v-if='row.category === "novel"' class="card-cover-wrap novel-cover-wrap" :style='bookStyle(row.id)' @click='goDetail(row)'>
              <div class="novel-spine"></div>
              <div class="novel-cover-body">
                <div class="novel-header-line">
                  <span class="novel-stamp">{{ t("history.category.novel") }}</span>
                  <span v-if="row.series_id" class="novel-badge">#{{ row.series_id }}</span>
                </div>
                <h3 class="novel-title" :title="row.title">{{ row.title }}</h3>
                <div class="novel-author-line">
                  <span class="novel-author">{{ row.author_name || '-' }}</span>
                </div>
                <div class="novel-excerpt-box">
                  <p class="novel-excerpt">{{ row.preview_text || t("history.noPreview") }}</p>
                </div>
              </div>
              <div class="novel-pages-edge"></div>
              <label class="card-checkbox" @click.stop>
                <md-checkbox :checked='checkedRowKeys.includes(rowKey(row))' :aria-label="row.title" @change='toggleRow(rowKey(row), ($event.target as HTMLInputElement).checked)' />
              </label>
              <div class="card-hover-actions" @click.stop>
                <md-icon-button :aria-label='t("history.viewDetail")' :title='t("history.viewDetail")' @click='goDetail(row)'><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" /><circle cx="12" cy="12" r="3" /></svg></md-icon-button>
                <md-icon-button :aria-label='t("history.openFile")' :title='t("history.openFile")' @click='openFolder(row)'><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2" /></svg></md-icon-button>
                <md-icon-button class="btn-delete" :aria-label='t("common.delete")' :title='t("common.delete")' @click='openConfirm(row)'><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10 11v6" /><path d="M14 11v6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M3 6h18" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></svg></md-icon-button>
              </div>
            </div>

            <!-- 插画本地缩略图封面（方形） -->
            <div v-else class="card-cover-wrap illust-cover-wrap" @click='goDetail(row)'>
              <img v-if="row.cover_path" class="card-img" :src="localDownloadSrc(row.cover_path)" :alt="row.title" loading="lazy" />
              <div v-else class="card-img-placeholder">
                <svg viewBox="0 0 24 24" aria-hidden="true"><rect width="18" height="18" x="3" y="3" rx="2" ry="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/></svg>
                <span>{{ row.illust_type === 2 ? 'UGOIRA' : t("history.category.illustration") }}</span>
              </div>
              <div class="card-badges">
                <span v-if="row.pages && row.pages > 1" class="card-pill">{{ t("history.pagesCount", { count: row.pages }) }}</span>
                <span v-else-if="row.illust_type === 2" class="card-pill">GIF</span>
              </div>
              <label class="card-checkbox" @click.stop>
                <md-checkbox :checked='checkedRowKeys.includes(rowKey(row))' :aria-label="row.title" @change='toggleRow(rowKey(row), ($event.target as HTMLInputElement).checked)' />
              </label>
              <div class="card-hover-actions" @click.stop>
                <md-icon-button :aria-label='t("history.viewDetail")' :title='t("history.viewDetail")' @click='goDetail(row)'><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" /><circle cx="12" cy="12" r="3" /></svg></md-icon-button>
                <md-icon-button :aria-label='t("history.openFolder")' :title='t("history.openFolder")' @click='openFolder(row)'><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2" /></svg></md-icon-button>
                <md-icon-button class="btn-delete" :aria-label='t("common.delete")' :title='t("common.delete")' @click='openConfirm(row)'><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10 11v6" /><path d="M14 11v6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M3 6h18" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></svg></md-icon-button>
              </div>
            </div>

            <!-- 卡片底部信息（紧凑无多余操作行） -->
            <div class="card-footer">
              <div class="card-title-row">
                <span class="m3-label">{{ row.category === "novel" ? t("history.category.novel") : t("history.category.illustration") }}</span>
                <h4 class="card-title" :title="row.title">{{ row.title }}</h4>
              </div>
              <div class="card-meta">
                <span class="card-author" :title='row.author_name || ""'>{{ row.author_name || '-' }}</span>
                <span class="card-date">{{ formatCapturedAt(row.captured_at) }}</span>
              </div>
            </div>
          </article>
        </div>
      </div>
    </template>
    <AppPagination class="pagination" v-model:current-page="historyStore.page" :total="historyStore.total" :page-size="historyStore.pageSize" :page-size-options="pageSizes" @change="onPagerChange" />
    <dialog ref="confirmDialog" class="m3-dialog" :aria-label="confirmText" @close="pendingAction = null"><p>{{ confirmText }}</p><div class="m3-row dialog-actions"><md-text-button @click='confirmDialog?.close()'>{{ t("common.cancel") }}</md-text-button><md-filled-button @click="runPendingAction">{{ t("common.delete") }}</md-filled-button></div></dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import AppPagination from "../components/common/AppPagination.vue";
import { useHistoryStore } from "../stores/history";
import type { HistoryCategory, HistoryItem } from "../stores/history";
import { notify } from "../ui/notify";
import { errorMessage, localDownloadSrc } from "../api/tauri";

const BOOK_THEMES = [
  { bg: "#1e3a5f", text: "#f0f4f8", border: "rgba(255,255,255,0.14)" },
  { bg: "#2d4a3e", text: "#f2f7f4", border: "rgba(255,255,255,0.14)" },
  { bg: "#523232", text: "#fdf5f5", border: "rgba(255,255,255,0.14)" },
  { bg: "#363636", text: "#f5f5f5", border: "rgba(255,255,255,0.14)" },
  { bg: "#4a3c28", text: "#faf6ed", border: "rgba(255,255,255,0.14)" },
];

function bookStyle(id: number) {
  const theme = BOOK_THEMES[Math.abs(id) % BOOK_THEMES.length];
  return {
    backgroundColor: theme.bg,
    color: theme.text,
  };
}

const viewMode = ref<"list" | "grid">(
  (localStorage.getItem("pixiv-tool-history-view-mode") as "list" | "grid") || "grid"
);
function setViewMode(mode: "list" | "grid") {
  viewMode.value = mode;
  localStorage.setItem("pixiv-tool-history-view-mode", mode);
}
 
 const { t } = useI18n(); const historyStore = useHistoryStore(); const router = useRouter();
 const loading = ref(false), keyword = ref(""), checkedRowKeys = ref<string[]>([]), batchDeleting = ref(false), clearing = ref(false), category = ref<HistoryCategory>("all");
const confirmDialog = ref<HTMLDialogElement>(), pendingAction = ref<"batch" | "all" | HistoryItem | null>(null);
const rows = computed(() => historyStore.items);
const pageSizes = [20, 50, 100];
const categories = computed(() => [{ value: "all" as const, label: t("history.category.all") }, { value: "novel" as const, label: t("history.category.novel") }, { value: "illustration" as const, label: t("history.category.illustration") }]);
const rowKey = (row: HistoryItem) => `${row.category}:${row.id}`;
const allRowsSelected = computed(() => rows.value.length > 0 && rows.value.every((row) => checkedRowKeys.value.includes(rowKey(row))));
const someRowsSelected = computed(() => !allRowsSelected.value && checkedRowKeys.value.length > 0);
const confirmText = computed(() => pendingAction.value === "batch" ? t("history.batchDeleteConfirm", { count: checkedRowKeys.value.length }) : pendingAction.value === "all" ? t("history.deleteAllConfirm") : t("history.deleteConfirm"));
function formatCapturedAt(iso: string) { const date = new Date(iso); if (Number.isNaN(date.getTime())) return iso; const utc8 = new Date(date.getTime() + 8 * 3600 * 1000), pad = (n: number) => String(n).padStart(2, "0"); return `${utc8.getUTCFullYear()}-${pad(utc8.getUTCMonth() + 1)}-${pad(utc8.getUTCDate())} ${pad(utc8.getUTCHours())}:${pad(utc8.getUTCMinutes())}:${pad(utc8.getUTCSeconds())}`; }
function toggleRow(key: string, checked: boolean) { checkedRowKeys.value = checked ? [...checkedRowKeys.value, key] : checkedRowKeys.value.filter((item) => item !== key); } function toggleAll(checked: boolean) { checkedRowKeys.value = checked ? rows.value.map(rowKey) : []; }
async function loadData() { loading.value = true; try { await historyStore.fetchHistory(category.value, keyword.value || undefined); checkedRowKeys.value = checkedRowKeys.value.filter((key) => rows.value.some((row) => rowKey(row) === key)); } catch (error) { notify(errorMessage(error) || t("common.browseLoadFailed")); } finally { loading.value = false; } }
function search() { historyStore.page = 1; loadData(); } function changeCategory(value: HistoryCategory) { category.value = value; checkedRowKeys.value = []; historyStore.page = 1; loadData(); } function openConfirm(action: "batch" | "all" | HistoryItem) { pendingAction.value = action; confirmDialog.value?.showModal(); }
/** 分页回调：页码经 v-model:currentPage 已回写 store；容量切换由父级回第 1 页再重查（DESIGN.md 分页区规则）。 */
function onPagerChange({ pageSize: nextSize }: { page: number; pageSize: number | undefined }) { if (nextSize !== undefined && nextSize !== historyStore.pageSize) { historyStore.pageSize = nextSize; historyStore.page = 1; } loadData(); }
/** 跳浏览模式作品详情：小说走 novel 阅读器；插画按 illust_type 落 manga/illust（ugoira 回落 illust，与 BrowseWorkView.routeKindOf 约定一致）。 */
function goDetail(row: HistoryItem) { const kind = row.category === "novel" ? "novel" : row.illust_type === 1 ? "manga" : "illust"; void router.push(`/browse/work/${kind}/${row.id}`); }
async function openFolder(row: HistoryItem) { try { const result = row.category === "novel" ? await historyStore.openNovelFile(row.id) : await historyStore.openIllustrationFolder(row.id); if (result.error) notify(t("history.openFolderFailed")); } catch { notify(t("history.openFolderFailed")); } }
async function deleteRow(row: HistoryItem) { if (row.category === "novel") await historyStore.deleteNovel(row.id, true); else await historyStore.deleteIllustration(row.id, true); await loadData(); }
async function deleteBatch() { const keys = [...checkedRowKeys.value], novels = keys.filter((key) => key.startsWith("novel:")).map((key) => Number(key.slice(6))), illustrations = keys.filter((key) => key.startsWith("illustration:")).map((key) => Number(key.slice(14))); batchDeleting.value = true; try { if (novels.length) await historyStore.deleteNovelsBatch(novels, false); if (illustrations.length) await historyStore.deleteIllustrationsBatch(illustrations, false); checkedRowKeys.value = []; notify(t("history.batchDeleted", { count: keys.length })); await loadData(); } catch { notify(t("common.deleteFailed")); } finally { batchDeleting.value = false; } }
async function deleteAll() { clearing.value = true; try { if (category.value !== "illustration") await historyStore.deleteAllNovels(false); if (category.value !== "novel") await historyStore.deleteAllIllustrations(false); checkedRowKeys.value = []; notify(t("history.cleared")); historyStore.page = 1; await loadData(); } catch { notify(t("common.deleteFailed")); } finally { clearing.value = false; } }
async function runPendingAction() { const action = pendingAction.value; confirmDialog.value?.close(); try { if (action === "batch") await deleteBatch(); else if (action === "all") await deleteAll(); else if (action) await deleteRow(action); } catch { notify(t("common.deleteFailed")); } }
onMounted(loadData);
</script>

<style scoped>
.history-toolbar,.history-filters,.history-actions,.pagination{display:flex;align-items:center;gap:var(--space-md)}.history-toolbar{justify-content:space-between;flex-wrap:wrap}.history-filters,.history-actions,.m3-radio-group{flex-wrap:wrap}.history-filters{flex:1}.m3-radio-group{display:flex;gap:var(--space-sm)}.history-search{width:220px}.history-table-wrap{overflow:auto;border-radius:16px}.m3-table{min-width:760px}.m3-table td{max-width:220px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.m3-label{display:inline-flex;padding:2px 8px;border-radius:999px;font-size:12px;font-weight:600;background:var(--md-sys-color-primary-container);color:var(--md-sys-color-on-primary-container)}.row-actions{display:flex;gap:var(--space-xxs)}.row-actions svg,.card-hover-actions svg{width:16px;height:16px;fill:none;stroke:currentColor;stroke-linecap:round;stroke-linejoin:round;stroke-width:2}.btn-delete{--md-icon-button-icon-color:var(--md-sys-color-error);color:var(--md-sys-color-error)}.view-switch-group{display:inline-flex;padding:2px;background:var(--md-sys-color-surface-container);border-radius:8px;border:1px solid color-mix(in srgb,var(--md-sys-color-outline) 20%,transparent)}.view-switch-btn{display:inline-flex;align-items:center;justify-content:center;width:32px;height:32px;padding:0;background:none;border:none;border-radius:6px;cursor:pointer;color:var(--ink-muted);transition:background .15s ease,color .15s ease}.view-switch-btn svg{width:18px;height:18px;fill:none;stroke:currentColor;stroke-linecap:round;stroke-linejoin:round;stroke-width:2}.view-switch-btn:hover{background:var(--md-sys-color-surface-container-high);color:var(--md-sys-color-on-surface)}.view-switch-btn.is-active{background:var(--md-sys-color-primary-container);color:var(--md-sys-color-on-primary-container)}.grid-select-all{margin-right:var(--space-xs)}.history-grid-wrap{width:100%}.history-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(180px,1fr));gap:var(--space-md)}.history-card{position:relative;display:flex;flex-direction:column;border-radius:16px;background:var(--md-sys-color-surface-container);border:1px solid color-mix(in srgb,var(--md-sys-color-outline) 18%,transparent);overflow:hidden;transition:transform .18s ease,box-shadow .18s ease,border-color .18s ease}.history-card:hover{transform:translateY(-2px);box-shadow:0 6px 18px rgba(0,0,0,.08);border-color:color-mix(in srgb,var(--md-sys-color-outline) 40%,transparent)}.history-card.is-selected{border-color:var(--md-sys-color-primary);background:color-mix(in srgb,var(--md-sys-color-primary-container) 15%,var(--md-sys-color-surface-container))}.card-cover-wrap{position:relative;width:100%;aspect-ratio:1/1;cursor:pointer;overflow:hidden;background:var(--md-sys-color-surface-container-high)}.card-img{width:100%;height:100%;object-fit:cover;display:block;transition:transform .2s ease}.history-card:hover .card-img{transform:scale(1.03)}.card-img-placeholder{width:100%;height:100%;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:var(--space-xs);color:var(--ink-muted);font-size:12px}.card-img-placeholder svg{width:36px;height:36px;fill:none;stroke:currentColor;stroke-linecap:round;stroke-linejoin:round;stroke-width:1.5}.card-checkbox{position:absolute;top:8px;left:8px;z-index:2;width:32px;height:32px;display:flex;align-items:center;justify-content:center;border-radius:8px;filter:drop-shadow(0 1px 2px rgba(0,0,0,.45))}.card-hover-actions{position:absolute;top:8px;right:8px;z-index:3;height:32px;box-sizing:border-box;display:flex;align-items:center;gap:2px;padding:2px 4px;background:rgba(0,0,0,.6);backdrop-filter:blur(8px);border-radius:8px;opacity:0;pointer-events:none;transition:opacity .15s ease}.history-card:hover .card-hover-actions{opacity:1;pointer-events:auto}.card-hover-actions md-icon-button{--md-icon-button-state-layer-color:#fff;--md-icon-button-icon-color:#fff;width:28px;height:28px}.card-hover-actions md-icon-button.btn-delete{--md-icon-button-icon-color:#ffb4ab}.card-badges{position:absolute;bottom:8px;right:8px;z-index:2;display:flex;gap:4px}.card-pill{display:inline-flex;align-items:center;padding:2px 6px;border-radius:6px;font-size:11px;font-weight:600;background:rgba(0,0,0,.6);color:#fff;backdrop-filter:blur(4px);box-shadow:0 1px 3px rgba(0,0,0,.3)}.novel-cover-wrap{display:flex;box-shadow:inset 6px 0 10px -2px rgba(0,0,0,.45);border-right:3px solid rgba(255,255,255,.2);user-select:none}.novel-spine{width:10px;height:100%;background:linear-gradient(to right,rgba(0,0,0,.35),rgba(255,255,255,.08) 60%,rgba(0,0,0,.2));flex-shrink:0}.novel-cover-body{flex:1;min-width:0;padding:8px 8px 6px 4px;display:flex;flex-direction:column;gap:3px}.novel-header-line{display:flex;align-items:center;justify-content:space-between;gap:4px;padding-right:2px}.novel-stamp{display:inline-flex;padding:1px 4px;border-radius:4px;font-size:9.5px;font-weight:700;letter-spacing:.05em;border:1px solid currentColor;opacity:.9}.novel-badge{font-size:9.5px;opacity:.75}.novel-title{margin:0;font-size:12.5px;font-weight:700;line-height:1.3;color:inherit;text-shadow:0 1px 2px rgba(0,0,0,.5);display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}.novel-author-line{font-size:10.5px;opacity:.8;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;padding-bottom:1px;border-bottom:1px solid rgba(255,255,255,.15)}.novel-excerpt-box{flex:1;min-height:0;background:rgba(0,0,0,.22);border:1px solid rgba(255,255,255,.1);border-radius:6px;padding:4px 6px;overflow:hidden}.novel-excerpt{margin:0;font-size:10px;line-height:1.4;color:inherit;opacity:.88;display:-webkit-box;-webkit-line-clamp:3;-webkit-box-orient:vertical;overflow:hidden;word-break:break-all;white-space:pre-wrap}.novel-pages-edge{position:absolute;right:0;top:0;bottom:0;width:3px;background:repeating-linear-gradient(to bottom,rgba(255,255,255,.2) 0 1px,transparent 1px 3px)}.card-footer{padding:var(--space-xs) var(--space-sm);display:flex;flex-direction:column;gap:2px}.card-title-row{display:flex;align-items:center;gap:var(--space-xs);min-width:0}.card-title{margin:0;font-size:13px;font-weight:600;color:var(--md-sys-color-on-surface);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;flex:1}.card-meta{display:flex;align-items:center;justify-content:space-between;font-size:11px;color:var(--ink-muted);gap:var(--space-xs)}.card-author{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:90px}.card-date{white-space:nowrap;flex-shrink:0}.pagination{margin-top:var(--space-md)}.m3-empty,.m3-loading{padding:var(--space-xl);color:var(--ink-muted);text-align:center}@media (max-width:640px){.history-search{min-width:100%}.history-grid{grid-template-columns:repeat(auto-fill,minmax(140px,1fr))}}
</style>
