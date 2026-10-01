<script lang="ts">
/** 全屏浮层的阅读偏好（双图跨页 / 从右往左）：本会话记忆，不持久化。 */
const viewerPrefs = { spread: false, rtl: true };
</script>

<script setup lang="ts">
/**
 * 图片舞台：近黑底纵向滚动查看（深浅色主题一致的中性深底）。
 * - 多页：自上而下逐页排列，滚动到视口附近才发起加载（渐进式）；未加载页为按该页
 *   `width`/`height` 预留纵横比的纯色占位块（缺省 2:3），加载完成不产生跳动；
 * - 点击任意页进入全屏浮层并定位到该页，左右切换只发生在浮层内（‹ › / 键盘 ←/→ 与
 *   右侧胶卷缩略图）；上一页/下一页的滚动对齐由舞台内的 ←/→ 完成；浮层内另有
 *   双图（跨页）模式与阅读方向（从右往左 / 从左往右），会话内记忆——从右往左时
 *   翻页组整组镜像（前进在左、箭头朝左）且键盘 ← 为前进，与跨页阅读方向一致；
 * - 浮层图片按可用高度与页面纵横比撑满，控制条与关闭按钮 hover（或键盘聚焦）才显现，
 *   胶卷缩略图常驻；
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
  // 换作品时浮层一并复位：旧的下标可能落在新数组之外
  fullscreen.value = false;
  fsPage.value = 0;
  fsFallbackPages.value = [];
  fsStates.value = {};
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
 * 滚动同步：焦点页 = 目标页（平滑滚动途中）或视口内最靠上的页；加载窗口 = 视口内各页
 * 与目标页的并集，前后各留 1 页。用 getBoundingClientRect 而非 offsetTop：占位块高度
 * 随纵横比与窗口宽度变化。
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
  const focus = pendingFocus ?? first;
  focusedPage.value = focus;
  const from = Math.min(first, focus);
  const to = Math.max(last, focus);
  activate(from, to - from + 1, 1);
}

let rafId = 0;

function onScroll(): void {
  if (rafId) return;
  rafId = requestAnimationFrame(() => {
    rafId = 0;
    syncFromScroll();
  });
}

/** 页框纵横比以真实尺寸校正：接口缺尺寸或与实际不符时不至于一直错位（舞台与浮层共用）。 */
function correctAspect(index: number, e: Event): void {
  const img = e.target as HTMLImageElement;
  if (!img.naturalWidth || !img.naturalHeight) return;
  const cur = aspects.value[index];
  if (cur && cur.w === img.naturalWidth && cur.h === img.naturalHeight) return;
  const fixed = aspects.value.slice();
  fixed[index] = { w: img.naturalWidth, h: img.naturalHeight };
  aspects.value = fixed;
}

function onLoad(index: number, e: Event): void {
  correctAspect(index, e);
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

/**
 * 程序化滚动期间钉住焦点页：平滑动画会持续触发 scroll，若不钉住，
 * syncFromScroll 会把焦点页算回动画途中的旧页，连按 ←/→ 就会丢步。
 */
let pendingFocus: number | null = null;
let pendingTimer = 0;

function scrollToPage(index: number, behavior?: ScrollBehavior): void {
  const el = itemEls[index];
  if (!el) return;
  const smooth = behavior !== "auto" && !prefersReducedMotion();
  pendingFocus = smooth ? index : null;
  if (pendingTimer) clearTimeout(pendingTimer);
  if (smooth) pendingTimer = window.setTimeout(() => (pendingFocus = null), 500);
  el.scrollIntoView({ block: "start", behavior: smooth ? "smooth" : "auto" });
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
/** 浮层当前页（打开时定位到被点击的那一页；双图模式下为跨页起始页） */
const fsPage = ref(0);
/** 全屏档加载失败的页 → 回落到已加载的主图；按页记录，一处失败不影响其它页 */
const fsFallbackPages = ref<number[]>([]);
/** 全屏图按页状态（对齐 pages 下标；未记录即 loading） */
const fsStates = ref<Record<number, "loading" | "ok" | "error">>({});
const overlayEl = ref<HTMLElement | null>(null);
const thumbEls: (HTMLElement | null)[] = [];

function setThumbRef(index: number, el: unknown): void {
  thumbEls[index] = (el as HTMLElement | null) ?? null;
}

// ----- 双图（跨页）模式与阅读方向：会话内记忆（模块级 viewerPrefs） -----

const spread = ref(viewerPrefs.spread);
const rtl = ref(viewerPrefs.rtl);

watch(spread, (v) => (viewerPrefs.spread = v));
watch(rtl, (v) => (viewerPrefs.rtl = v));

/** 双图仅对多页作品生效（单页作品没有可跨的页） */
const spreadOn = computed(() => spread.value && multi.value);
/** 阅读方向生效：仅双图模式（单页一屏一张，方向不改变页面顺序与翻页朝向） */
const rtlActive = computed(() => spreadOn.value && rtl.value);
/** 翻页步进：双图一屏两张，步进 2 */
const pageStep = computed(() => (spreadOn.value ? 2 : 1));

/** 一屏显示的页（下标，按 DOM 从左到右排列；从右往左时当前页在右） */
const visiblePages = computed(() => {
  const first = fsPage.value;
  const list = spreadOn.value && first + 1 < props.pages.length ? [first, first + 1] : [first];
  return rtl.value ? [...list].reverse() : list;
});

/** 跨页的后一页（单页模式与末页单独显示时等于当前页） */
const spreadEnd = computed(() =>
  visiblePages.value.length > 1 ? fsPage.value + 1 : fsPage.value
);

const pageLabel = computed(() =>
  spreadEnd.value > fsPage.value
    ? t("browse.work.pageRangeOf", {
        from: fsPage.value + 1,
        to: spreadEnd.value + 1,
        total: props.pages.length,
      })
    : t("browse.work.pageOf", { current: fsPage.value + 1, total: props.pages.length })
);

interface FsSlot {
  index: number;
  src: string;
  low: string;
  state: "loading" | "ok" | "error";
}

function fsSrcOf(index: number): string {
  const p = props.pages[index];
  if (!p) return "";
  return fsFallbackPages.value.includes(index)
    ? stageSrc(index)
    : pageSrc(p, fullscreenTier.value);
}

/** 低清占位层：接口给了 medium 且与全屏图不是同一地址时才叠加。 */
function fsLowOf(index: number): string {
  const p = props.pages[index];
  if (!p?.medium) return "";
  const low = thumbSrc(p.medium, "medium");
  return low && low !== fsSrcOf(index) ? low : "";
}

const fsSlots = computed<FsSlot[]>(() =>
  visiblePages.value.map((index) => ({
    index,
    src: fsSrcOf(index),
    low: fsLowOf(index),
    state: fsStates.value[index] ?? "loading",
  }))
);

const canPrev = computed(() => multi.value && fsPage.value > 0);
const canNext = computed(
  () => multi.value && fsPage.value + pageStep.value <= props.pages.length - 1
);

function setFsState(index: number, state: "loading" | "ok" | "error"): void {
  if (fsStates.value[index] === state) return;
  fsStates.value = { ...fsStates.value, [index]: state };
}

function onFsLoad(index: number, e: Event): void {
  correctAspect(index, e);
  setFsState(index, "ok");
}

function onFsError(index: number, e: Event): void {
  if (fsFallbackPages.value.includes(index)) {
    setFsState(index, "error");
    return;
  }
  const failed = (e.target as HTMLImageElement).getAttribute("src") ?? "";
  fsFallbackPages.value = [...fsFallbackPages.value, index];
  // 回落目标与失败地址相同时不会再触发 load/error，直接落错误态（否则永远停在加载中）
  if (fsSrcOf(index) === failed) setFsState(index, "error");
}

/** 进入全屏：该页主图就绪且未处于 R-18 遮罩时才可用（遮罩不可被绕过）。 */
function openFullscreen(index: number): void {
  if (props.restricted || states.value[index] !== "ok") return;
  fsPage.value = index;
  fsFallbackPages.value = [];
  fsStates.value = {};
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

/** 双图模式按「跨页对」对齐（1-2 / 3-4 …）：点胶卷任意一页都落到所属跨页。 */
function stepTo(index: number): void {
  let next = Math.min(Math.max(index, 0), Math.max(props.pages.length - 1, 0));
  if (spreadOn.value && next % 2 === 1) next -= 1;
  fsPage.value = next;
}

function stepPage(delta: number): void {
  stepTo(fsPage.value + delta * pageStep.value);
}

function toggleSpread(): void {
  spread.value = !spread.value;
  if (spreadOn.value) stepTo(fsPage.value);
}

function toggleRtl(): void {
  rtl.value = !rtl.value;
}

/** 翻页时把当前缩略图滚入胶卷视野（block: nearest 不惊动已可见的项）。 */
watch(fsPage, async () => {
  if (!fullscreen.value) return;
  await nextTick();
  thumbEls[fsPage.value]?.scrollIntoView({ block: "nearest" });
});

/**
 * 输入焦点判定：md-* 输入组件的事件到 window 时已被重定向到宿主（tagName 不是 INPUT），
 * 只查 e.target 会漏判，须沿 composedPath 找真实目标，否则会吃掉输入框里的 ←/→。
 */
function isTypingEvent(e: KeyboardEvent): boolean {
  for (const node of e.composedPath()) {
    if (!(node instanceof HTMLElement)) continue;
    if (node.tagName === "INPUT" || node.tagName === "TEXTAREA" || node.isContentEditable) return true;
  }
  return false;
}

/**
 * 浮层键盘：Esc 关闭、←/→ 翻页（从右往左时 ← 为前进）。capture 阶段监听并
 * stopImmediatePropagation，阻止 BrowseWorkView 的 bubble 监听把同一次 Esc 变成路由返回。
 * 纵向模式下 ←/→ 为「跳上一页/下一页」（滚动对齐页顶），同样在此拦截。
 */
function onKeydown(e: KeyboardEvent): void {
  if (isTypingEvent(e)) return;
  if (fullscreen.value) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopImmediatePropagation();
      closeFullscreen();
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      e.stopImmediatePropagation();
      // 从右往左时跨页向左推进，方向键跟着翻页组的镜像走
      const leftDelta = rtlActive.value ? 1 : -1;
      stepPage(e.key === "ArrowLeft" ? leftDelta : -leftDelta);
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
  if (pendingTimer) clearTimeout(pendingTimer);
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
  () => [fullscreen.value, fsPage.value, pageStep.value, fullscreenTier.value] as const,
  () => {
    if (!fullscreen.value) return;
    const shown = new Set(visiblePages.value);
    const next = fsPage.value + pageStep.value;
    for (const i of [fsPage.value - 1, next, next + 1]) {
      const p = props.pages[i];
      if (!p || shown.has(i)) continue;
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
      <div class="fs-stage" :class="{ 'is-spread': spreadOn }" @click.self="closeFullscreen">
        <!-- 一屏一页或并排两页：页框按可用高度与页面纵横比定尺寸，图片撑满该框 -->
        <div v-for="slot in fsSlots" :key="slot.index" class="fs-slot" :style="shotStyle(slot.index)">
          <img
            v-if="slot.low && slot.state !== 'ok'"
            class="fs-img low"
            :src="slot.low"
            alt=""
            aria-hidden="true"
            decoding="async"
          />
          <img
            v-show="slot.state === 'ok'"
            class="fs-img"
            :src="slot.src"
            :alt="altOf(slot.index)"
            decoding="async"
            @load="onFsLoad(slot.index, $event)"
            @error="onFsError(slot.index, $event)"
          />
          <div v-if="slot.state === 'error'" class="fs-state">
            <p class="stage-text">{{ t("common.browseLoadFailed") }}</p>
          </div>
          <div v-else-if="slot.state === 'loading' && !slot.low" class="fs-state">
            <div class="loading-block" aria-hidden="true"></div>
          </div>
        </div>

        <!-- 底部控制条：默认隐藏，指针进入底部热区或键盘聚焦时显现 -->
        <div class="fs-chrome fs-chrome-bottom">
          <div v-if="multi" class="fs-controls">
            <!-- 翻页组：从右往左时整组镜像（前进按钮落到左侧、箭头改为朝左），与跨页
                 阅读方向一致；双图 / 方向开关不是方向性控件，不参与镜像 -->
            <div class="fs-pager" :class="{ 'is-rtl': rtlActive }">
              <md-icon-button :disabled="!canPrev" :aria-label="t('browse.work.prevPage')" :title="t('browse.work.prevPage')" @click="stepPage(-1)">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline :points="rtlActive ? '9 18 15 12 9 6' : '15 18 9 12 15 6'" /></svg>
              </md-icon-button>
              <span class="fs-label" aria-live="polite">{{ pageLabel }}</span>
              <md-icon-button :disabled="!canNext" :aria-label="t('browse.work.nextPage')" :title="t('browse.work.nextPage')" @click="stepPage(1)">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline :points="rtlActive ? '15 18 9 12 15 6' : '9 18 15 12 9 6'" /></svg>
              </md-icon-button>
            </div>
            <span class="fs-divider" aria-hidden="true"></span>
            <!-- 双图（跨页）开关 -->
            <md-icon-button
              toggle
              :selected="spreadOn"
              :aria-label="t('browse.work.spread')"
              :title="t('browse.work.spread')"
              @click="toggleSpread"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <rect x="7" y="4" width="10" height="16" rx="1.5" />
                <rect v-if="spreadOn" x="1.5" y="6" width="4" height="12" rx="1" />
                <rect v-if="spreadOn" x="18.5" y="6" width="4" height="12" rx="1" />
              </svg>
            </md-icon-button>
            <!-- 阅读方向：从右往左 / 从左往右（仅双图模式） -->
            <md-icon-button
              v-if="spreadOn"
              :aria-label="t('browse.work.readDir', { dir: rtl ? t('browse.work.dirRtl') : t('browse.work.dirLtr') })"
              :title="t('browse.work.readDir', { dir: rtl ? t('browse.work.dirRtl') : t('browse.work.dirLtr') })"
              @click="toggleRtl"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path :d="rtl ? 'M20 12H4' : 'M4 12h16'" />
                <polyline :points="rtl ? '10 6 4 12 10 18' : '14 6 20 12 14 18'" />
              </svg>
            </md-icon-button>
          </div>
        </div>

        <!-- 右上角关闭：默认隐藏，hover 热区或键盘聚焦时显现 -->
        <div class="fs-chrome fs-chrome-corner">
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

      <!-- 胶卷缩略图（常驻）：快速切页；当前屏幕上的页白色描边，翻页时自动滚入视野 -->
      <nav v-if="multi" class="fs-filmstrip" :aria-label="t('browse.work.pageThumbs')">
        <button
          v-for="(page, i) in pages"
          :key="i"
          :ref="(el) => setThumbRef(i, el)"
          type="button"
          class="fs-thumb"
          :class="{ active: visiblePages.includes(i) }"
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

/* 页码徽标：沿用舞台的既定近黑底 + 白字（同一例外，不引入新颜色） */
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
  /* 图片可用高度：视口高减去浮层上下内边距 */
  --fs-h: calc(100vh - 2 * var(--space-lg));
}

.fs-stage {
  position: relative;
  display: flex;
  flex: 1;
  min-width: 0;
  align-items: center;
  justify-content: center;
  gap: var(--space-sm);
}

/* 页框：宽 = min(可用宽, 可用高 × 页面纵横比)，纵横比由内联 --ar-w / --ar-h 给出，
 * 所以纵向页撑满高度、横向页撑满宽度，图片再以 object-fit: contain 兜底 */
.fs-slot {
  position: relative;
  width: min(100%, calc(var(--fs-h) * var(--ar-w, 2) / var(--ar-h, 3)));
  aspect-ratio: var(--ar-w, 2) / var(--ar-h, 3);
  max-height: 100%;
}

.fs-stage.is-spread .fs-slot {
  width: min(calc(50% - var(--space-sm) / 2), calc(var(--fs-h) * var(--ar-w, 2) / var(--ar-h, 3)));
}

/* 允许放大：小图也铺满页框（否则「撑满」受原始像素限制） */
.fs-img {
  position: absolute;
  inset: 0;
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.fs-state {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}

/* ===== 悬浮控件：默认隐藏，进入热区或键盘聚焦才显现 ===== */

.fs-chrome {
  position: absolute;
  z-index: 1;
}

.fs-chrome-bottom {
  right: 0;
  bottom: 0;
  left: 0;
  display: flex;
  justify-content: center;
  padding: var(--space-lg) 0;
}

/* 关闭按钮落在图片区右上角，避开右侧胶卷列 */
.fs-chrome-corner {
  top: 0;
  right: 0;
  display: flex;
  padding: var(--space-sm);
}

.fs-controls,
.fs-close {
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.15s ease;
}

.fs-chrome:hover .fs-controls,
.fs-chrome:focus-within .fs-controls,
.fs-chrome:hover .fs-close,
.fs-chrome:focus-within .fs-close {
  opacity: 1;
  pointer-events: auto;
}

/* 触摸设备没有 hover：控件常驻 */
@media (hover: none) {
  .fs-controls,
  .fs-close {
    opacity: 1;
    pointer-events: auto;
  }
}

.fs-controls {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
  padding: var(--space-xxs) var(--space-sm);
  border-radius: 999px;
  background: rgb(0 0 0 / 0.78);
}

/* 翻页组：从右往左时整组镜像——前进落到左侧（箭头同步改为朝左），后退落到右侧 */
.fs-pager {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
}

.fs-pager.is-rtl {
  flex-direction: row-reverse;
}

.fs-divider {
  width: 1px;
  height: 20px;
  margin: 0 var(--space-xxs);
  background: rgb(255 255 255 / 0.24);
}

.fs-label {
  min-width: 96px;
  color: rgb(255 255 255 / 0.85);
  font-size: 12px;
  font-weight: 600;
  text-align: center;
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

/* 滚动条常显（不随闲置自动隐藏）：近黑底上的既定白半透明 */
.fs-filmstrip::-webkit-scrollbar {
  width: 6px;
}

.fs-filmstrip::-webkit-scrollbar-track {
  background: rgb(255 255 255 / 0.06);
  border-radius: 999px;
}

.fs-filmstrip::-webkit-scrollbar-thumb {
  background: rgb(255 255 255 / 0.4);
  border-radius: 999px;
}

.fs-filmstrip::-webkit-scrollbar-thumb:hover {
  background: rgb(255 255 255 / 0.6);
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
