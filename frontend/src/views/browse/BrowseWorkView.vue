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
 * 桌面 ≥960px 双列：左图片舞台（近黑底，纵向渐进加载，翻页/全屏由 ImageViewer 自理）
 * + 右信息列（固定 320px 可滚动）；窄窗纵向堆叠（图片在上）。
 * 右列下段为可切换面板——相关推荐（默认）/ 评论，由顶栏评论按钮控制，评论按需分页拉取。
 * 相关推荐经 router.replace 原地跳转（watch 参数重拉）。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { useSettingsStore } from "../../stores/settings";
import {
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

/** 描述 HTML 剥标签后纯文本展示（DOMParser 惰性文档：不执行脚本、不加载图片，绝不 v-html） */
const plainDescription = computed(() => {
  const html = item.value?.description;
  if (!html) return "";
  const doc = new DOMParser().parseFromString(html, "text/html");
  return (doc.body.textContent ?? "").trim();
});

const dateText = computed(() => item.value?.create_date?.slice(0, 10) ?? "");

function formatCount(n?: number): string {
  return typeof n === "number" ? n.toLocaleString() : "-";
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
  void router.replace(`/browse/work/${routeKindOf(target.kind)}/${target.id}`);
}

function openAuthor(): void {
  const authorId = item.value?.author_id;
  if (authorId) void router.push(`/browse/user/${authorId}`);
}

function openTag(tag: string): void {
  void router.push({ path: "/browse/search", query: { word: tag } });
}

function openSeries(): void {
  const series = detail.value?.series;
  // 作品详情（illust/manga）系列导航 → 应用内系列分集页 illust 段
  if (series) void router.push(`/browse/series/illust/${series.id}`);
}

/** 无应用内历史（直达深链）时兜底回浏览首页。 */
function goBack(): void {
  if (window.history.state && typeof window.history.state.back === "string") router.back();
  else void router.push("/browse/home");
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
  goBack();
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <div class="work-view">
    <!-- 顶部条：返回 + 标题/作者 + 收藏 / 评论（面板切换）/ 返填表单 / 在浏览器中打开 -->
    <header class="work-topbar">
      <md-icon-button :aria-label="t('browse.work.back')" :title="t('browse.work.back')" @click="goBack">
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="15 18 9 12 15 6" /></svg>
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
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M20 5H4a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h3v4l5-4h8a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1z" />
        </svg>
      </md-icon-button>
      <!-- 返填到插画抓取页：来源=单篇，ID=当前作品 -->
      <md-icon-button
        :aria-label="t('browse.hooks.fillIllustForm')"
        :title="t('browse.hooks.fillIllustForm')"
        @click="fillDownloadForm({ form: 'illustration', sourceType: 'single', sourceId: props.id })"
      >
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M12 3v12" /><path d="m7 10 5 5 5-5" /><path d="M4 21h16" />
        </svg>
      </md-icon-button>
      <md-icon-button
        :aria-label="t('browse.hooks.openInBrowser')"
        :title="t('browse.hooks.openInBrowser')"
        @click="openInPixiv"
      >
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
          <polyline points="15 3 21 3 21 9" />
          <line x1="10" y1="14" x2="21" y2="3" />
        </svg>
      </md-icon-button>
    </header>

    <!-- 错误态（404 / 无权限等）：可读文案 + 返回 -->
    <div v-if="error" class="work-error" role="alert">
      <p class="error-title">{{ t("browse.work.errorTitle") }}</p>
      <p class="error-text">{{ error }}</p>
      <md-outlined-button @click="goBack">{{ t("browse.work.back") }}</md-outlined-button>
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
          @reveal="reveal"
        />
        <div v-else class="stage-skeleton" aria-hidden="true"></div>
      </section>

      <!-- 右：信息列（320px，可滚动；窄窗时排在图片下方） -->
      <aside ref="infoCol" class="info-col">
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
            <button v-for="tag in item.tags" :key="tag" class="tag-chip" type="button" @click="openTag(tag)">{{ tag }}</button>
          </div>

          <!-- 计数行 -->
          <div class="count-row">
            <span class="count" :title="t('browse.work.views')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" /><circle cx="12" cy="12" r="3" /></svg>
              {{ formatCount(item.view_count) }}
            </span>
            <span class="count" :title="t('browse.work.bookmarks')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z" /></svg>
              {{ formatCount(item.bookmark_count) }}
            </span>
            <span class="count" :title="t('browse.work.likes')">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z" /></svg>
              {{ formatCount(item.like_count) }}
            </span>
          </div>

          <!-- 尺寸与投稿日期 -->
          <p class="meta-line">
            <template v-if="item.width && item.height">{{ item.width }} × {{ item.height }} · </template>{{ dateText }}
          </p>

          <!-- 所属系列 -->
          <button v-if="detail?.series" class="series-link" type="button" @click="openSeries">
            {{ t("browse.work.seriesEp", { title: detail.series.title, order: detail.series.order }) }}
          </button>

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
            <CommentsSection v-else :kind="kind" :id="id" />
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
  height: calc(100vh - 2 * var(--space-xl));
  min-height: 480px;
}

/* ===== 顶部条 ===== */

.work-topbar {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
  flex-shrink: 0;
}

/* 顶栏图标动作：统一 20px 线性图标（stroke 1.8），点击域由 md-icon-button（40px）承载 */
.bar-icon {
  width: 20px;
  height: 20px;
  stroke-width: 1.8;
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

.series-link {
  align-self: flex-start;
  padding: 0;
  border: none;
  background: none;
  color: var(--md-sys-color-primary);
  font-size: 14px;
  line-height: 1.5;
  text-align: left;
  cursor: pointer;
}

.series-link:hover {
  text-decoration: underline;
}

.series-link:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
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
