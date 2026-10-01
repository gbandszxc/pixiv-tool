<script setup lang="ts">
/**
 * 图片舞台：近黑底大图查看（object-contain 居中，深浅色主题一致的中性深底）。
 * - 多页：点击舞台左/右区域或下方 ‹ › 按钮翻页（键盘 ←/→ 由父视图统一处理）；
 * - 档位：主图 thumb_quality_detail（默认 medium = 接口 regular 原样，URL 与改造前逐字一致），
 *   先铺 medium 档（540px）占位层再换高清；全屏浮层走 thumb_quality_fullscreen；
 * - 加载中纯色占位、失败显示重试；相邻页 new Image() 预加载；
 * - R-18 遮罩：blur(24px) + 中央文案 + 「显示」按钮（是否遮罩由父视图决定），
 *   遮罩期间不渲染未模糊占位层、不可进入全屏浮层；
 * - ugoira：仅显示封面帧 + 说明行（V1 不做帧动画）。
 */
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { pxSrc, thumbSrc, type ThumbTier } from "../../api/browse";
import { useThumbTier } from "../../composables/useThumbTier";

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

const detailTier = useThumbTier("thumb_quality_detail");
const fullscreenTier = useThumbTier("thumb_quality_fullscreen");

/** 页图片地址：medium 档 = 接口 regular 原样（不插 /c/，与改造前逐字一致）。 */
function pageSrc(p: { medium?: string; original: string }, tier: ThumbTier): string {
  if (tier === "original") return pxSrc(p.original);
  if (tier === "large") return thumbSrc(p.medium ?? p.original, "large");
  return p.medium ? pxSrc(p.medium) : pxSrc(p.original);
}

const src = computed(() => {
  const p = props.pages[props.page];
  return p ? pageSrc(p, detailTier.value) : "";
});

/** 低清占位层：接口给了 medium 且目标不是同一地址时才叠加（medium 档自身不占位）。 */
function lowResSrc(target: string): string {
  const p = props.pages[props.page];
  if (!p?.medium) return "";
  const low = thumbSrc(p.medium, "medium");
  return low && low !== target ? low : "";
}

const placeholderSrc = computed(() => lowResSrc(src.value));

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

// ===== 全屏浮层 =====

const fullscreen = ref(false);
const fsState = ref<"loading" | "ok" | "error">("loading");
/** 全屏档不可用时回落到已加载的主图；只回落一次，避免与 @error 循环。 */
const fsFallback = ref(false);
const triggerEl = ref<HTMLElement | null>(null);
const overlayEl = ref<HTMLElement | null>(null);

const fullscreenSrc = computed(() => {
  const p = props.pages[props.page];
  if (!p) return "";
  return fsFallback.value ? src.value : pageSrc(p, fullscreenTier.value);
});

const fullscreenPlaceholderSrc = computed(() => lowResSrc(fullscreenSrc.value));

watch(fullscreenSrc, () => {
  fsState.value = "loading";
});

function onFullscreenError(): void {
  if (fsFallback.value) {
    fsState.value = "error";
    return;
  }
  fsFallback.value = true;
}

/** 进入全屏：主图就绪且未处于 R-18 遮罩时才可用（遮罩不可被绕过）。 */
function openFullscreen(): void {
  if (imgState.value !== "ok" || props.restricted) return;
  fsFallback.value = false;
  fsState.value = "loading";
  fullscreen.value = true;
}

function closeFullscreen(): void {
  fullscreen.value = false;
  triggerEl.value?.focus();
}

/**
 * 浮层键盘：Esc 关闭、←/→ 翻页。capture 阶段监听并 stopImmediatePropagation，
 * 阻止 BrowseWorkView 的 bubble 监听把同一次 Esc 变成路由返回。
 */
function onFullscreenKeydown(e: KeyboardEvent): void {
  if (!fullscreen.value) return;
  if (e.key === "Escape") {
    e.preventDefault();
    e.stopImmediatePropagation();
    closeFullscreen();
  } else if (e.key === "ArrowLeft") {
    e.preventDefault();
    e.stopImmediatePropagation();
    emit("prev");
  } else if (e.key === "ArrowRight") {
    e.preventDefault();
    e.stopImmediatePropagation();
    emit("next");
  }
}

watch(fullscreen, async (open) => {
  if (open) {
    window.addEventListener("keydown", onFullscreenKeydown, { capture: true });
    await nextTick();
    overlayEl.value?.focus();
  } else {
    window.removeEventListener("keydown", onFullscreenKeydown, { capture: true });
  }
});

onBeforeUnmount(() => window.removeEventListener("keydown", onFullscreenKeydown, { capture: true }));

/** 预加载相邻页：翻页时立即可见（fire-and-forget），与主图同一档位。 */
watch(
  () => [props.page, props.pages, detailTier.value] as const,
  () => {
    for (const i of [props.page - 1, props.page + 1]) {
      const p = props.pages[i];
      if (!p) continue;
      const img = new Image();
      img.src = pageSrc(p, detailTier.value);
    }
  },
  { immediate: true }
);
</script>

<template>
  <div class="viewer">
    <div class="stage">
      <!-- 加载占位 / 失败重试（覆盖层；img 常驻渲染，避免「等 load 才渲染 img」的死锁） -->
      <!-- 先低清后高清：540 占位层在主图之下，主图就绪后即被覆盖；遮罩期间不渲染 -->
      <img
        v-if="placeholderSrc && !restricted"
        class="stage-img placeholder"
        :src="placeholderSrc"
        alt=""
        aria-hidden="true"
        decoding="async"
      />
      <img
        v-show="imgState === 'ok' || restricted"
        :key="`${src}#${retryTick}`"
        class="stage-img"
        :class="{ blurred: restricted }"
        :src="src"
        :alt="alt"
        fetchpriority="high"
        decoding="async"
        @load="onImgLoad"
        @error="onImgError"
      />
      <div v-if="imgState !== 'ok' && !restricted" class="stage-state">
        <div v-if="imgState === 'loading' && !placeholderSrc" class="loading-block" aria-hidden="true"></div>
        <template v-else-if="imgState === 'error'">
          <p class="stage-text">{{ t("common.browseLoadFailed") }}</p>
          <md-outlined-button @click="retry">{{ t("common.retry") }}</md-outlined-button>
        </template>
      </div>

      <!-- 全屏入口：主图就绪且未遮罩时才可点（浮层不做二次揭示） -->
      <md-icon-button
        ref="triggerEl"
        class="fs-trigger"
        :disabled="imgState !== 'ok' || restricted"
        :aria-label="t('browse.work.fullscreen')"
        :title="t('browse.work.fullscreen')"
        @click="openFullscreen"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M4 9V4h5" /><path d="M20 9V4h-5" /><path d="M4 15v5h5" /><path d="M20 15v5h-5" />
        </svg>
      </md-icon-button>

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

    <!-- 全屏浮层：自带 ‹/› 与 ✕（不复用 .stage-footer，后者会被 inset:0 浮层盖住） -->
    <div
      v-if="fullscreen"
      ref="overlayEl"
      class="fs-overlay"
      role="dialog"
      aria-modal="true"
      :aria-label="t('browse.work.fullscreen')"
      tabindex="-1"
      @click.self="closeFullscreen"
    >
      <img
        v-if="fullscreenPlaceholderSrc"
        class="fs-img placeholder"
        :src="fullscreenPlaceholderSrc"
        alt=""
        aria-hidden="true"
        decoding="async"
      />
      <img
        v-show="fsState === 'ok'"
        class="fs-img"
        :src="fullscreenSrc"
        :alt="alt"
        decoding="async"
        @load="fsState = 'ok'"
        @error="onFullscreenError"
      />
      <div v-if="fsState !== 'ok' && !fullscreenPlaceholderSrc" class="fs-state">
        <div v-if="fsState === 'loading'" class="loading-block" aria-hidden="true"></div>
        <p v-else class="stage-text">{{ t("common.browseLoadFailed") }}</p>
      </div>
      <div v-if="multi" class="fs-controls">
        <md-icon-button :disabled="!canPrev" :aria-label="t('browse.work.prevPage')" :title="t('browse.work.prevPage')" @click="emit('prev')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="15 18 9 12 15 6" /></svg>
        </md-icon-button>
        <span class="fs-label" aria-live="polite">{{ t("browse.work.pageOf", { current: page + 1, total: pages.length }) }}</span>
        <md-icon-button :disabled="!canNext" :aria-label="t('browse.work.nextPage')" :title="t('browse.work.nextPage')" @click="emit('next')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="9 18 15 12 9 6" /></svg>
        </md-icon-button>
      </div>
      <md-icon-button
        class="fs-close"
        :aria-label="t('browse.work.exitFullscreen')"
        :title="t('browse.work.exitFullscreen')"
        @click="closeFullscreen"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <line x1="6" y1="6" x2="18" y2="18" /><line x1="18" y1="6" x2="6" y2="18" />
        </svg>
      </md-icon-button>
    </div>
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

/* 主图与占位层同为定位元素：靠 DOM 顺序（占位在前）保证主图压在其上 */
.stage-img {
  position: relative;
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}

/* 540 低清占位层：主图就绪前可见，就绪后被主图覆盖 */
.stage-img.placeholder {
  position: absolute;
  inset: 0;
  margin: auto;
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

/* ===== 全屏浮层 ===== */

.fs-overlay {
  position: fixed;
  inset: 0;
  /* 层级沿用应用约定：页面级浮层 800 < 抽屉 900 < snackbar 1000 */
  z-index: 800;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-lg);
  /* 复用舞台近黑底（不新增颜色字面值） */
  background: rgb(0 0 0 / 0.78);
  outline: none;
}

.fs-img {
  position: relative;
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}

.fs-img.placeholder {
  position: absolute;
  inset: 0;
  margin: auto;
}

.fs-state {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}

.fs-controls {
  position: absolute;
  left: 50%;
  bottom: var(--space-lg);
  display: flex;
  align-items: center;
  gap: var(--space-sm);
  transform: translateX(-50%);
}

.fs-label {
  min-width: 96px;
  color: rgb(255 255 255 / 0.85);
  font-size: 12px;
  font-weight: 600;
  text-align: center;
}

.fs-close {
  position: absolute;
  top: var(--space-lg);
  right: var(--space-lg);
}

/* 近黑底上的图标按钮统一白色图标（沿用 .stage-text 的白） */
.fs-trigger,
.fs-controls md-icon-button,
.fs-close {
  --md-icon-button-icon-color: #fff;
  --md-icon-button-hover-icon-color: #fff;
  --md-icon-button-focus-icon-color: #fff;
  --md-icon-button-pressed-icon-color: #fff;
  --md-icon-button-disabled-icon-color: #fff;
}

/* 全屏入口：封面右上角，位于遮罩（z-index 2）之下 */
.fs-trigger {
  position: absolute;
  top: var(--space-sm);
  right: var(--space-sm);
  z-index: 1;
}

.fs-trigger svg,
.fs-controls svg {
  width: 20px;
  height: 20px;
  stroke-width: 1.8;
}
</style>
