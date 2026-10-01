<script setup lang="ts">
/**
 * 图片舞台：近黑底纵向滚动查看（深浅色主题一致的中性深底）。
 * - 多页：自上而下逐页排列，滚动到视口附近才发起加载（渐进式）；未加载页为按该页
 *   `width`/`height` 预留纵横比的纯色占位块（缺省 2:3），加载完成不产生跳动；
 * - 点击任意页进入全屏浮层并定位到该页，左右切换只发生在浮层内（‹ › / 键盘 ←/→ 与
 *   右侧胶卷缩略图）；上一页/下一页的滚动对齐由舞台内的 ←/→ 完成；
 * - 档位：主图 thumb_quality_detail（默认 medium = 接口 regular 原样，URL 与改造前逐字一致），
 *   先铺 medium 档（540px）占位层再换高清；全屏浮层走 thumb_quality_fullscreen，
 *   胶卷缩略图走 thumb_quality_grid；
 * - 键盘：本组件在 capture 阶段监听并 stopImmediatePropagation，避免与父视图的同名
 *   按键（Esc 返回）二次处理；浮层内 Esc 只关浮层；
 * - 加载中纯色占位、失败显示重试；
 * - R-18 遮罩：blur(24px) + 中央文案 + 「显示」按钮（是否遮罩由父视图决定），
 *   遮罩期间只加载首页、不可进入全屏浮层；
 * - ugoira：仅显示封面帧 + 说明行（V1 不做帧动画）。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { pxSrc, thumbSrc, type ThumbTier } from "../../api/browse";
import { useThumbTier } from "../../composables/useThumbTier";

/** 页加载态；未进入加载窗口的页不渲染 <img>，保持占位块。 */
type PageState = "loading" | "ok" | "error";

const props = withDefaults(
  defineProps<{
    /** 各页图片地址与原始尺寸（pximg 原始 URL，内部经 pxSrc() 走代理） */
    pages: { medium?: string; original: string; width?: number; height?: number }[];
    /** 图片替代文本（作品标题）；多页时自动追加页码 */
    alt?: string;
    /** R-18 遮罩态（父视图算好，含设置开关与会话记忆） */
    restricted?: boolean;
    /** 遮罩文案（R-18 / R-18G 区分） */
    restrictLabel?: string;
    /** ugoira 说明行 */
    ugoira?: boolean;
  }>(),
  { alt: "", restricted: false, restrictLabel: "", ugoira: false }
);

const emit = defineEmits<{
  (e: "reveal"): void;
}>();

const { t } = useI18n();

const detailTier = useThumbTier("thumb_quality_detail");
const fullscreenTier = useThumbTier("thumb_quality_fullscreen");
const gridTier = useThumbTier("thumb_quality_grid");

const multi = computed(() => props.pages.length > 1 && !props.ugoira);

/** 页图片地址：medium 档 = 接口 regular 原样（不插 /c/，与改造前逐字一致）。 */
function pageSrc(p: { medium?: string; original: string }, tier: ThumbTier): string {
  if (tier === "original") return pxSrc(p.original);
  if (tier === "large") return thumbSrc(p.medium ?? p.original, "large");
  return p.medium ? pxSrc(p.medium) : pxSrc(p.original);
}

function altOf(index: number): string {
  return multi.value ? t("browse.work.imageAlt", { title: props.alt, page: index + 1 }) : props.alt;
}

/** 舞台主图（详情档）。 */
function stageSrc(index: number): string {
  const p = props.pages[index];
  return p ? pageSrc(p, detailTier.value) : "";
}

/** 低清占位层：接口给了 medium 且与主图不是同一地址时才叠加（medium 档自身不占位）。 */
function lowSrc(index: number): string {
  const p = props.pages[index];
  if (!p?.medium) return "";
  const low = thumbSrc(p.medium, "medium");
  return low && low !== stageSrc(index) ? low : "";
}

// ===== 渐进加载 =====

const scrollEl = ref<HTMLElement | null>(null);
/** 页元素引用（下标与 pages 对齐） */
const itemEls: (HTMLElement | null)[] = [];

function setItemRef(index: number, el: unknown): void {
  itemEls[index] = (el as HTMLElement | null) ?? null;
}

/** 视口内焦点页：全屏入口与页码徽标的依据 */
const focusedPage = ref(0);
/** 已进入加载窗口的页（只增不减：回滚不重复请求） */
const live = ref<boolean[]>([]);
const states = ref<PageState[]>([]);
/** 页级重试计数：作为 <img :key> 的一部分，重挂载以重新发起加载 */
const ticks = ref<number[]>([]);
/** 页纵横比：接口尺寸缺失时退回 2:3，加载完成后用真实尺寸校正 */
const aspects = ref<{ w: number; h: number }[]>([]);

function resetPages(): void {
  live.value = props.pages.map(() => false);
  states.value = props.pages.map(() => "loading");
  ticks.value = props.pages.map(() => 0);
  aspects.value = props.pages.map((p) =>
    p.width && p.height ? { w: p.width, h: p.height } : { w: 2, h: 3 }
  );
  focusedPage.value = 0;
  // 首屏先放最靠前的两页；其余交给滚动/布局同步
  activate(0, props.restricted ? 0 : 1, 0);
}

function shotStyle(index: number): Record<string, string> {
  const a = aspects.value[index] ?? { w: 2, h: 3 };
  return { "--ar-w": String(a.w), "--ar-h": String(a.h) };
}

/** 把 [from - behind, from + ahead] 内的页置为已加载。 */
function activate(from: number, ahead = 1, behind = 1): void {
  const total = props.pages.length;
  let next: boolean[] | null = null;
  for (let i = Math.max(0, from - behind); i <= Math.min(total - 1, from + ahead); i += 1) {
    if (!live.value[i]) {
      next ??= live.value.slice();
      next[i] = true;
    }
  }
  if (next) live.value = next;
}

/**
 * 滚动同步：焦点页 = 视口内最靠上的页，加载窗口 = 视口内各页 + 前 1 页 + 后 2 页。
 * 用 getBoundingClientRect 而非 offsetTop：占位块高度随纵横比与窗口宽度变化。
 */
function syncFromScroll(): void {
  const el = scrollEl.value;
  if (!el || props.restricted) return;
  const box = el.getBoundingClientRect();
  let first = 0;
  let last = 0;
  let seen = false;
  for (let i = 0; i < itemEls.length; i += 1) {
    const item = itemEls[i];
    if (!item) continue;
    const rect = item.getBoundingClientRect();
    if (!seen && rect.bottom > box.top + 1) {
      first = i;
      seen = true;
    }
    if (rect.top < box.bottom - 1) last = i;
  }
  focusedPage.value = first;
  activate(first, last - first + 2, 1);
}

let rafId = 0;

function onScroll(): void {
  if (rafId) return;
  rafId = requestAnimationFrame(() => {
    rafId = 0;
    syncFromScroll();
  });
}

function onLoad(index: number, e: Event): void {
  const img = e.target as HTMLImageElement;
  // 占位块纵横比以真实尺寸校正：接口缺尺寸或与实际不符时不至于一直错位
  if (img.naturalWidth && img.naturalHeight) {
    const cur = aspects.value[index];
    if (!cur || cur.w !== img.naturalWidth || cur.h !== img.naturalHeight) {
      const fixed = aspects.value.slice();
      fixed[index] = { w: img.naturalWidth, h: img.naturalHeight };
      aspects.value = fixed;
    }
  }
  const next = states.value.slice();
  next[index] = "ok";
  states.value = next;
}

function onError(index: number): void {
  const next = states.value.slice();
  next[index] = "error";
  states.value = next;
}

/** 加载失败重试：同地址重挂载 <img> 触发重新请求。 */
function retry(index: number): void {
  const next = states.value.slice();
  next[index] = "loading";
  const bumped = ticks.value.slice();
  bumped[index] += 1;
  states.value = next;
  ticks.value = bumped;
}

// ===== 纵向舞台的翻页滚动 =====

function prefersReducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function scrollToPage(index: number, behavior?: ScrollBehavior): void {
  const el = itemEls[index];
  if (!el) return;
  el.scrollIntoView({
    block: "start",
    behavior: behavior ?? (prefersReducedMotion() ? "auto" : "smooth"),
  });
  focusedPage.value = index;
  activate(index);
}

function jumpPage(delta: number): void {
  const last = props.pages.length - 1;
  const next = Math.min(Math.max(focusedPage.value + delta, 0), last);
  if (next !== focusedPage.value) scrollToPage(next);
}

// ===== 全屏浮层 =====

const fullscreen = ref(false);
/** 浮层当前页（打开时定位到被点击的那一页） */
const fsPage = ref(0);
const fsState = ref<"loading" | "ok" | "error">("loading");
/** 全屏档不可用时回落到已加载的主图；只回落一次，避免与 @error 循环。 */
const fsFallback = ref(false);
const overlayEl = ref<HTMLElement | null>(null);
const thumbEls: (HTMLElement | null)[] = [];

function setThumbRef(index: number, el: unknown): void {
  thumbEls[index] = (el as HTMLElement | null) ?? null;
}

const fsSrc = computed(() => {
  const p = props.pages[fsPage.value];
  if (!p) return "";
  return fsFallback.value ? stageSrc(fsPage.value) : pageSrc(p, fullscreenTier.value);
});

const fsLowSrc = computed(() => {
  const p = props.pages[fsPage.value];
  if (!p?.medium) return "";
  const low = thumbSrc(p.medium, "medium");
  return low && low !== fsSrc.value ? low : "";
});

const canPrev = computed(() => multi.value && fsPage.value > 0);
const canNext = computed(() => multi.value && fsPage.value < props.pages.length - 1);

watch(fsSrc, () => {
  fsState.value = "loading";
});

function onFullscreenError(): void {
  if (fsFallback.value) {
    fsState.value = "error";
    return;
  }
  fsFallback.value = true;
}

/** 进入全屏：该页主图就绪且未处于 R-18 遮罩时才可用（遮罩不可被绕过）。 */
function openFullscreen(index: number): void {
  if (props.restricted || states.value[index] !== "ok") return;
  fsPage.value = index;
  fsFallback.value = false;
  fsState.value = "loading";
  fullscreen.value = true;
}

function closeFullscreen(): void {
  fullscreen.value = false;
  // 退出后纵向舞台对齐到浮层最后停留的那一页（瞬时，避免浮层背后的滚动动画）
  scrollToPage(fsPage.value, "auto");
  // 焦点交给舞台本身（可聚焦的滚动区，键盘可继续 ↑/↓ 与 ←/→）；
  // 不还给右上角按钮：md-icon-button 是自定义元素，宿主 focus() 不会落到内部 button。
  scrollEl.value?.focus({ preventScroll: true });
}

function stepTo(index: number): void {
  fsPage.value = Math.min(Math.max(index, 0), props.pages.length - 1);
}

function stepPage(delta: number): void {
  stepTo(fsPage.value + delta);
}

/** 翻页时把当前缩略图滚入胶卷视野（block: nearest 不惊动已可见的项）。 */
watch(fsPage, async () => {
  if (!fullscreen.value) return;
  await nextTick();
  thumbEls[fsPage.value]?.scrollIntoView({ block: "nearest" });
});

/**
 * 浮层键盘：Esc 关闭、←/→ 翻页。capture 阶段监听并 stopImmediatePropagation，
 * 阻止 BrowseWorkView 的 bubble 监听把同一次 Esc 变成路由返回。
 * 纵向模式下 ←/→ 为「跳上一页/下一页」（滚动对齐页顶），同样在此拦截。
 */
function onKeydown(e: KeyboardEvent): void {
  const target = e.target as HTMLElement | null;
  if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) return;
  if (fullscreen.value) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopImmediatePropagation();
      closeFullscreen();
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      e.stopImmediatePropagation();
      stepPage(e.key === "ArrowLeft" ? -1 : 1);
    }
    return;
  }
  if (props.restricted || !multi.value) return;
  if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
    e.preventDefault();
    e.stopImmediatePropagation();
    jumpPage(e.key === "ArrowLeft" ? -1 : 1);
  }
}

watch(fullscreen, async (open) => {
  if (open) {
    await nextTick();
    overlayEl.value?.focus();
  }
});

watch(
  () => props.pages,
  () => resetPages(),
  { immediate: true }
);

onMounted(() => {
  window.addEventListener("keydown", onKeydown, { capture: true });
  window.addEventListener("resize", onScroll, { passive: true });
  syncFromScroll();
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown, { capture: true });
  window.removeEventListener("resize", onScroll);
  if (rafId) cancelAnimationFrame(rafId);
});

/** 遮罩解除（设置里打开 show_r18）后补一次窗口同步：滚动容器回到多页布局时无需等滚动。 */
watch(
  () => props.restricted,
  async () => {
    await nextTick();
    syncFromScroll();
  }
);

/** 预加载相邻页：浮层翻页时立即可见（fire-and-forget），与浮层同一档位。 */
watch(
  () => [fullscreen.value, fsPage.value, fullscreenTier.value] as const,
  () => {
    if (!fullscreen.value) return;
    for (const i of [fsPage.value - 1, fsPage.value + 1]) {
      const p = props.pages[i];
      if (!p) continue;
      const img = new Image();
      img.src = pageSrc(p, fullscreenTier.value);
    }
  },
  { immediate: true }
);
</script>

<template>
  <div class="viewer">
    <div class="stage">
      <!-- 纵向舞台：滚动渐进加载；遮罩期间仅保留首页（其余 v-show 收起） -->
      <div
        ref="scrollEl"
        class="stage-scroll"
        :class="{ 'is-locked': restricted, 'is-single': restricted || pages.length < 2 }"
        tabindex="0"
        role="group"
        :aria-label="
          pages.length > 1
            ? t('browse.work.stageLabelMulti', { total: pages.length })
            : t('browse.work.stageLabel')
        "
        @scroll.passive="onScroll"
      >
        <div
          v-for="(page, i) in pages"
          v-show="!restricted || i === 0"
          :key="i"
          :ref="(el) => setItemRef(i, el)"
          class="page-item"
        >
          <!-- 页框：纵横比先占位，图片加载完成后不再改变高度 -->
          <div
            class="shot"
            :class="{ ready: states[i] === 'ok' && !restricted }"
            :style="shotStyle(i)"
            :title="states[i] === 'ok' && !restricted ? t('browse.work.fullscreen') : undefined"
            @click="openFullscreen(i)"
          >
            <!-- 先低清后高清：540 占位层在主图之下，主图就绪后即被覆盖 -->
            <img
              v-if="live[i] && states[i] !== 'ok' && lowSrc(i)"
              class="shot-img low"
              :src="lowSrc(i)"
              alt=""
              aria-hidden="true"
              decoding="async"
            />
            <img
              v-if="live[i]"
              v-show="states[i] === 'ok'"
              :key="`${stageSrc(i)}#${ticks[i]}`"
              class="shot-img"
              :class="{ blurred: restricted }"
              :src="stageSrc(i)"
              :alt="altOf(i)"
              decoding="async"
              @load="onLoad(i, $event)"
              @error="onError(i)"
            />
            <div v-if="live[i] && states[i] === 'error' && !restricted" class="shot-state">
              <p class="stage-text">{{ t("common.browseLoadFailed") }}</p>
              <md-outlined-button @click.stop="retry(i)">{{ t("common.retry") }}</md-outlined-button>
            </div>
          </div>
        </div>
      </div>

      <!-- 全屏入口：焦点页主图就绪且未遮罩时才可点（浮层不做二次揭示） -->
      <md-icon-button
        class="fs-trigger"
        :disabled="restricted || states[focusedPage] !== 'ok'"
        :aria-label="t('browse.work.fullscreen')"
        :title="t('browse.work.fullscreen')"
        @click="openFullscreen(focusedPage)"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M4 9V4h5" /><path d="M20 9V4h-5" /><path d="M4 15v5h5" /><path d="M20 15v5h-5" />
        </svg>
      </md-icon-button>

      <!-- 页码徽标：跟随滚动（多页且未遮罩） -->
      <div v-if="multi && !restricted" class="page-badge" aria-hidden="true">
        {{ t("browse.work.pageOf", { current: focusedPage + 1, total: pages.length }) }}
      </div>

      <!-- R-18 遮罩 -->
      <div v-if="restricted" class="restrict-overlay">
        <p class="stage-text strong">{{ restrictLabel || t("browse.work.restrictedTitle") }}</p>
        <md-filled-button @click="emit('reveal')">{{ t("browse.work.show") }}</md-filled-button>
      </div>
    </div>

    <!-- ugoira 说明行 -->
    <p v-if="ugoira" class="ugoira-note">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="12" cy="12" r="9" />
        <polygon points="10 8.5 16 12 10 15.5" fill="currentColor" stroke="none" />
      </svg>
      {{ t("browse.work.ugoiraNote") }}
    </p>

    <!-- 全屏浮层：左右切换 + 右侧胶卷缩略图（不复用舞台内控件，后者被浮层盖住） -->
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
      <div class="fs-stage" @click.self="closeFullscreen">
        <img
          v-if="fsLowSrc && fsState !== 'ok'"
          class="fs-img low"
          :src="fsLowSrc"
          alt=""
          aria-hidden="true"
          decoding="async"
        />
        <img
          v-show="fsState === 'ok'"
          class="fs-img"
          :src="fsSrc"
          :alt="altOf(fsPage)"
          decoding="async"
          @load="fsState = 'ok'"
          @error="onFullscreenError"
        />
        <div v-if="fsState !== 'ok' && !fsLowSrc" class="fs-state">
          <div v-if="fsState === 'loading'" class="loading-block" aria-hidden="true"></div>
          <p v-else class="stage-text">{{ t("common.browseLoadFailed") }}</p>
        </div>

        <div v-if="multi" class="fs-controls">
          <md-icon-button :disabled="!canPrev" :aria-label="t('browse.work.prevPage')" :title="t('browse.work.prevPage')" @click="stepPage(-1)">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="15 18 9 12 15 6" /></svg>
          </md-icon-button>
          <span class="fs-label" aria-live="polite">{{ t("browse.work.pageOf", { current: fsPage + 1, total: pages.length }) }}</span>
          <md-icon-button :disabled="!canNext" :aria-label="t('browse.work.nextPage')" :title="t('browse.work.nextPage')" @click="stepPage(1)">
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

      <!-- 胶卷缩略图：快速切页；当前页白色描边 + aria-current，翻页时自动滚入视野 -->
      <nav v-if="multi" class="fs-filmstrip" :aria-label="t('browse.work.pageThumbs')">
        <button
          v-for="(page, i) in pages"
          :key="i"
          :ref="(el) => setThumbRef(i, el)"
          type="button"
          class="fs-thumb"
          :class="{ active: i === fsPage }"
          :aria-label="t('browse.work.gotoPage', { page: i + 1 })"
          :aria-current="i === fsPage ? 'true' : undefined"
          @click="stepTo(i)"
        >
          <img :src="thumbSrc(page.medium ?? page.original, gridTier)" alt="" loading="lazy" decoding="async" />
        </button>
      </nav>
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
  border-radius: 16px;
  background: rgb(0 0 0 / 0.78);
  overflow: hidden;
}

/* 纵向滚动舞台：逐页排列，滚动渐进加载 */
.stage-scroll {
  flex: 1;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  padding: var(--space-lg) var(--space-lg) var(--space-xl);
  outline: none;
}

.stage-scroll.is-locked {
  overflow: hidden;
}

/* 单页与遮罩态：整幅在舞台内居中（auto margin 在内容超高时不裁剪，可继续滚动） */
.stage-scroll.is-single {
  display: flex;
  flex-direction: column;
}

.stage-scroll.is-single .page-item {
  margin-top: auto;
  margin-bottom: auto;
}

/* 近黑底上的焦点环用既定白（primary 在深底上对比不足） */
.stage-scroll:focus-visible {
  outline: 2px solid #fff;
  outline-offset: calc(-1 * var(--space-xxs));
}

.page-item {
  display: flex;
  justify-content: center;
  /* 翻页滚动时页顶不贴着容器上沿 */
  scroll-margin-top: var(--space-lg);
}

.page-item + .page-item {
  margin-top: var(--space-lg);
}

/* 页框：纵横比先行占位（--ar-w / --ar-h 为内联样式），加载完成后高度不变 */
.shot {
  position: relative;
  width: 100%;
  aspect-ratio: var(--ar-w, 2) / var(--ar-h, 3);
  border-radius: var(--radius-control);
  background: rgb(255 255 255 / 0.08);
  overflow: hidden;
}

.shot.ready {
  cursor: zoom-in;
}

.shot-img {
  position: absolute;
  inset: 0;
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.shot-img.blurred {
  filter: blur(24px);
  /* 放大避免 blur 边缘透出底色（页框自身 overflow: hidden） */
  transform: scale(1.08);
}

.shot-state {
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

/* 页码徽标：近黑底 + 白字（与舞台同一例外） */
.page-badge {
  position: absolute;
  right: var(--space-sm);
  bottom: var(--space-sm);
  z-index: 1;
  padding: 2px 10px;
  border-radius: 999px;
  background: rgb(0 0 0 / 0.78);
  color: #fff;
  font-size: 12px;
  font-weight: 600;
  line-height: 1.6;
  pointer-events: none;
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
  padding: var(--space-lg);
  /* 复用舞台近黑底（不新增颜色字面值） */
  background: rgb(0 0 0 / 0.78);
  outline: none;
}

.fs-stage {
  position: relative;
  display: flex;
  flex: 1;
  min-width: 0;
  align-items: center;
  justify-content: center;
}

.fs-img {
  position: relative;
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
}

.fs-img.low {
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

/* 关闭按钮落在图片区右上角，避开右侧胶卷列 */
.fs-close {
  position: absolute;
  top: var(--space-sm);
  right: var(--space-sm);
}

/* 胶卷列：80px 宽（64px 缩略图 + 6px 内边），自身滚动，窄窗不隐藏 */
.fs-filmstrip {
  display: flex;
  width: 80px;
  flex-shrink: 0;
  flex-direction: column;
  gap: var(--space-sm);
  overflow-x: hidden;
  overflow-y: auto;
  padding: var(--space-xs);
  margin-left: var(--space-md);
}

/* 列内出现纵向滚动条时按剩余宽度收窄，始终方形、不出横向滚动条 */
.fs-thumb {
  width: 100%;
  max-width: 64px;
  aspect-ratio: 1;
  flex-shrink: 0;
  padding: 0;
  border: none;
  border-radius: var(--radius-control);
  background: rgb(255 255 255 / 0.08);
  overflow: hidden;
  opacity: 0.65;
  cursor: pointer;
  transition: opacity 0.15s ease;
}

.fs-thumb img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.fs-thumb:hover,
.fs-thumb:focus-visible,
.fs-thumb.active {
  opacity: 1;
}

/* 当前页与键盘焦点：近黑底上的既定白 2px 环 */
.fs-thumb.active,
.fs-thumb:focus-visible {
  outline: 2px solid #fff;
  outline-offset: 2px;
}

@media (prefers-reduced-motion: reduce) {
  .fs-thumb {
    transition: none;
  }
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

/* 全屏入口：图片区右上角，位于遮罩（z-index 2）之下、滚动容器之上 */
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
