<script setup lang="ts">
/**
 * 作者主页（F5）：头部信息卡（无阴影 surface-container 区块）+ 三类作品 tab。
 *
 * - 头部：头像（pxSrc 代理，失败回退占位）、昵称、@pixiv_id、统计行、
 *   简介(comment_html 剥 HTML 标签为纯文本，3 行截断 + 展开/收起)、
 *   「在 Pixiv 浏览器中打开」→ 切到 /pixiv 后 browse_navigate 到用户页。
 * - 作品区：插画 / 漫画 / 小说三个 tab 各自持有独立的 useInfiniteList
 *   （切 tab 不丢已加载内容，回到该 tab 经 IntersectionObserver 续传），
 *   空态按类型给文案；追加页失败在网格下方就地重试。
 * - 未登录 / 无权限等错误直接展示 api 层归一文案（invokeBrowse 已联动登录弹窗）。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import "@material/web/iconbutton/outlined-icon-button.js";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import SectionTabs from "../../components/browse/SectionTabs.vue";
import {
  browseUserProfile,
  browseUserWorks,
  errorMessage,
  invoke,
  isTauri,
  pxSrc,
  type BrowseUserProfile,
  type BrowseWorkItem,
  type ListWorkKind,
} from "../../api/browse";
import { useInfiniteList } from "../../composables/useInfiniteList";

const props = defineProps<{ id: number }>();

const { t } = useI18n();
const router = useRouter();

// ===== 头部：作者信息 =====

const profile = shallowRef<BrowseUserProfile | null>(null);
const profileLoading = ref(false);
const profileError = ref("");

async function loadProfile(): Promise<void> {
  if (profileLoading.value) return;
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

// ===== 作品区：三类 tab，各自独立分页 =====

const KINDS: ListWorkKind[] = ["illust", "manga", "novel"];

const tabs = computed(() => [
  { value: "illust", label: t("nav.browseIllustration") },
  { value: "manga", label: t("nav.browseManga") },
  { value: "novel", label: t("nav.browseNovel") },
]);

const activeTab = ref<ListWorkKind>("illust");

type WorkList = ReturnType<typeof useInfiniteList<BrowseWorkItem>>;

/** 每个 kind 一个独立列表状态机：切 tab 不丢已加载内容，回到该 tab 续传。 */
const lists = {
  illust: useInfiniteList<BrowseWorkItem>((page) => browseUserWorks(props.id, "illust", page)),
  manga: useInfiniteList<BrowseWorkItem>((page) => browseUserWorks(props.id, "manga", page)),
  novel: useInfiniteList<BrowseWorkItem>((page) => browseUserWorks(props.id, "novel", page)),
} satisfies Record<ListWorkKind, WorkList>;

/** 尚未加载过首屏的 tab 在激活时拉起第 1 页。 */
function ensureStarted(kind: ListWorkKind): void {
  const list = lists[kind];
  if (list.items.value.length || list.loading.value || list.loadingMore.value || list.error.value || !list.hasMore.value) return;
  void list.loadMore();
}

watch(activeTab, ensureStarted);

/** 首次进入 / 换作者：清空三类列表并重拉第 1 页 + 头部信息。 */
function resetAll(): void {
  profile.value = null;
  profileError.value = "";
  for (const kind of KINDS) lists[kind].reset();
  void loadProfile();
  ensureStarted("illust");
}

watch(
  () => props.id,
  () => resetAll()
);

/** panes：把三类列表的响应式值快照成模板友好的普通对象（ref 嵌在对象里不自动解包）。 */
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
const EMPTY_KEY: Record<ListWorkKind, string> = {
  illust: "browse.author.emptyIllust",
  manga: "browse.author.emptyManga",
  novel: "browse.author.emptyNovel",
};

function openWork(item: BrowseWorkItem): void {
  router.push(`/browse/work/${item.kind}/${item.id}`);
}

// ===== 在 Pixiv 浏览器中打开 =====

const openingBrowse = ref(false);

async function openInPixivBrowser(): Promise<void> {
  const url = `https://www.pixiv.net/users/${props.id}`;
  if (!isTauri()) {
    window.open(url, "_blank", "noopener");
    return;
  }
  if (openingBrowse.value) return;
  openingBrowse.value = true;
  try {
    // 先切到 Pixiv 浏览器路由：browse_open 在其 onMounted 异步创建子 webview；
    // webview 就绪前 browse_navigate 是静默 no-op，故短轮询重试直到导航被接受
    //（同一 URL 重复导航无害，就绪后的首次调用即生效）。
    void router.push("/pixiv");
    for (let attempt = 0; attempt < 20; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 150));
      try {
        await invoke("browse_navigate", { url });
        return;
      } catch {
        // 导航失败（如 webview 恰在创建中）→ 下一轮重试
      }
    }
  } finally {
    openingBrowse.value = false;
  }
}

onMounted(() => {
  void loadProfile();
  ensureStarted("illust");
  window.addEventListener("resize", onWindowResize, { passive: true });
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", onWindowResize);
});
</script>

<template>
  <div class="page-view author-view">
    <!-- ===== 头部信息卡（surface-container 区块，无阴影）===== -->
    <section class="author-card">
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
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <circle cx="12" cy="8" r="4" />
                <path d="M4.5 20a7.5 7.5 0 0 1 15 0" />
              </svg>
            </div>
          </div>
          <div class="card-id">
            <h1 class="author-name">{{ profile.name }}</h1>
            <p class="author-pixiv-id">@{{ profile.pixiv_id }}</p>
            <p class="author-stats">
              {{ t("browse.author.followingCount", { n: profile.following_count ?? 0 }) }}
              <span class="stats-divider" aria-hidden="true">·</span>
              {{ t("browse.author.myPixivCount", { n: profile.mypixiv_count ?? 0 }) }}
            </p>
          </div>
          <md-outlined-icon-button
            class="open-browse"
            :disabled="openingBrowse"
            :aria-label="t('browse.author.openInBrowser')"
            :title="t('browse.author.openInBrowser')"
            @click="openInPixivBrowser"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M14 4h6v6" />
              <path d="M20 4 11 13" />
              <path d="M19 14v5a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h5" />
            </svg>
          </md-outlined-icon-button>
        </div>

        <!-- 简介：剥标签纯文本，3 行截断 + 展开/收起 -->
        <div v-if="bioText" class="bio-block">
          <p ref="bioEl" class="bio-text" :class="{ clamped: !bioExpanded }">{{ bioText }}</p>
          <md-text-button v-if="bioOverflow || bioExpanded" class="bio-toggle" @click="toggleBio">
            {{ bioExpanded ? t("browse.author.collapseBio") : t("browse.author.expandBio") }}
          </md-text-button>
        </div>
      </template>
    </section>

    <!-- ===== 作品区：三类 tab ===== -->
    <SectionTabs class="works-tabs" :tabs="tabs" :value="activeTab" @change="activeTab = $event as ListWorkKind" />

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
  stroke-width: 1.6;
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

.open-browse svg {
  width: 20px;
  height: 20px;
  stroke-width: 1.8;
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
.works-tabs {
  margin-top: var(--space-sm);
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
