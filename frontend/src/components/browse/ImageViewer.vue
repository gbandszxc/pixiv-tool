<script setup lang="ts">
/**
 * 图片舞台：近黑底大图查看（object-contain 居中，深浅色主题一致的中性深底）。
 * - 多页：点击舞台左/右区域或下方 ‹ › 按钮翻页（键盘 ←/→ 由父视图统一处理）；
 * - 加载中纯色占位、失败显示重试；相邻页 new Image() 预加载；
 * - R-18 遮罩：blur(24px) + 中央文案 + 「显示」按钮（是否遮罩由父视图决定）；
 * - ugoira：仅显示封面帧 + 说明行（V1 不做帧动画）。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { pxSrc } from "../../api/browse";

const props = withDefaults(
  defineProps<{
    /** 各页图片地址（pximg 原始 URL，内部经 pxSrc() 走代理） */
    pages: { medium?: string; original: string }[];
    /** 当前页下标（0 起；状态由父视图持有，配合键盘翻页） */
    page: number;
    alt?: string;
    /** 是否显示翻页交互（多页且非 ugoira） */
    multi?: boolean;
    /** R-18 遮罩态（父视图算好，含会话记忆） */
    restricted?: boolean;
    /** 遮罩文案（R-18 / R-18G 区分） */
    restrictLabel?: string;
    /** ugoira 说明行 */
    ugoira?: boolean;
  }>(),
  { alt: "", multi: false, restricted: false, restrictLabel: "", ugoira: false }
);

const emit = defineEmits<{
  (e: "reveal"): void;
  (e: "prev"): void;
  (e: "next"): void;
}>();

const { t } = useI18n();

/** 重试计数：作为 <img :key> 的一部分，重挂载以重新发起加载。 */
const retryTick = ref(0);
const imgState = ref<"loading" | "ok" | "error">("loading");

const src = computed(() => {
  const p = props.pages[props.page];
  return p ? pxSrc(p.medium || p.original) : "";
});

const canPrev = computed(() => props.multi && props.page > 0);
const canNext = computed(() => props.multi && props.page < props.pages.length - 1);

watch(src, () => {
  imgState.value = "loading";
});

function onImgLoad(): void {
  imgState.value = "ok";
}

function onImgError(): void {
  imgState.value = "error";
}

/** 加载失败重试：同地址重挂载 <img> 触发重新请求。 */
function retry(): void {
  retryTick.value += 1;
  imgState.value = "loading";
}

/** 预加载相邻页：翻页时立即可见（fire-and-forget）。 */
watch(
  () => [props.page, props.pages] as const,
  () => {
    for (const i of [props.page - 1, props.page + 1]) {
      const p = props.pages[i];
      if (!p) continue;
      const img = new Image();
      img.src = pxSrc(p.medium || p.original);
    }
  },
  { immediate: true }
);
</script>

<template>
  <div class="viewer">
    <div class="stage">
      <!-- 加载占位 / 失败重试（遮罩态下图片照常加载，覆盖层负责呈现） -->
      <div v-if="imgState !== 'ok' && !restricted" class="stage-state">
        <div v-if="imgState === 'loading'" class="loading-block" aria-hidden="true"></div>
        <template v-else>
          <p class="stage-text">{{ t("common.browseLoadFailed") }}</p>
          <md-outlined-button @click="retry">{{ t("common.retry") }}</md-outlined-button>
        </template>
      </div>
      <img
        v-else
        :key="`${src}#${retryTick}`"
        class="stage-img"
        :class="{ blurred: restricted }"
        :src="src"
        :alt="alt"
        @load="onImgLoad"
        @error="onImgError"
      />

      <!-- R-18 遮罩 -->
      <div v-if="restricted" class="restrict-overlay">
        <p class="stage-text strong">{{ restrictLabel || t("browse.work.restrictedTitle") }}</p>
        <md-filled-button @click="emit('reveal')">{{ t("browse.work.show") }}</md-filled-button>
      </div>

      <!-- 点击翻页区（指针辅助；键盘走 ←/→ 与下方按钮） -->
      <template v-else-if="multi">
        <button v-if="canPrev" class="zone zone-left" type="button" tabindex="-1" aria-hidden="true" @click="emit('prev')"></button>
        <button v-if="canNext" class="zone zone-right" type="button" tabindex="-1" aria-hidden="true" @click="emit('next')"></button>
      </template>
    </div>

    <!-- 翻页控件（多页） -->
    <div v-if="multi" class="stage-footer">
      <md-icon-button :disabled="!canPrev" :aria-label="t('browse.work.prevPage')" :title="t('browse.work.prevPage')" @click="emit('prev')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="15 18 9 12 15 6" /></svg>
      </md-icon-button>
      <span class="page-label" aria-live="polite">{{ t("browse.work.pageOf", { current: page + 1, total: pages.length }) }}</span>
      <md-icon-button :disabled="!canNext" :aria-label="t('browse.work.nextPage')" :title="t('browse.work.nextPage')" @click="emit('next')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="9 18 15 12 9 6" /></svg>
      </md-icon-button>
    </div>

    <!-- ugoira 说明行 -->
    <p v-if="ugoira" class="ugoira-note">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="9" />
        <polygon points="10 8.5 16 12 10 15.5" fill="currentColor" stroke="none" />
      </svg>
      {{ t("browse.work.ugoiraNote") }}
    </p>
  </div>
</template>

<style scoped>
.viewer {
  display: flex;
  flex-direction: column;
  gap: var(--space-sm);
  height: 100%;
  min-height: 0;
}

/* 近黑底：中性深色，深浅色主题下一致（主会话既定决策） */
.stage {
  position: relative;
  display: flex;
  flex: 1;
  min-height: 320px;
  align-items: center;
  justify-content: center;
  border-radius: 16px;
  background: rgb(0 0 0 / 0.78);
  overflow: hidden;
}

.stage-img {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}

.stage-img.blurred {
  filter: blur(24px);
  /* 放大避免 blur 边缘透出底色 */
  transform: scale(1.08);
}

.stage-state {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-md);
}

.loading-block {
  width: 200px;
  height: 200px;
  border-radius: var(--radius-control);
  background: rgb(255 255 255 / 0.08);
}

.stage-text {
  margin: 0;
  color: rgb(255 255 255 / 0.85);
  font-size: 14px;
  line-height: 1.5;
}

.stage-text.strong {
  color: #fff;
  font-weight: 600;
}

.restrict-overlay {
  position: absolute;
  inset: 0;
  z-index: 2;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-md);
  background: rgb(0 0 0 / 0.4);
}

.zone {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 42%;
  padding: 0;
  border: none;
  background: transparent;
  cursor: pointer;
}

.zone-left {
  left: 0;
}

.zone-right {
  right: 0;
}

.stage-footer {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-sm);
}

.stage-footer svg {
  width: 20px;
  height: 20px;
  stroke-width: 1.8;
}

.page-label {
  min-width: 96px;
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
  text-align: center;
}

.ugoira-note {
  display: flex;
  align-items: center;
  gap: var(--space-xs);
  margin: 0;
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
}

.ugoira-note svg {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}
</style>
