<script setup lang="ts">
/**
 * 小说翻译浮层（整篇 Job 版，计划 §8.3）。
 *
 * 头部：整篇作品 · N 页 · M 块 + 状态点/状态摘要；四个 tab：
 * - 状态：阶段、计数、进度、当前页/块、实时摘要、已用与用量（TranslationProgressBanner）；
 * - 术语与风格：等待确认时编辑并「确认并继续」，否则只读展示冻结版本；
 * - 预览：块列表 + 选中块原文/译文对照（未译块明确标示）；
 * - 问题：审查问题汇总，未解决块置顶。
 *
 * 关闭浮层 / Esc / 外部点击只关面板，不取消任务；焦点回到入口。
 * 状态摘要是唯一 aria-live 区，进度与逐帧预览不播报。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { errorMessage } from "../../api/tauri";
import {
  isActiveJobState,
  type ChunkPreview,
  type ConstraintConflict,
  type ConstraintTermEdit,
  type PublishStatus,
  type TranslationConstraints,
  type TranslationIssueRow,
  type TranslationIssues,
  type TranslationJobSnapshot,
  type TranslationMode,
} from "../../api/translation";
import { useTranslationStore } from "../../stores/translation";
import { useSettingsStore } from "../../stores/settings";
import TranslationProgressBanner from "./TranslationProgressBanner.vue";

const props = defineProps<{
  modelValue: TranslationMode;
  /** 该小说最新的 Job 快照（含终态）；null = 从未启动过整篇翻译。 */
  job: TranslationJobSnapshot | null;
  /** 阅读器给出的当前状态摘要（唯一播报文本）。 */
  status: string;
  error: boolean;
  disabled: boolean;
  /** 已有可读译文（任意页）。 */
  translated: boolean;
  /** 原文已是目标语言、本次未请求模型。 */
  skipped: boolean;
  /** 作品总页数（来自阅读器分页）。 */
  pages: number;
  /** 启动请求在途（读者持有 store.start 的 promise，用于禁用重复点击）。 */
  starting: boolean;
  /** 投影结果状态；partial / needs_attention 时提示存在未解决块。 */
  resultStatus: PublishStatus | null;
  resultUnresolved: number;
}>();
const emit = defineEmits<{
  "update:modelValue": [mode: TranslationMode];
  /** 请求启动整篇翻译；force = 重新翻译。 */
  start: [force: boolean];
}>();

const { t } = useI18n();
const store = useTranslationStore();
const settingsStore = useSettingsStore();

const currentModel = computed(() => settingsStore.settings.translation_model);
const availableModels = computed(() => {
  const models = settingsStore.settings.translation_models || [];
  if (currentModel.value && !models.includes(currentModel.value)) {
    return [currentModel.value, ...models];
  }
  return models;
});

async function onModelChange(model: string) {
  if (!model || model === currentModel.value) return;
  try {
    await settingsStore.saveSettings({ translation_model: model });
  } catch (err) {
    actionError.value = errorMessage(err);
  }
}

const open = ref(false);
const panel = ref<HTMLElement | null>(null);
interface Focusable { focus(): void }
const trigger = ref<Focusable | null>(null);

type TabKey = "state" | "terms" | "preview" | "issues";
const tab = ref<TabKey>("state");
const tabs = computed(() => [
  { key: "state" as const, label: t("translation.job.tabState") },
  { key: "terms" as const, label: t("translation.job.tabTerms") },
  { key: "preview" as const, label: t("translation.job.tabPreview") },
  { key: "issues" as const, label: t("translation.job.tabIssues") },
]);
const tablist = ref<HTMLElement | null>(null);

const job = computed(() => props.job);
const state = computed(() => job.value?.state ?? "none");
const chunks = computed(() => job.value?.counts.chunks ?? 0);
const scopeText = computed(() => t("translation.job.scope", { pages: props.pages, chunks: chunks.value }));
const stateLabel = computed(() => (job.value ? t(`translation.job.state.${state.value}`) : t("translation.job.state.none")));
const phaseLabel = computed(() => (job.value ? t(`translation.job.phase.${job.value.phase}`) : ""));

/** 忙碌：启动中或后端仍在推进；用于禁用动作按钮（重复点击不重复派发）。 */
const busy = computed(() => Boolean(job.value && ["queued", "running", "pausing", "cancelling"].includes(job.value.state)));
const awaiting = computed(() => state.value === "awaiting_confirmation");

/** 状态点语义：失败/待处理 error、进行中 primary、完成绿、其余中性。 */
const tone = computed(() => {
  if (props.error || state.value === "needs_attention") return "error";
  if (state.value === "completed") return "done";
  if (busy.value || awaiting.value) return "busy";
  return "idle";
});

const canStart = computed(() => !props.disabled && !busy.value && !awaiting.value);
const canPause = computed(() => Boolean(job.value && ["queued", "running"].includes(job.value.state)));
const canResume = computed(() => Boolean(job.value && ["paused", "needs_attention", "interrupted"].includes(job.value.state)));
const canCancel = computed(() => Boolean(job.value && isActiveJobState(job.value.state)));
/** 可重新发起（整篇原文一次提交）：从未开始、已完成或已取消。 */
const startable = computed(() => !job.value || job.value.state === "completed" || job.value.state === "cancelled" || job.value.state === "deleted");
const startLabel = computed(() => {
  if (props.skipped) return t("translation.translateAnyway");
  return job.value ? t("translation.job.retranslateJob") : t("translation.job.startJob");
});
/** 只在可发起任务时显示「开始 / 重新翻译」；失败 / 中断态与「继续」并存，
 * 作为资源版本失效等无法续跑场景的出路（force 新建任务）。 */
const showStart = computed(
  () =>
    canStart.value &&
    (startable.value ||
      props.skipped ||
      job.value?.state === "needs_attention" ||
      job.value?.state === "interrupted"),
);
/** 错误码只对需要处理/失败的任务有意义；完成态残留的旧错误码不展示。 */
const errorCode = computed(() => {
  const current = job.value;
  if (!current?.error_code) return "";
  return current.state === "needs_attention" || current.state === "interrupted" ? current.error_code : "";
});
/** 错误码的可读描述：有本地化映射时显示描述并附原始码，否则原样显示。 */
const errorCodeText = computed(() => {
  const code = errorCode.value;
  if (!code) return "";
  const key = `translation.job.errorCodes.${code}`;
  const desc = t(key);
  return desc === key ? code : `${desc}（${code}）`;
});
/** 失败 / 中断时显示后端给出的可读原因（如「还有 2 个分块未翻译」）。 */
const failureMessage = computed(() => {
  const current = job.value;
  if (!current?.message) return "";
  return current.state === "needs_attention" || current.state === "interrupted" ? current.message : "";
});

const partialNotice = computed(() => {
  if (props.resultStatus === "complete") return "";
  if (props.resultStatus === "partial") return t("translation.job.partial", { count: props.resultUnresolved });
  if (props.resultStatus === "needs_attention") return t("translation.job.needsAttentionNotice", { count: props.resultUnresolved });
  return "";
});

// ===== 动作 =====

const pending = ref("");
const actionError = ref("");

async function run(key: string, action: () => Promise<void>): Promise<void> {
  if (pending.value) return;
  pending.value = key;
  actionError.value = "";
  try {
    await action();
  } catch (error) {
    actionError.value = errorMessage(error);
  } finally {
    pending.value = "";
  }
}

/** 启动请求的本地去抖：parent 的 store.start 返回前按钮不可再次触发。 */
function requestStart(force: boolean): void {
  if (props.starting) return;
  emit("start", force);
}

function pauseJob(): void { if (job.value) void run("pause", () => store.pause(job.value!.job_id)); }
function resumeJob(): void { if (job.value) void run("resume", () => store.resume(job.value!.job_id)); }
function cancelJob(): void { if (job.value) void run("cancel", () => store.cancel(job.value!.job_id)); }

// ===== 术语与风格 =====

const constraints = ref<TranslationConstraints | null>(null);
const constraintsLoading = ref(false);
const constraintsUnavailable = ref(false);
const draftTerms = ref<ConstraintTermEdit[]>([]);
const draftStyle = ref("");
const termsEditable = computed(() => awaiting.value || constraints.value?.awaiting_confirmation === true);

function toEdits(terms: TranslationConstraints["terms"]): ConstraintTermEdit[] {
  return terms.map((term) => ({
    source_term: term.source_term,
    locked_translation: term.locked_translation,
    term_type: term.term_type ?? "",
    aliases: [...(term.aliases ?? [])],
    notes: term.notes ?? "",
  }));
}

async function loadConstraints(): Promise<void> {
  const current = job.value;
  if (!current) { constraints.value = null; constraintsUnavailable.value = false; return; }
  constraintsLoading.value = true;
  constraintsUnavailable.value = false;
  try {
    const data = await store.constraints(current.job_id);
    if (job.value?.job_id !== current.job_id) return;
    constraints.value = data;
    constraintsUnavailable.value = data === null;
    if (data) {
      draftTerms.value = toEdits(data.terms);
      draftStyle.value = data.style;
    }
  } finally {
    if (job.value?.job_id === current.job_id) constraintsLoading.value = false;
  }
}

async function submitConfirm(): Promise<void> {
  const current = job.value;
  if (!current) return;
  await run("confirm", async () => {
    await store.confirm(current.job_id, draftTerms.value, draftStyle.value);
    await loadConstraints();
  });
}

function setTermTranslation(index: number, value: string): void {
  const term = draftTerms.value[index];
  if (term) draftTerms.value[index] = { ...term, locked_translation: value };
}

function setTermNotes(index: number, value: string): void {
  const term = draftTerms.value[index];
  if (term) draftTerms.value[index] = { ...term, notes: value };
}

/** 冲突条目：优先 source_term + reason，缺字段时回退 JSON 片段（只读诊断信息）。 */
function conflictText(conflict: ConstraintConflict): string {
  const term = conflict.source_term ?? "";
  const reason = conflict.reason ?? (Array.isArray(conflict.candidates) ? conflict.candidates.join(" / ") : "");
  if (term && reason) return `${term} — ${reason}`;
  if (term) return term;
  return JSON.stringify(conflict);
}

/** 术语类型：已知类型走词库，未知回退后端原值。 */
const TERM_TYPES = ["person", "place", "organization", "term", "title", "other"] as const;
function termTypeLabel(type: string): string {
  return (TERM_TYPES as readonly string[]).includes(type) ? t(`translation.job.termTypes.${type}`) : type;
}

/** 严重度：已知级别走词库，未知回退原值。 */
const SEVERITIES = ["critical", "major", "minor"] as const;
function severityLabel(severity: string): string {
  return (SEVERITIES as readonly string[]).includes(severity) ? t(`translation.job.severity.${severity}`) : severity;
}

/** 问题处置口径：minor 只是建议，major / critical 才是待修。 */
function issueStatusLabel(severity: string): string {
  return severity === "minor" ? t("translation.job.issueStatus.suggestion") : t("translation.job.issueStatus.repair");
}

// ===== 预览 =====

const chunkIds = computed(() => {
  if (job.value?.chunk_ids?.length) return job.value.chunk_ids;
  // 旧快照（无计划字段）回退到序号；细分后的任务会因此漏块，仅作兼容。
  return Array.from({ length: Math.min(chunks.value, 200) }, (_, index) => `c${String(index + 1).padStart(4, "0")}`);
});
const previews = ref(new Map<string, ChunkPreview>());
const failedPreviews = ref(new Set<string>());
const loadingPreviews = ref(new Set<string>());
const previewListLoading = ref(false);
const selectedChunk = ref("");
let previewToken = 0;

function chunkStatus(id: string): "translated" | "untranslated" | "repair" | "pending" {
  if (failedPreviews.value.has(id)) return "untranslated";
  const data = previews.value.get(id);
  if (!data) return "pending";
  if (data.needs_repair) return "repair";
  return data.translated?.length ? "translated" : "untranslated";
}

async function loadChunk(id: string): Promise<void> {
  const current = job.value;
  if (!current || previews.value.has(id) || failedPreviews.value.has(id) || loadingPreviews.value.has(id)) return;
  loadingPreviews.value = new Set(loadingPreviews.value).add(id);
  try {
    const data = await store.preview(current.job_id, id);
    if (job.value?.job_id !== current.job_id) return;
    previews.value = new Map(previews.value).set(id, data);
  } catch {
    if (job.value?.job_id === current.job_id) failedPreviews.value = new Set(failedPreviews.value).add(id);
  } finally {
    const next = new Set(loadingPreviews.value);
    next.delete(id);
    loadingPreviews.value = next;
  }
}

/** 预取全部块（并发 3），列表状态点随到达逐步点亮；切 tab / 换 Job 时中止。 */
async function prefetchChunks(): Promise<void> {
  const token = ++previewToken;
  const ids = chunkIds.value;
  if (!ids.length) { previewListLoading.value = false; return; }
  previewListLoading.value = true;
  if (!selectedChunk.value) selectedChunk.value = ids[0];
  let cursor = 0;
  const workers = Array.from({ length: Math.min(3, ids.length) }, async () => {
    while (cursor < ids.length && token === previewToken) {
      const id = ids[cursor++];
      await loadChunk(id);
    }
  });
  await Promise.all(workers);
  if (token === previewToken) previewListLoading.value = false;
}

const selectedPreview = computed(() => previews.value.get(selectedChunk.value) ?? null);
const selectedFailure = computed(() => failedPreviews.value.has(selectedChunk.value));
const selectedLoading = computed(() => loadingPreviews.value.has(selectedChunk.value));

/**
 * 已通过块的建议条数：审查结论为 pass 的块可能附带 minor issues，
 * 它们不是待修项，只在通过标签上提示数量。
 */
const selectedSuggestionCount = computed(() => {
  const preview = selectedPreview.value;
  const review = preview?.review;
  if (!preview?.reviewed || !review || review.verdict !== "pass") return 0;
  return review.issues.filter((issue) => issue.severity === "minor").length;
});
const reviewedBadgeText = computed(() => {
  const preview = selectedPreview.value;
  if (!preview?.reviewed) return t("translation.job.notReviewed");
  if (selectedSuggestionCount.value > 0) {
    return t("translation.job.reviewedWithSuggestions", { count: selectedSuggestionCount.value });
  }
  return t("translation.job.reviewed");
});

function translatedFor(preview: ChunkPreview, segmentId: string): string {
  return preview.translated?.find((item) => item.id === segmentId)?.text ?? "";
}

function selectChunk(id: string): void {
  selectedChunk.value = id;
  void loadChunk(id);
}

// ===== 问题 =====

const issueData = ref<TranslationIssues | null>(null);
const issuesLoading = ref(false);
const issuesUnavailable = ref(false);

async function loadIssues(): Promise<void> {
  const current = job.value;
  if (!current) { issueData.value = null; return; }
  issuesLoading.value = true;
  issuesUnavailable.value = false;
  try {
    const data = await store.issues(current.job_id);
    if (job.value?.job_id !== current.job_id) return;
    issueData.value = data;
    issuesUnavailable.value = data === null;
  } finally {
    if (job.value?.job_id === current.job_id) issuesLoading.value = false;
  }
}

function chunkOf(issue: TranslationIssueRow): string {
  return issue.chunk_id ?? issue.chunk ?? "";
}
function segmentOf(issue: TranslationIssueRow): string {
  return issue.segment_id ?? issue.segment ?? "";
}
const SEVERITY_RANK: Record<string, number> = { critical: 0, major: 1, minor: 2 };
const sortedIssues = computed<TranslationIssueRow[]>(() => {
  const data = issueData.value;
  if (!data) return [];
  const unresolved = new Set(data.unresolved_chunks ?? []);
  return [...(data.issues ?? [])].sort((a, b) => {
    const ua = unresolved.has(chunkOf(a)) || a.resolved === false ? 0 : 1;
    const ub = unresolved.has(chunkOf(b)) || b.resolved === false ? 0 : 1;
    if (ua !== ub) return ua - ub;
    return (SEVERITY_RANK[a.severity] ?? 3) - (SEVERITY_RANK[b.severity] ?? 3);
  });
});

// ===== Tab 与生命周期 =====

function activateTab(next: TabKey): void {
  tab.value = next;
  if (next === "terms") void loadConstraints();
  else if (next === "preview") void prefetchChunks();
  else if (next === "issues") void loadIssues();
}

function onTabKeydown(event: KeyboardEvent): void {
  const keys = tabs.value.map((item) => item.key);
  const index = keys.indexOf(tab.value);
  let target = -1;
  if (event.key === "ArrowRight") target = (index + 1) % keys.length;
  else if (event.key === "ArrowLeft") target = (index - 1 + keys.length) % keys.length;
  else if (event.key === "Home") target = 0;
  else if (event.key === "End") target = keys.length - 1;
  if (target < 0) return;
  event.preventDefault();
  // tab 条吃掉方向键：不让阅读器的 ←/→ 翻页同时响应。
  event.stopPropagation();
  activateTab(keys[target]);
  void nextTick(() => {
    tablist.value?.querySelectorAll<HTMLElement>('[role="tab"]')[target]?.focus();
  });
}

/** 换 Job（重译 / 切换小说）：清空与上一 Job 绑定的本地数据。 */
function resetJobData(): void {
  constraints.value = null;
  constraintsUnavailable.value = false;
  constraintsLoading.value = false;
  draftTerms.value = [];
  draftStyle.value = "";
  previews.value = new Map();
  failedPreviews.value = new Set();
  loadingPreviews.value = new Set();
  previewListLoading.value = false;
  selectedChunk.value = "";
  issueData.value = null;
  issuesUnavailable.value = false;
  issuesLoading.value = false;
  previewToken += 1;
}

watch(() => job.value?.job_id, (next, previous) => {
  if (next === previous) return;
  resetJobData();
  if (next) activateTab(tab.value);
});

watch(() => job.value?.constraint_version, (next, previous) => {
  if (next === previous) return;
  constraints.value = null;
  if (tab.value === "terms") void loadConstraints();
});

// 进度计数变化说明有块完成翻译 / 审查：丢弃本地旧预览并重新预取，
// 否则早期读到的「未译」会一直留在列表里，与顶部计数矛盾。
// store 的 applySnapshot 会同步失效该 Job 的预览缓存，这里的重取才真正回到后端拿新内容。
watch(
  () => {
    const counts = job.value?.counts;
    return counts ? `${counts.translated}:${counts.reviewed}:${counts.needs_repair}` : "";
  },
  (next, previous) => {
    if (next === previous || !previews.value.size) return;
    previewToken += 1;
    previews.value = new Map();
    failedPreviews.value = new Set();
    if (!chunkIds.value.includes(selectedChunk.value)) selectedChunk.value = chunkIds.value[0] ?? "";
    if (open.value) void prefetchChunks();
  },
);

// 等待确认：已打开面板时切到术语 tab（用户关闭时不强制重开）。
watch(awaiting, (value) => {
  if (!value) return;
  if (open.value) activateTab("terms");
  else void loadConstraints();
});

function close(): void { open.value = false; }

/**
 * 视口内钳制：默认右缘对齐触发器，窄窗或触发器靠左时改为显式左边界，
 * 始终留在侧栏右侧与视口内（上一步的内联值先清掉，避免多次打开累积偏移）。
 */
function clampPanel(): void {
  const el = panel.value;
  if (!el) return;
  el.style.left = "";
  el.style.right = "";
  const gap = Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--space-sm")) || 8;
  const rect = el.getBoundingClientRect();
  const width = rect.width;
  const viewport = document.documentElement.clientWidth || window.innerWidth;
  const sidebar = document.querySelector(".sidebar");
  const minLeft = Math.max(gap, (sidebar?.getBoundingClientRect().right ?? 0) + gap);
  const maxLeft = Math.max(minLeft, viewport - width - gap);
  const target = Math.min(Math.max(rect.left, minLeft), maxLeft);
  if (Math.abs(target - rect.left) < 0.5) return;
  const parent = el.offsetParent as HTMLElement | null;
  const baseLeft = parent?.getBoundingClientRect().left ?? 0;
  el.style.left = `${Math.round(target - baseLeft)}px`;
  el.style.right = "auto";
}
function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape" && !document.querySelector("dialog[open]")) close();
}
watch(open, (value) => {
  if (value) {
    window.addEventListener("keydown", onKeydown);
    window.addEventListener("resize", clampPanel);
    activateTab(tab.value);
    void nextTick(() => {
      void document.fonts?.ready.then(() => clampPanel());
      clampPanel();
      panel.value?.focus();
    });
  } else {
    window.removeEventListener("keydown", onKeydown);
    window.removeEventListener("resize", clampPanel);
    trigger.value?.focus();
  }
});
// 挂载时已有活跃 / 待确认任务（从底栏或侧栏回来）→ 自动展开一次；用户手动关闭后不强制重开。
onMounted(() => {
  if (job.value && isActiveJobState(job.value.state)) open.value = true;
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("resize", clampPanel);
});
</script>

<template>
  <div class="translation-menu">
    <span class="translation-trigger-wrap">
      <md-icon-button
        ref="trigger"
        class="translation-trigger"
        :class="{ active: open || busy }"
        :aria-label="t('translation.menu')"
        :title="status ? `${t('translation.menu')}：${status}` : t('translation.menu')"
        aria-haspopup="dialog"
        :aria-expanded="open"
        @click="open = !open"
      >
        <!-- Lucide languages：https://lucide.dev/icons/languages -->
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 8 6 6m-7 0 6-6 2-3M2 5h12M7 2h1m14 20-5-10-5 10m2-4h6" /></svg>
      </md-icon-button>
      <span v-if="tone !== 'idle' || skipped || translated" class="translation-indicator" :class="`is-${tone}`" :title="status" aria-hidden="true"></span>
    </span>
    <div v-if="open" class="translation-backdrop" @click="close"></div>
    <div v-if="open" ref="panel" class="translation-popover" role="dialog" :aria-label="t('translation.menu')" tabindex="-1">
      <!-- 头部：标题 + 范围 + 状态摘要 + 关闭按钮 -->
      <div class="panel-head">
        <div class="panel-head-top">
          <div class="panel-head-title-group">
            <span class="translation-title">{{ t('translation.menu') }}</span>
            <span class="panel-scope">{{ scopeText }}</span>
          </div>
          <md-icon-button class="panel-close-btn" :aria-label="t('common.close')" @click="close">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg>
          </md-icon-button>
        </div>
        <div class="translation-status-row">
          <span class="translation-indicator" :class="`is-${tone}`" aria-hidden="true"></span>
          <span class="translation-status" :class="{ 'is-error': error }" role="status" aria-live="polite">{{ status }}</span>
        </div>
      </div>

      <!-- 模型快速切换栏 -->
      <div class="panel-model-bar">
        <div class="panel-model-label">
          <svg class="panel-model-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect width="16" height="16" x="4" y="4" rx="2" />
            <rect width="6" height="6" x="9" y="9" rx="1" />
            <path d="M15 2v2M15 20v2M2 15h2M2 9h2M20 15h2M20 9h2M9 2v2M9 20v2" />
          </svg>
          <span>{{ t('translation.model') }}</span>
        </div>
        <div class="panel-model-control">
          <md-outlined-select
            v-if="availableModels.length"
            class="panel-model-select"
            :value="currentModel"
            :disabled="busy"
            :aria-label="t('translation.model')"
            @change="onModelChange(($event.target as HTMLSelectElement).value)"
          >
            <md-select-option v-for="m in availableModels" :key="m" :value="m">{{ m }}</md-select-option>
          </md-outlined-select>
          <span v-else class="panel-model-text" :title="currentModel || t('translation.noModelConfigured')">
            {{ currentModel || t('translation.noModelConfigured') }}
          </span>
        </div>
      </div>

      <!-- 四个 tab -->
      <div ref="tablist" class="panel-tabs" role="tablist" :aria-label="t('translation.menu')" @keydown="onTabKeydown">
        <button
          v-for="item in tabs"
          :key="item.key"
          type="button"
          role="tab"
          class="panel-tab"
          :class="{ selected: tab === item.key }"
          :aria-selected="tab === item.key"
          :tabindex="tab === item.key ? 0 : -1"
          :id="`translation-tab-${item.key}`"
          :aria-controls="`translation-panel-${item.key}`"
          @click="activateTab(item.key)"
        >{{ item.label }}</button>
      </div>

      <div class="panel-body">
        <!-- 状态 -->
        <section
          v-show="tab === 'state'"
          class="panel-section"
          role="tabpanel"
          :id="`translation-panel-state`"
          :aria-labelledby="`translation-tab-state`"
        >
          <p class="panel-state-line">{{ stateLabel }}<template v-if="phaseLabel"> · {{ phaseLabel }}</template></p>
          <TranslationProgressBanner v-if="job" :job="job" />
          <p v-else class="panel-empty">{{ t('translation.job.notStarted') }}</p>
          <p v-if="partialNotice" class="panel-notice">{{ partialNotice }}</p>
          <p v-if="failureMessage" class="panel-notice">{{ failureMessage }}</p>
          <p v-if="errorCode" class="panel-error-code">{{ t('translation.job.errorCode', { code: errorCodeText }) }}</p>
          <p v-if="actionError" class="panel-error" role="alert">{{ actionError }}</p>
        </section>

        <!-- 术语与风格 -->
        <section
          v-show="tab === 'terms'"
          class="panel-section"
          role="tabpanel"
          :id="`translation-panel-terms`"
          :aria-labelledby="`translation-tab-terms`"
        >
          <p v-if="constraintsLoading" class="panel-empty">{{ t('common.loading') }}</p>
          <p v-else-if="constraintsUnavailable || !constraints" class="panel-empty">{{ t('translation.job.termsUnavailable') }}</p>
          <template v-else>
            <p class="panel-meta">
              <template v-if="termsEditable">{{ t('translation.job.termsAwaiting', { count: draftTerms.length }) }}</template>
              <template v-else>{{ t('translation.job.termsFrozen', { version: constraints.version }) }}</template>
            </p>
            <div v-if="constraints.conflicts.length" class="panel-subblock">
              <h3 class="panel-subtitle">{{ t('translation.job.conflicts', { count: constraints.conflicts.length }) }}</h3>
              <ul class="panel-list">
                <li v-for="(conflict, index) in constraints.conflicts" :key="index" class="panel-list-item">{{ conflictText(conflict) }}</li>
              </ul>
            </div>
            <div v-if="constraints.unresolved.length" class="panel-subblock">
              <h3 class="panel-subtitle">{{ t('translation.job.unresolvedTerms', { count: constraints.unresolved.length }) }}</h3>
              <ul class="panel-list">
                <li v-for="term in constraints.unresolved" :key="term" class="panel-list-item">{{ term }}</li>
              </ul>
            </div>
            <div v-if="!draftTerms.length" class="panel-empty">{{ t('translation.job.termsEmpty') }}</div>
            <div v-else class="term-list">
              <div v-for="(term, index) in draftTerms" :key="`${term.source_term}-${index}`" class="term-row">
                <div class="term-head">
                  <span class="term-source" :title="term.source_term">{{ term.source_term }}</span>
                  <span v-if="term.term_type" class="term-type">{{ termTypeLabel(term.term_type) }}</span>
                </div>
                <label class="term-field">
                  <span class="term-label">{{ t('translation.job.termTranslation') }}</span>
                  <input
                    v-if="termsEditable"
                    class="term-input"
                    type="text"
                    :value="term.locked_translation"
                    :aria-label="t('translation.job.termTranslationFor', { term: term.source_term })"
                    @input="setTermTranslation(index, ($event.target as HTMLInputElement).value)"
                  />
                  <span v-else class="term-value">{{ term.locked_translation || t('translation.job.termUnset') }}</span>
                </label>
                <label v-if="termsEditable" class="term-field">
                  <span class="term-label">{{ t('translation.job.termNotes') }}</span>
                  <input
                    class="term-input"
                    type="text"
                    :value="term.notes"
                    :aria-label="t('translation.job.termNotesFor', { term: term.source_term })"
                    @input="setTermNotes(index, ($event.target as HTMLInputElement).value)"
                  />
                </label>
                <p v-else-if="term.notes" class="term-notes">{{ term.notes }}</p>
              </div>
            </div>
            <label class="style-field">
              <span class="term-label">{{ t('translation.job.style') }}</span>
              <textarea
                class="style-input"
                rows="3"
                :value="draftStyle"
                :readonly="!termsEditable"
                :aria-label="t('translation.job.style')"
                @input="draftStyle = ($event.target as HTMLTextAreaElement).value"
              ></textarea>
            </label>
            <md-filled-button
              v-if="termsEditable"
              class="confirm-button"
              :disabled="Boolean(pending)"
              :aria-busy="pending === 'confirm'"
              @click="submitConfirm"
            >{{ pending === 'confirm' ? t('translation.job.confirming') : t('translation.job.confirmContinue') }}</md-filled-button>
          </template>
        </section>

        <!-- 预览 -->
        <section
          v-show="tab === 'preview'"
          class="panel-section"
          role="tabpanel"
          :id="`translation-panel-preview`"
          :aria-labelledby="`translation-tab-preview`"
        >
          <p v-if="!chunkIds.length" class="panel-empty">{{ t('translation.job.chunksPending') }}</p>
          <div v-else class="preview-layout">
            <ul class="chunk-list" :aria-label="t('translation.job.chunkList')">
              <li v-for="id in chunkIds" :key="id">
                <button
                  type="button"
                  class="chunk-item"
                  :class="{ selected: selectedChunk === id }"
                  :aria-pressed="selectedChunk === id"
                  @click="selectChunk(id)"
                >
                  <span class="chunk-dot" :class="`is-${chunkStatus(id)}`" aria-hidden="true"></span>
                  <span class="chunk-name">{{ id }}</span>
                  <span v-if="chunkStatus(id) !== 'pending'" class="chunk-state">{{ t(`translation.job.chunkStates.${chunkStatus(id)}`) }}</span>
                </button>
              </li>
            </ul>
            <div class="chunk-detail">
              <p v-if="previewListLoading && !selectedPreview" class="panel-empty">{{ t('common.loading') }}</p>
              <p v-else-if="selectedFailure" class="panel-empty">{{ t('translation.job.previewUnavailable') }}</p>
              <p v-else-if="!selectedPreview" class="panel-empty">{{ t('translation.job.previewSelect') }}</p>
              <template v-else>
                <div class="chunk-badges">
                  <span class="chunk-badge">{{ t('translation.job.chunkOrder', { order: selectedPreview.order }) }}</span>
                  <span class="chunk-badge" :class="{ ok: selectedPreview.reviewed }">
                    {{ reviewedBadgeText }}
                  </span>
                  <span v-if="selectedPreview.needs_repair" class="chunk-badge warn">{{ t('translation.job.needsRepair') }}</span>
                </div>
                <p v-if="!selectedPreview.translated.length" class="panel-notice">{{ t('translation.job.untranslatedChunk') }}</p>
                <p v-if="selectedPreview.context_prev" class="context-line">{{ t('translation.job.contextPrev') }} {{ selectedPreview.context_prev }}</p>
                <div class="segment-list">
                  <div v-for="segment in selectedPreview.segments" :key="segment.id" class="segment-row">
                    <p class="segment-original">{{ segment.text }}</p>
                    <p v-if="translatedFor(selectedPreview, segment.id)" class="segment-translated">{{ translatedFor(selectedPreview, segment.id) }}</p>
                    <p v-else class="segment-untranslated">{{ t('translation.job.untranslatedSegment') }}</p>
                  </div>
                </div>
                <p v-if="selectedPreview.context_next" class="context-line">{{ t('translation.job.contextNext') }} {{ selectedPreview.context_next }}</p>
              </template>
            </div>
          </div>
        </section>

        <!-- 问题 -->
        <section
          v-show="tab === 'issues'"
          class="panel-section"
          role="tabpanel"
          :id="`translation-panel-issues`"
          :aria-labelledby="`translation-tab-issues`"
        >
          <p v-if="issuesLoading" class="panel-empty">{{ t('common.loading') }}</p>
          <template v-else-if="issuesUnavailable || !issueData">
            <p class="panel-empty">{{ t('translation.job.issuesUnavailable') }}</p>
          </template>
          <template v-else>
            <p class="panel-meta">{{ t('translation.job.issuesSummary', { unresolved: issueData.unresolved_chunks.length, total: issueData.total }) }}</p>
            <p v-if="!issueData.issues.length" class="panel-empty">{{ t('translation.job.issuesEmpty') }}</p>
            <template v-else>
              <p v-if="issueData.unresolved_chunks.length" class="panel-notice">
                {{ t('translation.job.unresolvedChunks', { chunks: issueData.unresolved_chunks.join('、') }) }}
              </p>
              <ul class="issue-list">
                <li v-for="(issue, index) in sortedIssues" :key="index" class="issue-row">
                  <div class="issue-head">
                    <span class="issue-severity" :class="`is-${issue.severity}`" :title="severityLabel(issue.severity)">{{ issueStatusLabel(issue.severity) }}</span>
                    <span v-if="chunkOf(issue)" class="issue-chunk">{{ chunkOf(issue) }}</span>
                    <span v-if="segmentOf(issue)" class="issue-segment">{{ segmentOf(issue) }}</span>
                  </div>
                  <p class="issue-detail">{{ issue.detail }}</p>
                </li>
              </ul>
            </template>
          </template>
        </section>
      </div>

      <!-- 底部动作：常驻，不随内容滚走 -->
      <div class="panel-actions">
        <div class="translation-modes" role="group" :aria-label="t('translation.displayMode')">
          <button
            v-for="mode in (['original', 'translated', 'bilingual'] as const)"
            :key="mode"
            type="button"
            class="translation-mode"
            :aria-pressed="modelValue === mode"
            :class="{ selected: modelValue === mode }"
            @click="emit('update:modelValue', mode)"
          >{{ t(`translation.${mode}`) }}</button>
        </div>
        <div class="action-row">
          <md-filled-button
            v-if="showStart"
            :disabled="starting || disabled"
            :aria-busy="starting"
            @click="requestStart(Boolean(skipped) || Boolean(job))"
          >{{ startLabel }}</md-filled-button>
          <md-outlined-button v-if="canResume" :disabled="Boolean(pending)" :aria-busy="pending === 'resume'" @click="resumeJob">
            {{ pending === 'resume' ? t('translation.job.working') : t('translation.job.resume') }}
          </md-outlined-button>
          <md-outlined-button v-if="canPause" :disabled="Boolean(pending)" :aria-busy="pending === 'pause'" @click="pauseJob">
            {{ pending === 'pause' ? t('translation.job.working') : t('translation.job.pause') }}
          </md-outlined-button>
          <md-text-button v-if="canCancel" class="cancel-button" :disabled="Boolean(pending)" @click="cancelJob">
            {{ t('translation.job.cancel') }}
          </md-text-button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.translation-menu { position: relative; display: inline-flex; }
.translation-trigger-wrap { position: relative; display: inline-flex; }
.translation-trigger svg { width: 20px; height: 20px; }
.translation-trigger.active { --md-icon-button-icon-color: var(--md-sys-color-primary); }

/* 状态点：进行中 primary、完成绿、失败/待处理 error、其余中性；入口点带 surface 描边。 */
.translation-indicator { flex: none; width: 8px; height: 8px; border-radius: 50%; background: var(--ink-subtle); }
.translation-indicator.is-busy { background: var(--md-sys-color-primary); }
.translation-indicator.is-done { background: var(--state-success-ink); }
.translation-indicator.is-error { background: var(--md-sys-color-error); }
.translation-trigger-wrap > .translation-indicator { position: absolute; top: 3px; right: 3px; box-shadow: 0 0 0 2px var(--md-sys-color-surface); }

.translation-backdrop { position: fixed; inset: 0; z-index: 890; }
.translation-popover {
  position: absolute;
  bottom: calc(100% + var(--space-sm));
  right: 0;
  z-index: 900;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  width: min(420px, calc(100vw - var(--space-lg) * 2));
  max-width: calc(100vw - var(--space-lg) * 2);
  max-height: min(76vh, 620px);
  border-radius: 16px;
  background: var(--md-sys-color-surface-container);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 20%, transparent);
  color: var(--ink);
  box-shadow: 0 8px 28px -4px rgba(0, 0, 0, 0.16), 0 3px 8px -2px rgba(0, 0, 0, 0.08);
  outline: none;
}
.translation-popover:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 2px; }

.panel-head {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: var(--space-md) var(--space-md) var(--space-xs);
}
.panel-head-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-xs);
}
.panel-head-title-group {
  display: flex;
  align-items: baseline;
  gap: var(--space-sm);
  min-width: 0;
}
.translation-title { font-size: 15px; font-weight: 600; color: var(--ink); }
.panel-scope { color: var(--ink-muted); font-size: 12px; }
.panel-close-btn { width: 32px; height: 32px; --md-icon-button-icon-size: 18px; margin: -4px -4px -4px 0; }
.translation-status-row { display: flex; align-items: flex-start; gap: var(--space-xs); }
.translation-status-row .translation-indicator { margin-top: 5px; }
.translation-status { color: var(--ink-muted); font-size: 12px; line-height: 1.5; overflow-wrap: anywhere; }
.translation-status.is-error { color: var(--md-sys-color-error); }

/* 模型快速切换栏 */
.panel-model-bar {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-sm);
  padding: 6px var(--space-sm);
  margin: 0 var(--space-md) var(--space-xs);
  background: color-mix(in srgb, var(--md-sys-color-surface-container-highest) 50%, transparent);
  border-radius: 8px;
}
.panel-model-label {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 500;
  flex: none;
}
.panel-model-icon { width: 15px; height: 15px; color: var(--md-sys-color-primary); }
.panel-model-control { flex: 1; min-width: 0; display: flex; justify-content: flex-end; }
.panel-model-select {
  width: 100%;
  max-width: 250px;
  --md-outlined-field-top-space: 4px;
  --md-outlined-field-bottom-space: 4px;
  --md-outlined-select-text-field-input-text-size: 12px;
}
.panel-model-text {
  color: var(--ink);
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-align: right;
}

/* tab 条：M3 Primary Tab 质感与平滑下划线指示条 */
.panel-tabs {
  flex: none;
  display: flex;
  gap: 0;
  padding: 0 var(--space-sm);
  border-bottom: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 25%, transparent);
}
.panel-tab {
  position: relative;
  flex: 1;
  min-width: 0;
  min-height: 36px;
  padding: var(--space-xs) var(--space-xxs);
  border: 0;
  background: transparent;
  color: var(--ink-muted);
  font: inherit;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  transition: color 150ms ease;
}
.panel-tab:hover:not(.selected) { color: var(--ink); background: color-mix(in srgb, var(--md-sys-color-on-surface) 6%, transparent); }
.panel-tab:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: -2px; }
.panel-tab.selected { color: var(--md-sys-color-primary); font-weight: 600; }
.panel-tab.selected::after {
  content: "";
  position: absolute;
  bottom: -1px;
  left: 14%;
  right: 14%;
  height: 2px;
  border-radius: 2px 2px 0 0;
  background: var(--md-sys-color-primary);
}

/* 内容区是唯一滚动容器：长文、问题列表、预览都在这里内滚。 */
.panel-body { flex: 1; min-height: 0; overflow-y: auto; padding: var(--space-sm) var(--space-md); }
.panel-section { display: flex; flex-direction: column; gap: var(--space-sm); min-width: 0; }
.panel-state-line { margin: 0; color: var(--ink); font-size: 13px; font-weight: 600; overflow-wrap: anywhere; }
.panel-meta { margin: 0; color: var(--ink-muted); font-size: 12px; line-height: 1.5; overflow-wrap: anywhere; }
.panel-empty { margin: 0; color: var(--ink-muted); font-size: 12px; line-height: 1.5; overflow-wrap: anywhere; }
.panel-notice {
  margin: 0;
  padding: 8px 12px;
  border-radius: 8px;
  background: color-mix(in srgb, var(--md-sys-color-error) 10%, transparent);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-error) 20%, transparent);
  color: var(--md-sys-color-error);
  font-size: 12px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}
.panel-error { margin: 0; color: var(--md-sys-color-error); font-size: 12px; line-height: 1.5; overflow-wrap: anywhere; }
.panel-error-code { margin: 0; color: var(--ink-subtle); font-size: 11px; line-height: 1.5; overflow-wrap: anywhere; }
.panel-subblock { display: flex; flex-direction: column; gap: var(--space-xxs); }
.panel-subtitle { margin: 0; color: var(--ink); font-size: 12px; font-weight: 600; }
.panel-list { margin: 0; padding-inline-start: var(--space-lg); color: var(--ink-muted); font-size: 12px; line-height: 1.5; }
.panel-list-item { overflow-wrap: anywhere; }

/* 术语：source 只读、locked_translation / notes 可改；非确认态全部只读。 */
.term-list { display: flex; flex-direction: column; gap: var(--space-sm); }
.term-row { display: flex; flex-direction: column; gap: var(--space-xxs); padding: var(--space-sm); border-radius: var(--radius-control); background: var(--surface); }
.term-head { display: flex; align-items: center; gap: var(--space-xs); min-width: 0; }
.term-source { flex: 1; min-width: 0; overflow: hidden; color: var(--ink); font-size: 13px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
.term-type { flex: none; padding: 0 var(--space-xs); border-radius: 999px; background: var(--md-sys-color-surface-container); color: var(--ink-muted); font-size: 11px; line-height: 1.6; }
.term-field, .style-field { display: flex; flex-direction: column; gap: 2px; }
.term-label { color: var(--ink-muted); font-size: 11px; line-height: 1.4; }
.term-input, .style-input {
  box-sizing: border-box;
  width: 100%;
  min-height: 32px;
  padding: var(--space-xxs) var(--space-sm);
  border: 1px solid var(--md-sys-color-outline);
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--ink);
  font: inherit;
  font-size: 13px;
}
.term-input:focus-visible, .style-input:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 2px; border-color: transparent; }
.style-input { resize: vertical; line-height: 1.5; }
.term-value { color: var(--ink); font-size: 13px; overflow-wrap: anywhere; }
.term-notes { margin: 0; color: var(--ink-muted); font-size: 12px; line-height: 1.4; overflow-wrap: anywhere; }
.confirm-button { align-self: flex-start; }

/* 预览：左侧块列表（窄窗改为上下堆叠），右侧原文/译文对照。 */
.preview-layout { display: flex; flex-direction: column; gap: var(--space-sm); min-width: 0; }
.chunk-list { display: flex; flex-wrap: wrap; gap: var(--space-xxs); margin: 0; padding: 0; list-style: none; }
.chunk-item {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xxs);
  min-height: 28px;
  padding: 0 var(--space-xs);
  border: 0;
  border-radius: var(--radius-control);
  background: var(--surface);
  color: var(--ink-muted);
  font: inherit;
  font-size: 11px;
  cursor: pointer;
}
.chunk-item:hover { background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }
.chunk-item:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: -2px; }
.chunk-item.selected { background: var(--md-sys-color-secondary-container); color: var(--md-sys-color-on-secondary-container); }
.chunk-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--ink-subtle); }
.chunk-dot.is-translated { background: var(--state-success-ink); }
.chunk-dot.is-repair { background: var(--md-sys-color-error); }
.chunk-state { color: inherit; font-size: 10px; opacity: 0.8; }
.chunk-detail { display: flex; flex-direction: column; gap: var(--space-xs); min-width: 0; }
.chunk-badges { display: flex; flex-wrap: wrap; gap: var(--space-xxs); }
.chunk-badge { padding: 0 var(--space-xs); border-radius: 999px; background: var(--surface); color: var(--ink-muted); font-size: 11px; line-height: 1.8; }
.chunk-badge.ok { color: var(--state-success-ink); }
.chunk-badge.warn { color: var(--md-sys-color-error); }
.segment-list { display: flex; flex-direction: column; gap: var(--space-sm); }
.segment-row { display: flex; flex-direction: column; gap: 2px; padding-bottom: var(--space-xs); border-bottom: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 25%, transparent); }
.segment-original { margin: 0; color: var(--ink); font-size: 12px; line-height: 1.6; overflow-wrap: anywhere; }
.segment-translated { margin: 0; color: var(--md-sys-color-primary); font-size: 12px; line-height: 1.6; overflow-wrap: anywhere; }
.segment-untranslated { margin: 0; color: var(--ink-subtle); font-size: 12px; line-height: 1.6; }
.context-line { margin: 0; color: var(--ink-subtle); font-size: 11px; line-height: 1.5; overflow-wrap: anywhere; }

/* 问题 */
.issue-list { display: flex; flex-direction: column; gap: var(--space-xs); margin: 0; padding: 0; list-style: none; }
.issue-row { display: flex; flex-direction: column; gap: 2px; padding: var(--space-sm); border-radius: var(--radius-control); background: var(--surface); }
.issue-head { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-xs); }
.issue-severity { padding: 0 var(--space-xs); border-radius: 999px; background: var(--md-sys-color-surface-container); color: var(--ink-muted); font-size: 11px; line-height: 1.8; }
.issue-severity.is-critical, .issue-severity.is-major { color: var(--md-sys-color-error); }
.issue-chunk, .issue-segment { color: var(--ink-muted); font-size: 11px; }
.issue-detail { margin: 0; color: var(--ink); font-size: 12px; line-height: 1.5; overflow-wrap: anywhere; }

/* 底部动作常驻：紧凑分段控制器 + 规范 34px 按钮 */
.panel-actions {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: var(--space-xs);
  padding: var(--space-sm) var(--space-md) var(--space-md);
  border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 25%, transparent);
}
.translation-modes {
  display: flex;
  align-items: center;
  background: color-mix(in srgb, var(--md-sys-color-surface-container-highest) 60%, transparent);
  border-radius: 999px;
  padding: 3px;
  gap: 2px;
}
.translation-mode {
  flex: 1;
  min-width: 0;
  height: 28px;
  padding: 0 var(--space-xs);
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: var(--ink-muted);
  font: inherit;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 160ms cubic-bezier(0.2, 0, 0, 1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.translation-mode:hover:not(.selected) {
  color: var(--ink);
  background: color-mix(in srgb, var(--md-sys-color-on-surface) 6%, transparent);
}
.translation-mode:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 1px; }
.translation-mode.selected {
  background: var(--md-sys-color-surface);
  color: var(--md-sys-color-primary);
  font-weight: 600;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12);
}
.action-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-xs);
  margin-top: 2px;
  --md-filled-button-container-height: 34px;
  --md-outlined-button-container-height: 34px;
  --md-text-button-container-height: 34px;
}
.action-row md-filled-button,
.action-row md-outlined-button,
.action-row md-text-button {
  height: 34px;
}
.cancel-button { --md-text-button-label-text-color: var(--md-sys-color-error); margin-inline-start: auto; }

@media (prefers-reduced-motion: reduce) {
  .panel-tab, .translation-mode, .chunk-item { transition: none; }
}
</style>
