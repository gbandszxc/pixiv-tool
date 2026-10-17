<script setup lang="ts">
import { computed } from "vue";
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
</script>

<template>
  <!-- 页内进度带：阶段 + 已用时间 + 已接收 + 一行流式摘要；store.active 由全局翻译任务驱动，卸载不中断 -->
  <div class="translation-progress">
    <!-- 状态行：组件内唯一播报区（role=status，沿用 NovelTranslationMenu 配方）。
         只挂阶段驱动的文案；秒级 elapsed 与字节数不进入播报区，避免朗读刷屏。 -->
    <div class="progress-status-line">
      <span class="progress-indicator" aria-hidden="true"></span>
      <span class="progress-status-text" role="status" aria-live="polite">
        {{ t("translation.translating", { page: active.page }) }} · {{ phaseText }}
      </span>
      <span class="progress-meta">
        <span class="progress-elapsed">{{ t("translation.progressElapsed", { time: elapsedText }) }}</span>
        <span class="progress-sep" aria-hidden="true">·</span>
        <span class="progress-received">{{ t("translation.progressReceived", { size: receivedText }) }}</span>
      </span>
    </div>
    <md-linear-progress class="progress-bar" :indeterminate="true" aria-hidden="true" />
    <!-- 摘要行：定宽 ellipsis（禁跑马灯/自动滚动），等宽字体沿用 CopyPathButton 的 <code> 先例；aria-hidden 不参与播报 -->
    <code class="progress-summary" aria-hidden="true" :title="active.summary || undefined">{{ summaryLine }}</code>
  </div>
</template>

<style scoped>
.translation-progress {
  display: flex;
  flex-direction: column;
  gap: var(--space-sm);
  box-sizing: border-box;
  margin-bottom: var(--space-lg);
  padding: var(--space-md) var(--space-lg);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent);
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  text-align: left;
}

/* 状态行：窄窗允许换行（flex-wrap），不横向溢出 */
.progress-status-line {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-xs);
  font-size: 13px;
  line-height: 1.5;
}

/* 状态点：进行中 primary，配方同 NovelTranslationMenu 的 translation-indicator busy */
.progress-indicator {
  flex: none;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--md-sys-color-primary);
}

.progress-status-text {
  color: var(--ink);
}

.progress-meta {
  display: inline-flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-xs);
  color: var(--ink-muted);
}

.progress-sep {
  color: var(--ink-subtle);
}

.progress-bar {
  width: 100%;
}

/* 摘要行：<code> 取浏览器默认等宽字体（CopyPathButton 先例），12px ink-muted + ellipsis */
.progress-summary {
  display: block;
  overflow: hidden;
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.5;
  white-space: nowrap;
  text-overflow: ellipsis;
}
</style>
