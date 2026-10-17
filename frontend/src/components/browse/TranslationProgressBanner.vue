<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { TranslationActive } from "../../stores/translation";

const props = defineProps<{ active: TranslationActive }>();
const { t } = useI18n();

/** 阶段文案：queued 排队等待 / prepare 整理设定集（Pass 1）/ translate 翻译正文（Pass 2）。 */
const phaseText = computed(() => {
  switch (props.active.phase) {
    case "queued":
      return t("translation.progressQueued");
    case "prepare":
      return t("translation.progressPrepare");
    default:
      return t("translation.progressTranslate");
  }
});

/** 已用时间：分′秒″（1′42″），秒补零；时钟回拨不出现负值。 */
const elapsedText = computed(() => {
  const total = Math.max(0, Math.floor(props.active.elapsed / 1000));
  return `${Math.floor(total / 60)}′${String(total % 60).padStart(2, "0")}″`;
});

/** 已接收体积：≥1024 转 KB（一位小数），否则按 B 取整。 */
const receivedText = computed(() => {
  const bytes = Math.max(0, props.active.received);
  return bytes >= 1024 ? `${(bytes / 1024).toFixed(1)} KB` : `${Math.round(bytes)} B`;
});

/** 摘要行：有流式摘要显示摘要；排队 / Pass 1 阶段恒空串，降级显示后台安抚文案。 */
const summaryLine = computed(() => props.active.summary || t("translation.progressBackgroundHint"));

// ===== 摘要 marquee：仅单行溢出时滚动 =====

/** clip 盒（<code>）：clientWidth 即浮窗内容区可用宽度，溢出内容由其裁掉。 */
const summaryEl = ref<HTMLElement | null>(null);
/** 首份摘要文本：offsetWidth 即摘要自然宽度，与 clip 盒宽度比较判定是否溢出。 */
const textEl = ref<HTMLElement | null>(null);
const overflow = ref(false);

function measureOverflow() {
  const box = summaryEl.value;
  const text = textEl.value;
  if (!box || !text) return;
  overflow.value = text.offsetWidth > box.clientWidth;
}

// 摘要文案逐行刷新、面板宽度变化（窗口 resize / 窄窗 max-width）或减少动态效果偏好切换后重新判定。
function onResize() { measureOverflow(); }
const motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
function onMotionChange() { void nextTick(measureOverflow); }

watch(summaryLine, () => void nextTick(measureOverflow));
onMounted(() => {
  measureOverflow();
  window.addEventListener("resize", onResize);
  motionQuery.addEventListener("change", onMotionChange);
});
onBeforeUnmount(() => {
  window.removeEventListener("resize", onResize);
  motionQuery.removeEventListener("change", onMotionChange);
});
</script>

<template>
  <!-- 浮窗内嵌紧凑进度区（NovelTranslationMenu 状态行下方、模式组上方）：不定进度条 + 阶段/已用/已接收 + 一行流式摘要。
       无自有容器边框/背景/内外边距——浮窗自身即 surface-container；播报由浮窗状态行负责，本区整体不进 aria-live。 -->
  <div class="translation-progress">
    <md-linear-progress class="progress-bar" :indeterminate="true" aria-hidden="true" />
    <!-- meta 行：阶段 + 已用 + 已接收，12px ink-muted；窄窗允许换行，不横向溢出 -->
    <div class="progress-meta">
      <span class="progress-phase">{{ phaseText }}</span>
      <span class="progress-sep" aria-hidden="true">·</span>
      <span class="progress-elapsed">{{ t("translation.progressElapsed", { time: elapsedText }) }}</span>
      <span class="progress-sep" aria-hidden="true">·</span>
      <span class="progress-received">{{ t("translation.progressReceived", { size: receivedText }) }}</span>
    </div>
    <!-- 摘要行：单行 nowrap；JS 判定溢出才挂双份内容无缝滚动（mq-scroll），不溢出静态全显；
         reduced-motion 下降级 animation:none + 静态 ellipsis；整行 aria-hidden，全文经 title 悬停查看 -->
    <code
      ref="summaryEl"
      class="progress-summary"
      :class="{ 'is-overflow': overflow }"
      aria-hidden="true"
      :title="summaryLine"
    >
      <span class="progress-summary-track">
        <span ref="textEl" class="progress-summary-text">{{ summaryLine }}</span>
        <span v-if="overflow" class="progress-summary-text progress-summary-clone" aria-hidden="true">{{ summaryLine }}</span>
      </span>
    </code>
  </div>
</template>

<style scoped>
/* 内嵌进度区：只管排布与间距，无容器边框/背景/内外边距（浮窗自身即 surface-container + 合法轻阴影） */
.translation-progress {
  display: flex;
  flex-direction: column;
  gap: var(--space-sm);
  min-width: 0;
}

.progress-bar {
  width: 100%;
}

/* meta 行：12px ink-muted，窄窗换行不溢出 */
.progress-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-xs);
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.5;
}

.progress-sep {
  color: var(--ink-subtle);
}

/* 摘要行：clip 盒，单行 nowrap；静态态（未溢出 / reduced-motion）由 ellipsis 收尾 */
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

/* 轨道：首份 + 溢出时的克隆份等宽并排，flex gap 即循环间隔；溢出时整体左移一份实现无缝循环 */
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

/* reduced-motion：停止滚动，克隆份退场，单份静态 ellipsis */
@media (prefers-reduced-motion: reduce) {
  .progress-summary-track { display: block; width: auto; animation: none; }
  .progress-summary-text { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .progress-summary-clone { display: none; }
}
</style>
