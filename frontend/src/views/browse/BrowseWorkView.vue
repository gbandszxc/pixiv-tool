<script lang="ts">
/**
 * 本会话已确认 R-18 遮罩的作品 id（模块级存储）：
 * 只在设置关闭 `show_r18` 时用得上（开启时详情页直接展示、不出现遮罩）；
 * 同一会话内对同一作品不再二次询问；不持久化，重启后恢复遮罩。
 */
const revealedWorkIds = new Set<number>();
</script>

<script setup lang="ts">
/**
 * 作品查看器（插画/漫画）：整页路由视图 /browse/work/:kind/:id。
 * 桌面 ≥960px 双列：左图片舞台（页面底色，纵向渐进加载，翻页/全屏由 ImageViewer 自理）
 * + 右信息列（固定 320px 可滚动）；窄窗纵向堆叠（图片在上）。
 * 右列下段为可切换面板——相关推荐（默认）/ 评论，由顶栏评论按钮控制，评论按需分页拉取。
 * 相关推荐经 router.push 保留来路（watch 参数重拉）。
 */
import { computed, nextTick, onActivated, onBeforeUnmount, onDeactivated, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { goBack } from "../../router/navigation";
import { useI18n } from "vue-i18n";
import { useSettingsStore } from "../../stores/settings";
import {
  browseHistoryRecord,
  browseRelated,
  browseWorkDetail,
  errorMessage,
  pxSrc,
  type BrowseIllustDetail,
  type BrowseWorkItem,
  type ListWorkKind,
} from "../../api/browse";
import ImageViewer from "../../components/browse/ImageViewer.vue";
import CommentsSection from "../../components/browse/CommentsSection.vue";
import RelatedGrid from "../../components/browse/RelatedGrid.vue";
import BookmarkButton from "../../components/browse/BookmarkButton.vue";
import { notify } from "../../ui/notify";
import { fillDownloadForm, openInBrowser } from "../../utils/pixivHooks";
import { pixivWorkUrl } from "../../utils/pixivUrl";
import { descriptionText } from "../../utils/descriptionText";
import type { WorkBookmarkState } from "../../api/browse";

const props = defineProps<{
  kind: "illust" | "manga";
  id: number;
}>();

const router = useRouter();
const { t } = useI18n();
const settings = useSettingsStore();

// ===== 状态 =====

const detail = ref<BrowseIllustDetail | null>(null);
const loading = ref(true);
/** 详情加载失败文案（404 / 无权限等，非空即错误态） */
const error = ref("");
/** 查看者收藏态（来自详情响应 bookmarkState；收藏按钮经 change 回写） */
const bookmarkState = ref<WorkBookmarkState | null>(null);
/** 本作品是否已确认 R-18 遮罩（仅关闭 show_r18 时参与判定） */
const revealed = ref(false);

const relatedItems = ref<BrowseWorkItem[]>([]);
const relatedLoading = ref(false);
const relatedError = ref("");

const infoCol = ref<HTMLElement | null>(null);
/** 右列下段面板：相关推荐（默认）/ 评论，由顶栏评论按钮切换 */
const panel = ref<"related" | "comments">("related");
/** 面板锚点：切换后滚进视野用 */
const panelEl = ref<HTMLElement | null>(null);
/** 请求序号：快速连续跳转作品时丢弃过期响应 */
let reqSeq = 0;
let relSeq = 0;

// ===== 派生 =====

const item = computed(() => detail.value?.item ?? null);
const pages = computed(() => detail.value?.pages ?? []);
/** ugoira 只显示封面帧（kind 路由侧只有 illust/manga，运行时以 item.kind 判定） */
const isUgoira = computed(() => item.value?.kind === "ugoira");
/**
 * R-18 遮罩只在设置关闭 `show_r18` 时出现——开启（默认）即用户已表态要看 R-18，
 * 详情页不再二次确认；关闭时作为深链直访的兜底，保留本会话记忆。
 */
const restricted = computed(
  () => !settings.settings.show_r18 && !revealed.value && (item.value?.x_restrict ?? 0) > 0
);
const restrictLabel = computed(() =>
  item.value?.x_restrict === 2 ? t("common.browseR18G") : t("common.browseR18")
);

/** 描述按纯文本展示并保留换行，绝不 v-html。 */
const plainDescription = computed(() => descriptionText(item.value?.description));

const dateText = computed(() => item.value?.create_date?.slice(0, 10) ?? "");

function formatCount(n?: number): string {
  return typeof n === "number" ? n.toLocaleString() : "-";
}

// ===== 详情页图片缩放（设置键 detail_image_scale，默认 1.0 = 撑满 = 既有观感）=====

/** 区间与步进与后端校验一致（settings.rs 的 DETAIL_IMAGE_SCALE_*）。 */
const IMAGE_SCALE_MIN = 0.5;
const IMAGE_SCALE_MAX = 1;
const IMAGE_SCALE_STEP = 0.1;

function clampScale(value: number): number {
  return Math.min(IMAGE_SCALE_MAX, Math.max(IMAGE_SCALE_MIN, value));
}

/**
 * 乐观步进草稿：settings store 要等 IPC 返回才更新，直接以它为基数会让连点丢步
 * （0.1 步进下可感知）；外部（设置拉取等）改值后放弃草稿，回到以设置值为准。
 */
const zoomDraft = ref<number | null>(null);
/** 越界 / 缺省容错（浏览器 mock、旧配置、invoke 桩返回 {} 都不会得到 NaN）。 */
const imageScale = computed(() => clampScale(zoomDraft.value ?? (settings.settings.detail_image_scale || 1)));

watch(
  () => settings.settings.detail_image_scale,
  () => {
    zoomDraft.value = null;
  }
);

/** 步进 ±0.1（先取两位小数再夹取）；持久化失败回滚草稿并提示，不打断浏览。 */
async function stepZoom(delta: number): Promise<void> {
  const next = Math.round((imageScale.value + delta * IMAGE_SCALE_STEP) * 100) / 100;
  const value = clampScale(next);
  if (value === imageScale.value) return;
  zoomDraft.value = value;
  try {
    await settings.saveSettings({ detail_image_scale: value });
  } catch (err) {
    zoomDraft.value = null;
    notify(errorMessage(err));
  }
}

/** 重置为默认 100%（撑满）；已在默认值时直接返回，持久化失败回滚草稿并提示。 */
async function resetZoom(): Promise<void> {
  if (imageScale.value === 1) return;
  zoomDraft.value = 1;
  try {
    await settings.saveSettings({ detail_image_scale: 1 });
  } catch (err) {
    zoomDraft.value = null;
    notify(errorMessage(err));
  }
}

// ===== 加载 =====

async function loadDetail(): Promise<void> {
  const seq = ++reqSeq;
  loading.value = true;
  error.value = "";
  detail.value = null;
  bookmarkState.value = null;
  revealed.value = revealedWorkIds.has(props.id);
  void loadRelated();
  try {
    const data = await browseWorkDetail(props.kind, props.id);
    if (seq !== reqSeq) return;
    if (data.detail_kind !== "illust") {
      // 路由已限定 illust/manga，novel 归 BrowseNovelView；此处仅为类型收窄兜底
      error.value = t("common.browseLoadFailed");
      return;
    }
    detail.value = data;
    bookmarkState.value = data.bookmarkState ?? null;
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
    if (seq !== reqSeq) return;
    error.value = errorMessage(err) || t("common.browseLoadFailed");
  } finally {
    if (seq === reqSeq) loading.value = false;
  }
}

async function loadRelated(): Promise<void> {
  const seq = ++relSeq;
  relatedItems.value = [];
  relatedError.value = "";
  relatedLoading.value = true;
  try {
    const data = await browseRelated(props.kind, props.id, 18);
    if (seq !== relSeq) return;
    relatedItems.value = data.items;
  } catch (err) {
    if (seq !== relSeq) return;
    relatedError.value = errorMessage(err) || t("common.browseLoadFailed");
  } finally {
    if (seq === relSeq) relatedLoading.value = false;
  }
}

function scrollReset(): void {
  infoCol.value?.scrollTo({ top: 0 });
  window.scrollTo({ top: 0 });
}

/** 顶栏评论按钮：切换右列面板（评论只在切到该面板时挂载，首开即拉第一页）。 */
function togglePanel(): void {
  panel.value = panel.value === "related" ? "comments" : "related";
  void nextTick(() => panelEl.value?.scrollIntoView({ block: "nearest" }));
}

watch(
  () => [props.kind, props.id] as const,
  () => {
    scrollReset();
    panel.value = "related";
    void loadDetail();
  },
  { immediate: true }
);

// ===== 导航 =====

/** ugoira 经 illust 端点取详情（pixiv illustType=2），路由上归 illust。 */
function routeKindOf(kind: BrowseWorkItem["kind"]): ListWorkKind {
  return kind === "ugoira" ? "illust" : kind;
}

function openRelated(target: BrowseWorkItem): void {
  void router.push(`/browse/work/${routeKindOf(target.kind)}/${target.id}`);
}

function openAuthor(): void {
  const authorId = item.value?.author_id;
  if (authorId) void router.push(`/browse/user/${authorId}`);
}

function openSeries(): void {
  const series = detail.value?.series;
  // 作品详情（illust/manga）系列导航 → 应用内系列分集页 illust 段
  if (series) void router.push(`/browse/series/illust/${series.id}`);
}

/** 用系统默认浏览器打开 pixiv 原页。 */
function openInPixiv(): void {
  void openInBrowser(pixivWorkUrl(props.kind, props.id)).catch(() =>
    notify(t("browse.hooks.openFailed"))
  );
}

// ===== 遮罩 / 键盘 =====

function reveal(): void {
  revealedWorkIds.add(props.id);
  revealed.value = true;
}

/**
 * 这里只处理 Esc（返回上一页）：图片的翻页与全屏键盘（←/→、浮层 Esc）由 ImageViewer
 * 在 capture 阶段拦截，浮层打开时不会冒泡到这里。
 */
function onKeydown(e: KeyboardEvent): void {
  if (e.key !== "Escape" || e.defaultPrevented) return;
  // md-* 输入组件的事件到 window 时已被重定向到宿主（tagName 不是 INPUT），须走 composedPath
  for (const node of e.composedPath()) {
    if (!(node instanceof HTMLElement)) continue;
    if (node.tagName === "INPUT" || node.tagName === "TEXTAREA" || node.isContentEditable) return;
  }
  // 模态 dialog（设置 / 登录 / 退出确认）打开时 Esc 归 dialog 自己处理
  if (document.querySelector("dialog[open]")) return;
  e.preventDefault();
  goBack(router);
}

/**
 * 详情页按 path 进 KeepAlive（返回不重新加载）：停用期间必须摘掉 window 监听，
 * 否则 Esc 会在别的页面上触发本页返回。信息列滚动位置在滚动过程中记录、激活时还原
 * ——停用发生在 DOM 被移出文档之后，那时读 scrollTop 只会是 0（舞台同理，见 ImageViewer）。
 */
let infoScrollTop = 0;

function rememberInfoScroll(): void {
  infoScrollTop = infoCol.value?.scrollTop ?? 0;
}

function attachKeydown(): void {
  window.addEventListener("keydown", onKeydown);
}

function detachKeydown(): void {
  window.removeEventListener("keydown", onKeydown);
}

onMounted(attachKeydown);
onActivated(() => {
  attachKeydown();
  if (infoCol.value) infoCol.value.scrollTop = infoScrollTop;
});
onDeactivated(detachKeydown);
onBeforeUnmount(detachKeydown);
</script>

<template>
  <div class="work-view">
    <!-- 顶部条：返回 + 标题/作者 + 收藏 / 评论（面板切换）/ 返填表单 / 在浏览器中打开 -->
    <header class="work-topbar">
      <md-icon-button :aria-label="t('browse.work.back')" :title="t('browse.work.back')" @click="goBack(router)">
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
      </md-icon-button>
      <div class="topbar-main">
        <template v-if="loading">
          <div class="sk-line w60" aria-hidden="true"></div>
          <div class="sk-line w30" aria-hidden="true"></div>
        </template>
        <template v-else-if="item">
          <h1 class="work-title" :title="item.title">{{ item.title }}</h1>
          <button class="author-row" type="button" :title="item.author_name" @click="openAuthor">
            <img v-if="item.profile_img" class="author-avatar" :src="pxSrc(item.profile_img)" alt="" />
            <span v-else class="author-avatar author-fallback" aria-hidden="true">{{ (item.author_name || "?").slice(0, 1) }}</span>
            <span class="author-name">{{ item.author_name }}</span>
          </button>
        </template>
      </div>
      <!-- 收藏：未收藏=空心「收藏」；已收藏=实心「已收藏」（私密加角标）；详情就绪后显示 -->
      <BookmarkButton
        v-if="item"
        :kind="props.kind"
        :id="props.id"
        :state="bookmarkState"
        @change="bookmarkState = $event"
      />
      <!-- 评论：切换右列面板（相关推荐 ⇄ 评论）；选中态走 md-icon-button 的 toggle/selected -->
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
          <!-- lucide message-square -->
          <path d="M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z" />
        </svg>
      </md-icon-button>
      <!-- 返填到插画抓取页：来源=单篇，ID=当前作品 -->
      <md-icon-button
        :aria-label="t('browse.hooks.fillIllustForm')"
        :title="t('browse.hooks.fillIllustForm')"
        @click="fillDownloadForm({ form: 'illustration', sourceType: 'single', sourceId: props.id })"
      >
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <!-- lucide download -->
          <path d="M12 15V3" /><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="m7 10 5 5 5-5" />
        </svg>
      </md-icon-button>
      <md-icon-button
        :aria-label="t('browse.hooks.openInBrowser')"
        :title="t('browse.hooks.openInBrowser')"
        @click="openInPixiv"
      >
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <!-- lucide square-arrow-out-up-right -->
          <path d="M21 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h6" />
          <path d="m21 3-9 9" />
          <path d="M15 3h6v6" />
        </svg>
      </md-icon-button>
      <!-- 图片缩放（持久化到 settings.json）：[−] 值 [+] / 重置；100% = 撑满舞台（默认）。
           控件规格与小说阅读器底栏字号缩放一致（BrowseNovelView font-scale） -->
      <div class="zoom-group" role="group" :aria-label="t('browse.work.imageZoomLabel')">
        <md-icon-button
          :disabled="imageScale <= IMAGE_SCALE_MIN"
          :aria-label="t('browse.work.imageZoomOut')"
          :title="t('browse.work.imageZoomOut')"
          @click="stepZoom(-1)"
        >
          <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10" /><path d="M8 12h8" /></svg>
        </md-icon-button>
        <span class="zoom-value" aria-live="polite">{{ Math.round(imageScale * 100) }}%</span>
        <md-icon-button
          :disabled="imageScale >= IMAGE_SCALE_MAX"
          :aria-label="t('browse.work.imageZoomIn')"
          :title="t('browse.work.imageZoomIn')"
          @click="stepZoom(1)"
        >
          <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10" /><path d="M8 12h8" /><path d="M12 8v8" /></svg>
        </md-icon-button>
        <!-- 重置：逆时针回环箭头；100% 已是默认值时禁用 -->
        <md-icon-button
          :disabled="imageScale === 1"
          :aria-label="t('browse.work.imageZoomReset')"
          :title="t('browse.work.imageZoomReset')"
          @click="resetZoom"
        >
          <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" /><path d="M3 3v5h5" /></svg>
        </md-icon-button>
      </div>
    </header>

    <!-- 错误态（404 / 无权限等）：可读文案 + 返回 -->
    <div v-if="error" class="work-error" role="alert">
      <p class="error-title">{{ t("browse.work.errorTitle") }}</p>
      <p class="error-text">{{ error }}</p>
      <md-outlined-button @click="goBack(router)">{{ t("browse.work.back") }}</md-outlined-button>
    </div>

    <div v-else class="work-body">
      <!-- 左：图片舞台 -->
      <section class="stage-col">
        <ImageViewer
          v-if="detail"
          :pages="pages"
          :alt="item?.title ?? ''"
          :restricted="restricted"
          :restrict-label="restrictLabel"
          :ugoira="isUgoira"
          :stage-scale="imageScale"
          @reveal="reveal"
        />
        <div v-else class="stage-skeleton" aria-hidden="true"></div>
      </section>

      <!-- 右：信息列（320px，可滚动；窄窗时排在图片下方） -->
      <aside ref="infoCol" class="info-col" @scroll.passive="rememberInfoScroll">
        <template v-if="loading">
          <div class="sk-line w40" aria-hidden="true"></div>
          <div class="sk-line w80" aria-hidden="true"></div>
          <div class="sk-line w60" aria-hidden="true"></div>
          <div class="sk-line w70" aria-hidden="true"></div>
        </template>
        <template v-else-if="item">
          <span v-if="item.x_restrict" class="restrict-pill">{{ restrictLabel }}</span>

          <!-- 标签 chips -->
          <div v-if="item.tags?.length" class="tag-row">
            <router-link v-for="tag in item.tags" :key="tag" class="tag-chip" :to="{ path: '/browse/search', query: { word: tag, kind: props.kind, s_mode: 's_tag_full' } }" :title="t('common.search')">{{ tag }}</router-link>
          </div>

          <!-- 所属系列（合集）入口：紧邻标签区域；胶囊形态与标签 chip 区分
               （primary-container = 应用内跳转，标签的 surface-container = 检索）；
               id <= 0 不渲染（解析层守卫之外的前端双保险） -->
          <button
            v-if="detail?.series && detail.series.id > 0"
            class="series-chip"
            type="button"
            :title="t('browse.work.seriesEntry', { title: detail.series.title })"
            :aria-label="t('browse.work.seriesEntry', { title: detail.series.title })"
            @click="openSeries"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <!-- lucide layers -->
              <path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z" />
              <path d="M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12" />
              <path d="M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17" />
            </svg>
            <span>{{ t("browse.work.seriesEp", { title: detail.series.title, order: detail.series.order }) }}</span>
          </button>

          <!-- 计数行 -->
          <div class="count-row">
            <span class="count" :title="t('browse.work.views')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0" /><circle cx="12" cy="12" r="3" /></svg>
              {{ formatCount(item.view_count) }}
            </span>
            <span class="count" :title="t('browse.work.bookmarks')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M17 3a2 2 0 0 1 2 2v15a1 1 0 0 1-1.496.868l-4.512-2.578a2 2 0 0 0-1.984 0l-4.512 2.578A1 1 0 0 1 5 20V5a2 2 0 0 1 2-2z" /></svg>
              {{ formatCount(item.bookmark_count) }}
            </span>
            <span class="count" :title="t('browse.work.likes')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2 9.5a5.5 5.5 0 0 1 9.591-3.676.56.56 0 0 0 .818 0A5.49 5.49 0 0 1 22 9.5c0 2.29-1.5 4-3 5.5l-5.492 5.313a2 2 0 0 1-3 .019L5 15c-1.5-1.5-3-3.2-3-5.5" /></svg>
              {{ formatCount(item.like_count) }}
            </span>
          </div>

          <!-- 尺寸与投稿日期 -->
          <p class="meta-line">
            <template v-if="item.width && item.height">{{ item.width }} × {{ item.height }} · </template>{{ dateText }}
          </p>

          <!-- 描述（剥标签纯文本） -->
          <p v-if="plainDescription" class="description">{{ plainDescription }}</p>

          <!-- 面板：相关推荐（默认）/ 评论（顶栏按钮切换；评论挂载时才拉取，分页自持） -->
          <div ref="panelEl" class="side-panel">
            <RelatedGrid
              v-if="panel === 'related'"
              :items="relatedItems"
              :loading="relatedLoading"
              :error="relatedError"
              @retry="loadRelated"
              @select="openRelated"
            />
            <!-- CommentsSection 需要作品作者 id（发表评论的 author_user_id） -->
            <CommentsSection v-else :kind="kind" :id="id" :author-id="item?.author_id ?? 0" />
          </div>
        </template>
      </aside>
    </div>
  </div>
</template>

<style scoped>
/* 整页视图：占满内容区一屏（app-content 桌面端上下各 24px 内边距） */
.work-view {
  display: flex;
  flex-direction: column;
  gap: var(--space-md);
  height: calc(100cqh - 2 * var(--space-xl));
  min-height: 0;
}

/* ===== 顶部条 ===== */

.work-topbar {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
  flex-shrink: 0;
}

/* 顶栏图标动作：统一 20px 线性图标（stroke 2，lucide 官方路径），点击域由 md-icon-button（40px）承载 */
.bar-icon {
  width: 20px;
  height: 20px;
  stroke-width: 2;
}

.topbar-main {
  display: flex;
  flex: 1;
  min-width: 0;
  flex-direction: column;
  gap: 2px;
}

.work-title {
  display: -webkit-box;
  margin: 0;
  overflow: hidden;
  color: var(--ink);
  font-size: 16px;
  font-weight: 700;
  line-height: 1.4;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}

.author-row {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xs);
  align-self: flex-start;
  padding: 2px var(--space-xs);
  margin-left: calc(-1 * var(--space-xs));
  border: none;
  border-radius: 999px;
  background: none;
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.author-row:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.author-row:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.author-avatar {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  border-radius: 999px;
  object-fit: cover;
}

.author-fallback {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
  font-size: 11px;
  font-weight: 600;
}

.author-name {
  max-width: 320px;
  overflow: hidden;
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.4;
  white-space: nowrap;
  text-overflow: ellipsis;
}

/* 顶栏动作不参与收缩（空间由标题列 flex:1 吸收） */
.work-topbar md-icon-button {
  flex-shrink: 0;
}

/* 图片缩放组：[−] 值 [+] 重置（与小说阅读器底栏字号缩放同规格） */
.zoom-group {
  display: inline-flex;
  flex-shrink: 0;
  align-items: center;
  gap: var(--space-xxs);
}

.zoom-value {
  min-width: calc(2 * var(--space-xl));
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
  text-align: center;
}

/* 窄窗：动作组整体落行，标题不被挤没 */
@media (max-width: 640px) {
  .work-topbar {
    flex-wrap: wrap;
  }
}

/* ===== 双列主体 ===== */

.work-body {
  display: flex;
  flex: 1;
  min-height: 0;
  gap: var(--space-lg);
}

.stage-col {
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.stage-skeleton {
  height: 100%;
  border-radius: 16px;
  background: var(--md-sys-color-surface-container);
}

.info-col {
  display: flex;
  width: 320px;
  flex-shrink: 0;
  flex-direction: column;
  gap: var(--space-md);
  overflow-y: auto;
  padding-right: var(--space-xs);
}

/* 滚动条细化：浅色底用 outline 派生色（零新字面值） */
.info-col::-webkit-scrollbar {
  width: 6px;
}

.info-col::-webkit-scrollbar-track {
  background: transparent;
}

.info-col::-webkit-scrollbar-thumb {
  background: color-mix(in srgb, var(--md-sys-color-outline) 40%, transparent);
  border-radius: 999px;
}

.info-col::-webkit-scrollbar-thumb:hover {
  background: color-mix(in srgb, var(--md-sys-color-outline) 60%, transparent);
}

/* ===== 信息列内容 ===== */

.restrict-pill {
  align-self: flex-start;
  padding: 1px 10px;
  border-radius: 999px;
  background: var(--ink);
  color: var(--md-sys-color-surface);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
}

.tag-row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-xs);
}

.tag-chip {
  text-decoration: none;
  overflow-wrap: anywhere;
  padding: 3px 12px;
  border: none;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.tag-chip:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.tag-chip:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.count-row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-lg);
}

.count {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xxs);
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.4;
}

.count svg {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}

.meta-line {
  margin: 0;
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.4;
}

/* 系列（合集）入口 chip：primary-container 表达「应用内跳转」，与标签 chip 的
 * surface-container（检索）区分；配方与标签 chip 同族（999px / 12px·600 / 3px 12px） */
.series-chip {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xs);
  align-self: flex-start;
  max-width: 100%;
  padding: 3px 12px;
  border: none;
  border-radius: 999px;
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
  text-align: left;
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

.description {
  margin: 0;
  color: var(--ink);
  font-size: 14px;
  line-height: 1.6;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

/* ===== 骨架 ===== */

.sk-line {
  height: 14px;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
}

.sk-line.w30 { width: 30%; }
.sk-line.w40 { width: 40%; }
.sk-line.w60 { width: 60%; }
.sk-line.w70 { width: 70%; }
.sk-line.w80 { width: 80%; }

/* ===== 错误态 ===== */

.work-error {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-sm);
  text-align: center;
}

.error-title {
  margin: 0;
  color: var(--ink);
  font-size: 16px;
  font-weight: 700;
}

.error-text {
  max-width: 480px;
  margin: 0;
  color: var(--ink-muted);
  font-size: 14px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

/* ===== 窄窗：纵向堆叠，图片在上 ===== */

@media (max-width: 959px) {
  .work-view {
    height: auto;
    min-height: 0;
  }

  .work-body {
    flex-direction: column;
  }

  .stage-col {
    flex: none;
    height: 60vh;
    min-height: 320px;
  }

  .info-col {
    width: 100%;
    overflow: visible;
    padding-right: 0;
  }
}
</style>
