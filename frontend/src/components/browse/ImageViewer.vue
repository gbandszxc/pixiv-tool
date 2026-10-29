<script lang="ts">
/** 全屏浮层的阅读偏好（双图跨页 / 从右往左 / 竖屏 / 竖屏宽度）：本会话记忆，不持久化。 */
const viewerPrefs = { spread: false, rtl: true, vertical: false, verticalScale: 1 };
</script>

<script setup lang="ts">
/**
 * 图片舞台：无自有底色——舞台背景即页面底色（随主题），图片满幅纵向滚动查看。
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
 * - 全屏滚轮 / PgUp / PgDn 按页或跨页组前后切换，胶卷仍原生滚动；
 * - 浮层另有竖屏模式（连续纵向滚动，条漫友好）：固定单列、从上到下，与双图（跨页）
 *   模式互斥——进入竖屏即按单列渲染、双页开关随之隐藏，退出后开关恢复可见并保持此前
 *   选择；不提供方向切换；滚轮与 ↑/↓ 为原生滚动，PgUp/PgDn 走全局按屏滚动，←/→
 *   对齐上下页行首；胶卷点击滚到该页、标记视口内各页；页框宽度比例（0.5~1）会话
 *   内记忆；图片按视口窗口懒加载（占位盒预留高度，不一次性加载全部原图）；进出
 *   竖屏保持当前页；
 * - 舞台页框宽度占比 stageScale（0.5~1，父视图持久化到设置键 detail_image_scale）
 *   只收窄页框宽度并居中，默认 1 = 撑满 = 既有观感；
 * - 键盘：本组件在 capture 阶段监听并 stopImmediatePropagation，避免与父视图的同名
 *   按键（Esc 返回）二次处理；浮层内 Esc 只关浮层；
 * - 加载中纯色占位、失败显示重试；
 * - R-18 遮罩：blur(24px) + 中央文案 + 「显示」按钮（是否遮罩由父视图决定），
 *   遮罩期间只加载首页、不可进入全屏浮层；
 * - ugoira（动图）：父视图拉到帧后舞台主图改为帧轮播——单张 <img> 换 src，按每帧 delay
 *   递归 setTimeout（下限 20ms）播放，预载未就绪的帧跳过不阻塞；默认自动播放，
 *   prefers-reduced-motion 时初始暂停（舞台右下角胶囊控件可手动播放/暂停 + 帧计数）；
 *   KeepAlive 停用与卸载清计时器、激活时恢复此前播放态；无帧 / 加载中 / 失败退回封面帧
 *   + 说明行（加载中旋转图标、失败给「重试」、无帧保留「去 Pixiv 查看动画」旧文案）。
 */
import { computed, nextTick, onActivated, onBeforeUnmount, onDeactivated, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { pxSrc, thumbSrc, type ThumbTier } from "../../api/browse";
import { useThumbTier } from "../../composables/useThumbTier";
import { pageScrollTarget } from "../../composables/usePageScroll";

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
    /** ugoira 说明行（无帧 / 加载中 / 失败时展示降级文案） */
    ugoira?: boolean;
    /** ugoira 逐帧地址与时长（毫秒）；父视图拉取成功后才给，非空即走帧轮播 */
    ugoiraFrames?: { src: string; delay: number }[];
    /** ugoira 帧拉取中（说明行显示加载态） */
    ugoiraLoading?: boolean;
    /** ugoira 帧拉取失败原因（说明行显示降级文案 + 重试，原文经 title 可读） */
    ugoiraError?: string;
    /** 详情舞台页框宽度占比（0.5~1，1 = 撑满 = 既有观感；父视图读设置键 detail_image_scale） */
    stageScale?: number;
  }>(),
  {
    alt: "",
    restricted: false,
    restrictLabel: "",
    ugoira: false,
    ugoiraFrames: () => [],
    ugoiraLoading: false,
    ugoiraError: "",
    stageScale: 1,
  }
);

const emit = defineEmits<{
  (e: "reveal"): void;
  (e: "retry-ugoira"): void;
}>();

const { t } = useI18n();

const detailTier = useThumbTier("thumb_quality_detail");
const fullscreenTier = useThumbTier("thumb_quality_fullscreen");
const gridTier = useThumbTier("thumb_quality_grid");

const multi = computed(() => props.pages.length > 1 && !props.ugoira);

/** 舞台页框宽度占比（父视图传入；防御性夹取到 [0.5, 1]，异常值回落撑满）。 */
const stageScaleValue = computed(() => Math.min(1, Math.max(0.5, props.stageScale)));

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
/** 舞台滚动位置：KeepAlive 停用时 DOM 已被移出文档，那时读 scrollTop 只能得到 0，
 * 所以在滚动过程中记录、激活时还原（见 onScroll / onActivated）。 */
let savedScrollTop = 0;
/** 页元素引用（下标与 pages 对齐） */
const itemEls: (HTMLElement | null)[] = [];

function setItemRef(index: number, el: unknown): void {
  itemEls[index] = (el as HTMLElement | null) ?? null;
}

/** 视口内焦点页：页码徽标与 ←/→ 翻页的依据 */
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
  savedScrollTop = 0;
  // 换作品时浮层一并复位：旧的下标可能落在新数组之外
  fullscreen.value = false;
  fsPage.value = 0;
  fsFallbackPages.value = [];
  fsStates.value = {};
  fsLive.value = props.pages.map(() => false);
  fsVisible.value = [];
  fsSlotEls.length = 0;
  fsPendingPage = null;
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
  // 滚动即记录位置：停用时 DOM 已被 KeepAlive 移出文档，那时再读 scrollTop 只会是 0。
  savedScrollTop = scrollEl.value?.scrollTop ?? 0;
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

// ===== ugoira 帧轮播 =====

/** 首帧地址加载失败：退回静态封面帧 + 失败说明行（不让舞台永远停在骨架块） */
const frameBroken = ref(false);
/** 首帧已落定（帧轮播下页框纵横比与全屏入口的依据） */
const frameReady = ref(false);

/** 帧轮播生效：动图、父视图已给到帧、且帧图本身没坏（否则退回静态封面帧 + 说明行降级态）。 */
const ugoiraPlayable = computed(
  () => props.ugoira && props.ugoiraFrames.length > 0 && !frameBroken.value
);

/** 当前帧下标（轮播推进位；未就绪的帧跳过，见 nextReadyFrame）。 */
const frameIndex = ref(0);
/** 播放态：默认自动播放；系统偏好减少动效时初始暂停（控件仍可手动播放）。 */
const playing = ref(!prefersReducedMotion());
/** 已预载就绪的帧下标：非响应式（只有计时器与推进逻辑读它，模板不渲染）。 */
let readyFrames = new Set<number>();
/** 加载失败的帧下标：推进时跳过；全部损坏才整体退回静态封面 */
let brokenFrames = new Set<number>();
/** 预载代次：换作品后旧请求的 onload 不得写进新集合 */
let frameGen = 0;
/** 帧计时器：停用 / 卸载 / 换帧时统一清理 */
let frameTimer: number | undefined;
/** 组件是否在活动中：KeepAlive 停用期间不起表（帧可能在停用时才到达） */
let viewerActive = true;
/** 首帧已落定（就绪或失败）：此前不起表，首屏始终先停在首帧 */
let firstSettled = false;

/** 帧总数（模板与计时器共用） */
const frameTotal = computed(() => props.ugoiraFrames.length);

/** 安全下标：换作品的瞬间帧数可能先缩，取模钳住后各处索引都不会越界。 */
const framePos = computed(() => (frameTotal.value ? frameIndex.value % frameTotal.value : 0));

/** 帧数收缩时把下标落回区间内（否则计数会显示成「第 12 / 8 帧」）。 */
watch(frameTotal, (total) => {
  if (frameIndex.value >= total) frameIndex.value = 0;
});

/** 实际在动：单帧作品没有动画可言（控件按暂停态呈现且禁用，不谎报「正在播放」）。 */
const animating = computed(() => playing.value && frameTotal.value > 1);

/** 当前帧地址（未就绪时先挂它的地址，浏览器直接复用预载中的同一请求）。 */
const frameSrc = computed(() => props.ugoiraFrames[framePos.value]?.src ?? "");

function stopFrameTimer(): void {
  if (frameTimer !== undefined) {
    clearTimeout(frameTimer);
    frameTimer = undefined;
  }
}

/** 下一张就绪帧：从当前帧向后取模查找；一张都没就绪返回 -1（等预载完成再续）。 */
function nextReadyFrame(): number {
  const total = props.ugoiraFrames.length;
  for (let step = 1; step <= total; step += 1) {
    const i = (framePos.value + step) % total;
    if (readyFrames.has(i)) return i;
  }
  return -1;
}

/** 按当前帧 delay 递归调度下一帧：下限 20ms（后端给 0 时不能变成死循环）。 */
function scheduleFrame(): void {
  stopFrameTimer();
  // 遮罩期间不动画（沿用「遮罩期间不消费内容」的既定口径），解除后由 restricted 侦听恢复；
  // 尚无就绪帧时不起表（预载落定后自行续上，避免空转）
  if (!playing.value || !ugoiraPlayable.value || props.restricted) return;
  if (!viewerActive) return; // KeepAlive 停用期间不跑（激活时恢复）
  if (frameTotal.value < 2) return; // 单帧无动画可言，避免空转
  if (!firstSettled) return; // 首帧未落定前不推进（首屏始终先停在首帧）
  if (readyFrames.size === 0) return; // 一张都没就绪：落定后自行续上
  const delay = Math.max(20, props.ugoiraFrames[framePos.value]?.delay ?? 0);
  frameTimer = window.setTimeout(advanceFrame, delay);
}

function advanceFrame(): void {
  frameTimer = undefined;
  const next = nextReadyFrame();
  if (next < 0) return; // 无就绪帧：预载落定后会重新起表
  frameIndex.value = next;
  scheduleFrame();
}

/**
 * 逐帧预载：每帧各自 new Image()，就绪即记入集合；首帧（下标 0）落定后开始计时，
 * 其余帧不阻塞播放（未就绪的帧在推进时跳过）。
 * 换作品（ugoiraFrames 变化）与挂载各走一次，重复调用先清计时器、重置到首帧。
 */
function preloadUgoiraFrames(): void {
  // 先无条件停表并作废旧代次：帧被清空（换作品 / 重试）时旧计时器必须立即失效
  stopFrameTimer();
  frameIndex.value = 0;
  frameBroken.value = false;
  frameReady.value = false;
  frameGen += 1;
  const gen = frameGen;
  readyFrames = new Set<number>();
  brokenFrames = new Set<number>();
  firstSettled = false;
  if (!ugoiraPlayable.value) return;
  props.ugoiraFrames.forEach((frame, index) => {
    const img = new Image();
    img.decoding = "async";
    // 就绪与失败都算「落定」：坏帧不入集合（推进时跳过），但不会拖住整段播放
    const settle = (ok: boolean): void => {
      if (gen !== frameGen) return; // 过期预载（已换作品）
      if (ok) {
        readyFrames.add(index);
        // 当前帧尚未就绪（首帧即坏 / 跳帧后落空）：直接切到这张已就绪的帧
        if (!readyFrames.has(framePos.value)) frameIndex.value = index;
      } else {
        brokenFrames.add(index);
      }
      if (index === 0) firstSettled = true;
      // 播放中且表已空（等首帧 / 中途缺帧）：落定后重新起表
      if (frameTimer === undefined) scheduleFrame();
    };
    img.onload = () => settle(true);
    img.onerror = () => settle(false);
    img.src = frame.src;
  });
  // 首帧可能已在该浏览器缓存中同步命中（onload 在 src 赋值时即触发），此处兜底再试一次
  if (frameTimer === undefined) scheduleFrame();
}

/** 帧图 load：首帧落定后写纵横比（复用 correctAspect）并放开全屏入口。
 *  后续帧尺寸可能不同，不再改写页框（否则播放中抖动）。 */
function onUgoiraFrameLoad(e: Event): void {
  if (frameReady.value) return;
  correctAspect(0, e);
  frameReady.value = true;
}

/** 首页是否可进全屏：帧轮播时以帧图落定为准（页框不再渲染封面图，states[0] 只由帧写），
 *  其余页沿用逐页加载态。 */
function stageReady(index: number): boolean {
  if (index === 0 && ugoiraPlayable.value) return frameReady.value;
  return states.value[index] === "ok";
}

/**
 * 帧图加载失败（协议 / 文件缺失）：该帧记入损坏集合后跳过、继续放其余帧；
 * 全部帧都损坏才整体退回静态封面帧，说明行给出失败态与重试。
 */
function onUgoiraFrameError(e: Event): void {
  // 按地址反查下标：快速换帧时 error 可能晚于切帧到达，不能只信「当前帧」
  const src = (e.target as HTMLImageElement).getAttribute("src") ?? "";
  const pos = props.ugoiraFrames.findIndex((f) => f.src === src);
  if (pos < 0) return; // 已不在当前帧集合里（换作品了）
  brokenFrames.add(pos);
  readyFrames.delete(pos);
  if (brokenFrames.size >= frameTotal.value) {
    // 一张都放不出来：退回静态封面帧，说明行给失败态
    stopFrameTimer();
    frameBroken.value = true;
    frameReady.value = false;
    return;
  }
  // 只有当前显示的帧坏了才需要跳；其余帧的损坏由预载集合记录即可
  if (pos === framePos.value) {
    stopFrameTimer();
    advanceFrame();
  }
}

function toggleUgoiraPlayback(): void {
  playing.value = !playing.value;
  if (playing.value) scheduleFrame();
  else stopFrameTimer();
}

// 帧数组与动图标记任一变化都重来：父视图可能在 ugoira 判定之前就已给出帧
watch([() => props.ugoira, () => props.ugoiraFrames], preloadUgoiraFrames);

onMounted(preloadUgoiraFrames);

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

// ----- 双图（跨页）模式 / 阅读方向 / 竖屏：会话内记忆（模块级 viewerPrefs） -----

const spread = ref(viewerPrefs.spread);
const rtl = ref(viewerPrefs.rtl);
const vertical = ref(viewerPrefs.vertical);
const verticalScale = ref(viewerPrefs.verticalScale);

watch(spread, (v) => (viewerPrefs.spread = v));
watch(rtl, (v) => (viewerPrefs.rtl = v));
watch(vertical, (v) => (viewerPrefs.vertical = v));
watch(verticalScale, (v) => (viewerPrefs.verticalScale = v));

/** 竖屏（连续纵向滚动）仅多页作品生效；单页作品无滚动阅读可言（控件也随之隐藏） */
const verticalOn = computed(() => vertical.value && multi.value);
/** 双图仅对多页作品生效（单页作品没有可跨的页），且与竖屏互斥（竖屏固定单列）；
 *  `spread` 偏好本身不动：竖屏下只是不生效，退出竖屏即恢复此前选择 */
const spreadOn = computed(() => spread.value && multi.value && !verticalOn.value);
/** 阅读方向生效：仅 paged + 双图（单页一屏一张；竖屏与双图互斥，固定从上到下、不参与镜像） */
const rtlActive = computed(() => spreadOn.value && rtl.value);
/** 翻页步进：双图一屏两张，步进 2（竖屏与双图互斥，恒 1） */
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

/** 页码标签：paged 双图显示区间（第 N-M 页），竖屏固定单列即恒显示视口内最靠上的那一页 */
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

// ----- 竖屏（连续纵向滚动）：固定单列 + 懒加载窗口（paged 分支不参与） -----

/** 竖屏行模型的页视图：在 FsSlot 基础上附带「是否已进入加载窗口」。 */
interface FsSlotView {
  index: number;
  src: string;
  low: string;
  state: "loading" | "ok" | "error";
  live: boolean;
}

/** 竖屏每行一页（固定单列，与双图互斥）：DOM 全量占位（aspect-ratio 预留高度，滚动不跳动），
 * 图片仅在该页进入加载窗口（fsLive）后才挂载 —— 不一次性加载全部原图。 */
const fsVertSlots = computed<FsSlotView[]>(() =>
  props.pages.map((_, index) => ({
    index,
    src: fsSrcOf(index),
    low: fsLowOf(index),
    state: fsStates.value[index] ?? "loading",
    live: fsLive.value[index] === true,
  }))
);

const fsStageEl = ref<HTMLElement | null>(null);
/** 竖屏页元素引用（下标与 pages 对齐） */
const fsSlotEls: (HTMLElement | null)[] = [];
/** 已进入加载窗口的页（只增不减：回滚不重复请求，对齐舞台 live 的语义） */
const fsLive = ref<boolean[]>([]);
/** 当前视口内的页（胶卷标记；页码取其中最靠上的一页） */
const fsVisible = ref<number[]>([]);

function setFsSlotRef(index: number, el: unknown): void {
  fsSlotEls[index] = (el as HTMLElement | null) ?? null;
}

/** 把 [from, to] 置为已加载（对齐舞台 activate 的「只增不减」做法）。 */
function activateFs(from: number, to: number): void {
  const total = props.pages.length;
  let next: boolean[] | null = null;
  for (let i = Math.max(0, from); i <= Math.min(total - 1, to); i += 1) {
    if (!fsLive.value[i]) {
      next ??= fsLive.value.slice();
      next[i] = true;
    }
  }
  if (next) fsLive.value = next;
}

/**
 * 程序化滚动期间钉住页码（对齐舞台 scrollToPage 的 pendingFocus 做法）：平滑动画会
 * 持续触发 scroll，若不钉住，syncFsFromScroll 会把页码算回动画途中的旧页，连按 ←/→ 丢步。
 */
let fsPendingPage: number | null = null;
let fsPendingTimer = 0;

/** 竖屏：滚动到该页所在行（固定单列 = 该页本身）并对齐页顶（block: start），同时推进加载窗口。 */
function scrollToFsRow(index: number, behavior?: ScrollBehavior): void {
  const el = fsSlotEls[index];
  if (!el) return;
  const smooth = behavior !== "auto" && !prefersReducedMotion();
  fsPendingPage = smooth ? index : null;
  if (fsPendingTimer) clearTimeout(fsPendingTimer);
  if (smooth) fsPendingTimer = window.setTimeout(() => (fsPendingPage = null), 500);
  el.scrollIntoView({ block: "start", behavior: smooth ? "smooth" : "auto" });
  fsPage.value = index;
  activateFs(index, index + 1);
}

/** 滚动同步：页码 = 视口内最靠上的页；加载窗口 = 视口内各页向外各留 1 页余量。 */
function syncFsFromScroll(): void {
  const el = fsStageEl.value;
  if (!el) return;
  const box = el.getBoundingClientRect();
  let first = -1;
  let last = -1;
  const visible: number[] = [];
  for (let i = 0; i < fsSlotEls.length; i += 1) {
    const item = fsSlotEls[i];
    if (!item) continue;
    const rect = item.getBoundingClientRect();
    if (first < 0 && rect.bottom > box.top + 1) first = i;
    // 与视口相交才算可见：滚出视口上方（bottom <= top）或下方（top >= bottom）的页不计入
    if (rect.top < box.bottom - 1 && rect.bottom > box.top + 1) {
      last = i;
      visible.push(i);
    }
  }
  fsVisible.value = visible;
  if (first < 0) return;
  fsPage.value = fsPendingPage ?? first;
  activateFs(first - 1, last + 1);
}

let fsRafId = 0;

function onFsScroll(): void {
  if (!verticalOn.value || fsRafId) return;
  fsRafId = requestAnimationFrame(() => {
    fsRafId = 0;
    syncFsFromScroll();
  });
}

// ----- 竖屏宽度比例：会话内记忆（viewerPrefs），与详情页缩放设置键相互独立 -----

const FS_SCALE_MIN = 0.5;
const FS_SCALE_MAX = 1;
const FS_SCALE_STEP = 0.1;

const verticalScaleValue = computed(() =>
  Math.min(FS_SCALE_MAX, Math.max(FS_SCALE_MIN, verticalScale.value))
);

function stepVerticalScale(delta: number): void {
  const next = Math.round((verticalScaleValue.value + delta * FS_SCALE_STEP) * 100) / 100;
  verticalScale.value = Math.min(FS_SCALE_MAX, Math.max(FS_SCALE_MIN, next));
}

function resetVerticalScale(): void {
  verticalScale.value = 1;
}

/** 宽度比例变化会重排各页高度（同一 scrollTop 落到别的页），而用户没动滚动条、不产生
 *  scroll 事件：等布局落定后重算视口内的页，胶卷标记与页码立即跟上。 */
watch(verticalScaleValue, async () => {
  if (!fullscreen.value || !verticalOn.value) return;
  await nextTick();
  syncFsFromScroll();
});

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
  if (props.restricted || !stageReady(index)) return;
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

/** 双图模式按「跨页对」对齐（1-2 / 3-4 …）：点胶卷任意一页都落到所属跨页。
 *  竖屏固定单列：不改 fsPage 而是滚动到该页行首（每页自成一行）。 */
function stepTo(index: number): void {
  let next = Math.min(Math.max(index, 0), Math.max(props.pages.length - 1, 0));
  if (spreadOn.value && next % 2 === 1) next -= 1;
  if (verticalOn.value) {
    scrollToFsRow(next);
    return;
  }
  fsPage.value = next;
}

function stepPage(delta: number): void {
  stepTo(fsPage.value + delta * pageStep.value);
}

/** 一次滚轮手势限速，防止触控板惯性或密集滚轮事件连续跳过多页。胶卷仍原生滚动。
 *  竖屏模式下不拦截：滚轮交给原生连续滚动（条漫自由浏览，无整页翻页）。 */
let lastWheelPageAt = -Infinity;
function onFullscreenWheel(e: WheelEvent): void {
  if (verticalOn.value || !multi.value || e.ctrlKey || e.metaKey || !e.deltaY || Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
  if (e.composedPath().some(node => node instanceof HTMLElement && node.classList.contains("fs-filmstrip"))) return;
  e.preventDefault();
  const now = performance.now();
  if (now - lastWheelPageAt < 250) return;
  lastWheelPageAt = now;
  stepPage(e.deltaY > 0 ? 1 : -1);
}

/** 双图开关（仅 paged 分支可见）：开启后按跨页对对齐，关掉或开启都保持当前页。 */
function toggleSpread(): void {
  spread.value = !spread.value;
  if (spreadOn.value) stepTo(fsPage.value);
}

function toggleRtl(): void {
  rtl.value = !rtl.value;
}

/** 竖屏开关：进出都保持当前页——回 paged 时跨页对齐行首；进竖屏时滚动定位到该页行首
 *  （DOM 切换是异步的，须等 nextTick 后再滚动与聚焦，焦点给滚动容器让 ↑/↓ 立即可用）。
 *  竖屏与双图互斥：进竖屏即按单列渲染（双页开关随之隐藏），退出后恢复此前选择。 */
function toggleVertical(): void {
  vertical.value = !vertical.value;
  if (!verticalOn.value && spreadOn.value) stepTo(fsPage.value);
  void nextTick(() => {
    if (!verticalOn.value) return;
    activateFs(fsPage.value, fsPage.value + 1);
    scrollToFsRow(fsPage.value, "auto");
    syncFsFromScroll();
    fsStageEl.value?.focus({ preventScroll: true });
  });
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
 * 浮层键盘：Esc 关闭、PgUp/PgDn 前后翻页、←/→ 翻页（从右往左时 ← 为前进）。capture 阶段监听并
 * stopImmediatePropagation，阻止 BrowseWorkView 的 bubble 监听把同一次 Esc 变成路由返回。
 * 纵向舞台（非浮层）的 ←/→ 为「跳上一页/下一页」（滚动对齐页顶），同样在此拦截；
 * 竖屏浮层内 ←/→ 对齐上下页行首（不做 rtl 镜像），↑/↓ 与 PgUp/PgDn 交给原生滚动与
 * 全局按屏滚动（usePageScroll 会把本浮层舞台识别为模态内滚动容器），不在此拦截。
 */
function onKeydown(e: KeyboardEvent): void {
  if (isTypingEvent(e) || document.querySelector("dialog[open]")) return;
  if (fullscreen.value) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopImmediatePropagation();
      closeFullscreen();
    } else if (verticalOn.value) {
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        e.stopImmediatePropagation();
        stepPage(e.key === "ArrowLeft" ? -1 : 1);
      }
    } else if ((e.key === "PageUp" || e.key === "PageDown") && !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
      // 鼠标或焦点落在胶卷滚动区时，交给公共一屏滚动，避免切换作品页。
      if (pageScrollTarget(e)?.classList.contains("fs-filmstrip")) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      stepPage(e.key === "PageUp" ? -1 : 1);
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
  window.dispatchEvent(new CustomEvent('pixiv-tool:image-fullscreen', { detail: open }));
  if (open) {
    lastWheelPageAt = -Infinity;
    await nextTick();
    if (verticalOn.value) {
      // 竖屏：定位到进入前停留的页（行首对齐），焦点给滚动容器让 ↑/↓ 立即可用
      activateFs(fsPage.value, fsPage.value + 1);
      scrollToFsRow(fsPage.value, "auto");
      syncFsFromScroll();
      fsStageEl.value?.focus({ preventScroll: true });
    } else {
      overlayEl.value?.focus();
    }
  }
});

watch(
  () => props.pages,
  () => resetPages(),
  { immediate: true }
);

/**
 * 详情页进 KeepAlive：停用期间摘掉 window 监听，否则 ←/→ 会在别的页面上翻本页的图。
 */
function attachWindowListeners(): void {
  window.addEventListener("keydown", onKeydown, { capture: true });
  window.addEventListener("resize", onScroll, { passive: true });
  // 窗口尺寸变化同样重排竖屏各页高度却不产生 scroll 事件：与滚动同路（rAF 限流）重算视口内的页
  window.addEventListener("resize", onFsScroll, { passive: true });
}

function detachWindowListeners(): void {
  window.removeEventListener("keydown", onKeydown, { capture: true });
  window.removeEventListener("resize", onScroll);
  window.removeEventListener("resize", onFsScroll);
}

onMounted(() => {
  attachWindowListeners();
  syncFromScroll();
});

onActivated(() => {
  viewerActive = true;
  attachWindowListeners();
  const el = scrollEl.value;
  if (el) el.scrollTop = savedScrollTop;
  syncFromScroll();
  // 停用期间计时器已清：此前处于播放态就接着播（playing 未被停用改写）
  if (playing.value) scheduleFrame();
});

onDeactivated(() => {
  viewerActive = false;
  detachWindowListeners();
  stopFrameTimer();
});

onBeforeUnmount(() => {
  viewerActive = false;
  window.dispatchEvent(new CustomEvent("pixiv-tool:image-fullscreen", { detail: false }));
  detachWindowListeners();
  stopFrameTimer();
  if (rafId) cancelAnimationFrame(rafId);
  if (pendingTimer) clearTimeout(pendingTimer);
  if (fsRafId) cancelAnimationFrame(fsRafId);
  if (fsPendingTimer) clearTimeout(fsPendingTimer);
});

/** 遮罩解除（设置里打开 show_r18）后补一次窗口同步：滚动容器回到多页布局时无需等滚动。
 *  ugoira 同时停/续帧计时器（遮罩期间不播放）。 */
watch(
  () => props.restricted,
  async () => {
    if (props.restricted) stopFrameTimer();
    else if (playing.value) scheduleFrame();
    await nextTick();
    syncFromScroll();
  }
);

/** 预加载相邻页：浮层翻页时立即可见（fire-and-forget），与浮层同一档位。
 *  竖屏的加载窗口由 activateFs 随滚动推进，不走这条相邻页预载。 */
watch(
  () => [fullscreen.value, fsPage.value, pageStep.value, fullscreenTier.value] as const,
  () => {
    if (!fullscreen.value || verticalOn.value) return;
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
      <!-- 纵向舞台：滚动渐进加载；遮罩期间仅保留首页（其余 v-show 收起）；
           --viewer-scale = 页框宽度占比（1 = 撑满 = 既有观感，见 .shot 宽度计算） -->
      <div
        ref="scrollEl"
        class="stage-scroll"
        :class="{ 'is-locked': restricted, 'is-single': restricted || pages.length < 2 }"
        :style="{ '--viewer-scale': String(stageScaleValue) }"
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
          <!-- 页框：纵横比先占位，图片加载完成后不再改变高度；页框本身即全屏入口
               （点击 / Enter / Space，openFullscreen 内有就绪与遮罩守卫），遮罩态不可聚焦 -->
          <div
            class="shot"
            :class="{ ready: stageReady(i) && !restricted }"
            :style="shotStyle(i)"
            :tabindex="restricted ? -1 : 0"
            role="button"
            :aria-label="`${altOf(i)} · ${t('browse.work.fullscreen')}`"
            :title="stageReady(i) && !restricted ? t('browse.work.fullscreen') : undefined"
            @click="openFullscreen(i)"
            @keydown.enter.prevent="openFullscreen(i)"
            @keydown.space.prevent="openFullscreen(i)"
          >
            <!-- ugoira 帧轮播：单张 <img> 换 src（不重挂载，避免闪烁）；帧地址与封面同源，
                 已预载的帧直接复用缓存。点击/键盘仍按既有行为进全屏（首帧就绪后开放）；
                 限定首页（ugoira 只有 1 页，多页数据异常时其余页仍走普通封面分支） -->
            <img
              v-if="i === 0 && ugoiraPlayable"
              class="shot-img ugoira-img"
              :class="{ blurred: restricted }"
              :src="frameSrc"
              :alt="altOf(0)"
              decoding="async"
              @load="onUgoiraFrameLoad"
              @error="onUgoiraFrameError($event)"
            />
            <template v-else>
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
            </template>
          </div>
        </div>
      </div>

      <!-- ugoira 播放控件：舞台右下角胶囊（播放/暂停 + 帧计数）。放在舞台层而非页框内：
           页框本身是 role="button" 的全屏入口，交互控件不得嵌在其中；
           遮罩期间不渲染（遮罩层已盖住内容，也不该操作） -->
      <div v-if="ugoiraPlayable && !restricted" class="ugoira-controls">
        <md-icon-button
          class="ugoira-toggle"
          :disabled="frameTotal < 2"
          :aria-label="animating ? t('browse.work.ugoiraPause') : t('browse.work.ugoiraPlay')"
          :title="animating ? t('browse.work.ugoiraPause') : t('browse.work.ugoiraPlay')"
          @click="toggleUgoiraPlayback"
        >
          <!-- 播放中显示暂停（lucide pause），暂停中显示播放（lucide play） -->
          <svg v-if="animating" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="14" y="3" width="5" height="18" rx="1" />
            <rect x="5" y="3" width="5" height="18" rx="1" />
          </svg>
          <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z" />
          </svg>
        </md-icon-button>
        <span class="ugoira-count" aria-live="off">
          {{ t("browse.work.ugoiraFrame", { current: frameIndex + 1, total: ugoiraFrames.length }) }}
        </span>
      </div>

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

    <!-- ugoira 说明行：加载中 → 失败（可重试）→ 无帧退回「去 Pixiv 查看动画」旧文案；
         可播放且无加载/失败时不占位。失败态不展示后端原文，原文经 title 悬停可读 -->
    <p v-if="ugoira && (ugoiraLoading || ugoiraError || !ugoiraPlayable)" class="ugoira-note" :title="ugoiraError || undefined">
      <template v-if="ugoiraLoading">
        <svg class="ugoira-spinner" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <!-- lucide loader-circle -->
          <path d="M21 12a9 9 0 1 1-6.219-8.56" />
        </svg>
        {{ t("browse.work.ugoiraLoading") }}
      </template>
      <template v-else-if="ugoiraError || frameBroken">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <!-- lucide circle-alert -->
          <circle cx="12" cy="12" r="10" />
          <path d="M12 8v4" />
          <path d="M12 16h.01" />
        </svg>
        {{ t("browse.work.ugoiraFallback") }}
        <md-text-button class="ugoira-retry" @click="emit('retry-ugoira')">{{ t("common.retry") }}</md-text-button>
      </template>
      <template v-else>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <!-- lucide circle-play -->
          <path d="M9 9.003a1 1 0 0 1 1.517-.859l4.997 2.997a1 1 0 0 1 0 1.718l-4.997 2.997A1 1 0 0 1 9 14.996z" />
          <circle cx="12" cy="12" r="10" />
        </svg>
        {{ t("browse.work.ugoiraNote") }}
      </template>
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
      @wheel="onFullscreenWheel"
    >
      <div
        class="fs-stage"
        :class="{ 'is-spread': spreadOn, 'is-vertical': verticalOn }"
        :style="{ '--fs-scale': String(verticalScaleValue) }"
        @click.self="closeFullscreen"
      >
        <!-- paged：一屏一页或并排两页（页框按可用高度与页面纵横比定尺寸，图片撑满该框）。
             此分支结构与样式是既有行为基线，竖屏走 v-else 独立行模型，互不影响 -->
        <template v-if="!verticalOn">
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
        </template>

        <!-- 竖屏：连续纵向滚动、固定单列（条漫友好），滚动由内层 .fs-scroll 承担；
             双图（跨页）与竖屏互斥，页框宽度 = 舞台宽 × 比例（--fs-scale），高度随纵横比自适应 -->
        <div
          v-else
          ref="fsStageEl"
          class="fs-scroll"
          tabindex="0"
          role="group"
          :aria-label="t('browse.work.stageLabelMulti', { total: pages.length })"
          @click.self="closeFullscreen"
          @scroll.passive="onFsScroll"
        >
          <div v-for="slot in fsVertSlots" :key="slot.index" class="fs-row">
            <div
              :ref="(el) => setFsSlotRef(slot.index, el)"
              class="fs-slot"
              :style="shotStyle(slot.index)"
            >
              <!-- 未进入加载窗口的页只留纵横比占位盒（高度预留，滚动不跳动） -->
              <template v-if="slot.live">
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
              </template>
            </div>
          </div>
        </div>

        <!-- 底部控制条：默认隐藏，指针进入底部热区或键盘聚焦时显现 -->
        <div class="fs-chrome fs-chrome-bottom">
          <div v-if="multi" class="fs-controls">
            <!-- 翻页组：从右往左时整组镜像（前进按钮落到左侧、箭头改为朝左），与跨页
                 阅读方向一致；双图 / 方向开关不是方向性控件，不参与镜像 -->
            <div class="fs-pager" :class="{ 'is-rtl': rtlActive }">
              <md-icon-button :disabled="!canPrev" :aria-label="t('browse.work.prevPage')" :title="t('browse.work.prevPage')" @click="stepPage(-1)">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path :d="rtlActive ? 'm9 18 6-6-6-6' : 'm15 18-6-6 6-6'" /></svg>
              </md-icon-button>
              <span class="fs-label" aria-live="polite">{{ pageLabel }}</span>
              <md-icon-button :disabled="!canNext" :aria-label="t('browse.work.nextPage')" :title="t('browse.work.nextPage')" @click="stepPage(1)">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path :d="rtlActive ? 'm15 18-6-6 6-6' : 'm9 18 6-6-6-6'" /></svg>
              </md-icon-button>
            </div>
            <span class="fs-divider" aria-hidden="true"></span>
            <!-- 双图（跨页）开关：与竖屏互斥——竖屏固定单列，开关随之隐藏（退出竖屏恢复可见） -->
            <md-icon-button
              v-if="!verticalOn"
              toggle
              :selected="spreadOn"
              :aria-label="t('browse.work.spread')"
              :title="t('browse.work.spread')"
              @click="toggleSpread"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <!-- lucide book-open：双图（跨页） -->
                <path d="M12 5v16" />
                <path d="M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z" />
              </svg>
            </md-icon-button>
            <!-- 竖屏模式（连续纵向滚动）：固定单列、从上到下，与双图（跨页）互斥
                 （双页与方向开关随之隐藏、翻页组不镜像） -->
            <md-icon-button
              toggle
              :selected="verticalOn"
              :aria-label="t('browse.work.verticalMode')"
              :title="t('browse.work.verticalMode')"
              @click="toggleVertical"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <!-- lucide move-vertical：竖屏（连续纵向滚动） -->
                <path d="M12 2v20" />
                <path d="m8 18 4 4 4-4" />
                <path d="m8 6 4-4 4 4" />
              </svg>
            </md-icon-button>
            <!-- 阅读方向：从右往左 / 从左往右（仅双图模式；竖屏与双图互斥、固定从上到下） -->
            <md-icon-button
              v-if="spreadOn"
              :aria-label="t('browse.work.readDir', { dir: rtl ? t('browse.work.dirRtl') : t('browse.work.dirLtr') })"
              :title="t('browse.work.readDir', { dir: rtl ? t('browse.work.dirRtl') : t('browse.work.dirLtr') })"
              @click="toggleRtl"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <!-- 阅读方向：从右往左 → lucide arrow-left-to-line；从左往右 → lucide arrow-right-to-line -->
                <template v-if="rtl"><path d="M3 19V5" /><path d="m13 6-6 6 6 6" /><path d="M7 12h14" /></template>
                <template v-else><path d="M17 12H3" /><path d="m11 18 6-6-6-6" /><path d="M21 5v14" /></template>
              </svg>
            </md-icon-button>
            <!-- 竖屏宽度比例（会话内记忆，独立于详情页缩放设置键）：[−] 值 [+] 重置 -->
            <div v-if="verticalOn" class="fs-scale" role="group" :aria-label="t('browse.work.imageZoomLabel')">
              <span class="fs-divider" aria-hidden="true"></span>
              <md-icon-button
                :disabled="verticalScaleValue <= FS_SCALE_MIN"
                :aria-label="t('browse.work.imageZoomOut')"
                :title="t('browse.work.imageZoomOut')"
                @click="stepVerticalScale(-1)"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10" /><path d="M8 12h8" /></svg>
              </md-icon-button>
              <span class="fs-scale-value" aria-live="polite">{{ Math.round(verticalScaleValue * 100) }}%</span>
              <md-icon-button
                :disabled="verticalScaleValue >= FS_SCALE_MAX"
                :aria-label="t('browse.work.imageZoomIn')"
                :title="t('browse.work.imageZoomIn')"
                @click="stepVerticalScale(1)"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10" /><path d="M8 12h8" /><path d="M12 8v8" /></svg>
              </md-icon-button>
              <md-icon-button
                :disabled="verticalScaleValue === 1"
                :aria-label="t('browse.work.imageZoomReset')"
                :title="t('browse.work.imageZoomReset')"
                @click="resetVerticalScale"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" /><path d="M3 3v5h5" /></svg>
              </md-icon-button>
            </div>
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
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <!-- lucide x -->
              <path d="M18 6 6 18" /><path d="m6 6 12 12" />
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
          :class="{ active: verticalOn ? fsVisible.includes(i) : visiblePages.includes(i) }"
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

/* 无自有底色：舞台背景即页面底色（随主题），图片满幅铺满 */
.stage {
  position: relative;
  display: flex;
  flex: 1;
  min-height: 320px;
  border-radius: 16px;
  background: transparent;
  overflow: hidden;
}

/* 纵向滚动舞台：逐页排列，滚动渐进加载 */
.stage-scroll {
  flex: 1;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  /* 滚动条与图片留 6px 间距（与 BrowseWorkView 信息列同配方），图片不贴靠滚动条；
   * 左侧不加内边距，图片仍从舞台左缘满幅开始。 */
  padding: 0 var(--space-xs) 0 0;
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

/* 键盘焦点环：应用标准主色（与全局 focus-visible 口径一致） */
.stage-scroll:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: calc(-1 * var(--space-xxs));
}

/* 舞台滚动条细化：常显 6px 细条，拇指用 outline 派生描边色（与 .info-col 同配方） */
.stage-scroll::-webkit-scrollbar {
  width: 6px;
}

.stage-scroll::-webkit-scrollbar-track {
  background: transparent;
}

.stage-scroll::-webkit-scrollbar-thumb {
  background: color-mix(in srgb, var(--md-sys-color-outline) 40%, transparent);
  border-radius: 999px;
}

.stage-scroll::-webkit-scrollbar-thumb:hover {
  background: color-mix(in srgb, var(--md-sys-color-outline) 60%, transparent);
}

.page-item {
  display: flex;
  justify-content: center;
  /* 舞台上缘无内边距，翻页滚动时页顶直接贴齐舞台顶 */
  scroll-margin-top: 0;
}

.page-item + .page-item {
  margin-top: var(--space-lg);
}

/* 页框：纵横比先行占位（--ar-w / --ar-h 为内联样式），加载完成后高度不变；
 * 直角（0 圆角）：图片满幅，整体圆角观感由舞台 16px 圆角 + 裁剪负责；
 * 未就绪时铺骨架色占位块（与全应用骨架配方一致，浅底可见） */
.shot {
  position: relative;
  /* 宽度占比由 --viewer-scale 控制（详情页顶栏缩放控件写入；缺省 1 = 撑满 = 既有观感），
   * 居中由 .page-item 的 flex + justify-content: center 承担 */
  width: calc(100% * var(--viewer-scale, 1));
  aspect-ratio: var(--ar-w, 2) / var(--ar-h, 3);
  border-radius: 0;
  background: var(--md-sys-color-surface-container);
  overflow: hidden;
}

.shot.ready {
  cursor: zoom-in;
}

/* 键盘焦点：应用标准主色 2px 环（对齐 .stage-scroll:focus-visible 配方） */
.shot:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: -2px;
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
  background: var(--md-sys-color-surface-container);
}

.stage-text {
  margin: 0;
  color: var(--ink-muted);
  font-size: 14px;
  line-height: 1.5;
}

.stage-text.strong {
  color: var(--ink);
  font-weight: 600;
}

/* 深色背板上的文案覆盖回白：R-18 遮罩洗层与全屏浮层（基色已随主题为浅色） */
.restrict-overlay .stage-text {
  color: rgb(255 255 255 / 0.9);
}

.restrict-overlay .stage-text.strong {
  color: #fff;
}

.fs-overlay .stage-text {
  color: rgb(255 255 255 / 0.85);
}

.fs-overlay .stage-text.strong {
  color: #fff;
}

/* 页码徽标：浮于图片之上的深色胶囊 + 白字（阅读器惯例，不引入新颜色） */
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

/* 加载中图标：16px 旋转环（系统偏好减少动效时静止，只留图标表意） */
.ugoira-note svg.ugoira-spinner {
  width: 16px;
  height: 16px;
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .ugoira-note svg.ugoira-spinner {
    animation: none;
  }
}

/* 失败态的重试按钮：与说明行同字号层级，收紧 Material 默认水平内距以免整行过高 */
.ugoira-note .ugoira-retry {
  --md-text-button-container-height: calc(var(--space-xl) + var(--space-xxs));
  --md-text-button-leading-space: var(--space-sm);
  --md-text-button-trailing-space: var(--space-sm);
  --md-text-button-label-text-size: 12px;
  --md-text-button-label-text-weight: 600;
}

/* ugoira 播放控件：舞台右下角胶囊（32px 高）。浮于图片之上，与页码徽标同位置策略、
 * 同配色配方（近黑底 + 白字，任意画面下可读）；但需响应指针（不 pointer-events: none） */
.ugoira-controls {
  position: absolute;
  right: var(--space-sm);
  bottom: var(--space-sm);
  z-index: 1;
  display: flex;
  /* 32px = 2 × space-lg（与 CopyPathButton 等 32px 控件的写法一致） */
  height: calc(var(--space-lg) * 2);
  align-items: center;
  gap: var(--space-xs);
  padding: 0 var(--space-sm);
  border-radius: var(--radius-control);
  background: rgb(0 0 0 / 0.78);
}

/* 32px 胶囊内的图标按钮收敛到 28px 触控域（Material 默认 40px 会撑破胶囊高度）；图标与状态层走白色系 */
.ugoira-toggle {
  --md-icon-button-state-layer-height: calc(var(--space-xl) + var(--space-xxs));
  --md-icon-button-state-layer-width: calc(var(--space-xl) + var(--space-xxs));
  --md-icon-button-icon-size: 16px;
  --md-icon-button-icon-color: rgb(255 255 255 / 0.92);
  --md-icon-button-hover-state-layer-color: rgb(255 255 255 / 0.92);
  --md-icon-button-focus-state-layer-color: rgb(255 255 255 / 0.92);
  --md-icon-button-pressed-state-layer-color: rgb(255 255 255 / 0.92);
}

.ugoira-toggle svg {
  width: 16px;
  height: 16px;
  stroke-width: 2;
}

.ugoira-count {
  color: rgb(255 255 255 / 0.92);
  font-size: 12px;
  font-weight: 600;
  white-space: nowrap;
}

/* ===== 全屏浮层 ===== */

.fs-overlay {
  position: fixed;
  inset: 0;
  /* 层级沿用应用约定：页面级浮层 800 < 抽屉 900 < snackbar 1000 */
  z-index: 800;
  display: flex;
  padding: var(--space-lg);
  /* 全屏背板：深色遮罩（既定设计，深色图片浏览器惯例） */
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

/* ===== 竖屏（连续纵向滚动）：以下规则全部挂在 .fs-stage.is-vertical 作用域下，
   paged 分支在结构与样式上都拿不到它们 ===== */

/* 舞台转块布局，滚动由内层 .fs-scroll 承担——控制条 / 关闭按钮仍以 .fs-stage 为
 * 定位包含块，钉在舞台边缘不随内容滚走（触屏设备没有 Esc，关闭按钮不可被滚没） */
.fs-stage.is-vertical {
  display: block;
}

.fs-stage.is-vertical .fs-scroll {
  height: 100%;
  overflow-x: hidden;
  overflow-y: auto;
  padding-right: var(--space-xs);
  outline: none;
}

/* 键盘焦点环：近黑底上的既定白 2px 环（与 .fs-thumb.active 同配方） */
.fs-stage.is-vertical .fs-scroll:focus-visible {
  outline: 2px solid #fff;
  outline-offset: -2px;
}

/* 滚动条与胶卷同 recipe（6px 白半透明，常显） */
.fs-stage.is-vertical .fs-scroll::-webkit-scrollbar {
  width: 6px;
}

.fs-stage.is-vertical .fs-scroll::-webkit-scrollbar-track {
  background: rgb(255 255 255 / 0.06);
  border-radius: 999px;
}

.fs-stage.is-vertical .fs-scroll::-webkit-scrollbar-thumb {
  background: rgb(255 255 255 / 0.4);
  border-radius: 999px;
}

.fs-stage.is-vertical .fs-scroll::-webkit-scrollbar-thumb:hover {
  background: rgb(255 255 255 / 0.6);
}

/* 行模型：固定单列（每行一页，双图与竖屏互斥），页框水平居中，行间距 16px */
.fs-stage.is-vertical .fs-row {
  display: flex;
  align-items: flex-start;
  justify-content: center;
}

.fs-stage.is-vertical .fs-row + .fs-row {
  margin-top: var(--space-lg);
}

/* 页框宽度 = 舞台宽 × 比例（--fs-scale 由控制条宽度组写入）；高度随纵横比自适应，
 * 不设 max-height——条漫长图不压扁（覆盖 .fs-slot 的高度封顶与可用宽取小） */
.fs-stage.is-vertical .fs-slot {
  width: calc(var(--fs-scale, 1) * 100%);
  max-height: none;
}

/* 竖屏宽度比例组（与详情页顶栏缩放组同规格；浮层内为白系配色） */
.fs-scale {
  display: inline-flex;
  align-items: center;
  gap: var(--space-sm);
}

.fs-scale-value {
  min-width: calc(2 * var(--space-xl));
  color: rgb(255 255 255 / 0.85);
  font-size: 12px;
  font-weight: 600;
  text-align: center;
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

/* 全屏背板上的图标按钮统一白色图标（与浮层文案同色） */
.fs-controls md-icon-button,
.fs-close {
  --md-icon-button-icon-color: #fff;
  --md-icon-button-hover-icon-color: #fff;
  --md-icon-button-focus-icon-color: #fff;
  --md-icon-button-pressed-icon-color: #fff;
  --md-icon-button-disabled-icon-color: #fff;
}

.fs-controls svg {
  width: 20px;
  height: 20px;
  stroke-width: 2;
}
</style>
