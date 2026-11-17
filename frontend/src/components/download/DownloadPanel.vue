<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useAuthStore } from "../../stores/auth";
import { useDownloadPanelStore, type DownloadFormKind, type PageCache } from "../../stores/downloadPanel";
import { useTaskStore } from "../../stores/tasks";
import { parsePixivUrl } from "../../utils/pixivUrl";
import { browseWorkDetail, errorMessage, illustSavedPages } from "../../api/browse";
import { exportNovelTranslation, exportNovelTranslationSeries, type NovelTranslationExportFormat } from "../../api/translation";
import { checkNovelTranslationReadiness, checkSeriesTranslationReadiness, type TranslationReadiness } from "../../utils/translationReadiness";
import { notify } from "../../ui/notify";
import PageDownloadPicker from "./PageDownloadPicker.vue";
import SectionTabs from "../browse/SectionTabs.vue";

const panel = useDownloadPanelStore(); const tasks = useTaskStore(); const auth = useAuthStore(); const { t } = useI18n();
const root = ref<HTMLElement>(); const confirm = ref<HTMLDialogElement>();
const draft = computed(() => panel.drafts[panel.kind]);
const prefix = computed(() => panel.kind === "novel" ? "crawl" : "illust");
const tabs = computed(() => [{ value: "novel", label: t("nav.crawlNovel") }, { value: "illustration", label: t("nav.crawlIllustration") }]);
const sources = computed(() => panel.kind === "novel" ? ["single", "series", "user"] : ["single", "user"]);
/** 小说抓取输出格式（后端 task_create 白名单 txt|markdown|epub），展示顺序即 UI 顺序 */
const CRAWL_FORMATS = ["txt", "markdown", "epub"];
/** 译文导出格式（后端 novel_translation_export / _export_series 白名单），UI 暴露 txt / markdown / epub */
const TRANSLATION_FORMATS: NovelTranslationExportFormat[] = ["txt", "markdown", "epub"];
/** 格式展示名（字面量，跟随现有惯例不做 i18n） */
const FORMAT_LABELS: Record<string, string> = { txt: "TXT", markdown: "Markdown", epub: "EPUB" };
function switchKind(value: string) { if (!panel.submitting) { panel.kind = value as DownloadFormKind; panel.error = ""; } }
function editSource(value: string) { draft.value.sourceId = value; draft.value.edited = true; panel.error = ""; }
function editType(value: string) { draft.value.sourceType = value; draft.value.edited = true; panel.error = ""; }
function formatChange(value: string, checked: boolean) { draft.value.formats = checked ? [...draft.value.formats, value] : draft.value.formats.filter(format => format !== value); }
/** 译文格式勾选（小说单篇 / 系列表单渲染）；独立于原文 formats，提交时走本地导出命令 */
function translationFormatChange(value: NovelTranslationExportFormat, checked: boolean) { draft.value.translationFormats = checked ? [...draft.value.translationFormats, value] : draft.value.translationFormats.filter(format => format !== value); }
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
onBeforeUnmount(() => { if (timer) clearTimeout(timer); if (readinessTimer) clearTimeout(readinessTimer); });

// ===== 小说译文可用性预检 =====
/** 预检状态：idle = 不检查（无有效 ID / 插画 tab / 无条件来源），checking = 检查中 */
type TranslationStatus = "idle" | "checking" | "ready" | "unavailable" | "failed";
const translationStatus = ref<TranslationStatus>("idle");
const translationReadiness = ref<TranslationReadiness | null>(null);
/** 译文入口是否可选：仅小说单篇 / 系列，且该来源已有可用译文 */
const translationEnabled = computed(() => panel.kind === "novel" && draft.value.sourceType !== "user" && translationStatus.value === "ready");
/** 译文入口的提示文案（空串 = 不显示）；只解释「为什么不可选」 */
const translationHint = computed(() => {
  if (panel.kind !== "novel" || translationEnabled.value) return "";
  if (draft.value.sourceType === "user") return t("crawl.translationUserUnsupported");
  if (draft.value.sourceType === "series" && !auth.isLoggedIn) return t("crawl.translationLoginRequired");
  if (translationStatus.value === "checking") return t("crawl.translationChecking");
  if (translationStatus.value === "failed") return t("crawl.translationCheckFailed");
  if (translationStatus.value === "unavailable") {
    const readiness = translationReadiness.value;
    return draft.value.sourceType === "series"
      ? t("crawl.translationUnavailableSeries", { total: readiness?.total ?? 0, missing: readiness?.missingOrders.length ?? 0 })
      : t("crawl.translationUnavailableSingle");
  }
  return "";
});
/** 预检代次：输入连续变化时只接受最后一次结果 */
let readinessSeq = 0;
/** 450ms 防抖句柄 */
let readinessTimer: ReturnType<typeof setTimeout> | undefined;

/** 解析小说单篇 / 系列 id：纯数字，或与当前来源类型匹配的 pixiv URL；其余视为空 */
function resolveNovelId(): string {
  const raw = draft.value.sourceId.trim();
  if (!raw) return "";
  const parsed = parsePixivUrl(raw);
  if (parsed) {
    const expected = draft.value.sourceType === "single" ? "novel-single" : "novel-series";
    return parsed.kind === expected ? parsed.id : "";
  }
  return /^\d+$/.test(raw) && Number(raw) > 0 ? raw : "";
}

/** 换来源 / 换 ID 立即回落「原文」，避免带着旧译文选择提交新作品 */
watch(
  () => [panel.drafts.novel.sourceType, panel.drafts.novel.sourceId] as const,
  () => { panel.drafts.novel.variant = "original"; }
);

/** 译文预检：面板可见 + 小说单篇 / 系列 + 有效 ID；450ms 防抖，过期响应按代次丢弃 */
watch(
  () => [panel.visible, panel.kind, draft.value.sourceType, draft.value.sourceId, auth.isLoggedIn] as const,
  () => {
    if (readinessTimer) { clearTimeout(readinessTimer); readinessTimer = undefined; }
    readinessSeq += 1;
    translationReadiness.value = null;
    // 关闭面板 / 插画 tab / 用户全集 / 系列未登录：不发请求，入口直接置灰
    if (!panel.visible || panel.kind !== "novel" || draft.value.sourceType === "user" || (draft.value.sourceType === "series" && !auth.isLoggedIn)) {
      translationStatus.value = "idle";
      return;
    }
    const id = resolveNovelId();
    if (!id) { translationStatus.value = "idle"; return; }
    // 有效 ID 立即进入 checking：旧 ready 不能残留，否则可能在防抖窗口内误提交
    translationStatus.value = "checking";
    const token = readinessSeq;
    readinessTimer = setTimeout(() => { readinessTimer = undefined; void runTranslationCheck(token); }, 450);
  },
  { immediate: true }
);

async function runTranslationCheck(token: number): Promise<void> {
  const sourceType = draft.value.sourceType;
  const id = resolveNovelId();
  if (token !== readinessSeq) return;
  if (panel.kind !== "novel" || !id || (sourceType !== "single" && sourceType !== "series")) return;
  try {
    const result = sourceType === "series"
      ? await checkSeriesTranslationReadiness(Number(id))
      : await checkNovelTranslationReadiness(Number(id));
    if (token !== readinessSeq) return;
    translationReadiness.value = result;
    translationStatus.value = result.available ? "ready" : "unavailable";
    if (!result.available) panel.drafts.novel.variant = "original";
  } catch {
    if (token !== readinessSeq) return;
    translationStatus.value = "failed";
    // 检查失败同样回落原文：译文 radio 已禁用，不能让禁用项保持选中
    panel.drafts.novel.variant = "original";
  }
}

async function submit() {
  if (panel.submitting) return;
  const kind = panel.kind;
  const session = panel.session;
  const form = panel.drafts[kind];
  const parsed = parsePixivUrl(form.sourceId);
  const supported = kind === "novel" ? ["novel-single", "novel-series", "user"] : ["illustration", "user"];
  const id = parsed && supported.includes(parsed.kind) ? parsed.id : form.sourceId.trim();
  // 小说下载内容二选一：原文创建抓取任务，译文走本地导出（不再同时提交）
  const translationVariant = kind === "novel" && form.sourceType !== "user" && form.variant === "translation";
  const formatCount = translationVariant ? form.translationFormats.length : form.formats.length;
  if (!id) { panel.error = t(`${prefix.value}.invalidInput`); return; }
  // 译文未通过预检时不可提交：radio 禁用只能防点击，Enter 仍需兜底
  if (translationVariant && translationStatus.value !== "ready") { panel.error = translationHint.value || t("crawl.translationCheckFailed"); return; }
  if (kind === "novel" && !formatCount) { panel.error = t("crawl.formatRequired"); return; }
  // 选择器可见但一页未选：阻止 Enter 绕过按钮禁用态提交整件下载
  if (showPicker.value && pageSelectionCount.value === 0) { panel.error = t("browse.work.pageSelectRequired"); return; }
  // 插画单篇且当前作品确实展示着选择器：只下载选中页（升序由 store 保证）；否则整件下载
  const pages = showPicker.value ? panel.pageSelection.illustration : [];
  panel.submitting = true; panel.error = "";
  let exportedDir = "";
  let createdTaskId = "";
  /** 失败兜底文案按阶段区分：译文导出失败与任务创建失败各有默认提示 */
  let exportingTranslation = false;
  try {
    // 译文走本地导出（不联网、不建任务）：系列整包导出要求全系列每一话都已完成翻译，单篇走既有单篇导出；
    // 原文照旧创建抓取任务（插画没有译文分支）
    if (translationVariant) {
      exportingTranslation = true;
      const files = form.sourceType === "series"
        ? await exportNovelTranslationSeries(Number(id), [...form.translationFormats])
        : await exportNovelTranslation(Number(id), [...form.translationFormats]);
      if (session !== panel.session) return;
      exportedDir = files[0]?.replace(/[\\/][^\\/]+$/, "") ?? "";
    } else {
      const result = await tasks.createTask(form.sourceType, id, kind === "novel" ? [...form.formats] : [], kind, pages.length ? [...pages] : undefined);
      if (session !== panel.session) return;
      if (result.error) { panel.error = result.error; return; }
      createdTaskId = result.task_id ?? "";
    }
    form.sourceId = ""; form.edited = false; form.variant = "original";
    panel.setPageSelection("illustration", []);
    panel.close();
    if (exportedDir) notify(t("crawl.translationExported", { dir: exportedDir }));
    if (pages.length) notify(t("browse.work.pageDownloadStarted", { count: pages.length }));
    else if (createdTaskId) notify(t(`${kind === "novel" ? "crawl" : "illust"}.taskCreated`, { id: createdTaskId }));
    await tasks.fetchTasks().catch(() => {});
  } catch (error) {
    if (session === panel.session) {
      const fallback = exportingTranslation ? t("crawl.translationExportFailed") : t(`${prefix.value}.createFailed`);
      panel.error = errorMessage(error) || fallback;
    }
  }
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
          <!-- 下载内容：原文创建抓取任务 / 译文本地导出；译文入口按预检结果置灰 -->
          <div v-if="panel.kind === 'novel'" class="m3-field">
            <label>{{ t('crawl.downloadContent') }}</label>
            <div class="m3-row">
              <label class="m3-choice"><md-radio name="novel-download-content" :disabled="panel.submitting" :checked="draft.variant === 'original'" @change="draft.variant = 'original'" />{{ t('crawl.original') }}</label>
              <label class="m3-choice"><md-radio name="novel-download-content" :disabled="panel.submitting || !translationEnabled" :checked="draft.variant === 'translation'" @change="draft.variant = 'translation'" />{{ t('crawl.translation') }}</label>
            </div>
            <p v-if="translationHint" class="field-hint">{{ translationHint }}</p>
          </div>
          <!-- 格式区按下载内容切换：原文绑定抓取格式，译文绑定导出格式 -->
          <div v-if="panel.kind === 'novel' && draft.variant === 'translation'" class="m3-field"><label>{{ t('crawl.translationFormats') }}</label><div class="m3-row"><label v-for="format in TRANSLATION_FORMATS" :key="format" class="m3-choice"><md-checkbox :disabled="panel.submitting" :checked="draft.translationFormats.includes(format)" @change="translationFormatChange(format, ($event.target as HTMLInputElement).checked)" />{{ FORMAT_LABELS[format] }}</label></div></div>
          <div v-else-if="panel.kind === 'novel'" class="m3-field"><label>{{ t('crawl.outputFormats') }}</label><div class="m3-row"><label v-for="format in CRAWL_FORMATS" :key="format" class="m3-choice"><md-checkbox :disabled="panel.submitting" :checked="draft.formats.includes(format)" @change="formatChange(format, ($event.target as HTMLInputElement).checked)" />{{ FORMAT_LABELS[format] }}</label></div></div>
          <!-- series 来源提示：整包语义，supporting 文字样式 -->
          <div v-if="panel.kind === 'novel' && draft.sourceType === 'series'" class="series-hints">
            <p class="field-hint">{{ t('crawl.seriesBundleHint') }}</p>
          </div>
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
    <dialog ref="confirm" class="m3-dialog" aria-labelledby="replace-source-dialog-title" @cancel="panel.pendingTarget = null" @close="panel.pendingTarget = null"><h2 id="replace-source-dialog-title">{{ t('workspace.replaceSource') }}</h2><p>{{ t('workspace.replaceSourceHint') }}</p><div class="dialog-actions m3-row"><md-text-button @click="panel.pendingTarget = null">{{ t('common.cancel') }}</md-text-button><md-filled-button @click="panel.confirmTarget()">{{ t('common.confirm') }}</md-filled-button></div></dialog>
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
/* series 来源提示行：supporting 文字样式（沿用 M3 on-surface-variant token，不新增颜色） */
.series-hints { display:flex; flex-direction:column; gap:var(--space-xs); }
.field-hint { margin:0; color:var(--ink-muted); font-size:12px; }
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
