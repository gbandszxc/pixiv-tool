<script setup lang="ts">
import PageBackButton from "../../components/navigation/PageBackButton.vue";
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
/**
 * 作者主页（F5，bookmark-ui-v1 追加收藏 tab）：
 * 头部信息卡（无阴影 surface-container 区块）+ 三类作品 tab + 收藏 tab。
 *
 * - 头部：头像（pxSrc 代理，失败回退占位）、昵称、@pixiv_id、统计行、
 *   简介(comment_html 剥 HTML 标签为纯文本，3 行截断 + 展开/收起)、
 *   「在浏览器中打开」→ 系统默认浏览器打开用户主页。
 * - 作品区：插画 / 漫画 / 小说 / 收藏四个 tab 各自持有独立的 useInfiniteList
 *   （切 tab 不丢已加载内容，回到该 tab 经 IntersectionObserver 续传），
 *   空态按类型给文案；追加页失败在网格下方就地重试。
 *   三类作品 tab 支持升降序切换（默认时间倒序，localStorage 跨会话记忆；
 *   收藏 tab 是 offset 游标语义，与排序无关，不显示切换控件）。
 *   收藏 tab = 该作者的他人公开收藏（契约：uid 直传 + 显式 rest=show，后端据此走
 *   /ajax/user/{uid}/... 他人路径；offset 游标经适配转 page 语义；
 *   不显示取消收藏动作 —— 列表项 bookmarkId 是查看者态，UI 不使用）。
 * - 未登录 / 无权限等错误直接展示 api 层归一文案（invokeBrowse 已联动登录弹窗）。
 */
import { computed, nextTick, onActivated, onBeforeUnmount, onDeactivated, onMounted, ref, shallowRef, watch } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import "@material/web/iconbutton/outlined-icon-button.js";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import SectionTabs from "../../components/browse/SectionTabs.vue";
import SortDirectionToggle from "../../components/common/SortDirectionToggle.vue";
import {
  browseBookmarkList,
  browseUserProfile,
  browseUserFollow,
  browseUserWorks,
  errorMessage,
  pxSrc,
  type BrowseUserProfile,
  type BrowseWorkItem,
  type ListWorkKind,
  type WorkOrder,
} from "../../api/browse";
import { notify } from "../../ui/notify";
import { fillDownloadForm, openInBrowser } from "../../utils/pixivHooks";
import { pixivUserUrl } from "../../utils/pixivUrl";
import { useInfiniteList } from "../../composables/useInfiniteList";
import { useAuthStore } from "../../stores/auth";

const props = defineProps<{ id: number }>();

const { t } = useI18n();
const router = useRouter();
const authStore = useAuthStore();

// ===== 头部：作者信息 =====

const profile = shallowRef<BrowseUserProfile | null>(null);
const profileLoading = ref(false);
const profileError = ref("");
const followBusy = ref(false);

async function toggleFollow(): Promise<void> {
  const current = profile.value;
  if (followBusy.value || profileLoading.value || typeof current?.is_followed !== "boolean" || String(current.id) === authStore.userId) return;
  const account = authStore.userId;
  const followed = !current.is_followed;
  followBusy.value = true;
  try {
    const result = await browseUserFollow(current.id, followed);
    if (profile.value !== current || authStore.userId !== account) return;
    profile.value = { ...current, is_followed: result.is_followed };
    notify(t(result.is_followed ? "browse.author.followSuccess" : "browse.author.unfollowSuccess"));
  } catch (err) {
    if (profile.value === current && authStore.userId === account) notify(errorMessage(err) || t("browse.author.followFailed"));
  } finally {
    followBusy.value = false;
  }
}

async function loadProfile(): Promise<void> {
  if (profileLoading.value || followBusy.value) return;
  profileLoading.value = true;
  profileError.value = "";
  try {
    profile.value = await browseUserProfile(props.id);
  } catch (err) {
    // 未登录 / 无权限等按 api 层错误文案直接展示（未登录时 api 层已派发登录弹窗事件）
    profileError.value = errorMessage(err);
  } finally {
    profileLoading.value = false;
  }
}

/** 头像加载失败 → 占位图（圆形 primary 弱色底 + 人形 icon）。 */
const avatarBroken = ref(false);

// ===== 简介：剥 HTML 标签 + 3 行截断 =====

const bioText = computed<string>(() => {
  const html = profile.value?.comment_html;
  return html ? htmlToText(html) : "";
});

/** comment_html → 纯文本。DOMParser 不执行脚本与内联事件，安全；
 * <br> 与块级闭合先换行，避免 textContent 把段落粘连成一行。 */
function htmlToText(html: string): string {
  const withBreaks = html
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/(?:p|div|li)>/gi, "\n");
  const text = new DOMParser().parseFromString(withBreaks, "text/html").body.textContent ?? "";
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .join("\n");
}

const bioExpanded = ref(false);
const bioOverflow = ref(false);
const bioEl = ref<HTMLParagraphElement | null>(null);

/** 收起态下测量是否溢出（scrollHeight > clientHeight 即超 3 行）；
 * 展开态保留上次测量值，保证「收起」按钮不消失。 */
function measureBio(): void {
  const el = bioEl.value;
  if (!el || bioExpanded.value) return;
  bioOverflow.value = el.scrollHeight > el.clientHeight + 1;
}

function toggleBio(): void {
  bioExpanded.value = !bioExpanded.value;
  if (!bioExpanded.value) void nextTick(measureBio);
}

function onWindowResize(): void {
  if (!bioExpanded.value) measureBio();
}

watch(profile, async () => {
  avatarBroken.value = false;
  bioExpanded.value = false;
  bioOverflow.value = false;
  await nextTick();
  measureBio();
});

// ===== 作品区：三类作品 + 收藏 tab，各自独立分页 =====

// ===== 排序偏好（localStorage，跨会话记忆；不写 settings.json）=====

/** 与生产默认一致：desc = 时间倒序（最新在前）。 */
const ORDER_STORAGE_KEY = "pixiv-tool-author-order";

function loadOrder(): WorkOrder {
  try {
    return localStorage.getItem(ORDER_STORAGE_KEY) === "asc" ? "asc" : "desc";
  } catch {
    return "desc"; // 隐私模式 / 存储被禁：静默回落默认，不影响使用
  }
}

function saveOrder(value: WorkOrder): void {
  try { localStorage.setItem(ORDER_STORAGE_KEY, value); } catch { /* 同上，仅本次会话生效 */ }
}

const order = ref<WorkOrder>(loadOrder());

/** tab 取值域：三类作品 + 收藏（他人公开收藏）。 */
type AuthorTab = ListWorkKind | "bookmark";

const KINDS: AuthorTab[] = ["illust", "manga", "novel", "bookmark"];

const tabs = computed(() => [
  { value: "illust", label: t("nav.browseIllustration") },
  { value: "manga", label: t("nav.browseManga") },
  { value: "novel", label: t("nav.browseNovel") },
  { value: "bookmark", label: t("browse.bookmark.authorTab") },
]);

const activeTab = ref<AuthorTab>("illust");

/** 作品区工具行（排序切换后滚回其顶部）。 */
const worksBar = ref<HTMLElement | null>(null);

type WorkList = ReturnType<typeof useInfiniteList<BrowseWorkItem>>;

/** 收藏 tab（他人公开收藏）分页游标：后端为 offset，转为 useInfiniteList 的 page 语义。 */
let bookmarkOffset = 0;

/** 每个 tab 一个独立列表状态机：切 tab 不丢已加载内容，回到该 tab 续传。 */
const lists = {
  illust: useInfiniteList<BrowseWorkItem>((page) => browseUserWorks(props.id, "illust", page, order.value)),
  manga: useInfiniteList<BrowseWorkItem>((page) => browseUserWorks(props.id, "manga", page, order.value)),
  novel: useInfiniteList<BrowseWorkItem>((page) => browseUserWorks(props.id, "novel", page, order.value)),
  bookmark: useInfiniteList<BrowseWorkItem>(async (page, isCurrent) => {
    const offset = page === 1 ? 0 : bookmarkOffset;
    const data = await browseBookmarkList("illust", "show", null, offset, 24, props.id); // 他人公开收藏：uid 直传 + rest=show，官方作者收藏页 24/页
    if (isCurrent()) bookmarkOffset = data.next ?? offset;
    return { items: data.items, total: data.total, next_page: data.next == null ? null : page + 1 };
  }),
} satisfies Record<AuthorTab, WorkList>;

/** 尚未加载过首屏的 tab 在激活时拉起第 1 页。 */
function ensureStarted(kind: AuthorTab): void {
  const list = lists[kind];
  if (list.items.value.length || list.loading.value || list.loadingMore.value || list.error.value || !list.hasMore.value) return;
  void list.loadMore();
}

watch(activeTab, ensureStarted);

/** 首次进入 / 换作者：清空四类列表并重拉第 1 页 + 头部信息。 */
function resetAll(): void {
  profile.value = null;
  profileError.value = "";
  for (const kind of KINDS) lists[kind].reset();
  void loadProfile();
  ensureStarted("illust");
}

function refresh(): void {
  if (followBusy.value) return;
  void loadProfile();
  if (activeTab.value === "bookmark") bookmarkOffset = 0;
  lists[activeTab.value].reload();
}

/** 排序切换：只重置三类作品列表（收藏 tab 与排序无关，保持不动），回到第 1 页并滚回作品区顶部。 */
function applyOrder(next: WorkOrder): void {
  if (next === order.value) return;
  order.value = next;
  saveOrder(next);
  for (const kind of ["illust", "manga", "novel"] as const) lists[kind].reset();
  worksBar.value?.scrollIntoView({ block: "start" });
  ensureStarted(activeTab.value); // 活动 tab 立即重拉第 1 页；其余 tab 由 watch(activeTab, ensureStarted) 在切回时拉起
}

watch(
  () => props.id,
  () => resetAll()
);

/** panes：把四类列表的响应式值快照成模板友好的普通对象（ref 嵌在对象里不自动解包）。 */
const panes = computed(() =>
  KINDS.map((kind) => {
    const list = lists[kind];
    return {
      kind,
      items: list.items.value,
      loading: list.loading.value,
      loadingMore: list.loadingMore.value,
      error: list.error.value,
      hasMore: list.hasMore.value,
      /** 有骨架/错误/内容时渲染 WorkGrid，否则渲染该类型的空态文案 */
      gridVisible: list.items.value.length > 0 || list.loading.value || !!list.error.value,
    };
  })
);

/** 空态文案按类型区分（「该作者还没有漫画作品」等）。 */
const EMPTY_KEY: Record<AuthorTab, string> = {
  illust: "browse.author.emptyIllust",
  manga: "browse.author.emptyManga",
  novel: "browse.author.emptyNovel",
  bookmark: "browse.bookmark.authorEmpty",
};

function openWork(item: BrowseWorkItem): void {
  // ugoira 经 illust 端点取详情，路由归 illust（与相关推荐一致）
  const routeKind = item.kind === "ugoira" ? "illust" : item.kind;
  void router.push(`/browse/work/${routeKind}/${item.id}`);
}

// ===== 打开原页 / 返填 =====

/** 用系统默认浏览器打开该作者的 pixiv 主页。 */
function openInPixiv(): void {
  void openInBrowser(pixivUserUrl(props.id)).catch(() => notify(t("browse.hooks.openFailed")));
}

/** 返填跟随当前 tab：插画/漫画/收藏 → 插画抓取页（用户全集）；小说 → 小说抓取页（用户全集）。 */
function fillActiveTab(): void {
  fillDownloadForm({
    form: activeTab.value === "novel" ? "novel" : "illustration",
    sourceType: "user",
    sourceId: props.id,
  });
}

onMounted(() => {
  void loadProfile();
  ensureStarted("illust");
  window.addEventListener("resize", onWindowResize, { passive: true });
});

onActivated(() => {
  window.addEventListener("resize", onWindowResize, { passive: true });
  void nextTick(measureBio);
});
onDeactivated(() => window.removeEventListener("resize", onWindowResize));

onBeforeUnmount(() => {
  window.removeEventListener("resize", onWindowResize);
});
</script>

<template>
  <div class="page-view author-view">
    <div class="browse-list-header">
      <!-- 主页（/browse/me）经 #lead 把写操作入口并入本行左端；普通作者页无插槽内容，DOM 与布局零变化 -->
      <slot name="lead" />
      <ListRefreshButton :busy="followBusy || profileLoading || lists[activeTab].loading.value || lists[activeTab].loadingMore.value" @refresh="refresh" />
    </div>
    <!-- ===== 头部信息卡（surface-container 区块，无阴影）===== -->
    <section class="author-card">
      <div class="author-heading">
        <PageBackButton class="author-back" />
        <div class="author-profile">
          <!-- 加载骨架：纯色块，无动画 -->
          <div v-if="profileLoading && !profile" class="card-skeleton" aria-hidden="true">
            <div class="avatar skeleton-avatar"></div>
            <div class="skeleton-lines">
              <div class="skeleton-line w45"></div>
              <div class="skeleton-line w30"></div>
            </div>
          </div>

          <!-- 错误态：api 层文案（含未登录/无权限）+ 重试 -->
          <div v-else-if="profileError && !profile" class="card-state" role="alert">
            <p class="state-text">{{ profileError }}</p>
            <md-outlined-button @click="loadProfile">{{ t("common.retry") }}</md-outlined-button>
          </div>

          <template v-else-if="profile">
            <div class="card-top">
              <div class="avatar">
                <img
                  v-if="!avatarBroken"
                  :src="pxSrc(profile.profile_img)"
                  alt=""
                  @error="avatarBroken = true"
                />
                <div v-else class="avatar-fallback">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <!-- lucide circle-user-round -->
                    <path d="M17.925 20.056a6 6 0 0 0-11.851.001" />
                    <circle cx="12" cy="11" r="4" />
                    <circle cx="12" cy="12" r="10" />
                  </svg>
                </div>
              </div>
              <div class="card-id">
                <h1 class="author-name">{{ profile.name }}</h1>
                <!-- /ajax/user/{id} 实测不返回 account（2026-10-01），pixiv_id 恒为空 → 隐藏整行 -->
                <p v-if="profile.pixiv_id" class="author-pixiv-id">@{{ profile.pixiv_id }}</p>
                <p class="author-stats">
                  {{ t("browse.author.followingCount", { n: profile.following_count ?? 0 }) }}
                  <span class="stats-divider" aria-hidden="true">·</span>
                  {{ t("browse.author.myPixivCount", { n: profile.mypixiv_count ?? 0 }) }}
                </p>
              </div>
              <div class="author-actions">
                <component
                  :is="profile.is_followed ? 'md-outlined-button' : 'md-filled-button'"
                  v-if="String(profile.id) !== authStore.userId"
                  class="follow-button"
                  :disabled="followBusy || profileLoading || typeof profile.is_followed !== 'boolean'"
                  :aria-busy="followBusy"
                  :aria-pressed="profile.is_followed === true"
                  :aria-label="t(profile.is_followed ? 'browse.author.unfollow' : 'browse.author.follow')"
                  :title="t(typeof profile.is_followed !== 'boolean' ? 'browse.author.followUnknown' : profile.is_followed ? 'browse.author.unfollow' : 'browse.author.follow')"
                  @click="toggleFollow"
                >{{ t(followBusy ? 'common.loading' : profile.is_followed ? 'browse.author.followed' : 'browse.author.follow') }}</component>
                <md-outlined-icon-button
                  class="open-browse"
                  :aria-label="t('browse.hooks.openInBrowser')"
                  :title="t('browse.hooks.openInBrowser')"
                  @click="openInPixiv"
                >
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <!-- lucide square-arrow-out-up-right -->
                    <path d="M21 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h6" />
                    <path d="m21 3-9 9" />
                    <path d="M15 3h6v6" />
                  </svg>
                </md-outlined-icon-button>
              </div>
            </div>

            <!-- 简介：剥标签纯文本，3 行截断 + 展开/收起 -->
            <div v-if="bioText" class="bio-block">
              <p ref="bioEl" class="bio-text" :class="{ clamped: !bioExpanded }">{{ bioText }}</p>
              <md-text-button v-if="bioOverflow || bioExpanded" class="bio-toggle" @click="toggleBio">
                {{ bioExpanded ? t("browse.author.collapseBio") : t("browse.author.expandBio") }}
              </md-text-button>
            </div>
          </template>
        </div>
      </div>
    </section>

    <!-- ===== 作品区：四类 tab + 排序 + 返填（目标随当前 tab）===== -->
    <div ref="worksBar" class="works-bar">
      <SectionTabs class="works-tabs" :tabs="tabs" :value="activeTab" @change="activeTab = $event as AuthorTab" />
      <!-- 排序：仅三类作品 tab 有意义（收藏 tab 为 offset 游标，与排序无关）。
           外层不设 role=group —— SortDirectionToggle 根节点自带 role=group + aria-label，
           再嵌一层同名分组会让读屏播报两层。 -->
      <div v-if="activeTab !== 'bookmark'" class="works-order">
        <span class="order-label" aria-hidden="true">{{ t("browse.author.orderLabel") }}</span>
        <SortDirectionToggle :value="order" @change="applyOrder" />
        <span class="order-state" aria-live="polite">
          {{ t(order === "desc" ? "browse.author.orderNewest" : "browse.author.orderOldest") }}
        </span>
      </div>
      <md-outlined-button @click="fillActiveTab()">
        {{ activeTab === "novel" ? t("browse.hooks.fillNovelForm") : t("browse.hooks.fillIllustForm") }}
      </md-outlined-button>
    </div>

    <div v-for="pane in panes" v-show="activeTab === pane.kind" :key="pane.kind" class="works-pane">
      <WorkGrid
        v-if="pane.gridVisible"
        :items="pane.items"
        :loading="pane.loading"
        :error="pane.error"
        :loading-more="pane.loadingMore"
        :has-more="pane.hasMore"
        @load-more="lists[pane.kind].loadMore()"
        @retry="lists[pane.kind].retry()"
        @select="openWork"
      />
      <!-- 空态按类型给文案 -->
      <div v-else class="works-empty">
        <p class="state-text">{{ t(EMPTY_KEY[pane.kind]) }}</p>
      </div>

      <!-- 追加页失败（已有内容时 WorkGrid 不展示错误）：就地提示 + 重试 -->
      <div v-if="pane.error && pane.items.length" class="append-error" role="alert">
        <span class="append-error-text">{{ pane.error }}</span>
        <md-text-button @click="lists[pane.kind].retry()">{{ t("common.retry") }}</md-text-button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.author-view {
  display: flex;
  flex-direction: column;
  gap: var(--space-lg);
}

/* ===== 头部信息卡：surface-container 区块，16px 圆角，无阴影（DESIGN.md Overlay-Only Shadow Rule）===== */
.author-card {
  padding: var(--space-xl);
  border-radius: 16px;
  background: var(--md-sys-color-surface-container);
}

.author-heading { display: flex; align-items: flex-start; gap: var(--space-sm); }
.author-back { margin-top: var(--space-lg); }
.author-profile { flex: 1; min-width: 0; }

.card-top {
  display: flex;
  align-items: center;
  gap: var(--space-lg);
}

.avatar {
  flex-shrink: 0;
  width: 72px;
  height: 72px;
  border-radius: 999px;
  overflow: hidden;
  /* 主色弱色底占位（与 WorkCard 封面占位同源派生） */
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface));
}

.avatar img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.avatar-fallback {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  color: var(--ink-muted);
}

.avatar-fallback svg {
  width: 32px;
  height: 32px;
  stroke-width: 2;
}

.card-id {
  flex: 1;
  min-width: 0;
}

.author-name {
  margin: 0;
  color: var(--ink);
  font-size: 20px;
  font-weight: 700;
  line-height: 1.4;
  overflow-wrap: anywhere;
}

.author-pixiv-id {
  margin: var(--space-xxs) 0 0;
  color: var(--ink-muted);
  font-size: 13px;
  line-height: 1.4;
  overflow-wrap: anywhere;
}

.author-stats {
  margin: var(--space-xs) 0 0;
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
}

.stats-divider {
  margin: 0 var(--space-xxs);
}

.open-browse {
  flex-shrink: 0;
}

.author-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-sm);
  flex-shrink: 0;
}

.open-browse svg {
  width: 20px;
  height: 20px;
  stroke-width: 2;
}

/* ===== 简介块 ===== */
.bio-block {
  margin-top: var(--space-md);
}

.bio-text {
  margin: 0;
  color: var(--ink-muted);
  font-size: 14px;
  line-height: 1.5;
  white-space: pre-line;
  overflow-wrap: anywhere;
}

/* 3 行截断 */
.bio-text.clamped {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 3;
  overflow: hidden;
}

.bio-toggle {
  margin-left: calc(-1 * var(--space-sm));
}

/* ===== 头部骨架 / 错误态 ===== */
.card-skeleton {
  display: flex;
  align-items: center;
  gap: var(--space-lg);
}

.skeleton-avatar {
  flex-shrink: 0;
  width: 72px;
  height: 72px;
  border-radius: 999px;
}

.skeleton-lines {
  flex: 1;
}

.skeleton-line {
  height: 14px;
  border-radius: 999px;
  background: var(--md-sys-color-surface);
}

.skeleton-line + .skeleton-line {
  margin-top: var(--space-sm);
}

.skeleton-line.w45 {
  width: 45%;
}

.skeleton-line.w30 {
  width: 30%;
}

.card-state {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--space-sm);
}

.state-text {
  margin: 0;
  color: var(--ink-muted);
  font-size: 14px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

/* ===== 作品区 ===== */
.works-bar {
  display: flex;
  flex-wrap: wrap; /* 窄窗时排序组整体落行 */
  align-items: center;
  gap: var(--space-md);
  margin-top: var(--space-sm);
}

.works-tabs {
  flex: 1;
  min-width: 0;
}

.works-order {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xs);
  flex-shrink: 0;
}

.order-label,
.order-state {
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
}

.order-state {
  min-width: calc(4 * var(--space-xl)); /* 升降切换时文案不抖 */
}

.works-pane {
  min-height: 240px;
}

.works-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-xl) var(--space-lg);
  text-align: center;
}

.append-error {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-xs);
  margin-top: var(--space-md);
}

.append-error-text {
  color: var(--ink-muted);
  font-size: 13px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

/* ===== 640px：头部卡内部纵向堆叠 ===== */
@media (max-width: 640px) {
  .card-top {
    flex-direction: column;
    align-items: flex-start;
  }

  .open-browse {
    order: 10;
  }
}
</style>
