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
import AppPagination from "../../components/common/AppPagination.vue";
import NovelContent from "../../components/browse/NovelContent.vue";
import CommentsSection from "../../components/browse/CommentsSection.vue";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import BookmarkButton from "../../components/browse/BookmarkButton.vue";
import {
  browseRelated,
  browseWorkDetail,
  errorMessage,
  pxSrc,
  type BrowseNovelDetail,
  type BrowseWorkItem,
  type WorkBookmarkState,
} from "../../api/browse";
import { notify } from "../../ui/notify";
import { fillDownloadForm, openInBrowser } from "../../utils/pixivHooks";
import { pixivWorkUrl } from "../../utils/pixivUrl";

const props = defineProps<{ kind: "illust" | "manga" | "novel"; id: number }>();

const { t } = useI18n();
const router = useRouter();

// ===== 详情 =====

const detail = shallowRef<BrowseNovelDetail | null>(null);
const loading = ref(false);
const error = ref("");
/** 查看者收藏态（来自详情响应 bookmarkState；收藏按钮经 change 回写） */
const bookmarkState = ref<WorkBookmarkState | null>(null);

const item = computed(() => detail.value?.item ?? null);
const content = computed(() => detail.value?.content ?? "");
const hasContent = computed(() => content.value.trim().length > 0);
const series = computed(() => detail.value?.series ?? null);
const nextEpisodeId = computed(() => series.value?.next_id ?? null);

const restricted = computed(() => item.value?.x_restrict === 1 || item.value?.x_restrict === 2);
const restrictedLabel = computed(() =>
  item.value?.x_restrict === 2 ? t("common.browseR18G") : t("common.browseR18")
);

/** 元信息：字数 / 阅读时长 / 收藏数（reading_time 契约无单位，按分钟展示）。 */
const metaText = computed(() => {
  const it = item.value;
  if (!it) return "";
  const parts: string[] = [];
  if (it.text_length != null) parts.push(t("browse.novel.words", { count: it.text_length.toLocaleString() }));
  if (it.reading_time != null) parts.push(t("browse.novel.readingTime", { count: it.reading_time }));
  if (it.bookmark_count != null)
    parts.push(t("browse.novel.bookmarks", { count: it.bookmark_count.toLocaleString() }));
  return parts.join(" · ");
});

async function load(): Promise<void> {
  loading.value = true;
  error.value = "";
  detail.value = null;
  bookmarkState.value = null;
  relatedItems.value = [];
  relatedError.value = "";
  panel.value = "related";
  // 进入/切换作品回到页首（SPA 内路由切换会保留上一页滚动位置）
  window.scrollTo(0, 0);
  try {
    const data = await browseWorkDetail("novel", props.id);
    if (data.detail_kind !== "novel") throw new Error("unexpected detail kind");
    detail.value = data;
    bookmarkState.value = data.bookmarkState ?? null;
  } catch (err) {
    error.value = errorMessage(err) || t("common.browseLoadFailed");
  } finally {
    loading.value = false;
  }
  if (detail.value) void loadRelated();
}

// ===== 翻页 =====

const page = ref(1);
const totalPages = ref(1);

function gotoPage(target: number): void {
  const clamped = Math.min(Math.max(1, target), Math.max(1, totalPages.value));
  if (clamped === page.value) return;
  page.value = clamped;
  // 切页回到正文顶部；默认瞬时滚动，自动跟随系统「减少动态效果」偏好。
  window.scrollTo(0, 0);
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
  if (event.key === "ArrowLeft") {
    event.preventDefault();
    gotoPage(page.value - 1);
  } else if (event.key === "ArrowRight") {
    event.preventDefault();
    gotoPage(page.value + 1);
  }
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));

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

function searchTag(tag: string): void {
  router.push({ path: "/browse/search", query: { word: tag } });
}

/** 用系统默认浏览器打开 pixiv 原页。 */
function openInPixiv(): void {
  void openInBrowser(pixivWorkUrl("novel", props.id)).catch(() =>
    notify(t("browse.hooks.openFailed"))
  );
}
</script>

<template>
  <div class="novel-view">
    <!-- 顶栏：返回 / 标题 / 作者 / 返填表单 / 在浏览器中打开 -->
    <header class="topbar">
      <md-icon-button :aria-label="t('browse.novel.back')" :title="t('browse.novel.back')" @click="router.back()">
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="15 18 9 12 15 6" /></svg>
      </md-icon-button>
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
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M20 5H4a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h3v4l5-4h8a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1z" />
        </svg>
      </md-icon-button>
      <md-icon-button
        :aria-label="t('browse.hooks.fillNovelForm')"
        :title="t('browse.hooks.fillNovelForm')"
        @click="fillDownloadForm({ form: 'novel', sourceType: 'single', sourceId: props.id })"
      >
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12" /><path d="m7 10 5 5 5-5" /><path d="M4 21h16" /></svg>
      </md-icon-button>
      <md-icon-button
        :aria-label="t('browse.hooks.openInBrowser')"
        :title="t('browse.hooks.openInBrowser')"
        @click="openInPixiv"
      >
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><polyline points="15 3 21 3 21 9" /><line x1="10" y1="14" x2="21" y2="3" /></svg>
      </md-icon-button>
    </header>

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

    <template v-else-if="detail && item">
      <div class="reader-column">
        <!-- 信息头 -->
        <div class="info-head">
          <h1 class="work-title" :title="item.title">{{ item.title }}</h1>
          <p class="author-line">
            <router-link class="author-link" :to="`/browse/user/${item.author_id}`">{{ item.author_name }}</router-link>
            <span v-if="restricted" class="r18-pill">{{ restrictedLabel }}</span>
          </p>
          <router-link
            v-if="series"
            class="series-link"
            :to="`/browse/series/novel/${series.id}`"
            :title="t('browse.novel.seriesLabel', { title: series.title, order: series.order })"
          >
            {{ t("browse.novel.seriesLabel", { title: series.title, order: series.order }) }}
          </router-link>
          <div v-if="item.tags?.length" class="tag-row">
            <button
              v-for="tag in item.tags"
              :key="tag"
              type="button"
              class="tag-chip"
              :title="t('common.search')"
              @click="searchTag(tag)"
            >
              {{ tag }}
            </button>
          </div>
          <p v-if="metaText" class="work-meta">{{ metaText }}</p>
        </div>

        <!-- 正文（NovelContent 分页渲染）；空内容容错 -->
        <NovelContent v-if="hasContent" :content="content" :page="page" @pages-change="totalPages = $event" />
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
          <CommentsSection v-else kind="novel" :id="id" />
        </section>
      </div>

      <!-- 翻页器：底部居中吸底（AppPagination reader 变体）；键盘 ←/→ 翻页仍由本视图层监听 -->
      <AppPagination
        v-if="hasContent"
        variant="reader"
        :current-page="page"
        :total-pages="totalPages"
        @update:currentPage="gotoPage"
      />
    </template>
  </div>
</template>

<style scoped>
/* 满血宽度：抵消 .app-content 的 24px 内边距（640px 下为 16px），让顶栏/翻页器整行贴边。 */
.novel-view {
  margin: calc(-1 * var(--space-xl)) calc(-1 * var(--space-xl)) calc(-1 * var(--space-xl));
}

@media (max-width: 640px) {
  .novel-view {
    margin: calc(-1 * var(--space-lg));
  }
}

/* ===== 顶栏 ===== */

.topbar {
  position: sticky;
  top: 0;
  z-index: 10;
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

/* ===== 正文列 ===== */

.reader-column {
  max-width: 720px;
  margin: 0 auto;
  padding: var(--space-lg) var(--space-lg) var(--space-xl);
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

.series-link {
  display: inline-block;
  margin-top: var(--space-sm);
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--md-sys-color-primary);
  font-size: 13px;
  text-decoration: none;
}

.series-link:hover {
  text-decoration: underline;
}

.tag-row {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: var(--space-xs);
  margin-top: var(--space-md);
}

.tag-chip {
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

/* 面板：相关推荐 / 评论（顶栏按钮切换）；吸顶顶栏之下留出滚动余量 */
.side-panel {
  margin-top: var(--space-xl);
  scroll-margin-top: 72px;
}

.section-title {
  margin: 0 0 var(--space-md);
  color: var(--ink);
  font-size: 16px;
  font-weight: 700;
  line-height: 1.4;
}

/* ===== 翻页器（AppPagination reader 变体自带吸底样式，此处无本地翻页器样式） ===== */

.bar-icon {
  width: 20px;
  height: 20px;
  stroke-width: 1.8;
}
</style>
