<script setup lang="ts">
/**
 * 小说阅读器整页（/browse/work/novel/:id，browse-ui-v1 / F4）。
 *
 * 结构：吸顶顶栏（返回 / 标题 / 作者 / 收藏 / 评论 / 返填 / 在浏览器中打开）+ 居中 720px 正文列
 * （信息头 → NovelContent 分页正文 → 下一话 → 面板）+ 底部吸底翻页器。
 * 面板 = 相关推荐（默认）/ 评论，由顶栏评论按钮切换，评论按需分页拉取。
 * 数据来自 browseWorkDetail("novel", id)，相关推荐 browseRelated("novel", id, 12) 一次性。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { goBack } from "../../router/navigation";
import AppPagination from "../../components/common/AppPagination.vue";
import NovelContent from "../../components/browse/NovelContent.vue";
import CommentsSection from "../../components/browse/CommentsSection.vue";
import NovelBgPicker from "../../components/browse/NovelBgPicker.vue";
import NovelTranslationMenu from "../../components/browse/NovelTranslationMenu.vue";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import BookmarkButton from "../../components/browse/BookmarkButton.vue";
import {
  browseHistoryRecord,
  browseRelated,
  browseWorkDetail,
  errorMessage,
  pxSrc,
  type BrowseNovelDetail,
  type BrowseWorkItem,
  type WorkBookmarkState,
} from "../../api/browse";
import { notify } from "../../ui/notify";
import { useSettingsStore } from "../../stores/settings";
import { fillDownloadForm, openInBrowser } from "../../utils/pixivHooks";
import { pixivWorkUrl } from "../../utils/pixivUrl";
import { descriptionText } from "../../utils/descriptionText";
import { getNovelTranslation, translateNovelPage, type NovelTranslationInput, type TranslatedLine, type TranslationMode } from "../../api/translation";

const props = defineProps<{ kind: "illust" | "manga" | "novel"; id: number }>();

const { t } = useI18n();
const router = useRouter();
const settings = useSettingsStore();

// ===== 正文字号缩放 =====

/** 缩放区间与步进（与设置键 novel_font_scale 的后端校验区间一致）。 */
const SCALE_MIN = 0.75;
const SCALE_MAX = 2;
const SCALE_STEP = 0.1;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** 正文缩放系数：以 settings.novel_font_scale 为准，越界 / 缺省容错后挂到 --novel-scale。 */
const fontScale = computed(() => clamp(settings.settings.novel_font_scale || 1, SCALE_MIN, SCALE_MAX));

/** 步进 ±0.1（取两位小数）；到边界直接返回，持久化失败仅提示、不打断阅读。 */
function stepScale(delta: number): void {
  const next = Math.round((fontScale.value + delta * SCALE_STEP) * 100) / 100;
  const value = clamp(next, SCALE_MIN, SCALE_MAX);
  if (value === fontScale.value) return;
  settings.saveSettings({ novel_font_scale: value }).catch((err) => notify(errorMessage(err)));
}

/** 重置为默认 100%；已在默认值时直接返回，持久化失败仅提示、不打断阅读。 */
function resetScale(): void {
  if (fontScale.value === 1) return;
  settings.saveSettings({ novel_font_scale: 1 }).catch((err) => notify(errorMessage(err)));
}

// ===== 阅读背景色 =====

/** 阅读背景语义键：以 settings.novel_bg_color 为准，空串 = 跟随主题（未知值同样回落默认）。 */
const readBg = computed(() => {
  const value = settings.settings.novel_bg_color || "";
  return ["green", "kraft", "warm", "mist", "blush"].includes(value) ? value : "";
});

/** 选色立即应用并持久化；失败仅提示、不打断阅读（与 stepScale 同策略）。 */
function setReadBg(value: string): void {
  if (value === readBg.value) return;
  settings.saveSettings({ novel_bg_color: value }).catch((err) => notify(errorMessage(err)));
}

// ===== 详情 =====

const detail = shallowRef<BrowseNovelDetail | null>(null);
const loading = ref(false);
const error = ref("");
/** 查看者收藏态（来自详情响应 bookmarkState；收藏按钮经 change 回写） */
const bookmarkState = ref<WorkBookmarkState | null>(null);
/** 中间滚动层：顶栏/底栏恒定贴边，只有正文区滚动（滚动复位目标）。 */
const readerScrollEl = ref<HTMLElement | null>(null);

const item = computed(() => detail.value?.item ?? null);
const plainDescription = computed(() => descriptionText(item.value?.description));
const content = computed(() => detail.value?.content ?? "");
const hasContent = computed(() => content.value.trim().length > 0);
/** 内嵌图 id → URL（详情响应 embedded_images）；正文渲染时按 id 取图 */
const embeddedImages = computed(() => detail.value?.embedded_images ?? {});
const series = computed(() => detail.value?.series ?? null);
const nextEpisodeId = computed(() => series.value?.next_id ?? null);

const translationMode = ref<TranslationMode>("bilingual");
const translations = ref<Record<string, TranslatedLine[]>>({});
/** 页 → 目标语言 code：该页原文已是目标语言，未请求模型（再次点击可强制翻译）。 */
const skippedPages = ref<Record<number, string>>({});
const translating = ref(false);
const translatingPage = ref(0);
const translationError = ref("");
const translationErrorPage = ref(0);
let translationGeneration = 0;
let cacheGeneration = 0;
const translationInput = computed<NovelTranslationInput>(() => ({
  novel_id: props.id, title: item.value?.title ?? "", tags: item.value?.tags ?? [],
  description: plainDescription.value, content: content.value,
}));
const currentTranslation = computed(() => translations.value[page.value] ?? []);
const currentSkipped = computed(() => skippedPages.value[page.value] ?? "");
// 设定集整理/精翻等内部阶段不面向用户，只回显统一的进行中状态与最终成败。
const translationStatus = computed(() => {
  if (translating.value) return t("translation.translating", { page: translatingPage.value });
  if (translationErrorPage.value === page.value && translationError.value) return translationError.value;
  if (currentSkipped.value) return t("translation.sameLanguage", { language: t(`translation.targetLanguages.${currentSkipped.value}`) });
  return currentTranslation.value.length ? t("translation.completed") : t("translation.noTranslation");
});

async function translatePage(): Promise<void> {
  if (translating.value || !hasContent.value) return;
  const generation = translationGeneration;
  const cacheVersion = cacheGeneration;
  const targetPage = page.value;
  translating.value = true;
  translatingPage.value = targetPage;
  translationError.value = "";
  try {
    // 本页已提示「无需翻译」时再次点击即强制翻译，避免语言判定误判后无法覆盖。
    const force = Boolean(translations.value[targetPage]) || Boolean(skippedPages.value[targetPage]);
    const result = await translateNovelPage(translationInput.value, targetPage, force);
    if (generation === translationGeneration && cacheVersion === cacheGeneration) {
      if (result.status === "already_target_language") {
        skippedPages.value = { ...skippedPages.value, [targetPage]: result.target_language };
      } else {
        const skipped = { ...skippedPages.value };
        delete skipped[targetPage];
        skippedPages.value = skipped;
        translations.value[targetPage] = result.lines;
      }
    }
  } catch (err) {
    if (generation === translationGeneration && cacheVersion === cacheGeneration) {
      translationError.value = errorMessage(err);
      translationErrorPage.value = targetPage;
    }
  } finally {
    if (generation === translationGeneration && cacheVersion === cacheGeneration) translating.value = false;
  }
}

async function restoreTranslations(generation: number): Promise<void> {
  const cacheVersion = cacheGeneration;
  try {
    const book = await getNovelTranslation(translationInput.value);
    if (generation === translationGeneration && cacheVersion === cacheGeneration) translations.value = { ...book.pages, ...translations.value };
  } catch (err) {
    if (generation === translationGeneration && cacheVersion === cacheGeneration) {
      translationError.value = errorMessage(err);
      translationErrorPage.value = page.value;
    }
  }
}

function onTranslationCacheCleared(): void {
  cacheGeneration++;
  translations.value = {};
  skippedPages.value = {};
  translating.value = false;
  translationError.value = "";
}
onMounted(() => window.addEventListener("pixiv-tool:translation-cache-cleared", onTranslationCacheCleared));
onBeforeUnmount(() => window.removeEventListener("pixiv-tool:translation-cache-cleared", onTranslationCacheCleared));

const restricted = computed(() => item.value?.x_restrict === 1 || item.value?.x_restrict === 2);
const restrictedLabel = computed(() =>
  item.value?.x_restrict === 2 ? t("common.browseR18G") : t("common.browseR18")
);

/** 元信息：字数 / 阅读时长 / 点赞、收藏、浏览数（缺失计数不冒充零）。 */
const metaText = computed(() => {
  const it = item.value;
  if (!it) return "";
  const parts: string[] = [];
  if (it.text_length != null) parts.push(t("browse.novel.words", { count: it.text_length.toLocaleString() }));
  if (it.reading_time != null) parts.push(t("browse.novel.readingTime", { count: it.reading_time }));
  for (const [field, label] of [["like_count", "likes"], ["bookmark_count", "bookmarks"], ["view_count", "views"]] as const) {
    const count = it[field];
    if (count != null) parts.push(`${t(`browse.work.${label}`)} ${count.toLocaleString()}`);
  }
  return parts.join(" · ");
});

async function load(): Promise<void> {
  const generation = ++translationGeneration;
  translations.value = {};
  skippedPages.value = {};
  translating.value = false;
  translationError.value = "";
  translationMode.value = "bilingual";
  loading.value = true;
  error.value = "";
  detail.value = null;
  bookmarkState.value = null;
  relatedItems.value = [];
  relatedError.value = "";
  panel.value = "related";
  // 进入/切换作品回到页首（SPA 内路由切换会保留上一页滚动位置）
  readerScrollEl.value?.scrollTo({ top: 0 });
  try {
    const data = await browseWorkDetail("novel", props.id);
    if (generation !== translationGeneration) return;
    if (data.detail_kind !== "novel") throw new Error("unexpected detail kind");
    detail.value = data;
    bookmarkState.value = data.bookmarkState ?? null;
    void restoreTranslations(generation);
    // 加载成功后上报浏览历史（失败静默）
    void browseHistoryRecord({
      workId: data.item.id,
      kind: props.kind,
      title: data.item.title,
      authorId: data.item.author_id,
      authorName: data.item.author_name,
      cover: data.item.cover ?? "",
      pageCount: data.item.page_count,
      xRestrict: data.item.x_restrict ?? 0,
    }).catch(() => {});
  } catch (err) {
    if (generation !== translationGeneration) return;
    error.value = errorMessage(err) || t("common.browseLoadFailed");
  } finally {
    if (generation === translationGeneration) loading.value = false;
  }
  if (detail.value) void loadRelated();
  // DOM 就绪后重挂 ResizeObserver（loading/error/正文分支会换掉滚动层首子节点）并回算进度
  void nextTick(() => {
    syncProgressResize();
    updateProgress();
  });
}

// ===== 翻页 =====

const page = ref(1);
const totalPages = ref(1);

function gotoPage(target: number): void {
  const clamped = Math.min(Math.max(1, target), Math.max(1, totalPages.value));
  if (clamped === page.value) return;
  page.value = clamped;
  // 切页回到正文顶部；默认瞬时滚动，自动跟随系统「减少动态效果」偏好。
  readerScrollEl.value?.scrollTo({ top: 0 });
  // 顶部若原本就是 0 则不触发 scroll 事件，切页后主动回算一次进度
  void nextTick(updateProgress);
}

/** 键盘 ←/→ 翻页；焦点在表单控件时交给控件自身。 */
function onKeydown(event: KeyboardEvent): void {
  if (!detail.value || !hasContent.value) return;
  const target = event.target as HTMLElement | null;
  if (
    target &&
    (target.tagName === "INPUT" || target.tagName === "SELECT" || target.tagName === "TEXTAREA" || target.isContentEditable)
  ) {
    return;
  }
  // md-slider 的焦点在其 shadow <input type="range"> 上，事件冒泡到 window 时 target 已被
  // 重定向为宿主元素；用 composedPath 还原真实事件源，焦点落在滑杆（role=slider / range
  // input）或输入类控件时，方向键交给控件自身调值，不触发翻页。
  const source = event.composedPath()[0];
  if (source instanceof Element && source.closest("input, select, [role=slider]")) {
    return;
  }
  if (event.key === "ArrowLeft") {
    event.preventDefault();
    gotoPage(page.value - 1);
  } else if (event.key === "ArrowRight") {
    event.preventDefault();
    gotoPage(page.value + 1);
  }
}

// ===== 阅读进度（底栏右侧 #trailing：md-slider 拉条 + 百分比回显） =====

/** 当前页内滚动进度百分比（0~100）；滚动层无余量（短内容）时恒 100。 */
const progressPercent = ref(100);

/** 依据滚动层几何回算进度：scrollTop / (scrollHeight - clientHeight)，余量 ≤ 0 视为读完。 */
function updateProgress(): void {
  const el = readerScrollEl.value;
  if (!el) return;
  const max = el.scrollHeight - el.clientHeight;
  progressPercent.value = max <= 0 ? 100 : clamp(Math.round((el.scrollTop / max) * 100), 0, 100);
}

/** 滚动跟随：被动监听滚动层，滚动即回算（拖动定位时滚回值与目标恒等，无反馈环）。 */
function onReaderScroll(): void {
  updateProgress();
}

/** 拉条快速定位：百分比 → 正文滚动位置（拖动过程 input 持续触发；短内容无可定位余量）。
 *  回显乐观同步：input 即更新 progressPercent，不依赖 scroll 事件回声——渲染被节流 /
 *  窗口不可见等场景程序化 scrollTo 不派发 scroll，只靠回声会让滑杆与百分比停在旧值；
 *  可见窗口下回声值与乐观值恒等，仅作确认，无反馈环。 */
function onProgressSeek(event: Event): void {
  const el = readerScrollEl.value;
  if (!el) return;
  const value = Number((event.target as HTMLInputElement).value);
  if (!Number.isFinite(value)) return;
  const max = el.scrollHeight - el.clientHeight;
  if (max <= 0) return;
  progressPercent.value = Math.round(clamp(value, 0, 100));
  el.scrollTo({ top: clamp(value / 100, 0, 1) * max });
}

/** 进度随内容高度变化（内嵌图懒加载 / 字号缩放 / 面板切换）与窗口高度变化回算：
 *  观察滚动层本身与其首个元素子节点（正文列）；加载 / 切页换 DOM 后需重挂（sync）。 */
let progressResize: ResizeObserver | null = null;

function syncProgressResize(): void {
  const el = readerScrollEl.value;
  if (!el || typeof ResizeObserver === "undefined") return;
  progressResize?.disconnect();
  progressResize ??= new ResizeObserver(updateProgress);
  progressResize.observe(el);
  if (el.firstElementChild) progressResize.observe(el.firstElementChild);
}

onMounted(() => {
  window.addEventListener("keydown", onKeydown);
  readerScrollEl.value?.addEventListener("scroll", onReaderScroll, { passive: true });
  syncProgressResize();
});
onBeforeUnmount(() => {
  translationGeneration += 1;
  window.removeEventListener("keydown", onKeydown);
  readerScrollEl.value?.removeEventListener("scroll", onReaderScroll);
  progressResize?.disconnect();
  progressResize = null;
});

// ===== 相关推荐 / 评论面板 =====

const relatedItems = shallowRef<BrowseWorkItem[]>([]);
const relatedLoading = ref(false);
const relatedError = ref("");

/** 正文列下段面板：相关推荐（默认）/ 评论，由顶栏评论按钮切换 */
const panel = ref<"related" | "comments">("related");
/** 面板锚点：切换后滚进视野用 */
const panelEl = ref<HTMLElement | null>(null);

async function loadRelated(): Promise<void> {
  relatedLoading.value = true;
  relatedError.value = "";
  try {
    const data = await browseRelated("novel", props.id, 12);
    relatedItems.value = data.items;
  } catch (err) {
    relatedError.value = errorMessage(err) || t("common.browseLoadFailed");
  } finally {
    relatedLoading.value = false;
  }
}

/** 顶栏评论按钮：切换面板（评论只在切到该面板时挂载，首开即拉第一页）。 */
function togglePanel(): void {
  panel.value = panel.value === "related" ? "comments" : "related";
  // 面板在正文之后：切换后滚进视野，否则顶栏点击时可能看不到任何反馈
  void nextTick(() => panelEl.value?.scrollIntoView({ block: "start" }));
}

// immediate：挂载即加载；id 变化（相关推荐/下一话跳转同路由）时整页重载。
watch(() => props.id, load, { immediate: true });

// ===== 动作 =====

function goAuthor(): void {
  if (item.value) router.push(`/browse/user/${item.value.author_id}`);
}

function goSeries(): void {
  if (series.value) router.push(`/browse/series/novel/${series.value.id}`);
}

function goEpisode(id: number): void {
  router.push(`/browse/work/novel/${id}`);
}

function goRelated(target: BrowseWorkItem): void {
  router.push(`/browse/work/novel/${target.id}`);
}

/** 用系统默认浏览器打开 pixiv 原页。 */
function openInPixiv(): void {
  void openInBrowser(pixivWorkUrl("novel", props.id)).catch(() =>
    notify(t("browse.hooks.openFailed"))
  );
}
</script>

<template>
  <div class="novel-view" :class="readBg ? `read-bg-${readBg}` : ''" :style="{ '--novel-scale': fontScale }">
    <!-- 顶栏：返回 / 标题 / 作者 / 返填表单 / 在浏览器中打开；flex 首行，恒贴窗口上边 -->
    <header class="topbar">
      <md-icon-button :aria-label="t('browse.novel.back')" :title="t('browse.novel.back')" @click="goBack(router)">
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
      </md-icon-button>
      <md-icon-button :aria-label="t('common.search')" :title="t('workspace.searchShortcut')" @click="router.push('/browse/search')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="8" /><path d="m21 21-4.3-4.3" /></svg></md-icon-button>
      <div class="topbar-title" :title="item?.title">
        {{ item?.title || t("common.browseReaderTitle") }}
      </div>
      <router-link v-if="item" class="topbar-author" :to="`/browse/user/${item.author_id}`">
        <img v-if="item.profile_img" class="avatar" :src="pxSrc(item.profile_img)" alt="" />
        <span v-else class="avatar avatar-fallback" aria-hidden="true">{{ item.author_name.slice(0, 1) }}</span>
        <span class="author-name">{{ item.author_name }}</span>
      </router-link>
      <!-- 收藏：未收藏=空心「收藏」；已收藏=实心「已收藏」（私密加角标）；详情就绪后显示 -->
      <BookmarkButton
        v-if="item"
        kind="novel"
        :id="props.id"
        :state="bookmarkState"
        @change="bookmarkState = $event"
      />
      <!-- 评论：切换正文列下段面板（相关推荐 ⇄ 评论）；选中态走 md-icon-button 的 toggle/selected -->
      <md-icon-button
        toggle
        :selected="panel === 'comments'"
        :aria-label="t('browse.comments.show')"
        :aria-label-selected="t('browse.comments.hide')"
        :title="panel === 'comments' ? t('browse.comments.hide') : t('browse.comments.show')"
        @click="togglePanel"
      >
        <svg
          class="bar-icon"
          :fill="panel === 'comments' ? 'currentColor' : 'none'"
          viewBox="0 0 24 24"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z" />
        </svg>
      </md-icon-button>
      <md-icon-button
        :aria-label="t('browse.hooks.fillNovelForm')"
        :title="t('browse.hooks.fillNovelForm')"
        @click="fillDownloadForm({ form: 'novel', sourceType: 'single', sourceId: props.id })"
      >
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 15V3" /><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="m7 10 5 5 5-5" /></svg>
      </md-icon-button>
      <md-icon-button
        :aria-label="t('browse.hooks.openInBrowser')"
        :title="t('browse.hooks.openInBrowser')"
        @click="openInPixiv"
      >
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h6" /><path d="m21 3-9 9" /><path d="M15 3h6v6" /></svg>
      </md-icon-button>
    </header>

    <!-- 中间滚动层：顶栏/底栏恒定贴窗口上下边，仅本层滚动（loading / error / 正文三个分支都在此层内） -->
    <div ref="readerScrollEl" class="reader-scroll">
      <!-- 详情骨架：纯色块，无动画 -->
      <div v-if="loading" class="reader-column" aria-hidden="true">
        <div class="skeleton">
          <div class="sk sk-title"></div>
          <div class="sk sk-sub"></div>
          <div v-for="n in 8" :key="n" class="sk sk-line" :class="{ short: n % 3 === 0 }"></div>
        </div>
      </div>

      <!-- 错误态：文案 + 重试 -->
      <div v-else-if="error" class="reader-column">
        <div class="reader-state" role="alert">
          <p class="state-text strong">{{ error }}</p>
          <md-outlined-button @click="load">{{ t("common.retry") }}</md-outlined-button>
        </div>
      </div>

      <div v-else-if="detail && item" class="reader-column">
        <!-- 信息头 -->
        <div class="info-head">
          <h1 class="work-title" :title="item.title">{{ item.title }}</h1>
          <p class="author-line">
            <router-link class="author-link" :to="`/browse/user/${item.author_id}`">{{ item.author_name }}</router-link>
            <span v-if="restricted" class="r18-pill">{{ restrictedLabel }}</span>
          </p>
          <!-- 所属系列（合集）入口：胶囊形态与下方标签 chip 区分（primary-container = 应用内跳转） -->
          <router-link
            v-if="series && series.id > 0"
            class="series-chip"
            :to="`/browse/series/novel/${series.id}`"
            :title="t('browse.work.seriesEntry', { title: series.title })"
            :aria-label="t('browse.work.seriesEntry', { title: series.title })"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <!-- lucide layers -->
              <path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z" />
              <path d="M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12" />
              <path d="M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17" />
            </svg>
            <span>{{ t("browse.novel.seriesLabel", { title: series.title, order: series.order }) }}</span>
          </router-link>
          <div v-if="item.tags?.length" class="tag-row">
            <router-link
              v-for="tag in item.tags"
              :key="tag"
              :to="{ path: '/browse/search', query: { word: tag, kind: 'novel', s_mode: 's_tag_full' } }"
              class="tag-chip"
              :title="t('common.search')"
            >
              {{ tag }}
            </router-link>
          </div>
          <p v-if="metaText" class="work-meta">{{ metaText }}</p>
          <p v-if="plainDescription" class="description">{{ plainDescription }}</p>
        </div>

        <!-- 正文（NovelContent 分页渲染，内嵌图经 images 取 URL）；空内容容错 -->
        <NovelContent
          v-if="hasContent"
          :content="content"
          :page="page"
          :translation="currentTranslation"
          :mode="translationMode"
          :images="embeddedImages"
          @pages-change="totalPages = $event"
        />
        <div v-else class="reader-state">
          <p class="state-text strong">{{ t("browse.novel.emptyContent") }}</p>
        </div>

        <!-- 系列导航：下一话 -->
        <div v-if="nextEpisodeId" class="series-nav">
          <md-outlined-button @click="goEpisode(nextEpisodeId)">
            {{ t("browse.novel.nextEpisode") }}<span class="nav-arrow" aria-hidden="true">→</span>
          </md-outlined-button>
        </div>

        <!-- 面板：相关推荐（默认）/ 评论（顶栏按钮切换；评论挂载时才拉取，分页自持） -->
        <section ref="panelEl" class="side-panel">
          <template v-if="panel === 'related'">
            <h2 class="section-title">{{ t("browse.novel.relatedTitle") }}</h2>
            <WorkGrid
              :items="relatedItems"
              :loading="relatedLoading"
              :error="relatedError"
              :has-more="false"
              @retry="loadRelated"
              @select="goRelated"
            />
          </template>
          <CommentsSection v-else kind="novel" :id="id" :author-id="item?.author_id ?? 0" />
        </section>
      </div>
    </div>

    <!-- 翻页器：flex 尾行贴窗口下边（AppPagination reader 变体）；#leading = 字号缩放控件；键盘 ←/→ 翻页仍由本视图层监听 -->
    <AppPagination
      v-if="hasContent"
      variant="reader"
      :current-page="page"
      :total-pages="totalPages"
      @update:currentPage="gotoPage"
    >
      <template #leading>
        <div class="font-scale">
          <md-icon-button
            :aria-label="t('browse.novel.fontSmaller')"
            :title="t('browse.novel.fontSmaller')"
            :disabled="fontScale <= SCALE_MIN"
            @click="stepScale(-1)"
          >
            <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10" /><path d="M8 12h8" /></svg>
          </md-icon-button>
          <span class="scale-value" aria-live="polite">{{ Math.round(fontScale * 100) }}%</span>
          <md-icon-button
            :aria-label="t('browse.novel.fontLarger')"
            :title="t('browse.novel.fontLarger')"
            :disabled="fontScale >= SCALE_MAX"
            @click="stepScale(1)"
          >
            <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10" /><path d="M8 12h8" /><path d="M12 8v8" /></svg>
          </md-icon-button>
          <!-- 重置：逆时针回环箭头；100% 已是默认值时禁用 -->
          <md-icon-button
            :aria-label="t('browse.novel.fontReset')"
            :title="t('browse.novel.fontReset')"
            :disabled="fontScale === 1"
            @click="resetScale"
          >
            <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" /><path d="M3 3v5h5" /></svg>
          </md-icon-button>
          <!-- 阅读背景色块按钮：圆形回显当前色，点击向上弹出居中一排气泡，选色立即应用 -->
          <NovelBgPicker :model-value="readBg" @update:model-value="setReadBg" />
          <NovelTranslationMenu
            v-model="translationMode"
            :status="translationStatus"
            :error="Boolean(!translating && translationErrorPage === page && translationError)"
            :busy="translating"
            :disabled="loading"
            :translated="currentTranslation.length > 0"
            :skipped="Boolean(currentSkipped)"
            @translate="translatePage"
          />
        </div>
      </template>
      <!-- 底栏右侧阅读进度：md-slider 拉条（拖动快速定位）+ 右侧百分比回显 -->
      <template #trailing>
        <div class="read-progress">
          <md-slider
            class="read-progress-slider"
            min="0"
            max="100"
            step="1"
            :value="progressPercent"
            :aria-label="t('browse.novel.readProgress')"
            @input="onProgressSeek"
          ></md-slider>
          <span class="read-progress-value" aria-hidden="true">{{ progressPercent }}%</span>
        </div>
      </template>
    </AppPagination>
  </div>
</template>

<style scoped>
/* 纸色模式译文色（--translation-ink）：纸面恒为浅纸，而深色主题的 primary 是浅色调、
 * 直接落上去就没了对比，故与纸面墨色混成「同色相、纸面可读」的一档。
 * 比例按主题分档，两档在五档纸色 × 五个色板下正文对比度均 ≥4.5:1（比例越高，与原文 ink 的色差越大）：
 * 浅色主题的 primary 本身够深，取 75% 仍达标且色差明显；深色主题的 primary 是浅色调，只能取 25%
 * 才守得住纸面对比度。原先两档都取 20%，浅色主题下与原文 ink 的色差只有 ΔE≈6，肉眼几乎同色。 */
.novel-view[class*="read-bg-"] {
  --translation-ink: color-mix(in srgb, var(--md-sys-color-primary) 75%, var(--ink));
}
html.dark .novel-view[class*="read-bg-"] {
  --translation-ink: color-mix(in srgb, var(--md-sys-color-primary) 25%, var(--ink));
}
.novel-view[class*="read-bg-"] :deep(.translated-text) {
  color: var(--translation-ink);
}
.novel-view[class*="read-bg-"] :deep(.translation-popover md-outlined-button) {
  --md-outlined-button-label-text-color: var(--translation-ink);
}
.novel-view[class*="read-bg-"] :deep(.translation-trigger.active) {
  --md-icon-button-icon-color: var(--translation-ink);
}
/* 满血宽度：抵消 .app-content 的 24px 内边距（640px 下为 16px），让顶栏/翻页器整行贴边。
   固定高度 flex 列：顶栏 / 滚动层 / 翻页器三行铺满视口，负 margin 抵消后顶栏贴窗口上边、
   翻页器贴窗口下边，.app-content 高度取自主工作区（排除状态栏 / 底部面板） 不再滚动，成为纯壳。 */
.novel-view {
  display: flex;
  flex-direction: column;
  height: 100cqh;
  margin: calc(-1 * var(--space-xl)) calc(-1 * var(--space-xl)) calc(-1 * var(--space-xl));
}

@media (max-width: 640px) {
  .novel-view {
    margin: calc(-1 * var(--space-lg));
  }
}

/* ===== 阅读背景（纸色模式） ===== */

/* 纸色模式挂在整页根节点（.read-bg-*），顶栏 / 正文 / 底栏全部随继承的变量落纸色。
 * 每档是自洽的「纸面 + 墨色」配对：surface 与 --surface 落纸面色、正文/次级文字落墨色，
 * surface-container（ink 6% 混入纸色）与 outline（ink 45% 混入纸色）按既定规则派生，
 * 供卡片 / 分隔线 / 滚动条 thumb 等派生色自动跟随；暗色主题下同样以浅纸面呈现
 * （配对自含、恒可读）。五个色板字面值见 DESIGN.md「小说阅读背景色板」。 */
.novel-view.read-bg-green {
  background: #c8e6ce;
  color: #1f3a2e;
  --ink: #1f3a2e;
  --ink-muted: #52685c;
  --ink-subtle: color-mix(in srgb, #1f3a2e 45%, #c8e6ce);
  --md-sys-color-on-surface: #1f3a2e;
  --md-sys-color-on-surface-variant: #52685c;
  --md-sys-color-surface: #c8e6ce;
  --surface: #c8e6ce;
  --md-sys-color-surface-container: color-mix(in srgb, #1f3a2e 6%, #c8e6ce);
  --md-sys-color-outline: color-mix(in srgb, #1f3a2e 45%, #c8e6ce);
}

.novel-view.read-bg-kraft {
  background: #e6d7b8;
  color: #433722;
  --ink: #433722;
  --ink-muted: #75684d;
  --ink-subtle: color-mix(in srgb, #433722 45%, #e6d7b8);
  --md-sys-color-on-surface: #433722;
  --md-sys-color-on-surface-variant: #75684d;
  --md-sys-color-surface: #e6d7b8;
  --surface: #e6d7b8;
  --md-sys-color-surface-container: color-mix(in srgb, #433722 6%, #e6d7b8);
  --md-sys-color-outline: color-mix(in srgb, #433722 45%, #e6d7b8);
}

.novel-view.read-bg-warm {
  background: #f3e4d0;
  color: #463526;
  --ink: #463526;
  --ink-muted: #7c6753;
  --ink-subtle: color-mix(in srgb, #463526 45%, #f3e4d0);
  --md-sys-color-on-surface: #463526;
  --md-sys-color-on-surface-variant: #7c6753;
  --md-sys-color-surface: #f3e4d0;
  --surface: #f3e4d0;
  --md-sys-color-surface-container: color-mix(in srgb, #463526 6%, #f3e4d0);
  --md-sys-color-outline: color-mix(in srgb, #463526 45%, #f3e4d0);
}

.novel-view.read-bg-mist {
  background: #e1ebf2;
  color: #263844;
  --ink: #263844;
  --ink-muted: #5c7180;
  --ink-subtle: color-mix(in srgb, #263844 45%, #e1ebf2);
  --md-sys-color-on-surface: #263844;
  --md-sys-color-on-surface-variant: #5c7180;
  --md-sys-color-surface: #e1ebf2;
  --surface: #e1ebf2;
  --md-sys-color-surface-container: color-mix(in srgb, #263844 6%, #e1ebf2);
  --md-sys-color-outline: color-mix(in srgb, #263844 45%, #e1ebf2);
}

.novel-view.read-bg-blush {
  background: #f5e7e5;
  color: #46302f;
  --ink: #46302f;
  --ink-muted: #7f6462;
  --ink-subtle: color-mix(in srgb, #46302f 45%, #f5e7e5);
  --md-sys-color-on-surface: #46302f;
  --md-sys-color-on-surface-variant: #7f6462;
  --md-sys-color-surface: #f5e7e5;
  --surface: #f5e7e5;
  --md-sys-color-surface-container: color-mix(in srgb, #46302f 6%, #f5e7e5);
  --md-sys-color-outline: color-mix(in srgb, #46302f 45%, #f5e7e5);
}

/* ===== 顶栏 ===== */

/* flex 首行，天然贴窗口上边（负 margin 抵消 .app-content padding 后无上缝隙）。 */
.topbar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: var(--space-sm);
  padding: var(--space-xs) var(--space-md);
  background: var(--surface);
  border-bottom: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 30%, transparent);
}

.topbar-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 14px;
  font-weight: 600;
  color: var(--ink);
}

/* 顶栏动作不参与收缩（空间由标题列 flex:1 吸收） */
.topbar md-icon-button {
  flex-shrink: 0;
}

.topbar-author {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xs);
  min-width: 0;
  max-width: 200px;
  padding: var(--space-xxs) var(--space-sm);
  border-radius: 999px;
  color: var(--ink-muted);
  text-decoration: none;
  transition: background-color 0.15s ease;
}

.topbar-author:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.topbar-author .author-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}

.avatar {
  width: 24px;
  height: 24px;
  border-radius: 999px;
  object-fit: cover;
  flex-shrink: 0;
}

.avatar-fallback {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
  font-size: 12px;
  font-weight: 600;
}

@media (max-width: 640px) {
  .topbar-author {
    display: none;
  }
}

/* ===== 中间滚动层 ===== */

/* 阅读区唯一滚动容器：顶栏/底栏在层外恒定可见，文字只在本层滚动 */
.reader-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

/* 滚动条细化：与 BrowseWorkView 信息列同 recipe（track 透明 / thumb outline 派生色） */
.reader-scroll::-webkit-scrollbar {
  width: 6px;
}

.reader-scroll::-webkit-scrollbar-track {
  background: transparent;
}

.reader-scroll::-webkit-scrollbar-thumb {
  background: color-mix(in srgb, var(--md-sys-color-outline) 40%, transparent);
  border-radius: 999px;
}

.reader-scroll::-webkit-scrollbar-thumb:hover {
  background: color-mix(in srgb, var(--md-sys-color-outline) 60%, transparent);
}

/* ===== 正文列 ===== */

/* 默认铺满中间区（≤16:9 1080p 不限宽）；更大屏幕才限 1280px 保持行宽可读
   （列内边距 --space-lg 16px，100% 字号下约 78 个全角字/行；1920×1080 及以下仍为满宽） */
.reader-column {
  margin: 0 auto;
  padding: var(--space-lg) var(--space-lg) var(--space-xl);
}

@media (min-width: 1921px), (min-height: 1081px) {
  .reader-column {
    max-width: 1280px;
  }
}

/* 骨架与状态 */

.skeleton {
  padding-top: var(--space-md);
}

.sk {
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
}

.sk-title {
  width: 60%;
  height: 22px;
  margin: 0 auto;
}

.sk-sub {
  width: 32%;
  height: 14px;
  margin: var(--space-md) auto var(--space-xl);
}

.sk-line {
  height: 14px;
  margin-top: var(--space-md);
}

.sk-line.short {
  width: 70%;
}

.reader-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-md);
  padding: var(--space-xl) var(--space-lg);
  text-align: center;
}

.state-text {
  margin: 0;
  color: var(--ink-muted);
  font-size: 14px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.state-text.strong {
  color: var(--ink);
  font-weight: 600;
}

/* 信息头 */

.info-head {
  margin-bottom: var(--space-xl);
  text-align: center;
}

.work-title {
  margin: 0;
  color: var(--ink);
  font-size: 20px;
  font-weight: 700;
  line-height: 1.4;
  overflow-wrap: anywhere;
}

.author-line {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-sm);
  margin: var(--space-sm) 0 0;
}

.author-link {
  color: var(--ink-muted);
  text-decoration: none;
}

.author-link:hover {
  color: var(--md-sys-color-primary);
  text-decoration: underline;
}

.r18-pill {
  padding: 0 var(--space-sm);
  border-radius: 999px;
  background: var(--ink);
  color: var(--md-sys-color-surface);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.6;
}

/* 系列（合集）入口 chip：primary-container 表达「应用内跳转」，与标签 chip 的
 * surface-container（检索）区分；信息头为居中块布局，chip 以 inline-flex 参与行内居中 */
.series-chip {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xs);
  max-width: 100%;
  margin-top: var(--space-sm);
  padding: 3px 12px;
  border-radius: 999px;
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
  text-align: left;
  text-decoration: none;
  overflow-wrap: anywhere;
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.series-chip:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 12%, var(--md-sys-color-primary-container));
}

.series-chip:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.series-chip svg {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}

@media (prefers-reduced-motion: reduce) {
  .series-chip {
    transition: none;
  }
}

.tag-row {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: var(--space-xs);
  margin-top: var(--space-md);
}

.tag-chip {
  text-decoration: none;
  overflow-wrap: anywhere;
  padding: 2px var(--space-sm);
  border: 0;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.6;
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.tag-chip:hover {
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

.tag-chip:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.description {
  margin: var(--space-md) 0 0;
  text-align: start;
  color: var(--ink);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.work-meta {
  margin: var(--space-md) 0 0;
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.4;
}

/* 系列导航 / 相关推荐 */

.series-nav {
  display: flex;
  justify-content: center;
  margin: var(--space-xl) 0 0;
}

.nav-arrow {
  margin-left: var(--space-xs);
}

/* 面板：相关推荐 / 评论（顶栏按钮切换）；滚动区内滚入视野只需少量上缘余量（顶栏在本层之外不遮挡） */
.side-panel {
  margin-top: var(--space-xl);
  scroll-margin-top: var(--space-md);
}

.section-title {
  margin: 0 0 var(--space-md);
  color: var(--ink);
  font-size: 16px;
  font-weight: 700;
  line-height: 1.4;
}

/* ===== 翻页器（AppPagination reader 变体自带吸底样式，此处无本地翻页器样式） ===== */

/* 翻页器在本视图是 flex 尾行、天然贴窗口下边；抵消 AppPagination reader 变体的
 * sticky（该 sticky 为系列分集页的长滚动页而设）——否则它会相对 .app-content
 * 内容盒（含 24px padding）上移，底栏下方漏出滚动内容。 */
:deep(.app-pagination.is-reader) {
  position: static;
}

/* 底栏左侧字号缩放控件（经 AppPagination #leading 插槽渲染） */
.font-scale {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xs);
}

.scale-value {
  min-width: 44px;
  color: var(--ink-muted);
  font-size: 13px;
  text-align: center;
}

/* 底栏右侧阅读进度（经 AppPagination #trailing 插槽渲染）：滑杆拉条 + 右侧百分比回显 */
.read-progress {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xs);
  min-width: 0;
}

/* md-slider：宿主高由 state-layer-size 驱动（默认 40px 触控域），收敛到底栏 32px 控件
 * 约定（同时缩触控域与滑杆高，handle 视觉 20px 不受影响）；宽度约 200px（与宿主内置
 * min-inline-size 同级，显式声明以便窄窗收窄时能压过宿主 min-width）。
 * 主题映射只落本应用已定义的角色——包内默认 inactive track 引用 surface-container-highest、
 * handle 阴影引用 shadow，本应用均未定义，不显式映射会漏出浅色兜底（暗色主题发灰）。 */
.read-progress md-slider {
  width: 200px;
  min-width: 200px;
  --md-slider-state-layer-size: calc(var(--space-lg) * 2);
  --md-slider-handle-color: var(--md-sys-color-primary);
  --md-slider-hover-handle-color: var(--md-sys-color-primary);
  --md-slider-focus-handle-color: var(--md-sys-color-primary);
  --md-slider-pressed-handle-color: var(--md-sys-color-primary);
  --md-slider-active-track-color: var(--md-sys-color-primary);
  --md-slider-inactive-track-color: color-mix(in srgb, var(--md-sys-color-outline) 30%, transparent);
  --md-slider-handle-shadow-color: transparent;
}

/* 百分比回显在滑杆右侧；定宽防 0%→100% 跳变抖动 */
.read-progress-value {
  min-width: 44px;
  color: var(--ink-muted);
  font-size: 13px;
  text-align: right;
}

@media (max-width: 800px) {
  :deep(.app-pagination.is-reader) {
    grid-template-columns: auto auto minmax(0, 1fr);
    gap: var(--space-sm);
  }
  :deep(.reader-trailing) { width: 100%; }
  .read-progress { width: 100%; }
  .read-progress md-slider {
    flex: 1;
    width: 100%;
    min-width: 0;
    max-width: 140px;
  }
}

@media (max-width: 600px) {
  :deep(.app-pagination.is-reader) { grid-template-columns: auto minmax(0, 1fr); }
  :deep(.reader-leading) { grid-column: 1 / -1; }
  :deep(.reader-inner) { grid-column: 1; grid-row: 2; }
  :deep(.reader-trailing) { grid-column: 2; grid-row: 2; }
}

.bar-icon {
  width: 20px;
  height: 20px;
  stroke-width: 2;
}
</style>
