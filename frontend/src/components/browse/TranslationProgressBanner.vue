<script setup lang="ts">
/**
 * 浮层「状态」tab 的进度区（整篇 Job 版）。
 *
 * 数据全部来自 store 的 Job 快照：阶段、计数、当前页/块、实时预览、已用与用量。
 * 进度条在块数未知（prepare_source）时保持 indeterminate；终态按计数收敛。
 * 本区整体不进 aria-live——状态摘要由浮层头部唯一播报，避免逐帧刷屏。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { isActiveJobState, type TranslationJobSnapshot } from "../../api/translation";
import { useTranslationStore } from "../../stores/translation";

const props = defineProps<{ job: TranslationJobSnapshot }>();
const { t } = useI18n();
const store = useTranslationStore();

/** 阶段文案（prepare_source … publish）。 */
const phaseText = computed(() => t(`translation.job.phase.${props.job.phase}`));

/** 计数：已译 / 已审 / 待修 / 总块；块数未知时只显示页数。 */
const countsText = computed(() => {
  const { chunks, translated, reviewed, needs_repair: repair } = props.job.counts;
  if (chunks <= 0) return t("translation.job.countsPending");
  return t("translation.job.counts", { translated, reviewed, repair, chunks });
});

/** 进度比例（0~1）：块数未知为 0；终态完成按 1 收敛。 */
const progress = computed(() => {
  const { chunks, translated } = props.job.counts;
  if (chunks <= 0) return 0;
  if (props.job.state === "completed") return 1;
  return Math.min(1, Math.max(0, translated / chunks));
});

/** 块数未知且任务仍活跃时用不定进度条（准备原文阶段）。 */
const indeterminate = computed(() => isActiveJobState(props.job.state) && props.job.counts.chunks <= 0);

/** 当前工作位置：有页码才渲染（准备阶段可能只有块或为空）。 */
const currentText = computed(() => {
  const current = props.job.current;
  if (!current || (current.page == null && !current.chunk_id)) return "";
  if (current.page != null && current.chunk_id) return t("translation.job.currentBoth", { page: current.page, chunk: current.chunk_id });
  if (current.page != null) return t("translation.job.currentPage", { page: current.page });
  return t("translation.job.currentChunk", { chunk: current.chunk_id ?? "" });
});

/** 已用时间：m′ss″；活跃任务每秒自走。 */
const elapsedText = computed(() => {
  const total = Math.max(0, Math.floor(store.elapsedFor(props.job) / 1000));
  return `${Math.floor(total / 60)}′${String(total % 60).padStart(2, "0")}″`;
});

const usageText = computed(() => {
  const total = props.job.usage?.total ?? 0;
  return total > 0 ? t("translation.job.usage", { tokens: total.toLocaleString() }) : "";
});

/** 预览文本：正文摘要优先，否则思考尾窗；都按 60 字符截断。 */
function clip(text: string): string {
  const value = text.trim();
  return value.length > 60 ? `${value.slice(0, 60)}…` : value;
}
const summaryLine = computed(() => clip(props.job.preview?.summary ?? ""));
const reasoningLine = computed(() => (summaryLine.value ? "" : clip(props.job.preview?.reasoning ?? "")));
const previewLine = computed(() => summaryLine.value || reasoningLine.value);
/** 「后台继续」只属于仍在推进的任务；完成/暂停/失败不应显示这句提示。 */
const backgroundHint = computed(() =>
  !previewLine.value && isActiveJobState(props.job.state) ? t("translation.progressBackgroundHint") : "",
);

// ===== 摘要 marquee：仅单行溢出时滚动（思考态静态省略） =====

const summaryEl = ref<HTMLElement | null>(null);
const textEl = ref<HTMLElement | null>(null);
const overflow = ref(false);

function measureOverflow(): void {
  const box = summaryEl.value;
  const text = textEl.value;
  if (!box || !text) return;
  overflow.value = text.offsetWidth > box.clientWidth;
}

const motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
function onMotionChange(): void { void nextTick(measureOverflow); }
watch(previewLine, () => void nextTick(measureOverflow));
onMounted(() => {
  measureOverflow();
  window.addEventListener("resize", measureOverflow);
  motionQuery.addEventListener("change", onMotionChange);
});
onBeforeUnmount(() => {
  window.removeEventListener("resize", measureOverflow);
  motionQuery.removeEventListener("change", onMotionChange);
});
</script>

<template>
  <div class="translation-progress">
    <md-linear-progress
      class="progress-bar"
      :value="indeterminate ? 0 : progress"
      :indeterminate="indeterminate"
      aria-hidden="true"
    />
    <div class="progress-meta">
      <span class="progress-phase">{{ phaseText }}</span>
      <span class="progress-sep" aria-hidden="true">·</span>
      <span class="progress-counts">{{ countsText }}</span>
    </div>
    <div class="progress-meta">
      <span class="progress-elapsed">{{ t("translation.job.elapsed", { time: elapsedText }) }}</span>
      <template v-if="usageText">
        <span class="progress-sep" aria-hidden="true">·</span>
        <span class="progress-usage">{{ usageText }}</span>
      </template>
    </div>
    <span v-if="currentText" class="progress-current">{{ currentText }}</span>
    <code
      v-if="reasoningLine"
      class="progress-reasoning"
      aria-hidden="true"
      :title="reasoningLine"
    >
      <span class="progress-reasoning-label">{{ t("translation.progressReasoning") }}</span>{{ reasoningLine }}
    </code>
    <code
      v-else
      ref="summaryEl"
      class="progress-summary"
      :class="{ 'is-overflow': overflow }"
      aria-hidden="true"
      :title="previewLine"
    >
      <span class="progress-summary-track">
        <span ref="textEl" class="progress-summary-text">{{ previewLine || backgroundHint }}</span>
        <span v-if="overflow" class="progress-summary-text progress-summary-clone" aria-hidden="true">{{ previewLine }}</span>
      </span>
    </code>
  </div>
</template>

<style scoped>
/* 只管排布与间距，无容器边框 / 背景 / 内外边距（浮窗自身即 surface-container） */
.translation-progress {
  display: flex;
  flex-direction: column;
  gap: var(--space-xs);
  min-width: 0;
}

.progress-bar {
  width: 100%;
}

.progress-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-xs);
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.5;
}

.progress-current {
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.progress-reasoning {
  display: block;
  min-width: 0;
  overflow: hidden;
  color: var(--ink-subtle);
  font-size: 12px;
  line-height: 1.5;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.progress-reasoning-label {
  margin-right: var(--space-xs);
  font-weight: 600;
}

.progress-sep {
  color: var(--ink-subtle);
}

.progress-summary {
  display: block;
  min-width: 0;
  overflow: hidden;
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.5;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.progress-summary-track {
  display: flex;
  width: max-content;
  gap: var(--space-xl);
}

.progress-summary-text {
  flex: none;
  white-space: nowrap;
}

.progress-summary.is-overflow .progress-summary-track {
  animation: mq-scroll 12s linear infinite;
}

@keyframes mq-scroll {
  from { transform: translateX(0); }
  to { transform: translateX(calc(-50% - var(--space-xl) / 2)); }
}

@media (prefers-reduced-motion: reduce) {
  .progress-summary-track { display: block; width: auto; animation: none; }
  .progress-summary-text { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .progress-summary-clone { display: none; }
}
</style>
