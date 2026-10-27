<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useAuthStore } from "../../stores/auth";
import { useDownloadPanelStore, type DownloadFormKind, type PageCache } from "../../stores/downloadPanel";
import { useTaskStore } from "../../stores/tasks";
import { parsePixivUrl } from "../../utils/pixivUrl";
import { browseWorkDetail, errorMessage, illustSavedPages } from "../../api/browse";
import { notify } from "../../ui/notify";
import PageDownloadPicker from "./PageDownloadPicker.vue";
import SectionTabs from "../browse/SectionTabs.vue";

const panel = useDownloadPanelStore(); const tasks = useTaskStore(); const auth = useAuthStore(); const { t } = useI18n();
const root = ref<HTMLElement>(); const confirm = ref<HTMLDialogElement>();
const draft = computed(() => panel.drafts[panel.kind]);
const prefix = computed(() => panel.kind === "novel" ? "crawl" : "illust");
const tabs = computed(() => [{ value: "novel", label: t("nav.crawlNovel") }, { value: "illustration", label: t("nav.crawlIllustration") }]);
const sources = computed(() => panel.kind === "novel" ? ["single", "series", "user"] : ["single", "user"]);
function switchKind(value: string) { if (!panel.submitting) { panel.kind = value as DownloadFormKind; panel.error = ""; } }
function editSource(value: string) { draft.value.sourceId = value; draft.value.edited = true; panel.error = ""; }
function editType(value: string) { draft.value.sourceType = value; draft.value.edited = true; panel.error = ""; }
function formatChange(value: string, checked: boolean) { draft.value.formats = checked ? [...draft.value.formats, value] : draft.value.formats.filter(format => format !== value); }
watch(() => panel.visible, async (visible) => { if (visible) { await nextTick(); root.value?.querySelector<HTMLElement>("md-outlined-text-field")?.focus(); } });
watch(() => panel.pendingTarget, async (target) => { await nextTick(); if (target) confirm.value?.showModal(); else confirm.value?.close(); });

// ===== 插画单篇：按页选择 =====
/** 只在插画单篇表单下工作；其余表单不渲染选择器 */
const pickerVisible = computed(() => panel.kind === "illustration" && draft.value.sourceType === "single");
const cache = computed(() => panel.pageCache.illustration);
const pickerPages = computed(() => cache.value?.pages ?? []);
const pickerSaved = computed(() => cache.value?.saved ?? []);
/** 缓存命中同一作品才渲染（ugoira 与单页作品没有按页选择语义） */
const pageSelectionCount = computed(() => panel.pageSelection.illustration.length);
/**
 * 主按钮文案跟随下载范围：单件来源为「下载」（小说单篇；插画单页与 ugoira，多页作品
 * 详情未加载时也先中立显示「下载」）；多件集合为「下载全部」（小说系列 / 用户、插画
 * 用户全集、多页作品全选或清空态），页选择器内部分选择时为「下载所选 N 项」。
 */
const submitLabel = computed(() => {
  if (panel.kind === "illustration" && showPicker.value) {
    const count = pageSelectionCount.value;
    const total = pickerPages.value.length;
    return count > 0 && count < total
      ? t("workspace.downloadSelected", { count })
      : t("workspace.downloadAll");
  }
  // 选择器未渲染：单件来源（小说单篇 / 插画单张）为「下载」，系列 / 用户等集合来源为「下载全部」
  return draft.value.sourceType === "single"
    ? t("workspace.download")
    : t("workspace.downloadAll");
});
const showPicker = computed(() => {
  const current = cache.value;
  return pickerVisible.value && current !== null && !current.ugoira && current.pages.length > 1 && current.id === resolveIllustId();
});
/** 已发起过「已下载」查询的缓存对象（含在途）：缓存每次打开/换作品都会重建，故按对象身份去重 */
const savedQueried = new WeakSet<PageCache>();
/** 详情请求代次：输入连续变化时只接受最后一次结果 */
let seq = 0;
/** 350ms 防抖句柄 */
let timer: ReturnType<typeof setTimeout> | undefined;

/** 从输入解析单篇插画 id（纯数字或 /artworks/{id} 链接）；用户/系列等其他形态不适用按页选择 */
function resolveIllustId(): string {
  const raw = draft.value.sourceId.trim();
  if (!raw) return "";
  const parsed = parsePixivUrl(raw);
  if (parsed) return parsed.kind === "illustration" ? parsed.id : "";
  return /^\d+$/.test(raw) ? raw : "";
}

/** 静默补「已下载」页码：失败按全部未下载处理，不影响手动选择 */
async function loadSaved(id: string, entry: PageCache): Promise<void> {
  try {
    const result = await illustSavedPages(Number(id));
    if (panel.pageCache.illustration === entry) entry.saved = result.pages;
  } catch { /* 静默降级 */ }
}

/** 缓存命中路径：pages 已在，只补一次本地「已下载」查询，不发详情请求 */
function syncSaved(id: string, entry: PageCache): void {
  if (entry.ugoira || entry.pages.length <= 1 || savedQueried.has(entry)) return;
  savedQueried.add(entry);
  void loadSaved(id, entry);
}

/** 取详情（缓存命中则不重复请求）并写入 store；失败静默，只是不渲染选择器 */
async function ensurePages(id: string): Promise<void> {
  const token = ++seq;
  if (!id) { panel.resetPageCache(); return; }
  // 未登录时不发详情请求：浏览命令会派发登录弹窗事件，边输入边弹窗会打扰填写
  if (!auth.isLoggedIn) { panel.resetPageCache(); return; }
  const current = panel.pageCache.illustration;
  if (current && current.id === id) { syncSaved(id, current); return; }
  // 换作品：旧页码选择立即失效，避免把上一件的页码提交给新作品
  panel.setPageSelection("illustration", []);
  try {
    const detail = await browseWorkDetail("illust", Number(id));
    if (token !== seq) return;
    if (detail.detail_kind !== "illust") { panel.resetPageCache(); return; }
    panel.setPageCache({ id, pages: detail.pages, saved: [], ugoira: detail.item.kind === "ugoira" });
    const stored = panel.pageCache.illustration;
    if (stored) syncSaved(id, stored);
  } catch { if (token === seq) panel.resetPageCache(); }
}

watch(
  () => [panel.visible, pickerVisible.value, draft.value.sourceId, cache.value] as const,
  () => {
    if (timer) { clearTimeout(timer); timer = undefined; }
    if (!panel.visible || !pickerVisible.value) return;
    const id = resolveIllustId();
    const current = panel.pageCache.illustration;
    // 详情页预置路径：缓存已在，立即补角标（既不等防抖，也不请求详情）；
    // 同时作废在途的详情请求，避免旧作品的响应把缓存换回去
    if (id && current && current.id === id) {
      seq += 1;
      syncSaved(id, current);
      // 新作品默认全选：任务侧按页差集只下缺的页，已下载页自动跳过；
      // 仅在本件尚未有选择时兜底，用户手动清空后不会被重新选上（watch 不再触发）
      if (panel.pageSelection.illustration.length === 0 && current.pages.length > 1) {
        panel.setPageSelection("illustration", current.pages.map((_, index) => index));
      }
      return;
    }
    timer = setTimeout(() => { timer = undefined; void ensurePages(id); }, 350);
  },
  { immediate: true }
);
onBeforeUnmount(() => { if (timer) clearTimeout(timer); });

async function submit() {
  if (panel.submitting) return;
  const kind = panel.kind;
  const session = panel.session;
  const form = panel.drafts[kind];
  const parsed = parsePixivUrl(form.sourceId);
  const supported = kind === "novel" ? ["novel-single", "novel-series", "user"] : ["illustration", "user"];
  const id = parsed && supported.includes(parsed.kind) ? parsed.id : form.sourceId.trim();
  if (!id || (kind === "novel" && !form.formats.length)) { panel.error = t(`${prefix.value}.invalidInput`); return; }
  // 选择器可见但一页未选：阻止 Enter 绕过按钮禁用态提交整件下载
  if (showPicker.value && pageSelectionCount.value === 0) { panel.error = t("browse.work.pageSelectRequired"); return; }
  // 插画单篇且当前作品确实展示着选择器：只下载选中页（升序由 store 保证）；否则整件下载
  const pages = showPicker.value ? panel.pageSelection.illustration : [];
  panel.submitting = true; panel.error = "";
  try {
    const result = await tasks.createTask(form.sourceType, id, kind === "novel" ? [...form.formats] : [], kind, pages.length ? [...pages] : undefined);
    if (session !== panel.session) return;
    if (result.error) { panel.error = result.error; return; }
    form.sourceId = ""; form.edited = false;
    panel.setPageSelection("illustration", []);
    panel.close();
    if (pages.length) notify(t("browse.work.pageDownloadStarted", { count: pages.length }));
    else notify(t(`${kind === "novel" ? "crawl" : "illust"}.taskCreated`, { id: result.task_id }));
    await tasks.fetchTasks().catch(() => {});
  } catch (error) { if (session === panel.session) panel.error = errorMessage(error) || t(`${prefix.value}.createFailed`); }
  finally { panel.submitting = false; }
}
</script>

<template>
  <aside v-show="panel.visible" ref="root" class="download-panel" :aria-label="t('workspace.newDownload')">
    <header><h2>{{ t('workspace.newDownload') }}</h2><md-icon-button :aria-label="t('common.close')" @click="panel.close()"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m18 6-12 12M6 6l12 12" /></svg></md-icon-button></header>
    <div class="panel-body">
      <!-- 包一层普通块：md-tabs 直接作为 grid item 时内部百分比高度会塌成 0（两栏落位下页签消失） -->
      <div class="panel-tabs">
        <SectionTabs :tabs="tabs" :value="panel.kind" :disabled="panel.submitting" @change="switchKind" />
      </div>
      <form :aria-busy="panel.submitting" @submit.prevent="submit">
        <fieldset :disabled="panel.submitting">
          <legend>{{ t(`${prefix}.sourceType`) }}</legend>
          <div class="m3-row"><label v-for="source in sources" :key="source" class="m3-choice"><md-radio name="download-source" :disabled="panel.submitting" :checked="draft.sourceType === source" @change="editType(source)" />{{ t(`${prefix}.${source}`) }}</label></div>
          <md-outlined-text-field :disabled="panel.submitting" :label="t(`${prefix}.input`)" :placeholder="t(`${prefix}.inputPlaceholder.${draft.sourceType}`)" :value="draft.sourceId" @input="editSource(($event.target as HTMLInputElement).value)" @keydown.enter.prevent="submit" />
          <div v-if="panel.kind === 'novel'" class="m3-field"><label>{{ t('crawl.outputFormats') }}</label><div class="m3-row"><label v-for="format in ['txt', 'markdown']" :key="format" class="m3-choice"><md-checkbox :disabled="panel.submitting" :checked="draft.formats.includes(format)" @change="formatChange(format, ($event.target as HTMLInputElement).checked)" />{{ format === 'txt' ? 'TXT' : 'Markdown' }}</label></div></div>
        </fieldset>
        <p v-if="panel.error" class="m3-alert error" role="alert">{{ panel.error }}</p>
        <md-filled-button :disabled="panel.submitting || (showPicker && pageSelectionCount === 0)" @click="submit">{{ panel.submitting ? t('common.loading') : submitLabel }}</md-filled-button>
      </form>
      <!-- 按页选择：仅在插画单篇且详情已到手（多页、非动图）时出现 -->
      <PageDownloadPicker
        v-if="showPicker"
        v-model="panel.pageSelection.illustration"
        :pages="pickerPages"
        :saved="pickerSaved"
        :disabled="panel.submitting"
      />
    </div>
    <dialog ref="confirm" class="m3-dialog" @cancel="panel.pendingTarget = null" @close="panel.pendingTarget = null"><h2>{{ t('workspace.replaceSource') }}</h2><p>{{ t('workspace.replaceSourceHint') }}</p><div class="dialog-actions m3-row"><md-text-button @click="panel.pendingTarget = null">{{ t('common.cancel') }}</md-text-button><md-filled-button @click="panel.confirmTarget()">{{ t('common.confirm') }}</md-filled-button></div></dialog>
  </aside>
</template>

<style scoped>
.download-panel { min-width:0; min-height:0; display:flex; flex-direction:column; background:var(--md-sys-color-surface-container); container-type:inline-size; }
header { display:flex; align-items:center; justify-content:space-between; padding:var(--space-sm) var(--space-xl); flex:none; }
h2 { font-size:18px; margin:0; }
svg { width:20px; height:20px; }
.panel-body { min-height:0; overflow:auto; padding:0 var(--space-xl) var(--space-xl); }
.panel-tabs { min-width:0; }
.panel-body :deep(.section-tabs) { --md-secondary-tab-container-color:var(--md-sys-color-surface-container); }
form { display:flex; flex-direction:column; gap:var(--space-lg); margin-top:var(--space-lg); }
fieldset { display:flex; flex-direction:column; gap:var(--space-lg); margin:0; padding:0; border:0; min-width:0; }
legend { padding:0; margin-bottom:var(--space-sm); }
.m3-row { flex-wrap:wrap; }
md-outlined-text-field { width:100%; }
md-filled-button { align-self:flex-start; }
.m3-alert { margin:0; overflow-wrap:anywhere; }
/* 单列落位（右侧 480px）时选择器排在表单下方，与表单同距 */
.panel-body > :deep(.page-picker) { margin-top:var(--space-lg); }

/*
 * 宽落位（底部整宽 ≥640px）且有选择器时改两栏：tabs 横跨，左表单右选择器。
 * 右侧 480px 落位天然 <640px，保持单列纵向。
 */
@container (min-width: 640px) {
  .panel-body:has(.page-picker) { display:grid; grid-template-columns:minmax(0,1fr) minmax(0,1fr); align-content:start; column-gap:var(--space-xl); }
  .panel-body:has(.page-picker) .panel-tabs { grid-column:1 / -1; }
  .panel-body:has(.page-picker) form { grid-column:1; }
  .panel-body:has(.page-picker) > :deep(.page-picker) { grid-column:2; }
  /* 底部落位整块高度受限：缩略图区最多两行，其余纵向滚动 */
  .panel-body:has(.page-picker) :deep(.picker-grid) { max-height:calc(2 * 56px + 2 * var(--space-xs) + var(--space-md)); overflow-y:auto; }
}
</style>
