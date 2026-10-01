<script setup lang="ts">
/**
 * 浏览·收藏页（bookmark-ui-v1 / F1）—— 自己的收藏（他人公开收藏在作者页 tab）。
 *
 * - 顶部 SectionTabs：插画·漫画 / 小说（kind illust|novel，每页 48/30 与官方一致）；
 * - 公开/私密切换（rest show|hide，胶囊分段按钮；切换重置分页与列表）；
 * - 左侧标签栏：全部 + browse_bookmark_tags 当前组标签（名称+计数；空名 = 未分类，i18n）；
 *   公开筛选展示 public 组、私密筛选展示 private 组（端点无 rest 参数，一次返回两组）；
 * - 右侧 WorkGrid + useInfiniteList 无限滚动 + 总数行 + 骨架屏/空态/错误态；
 * - 卡片 hover「取消收藏」：用列表项 bookmarkId 直接删除（乐观移除 + 失败恢复 + 通知）；
 * - 契约：tag=null 全部、"" 未分类；offset 游标经适配转为 useInfiniteList 的 page 语义。
 */
import { computed, onMounted, ref, shallowRef } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import {
  BOOKMARK_PAGE_SIZE,
  browseBookmarkList,
  browseBookmarkRemove,
  browseBookmarkTags,
  errorMessage,
  type BookmarkKind,
  type BookmarkRest,
  type BrowseBookmarkTag,
  type BrowseBookmarkTags,
  type BrowseWorkItem,
} from "../../api/browse";
import SectionTabs from "../../components/browse/SectionTabs.vue";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import { notify } from "../../ui/notify";
import { useInfiniteList } from "../../composables/useInfiniteList";

const { t } = useI18n();
const router = useRouter();

// ===== 筛选状态 =====

const kind = ref<BookmarkKind>("illust");
const rest = ref<BookmarkRest>("show");
/** null = 全部；"" = 未分类；其余为标签名 */
const tag = ref<string | null>(null);

const kindTabs = computed(() => [
  { value: "illust", label: t("browse.bookmark.kindIllust") },
  { value: "novel", label: t("browse.bookmark.kindNovel") },
]);

// ===== 标签栏 =====

const tags = shallowRef<BrowseBookmarkTags | null>(null);
const tagsError = ref("");

async function loadTags(): Promise<void> {
  tagsError.value = "";
  try {
    tags.value = await browseBookmarkTags(kind.value);
  } catch (err) {
    // 标签加载失败不阻塞列表：栏内提示，列表照常展示「全部」
    tagsError.value = errorMessage(err) || t("common.browseLoadFailed");
  }
}

/** 公开筛选展示 public 组、私密筛选展示 private 组。 */
const visibleTags = computed<BrowseBookmarkTag[]>(() => {
  if (!tags.value) return [];
  return rest.value === "hide" ? tags.value.private : tags.value.public;
});

/** 「全部」行计数 = 当前组标签计数合计（与列表 total 自洽，见调研 §11.2）。 */
const allCount = computed(() => visibleTags.value.reduce((sum, tg) => sum + tg.count, 0));

function tagName(name: string): string {
  return name === "" ? t("browse.bookmark.untagged") : name;
}

// ===== 列表（后端 offset 游标 → useInfiniteList 的 page 语义适配）=====

const total = ref<number | null>(null);
let offsetCursor = 0;

const list = useInfiniteList<BrowseWorkItem>(async (page) => {
  const offset = page === 1 ? 0 : offsetCursor;
  const data = await browseBookmarkList(
    kind.value,
    rest.value,
    tag.value,
    offset,
    BOOKMARK_PAGE_SIZE[kind.value]
  );
  offsetCursor = data.next ?? offset;
  if (page === 1) total.value = data.total;
  return { items: data.items, total: data.total, next_page: data.next == null ? null : page + 1 };
});
const { items, loading, loadingMore, error, hasMore, loadMore, retry } = list;

/** 筛选任一维度变化：重置游标与列表并回到页首（useInfiniteList.reload 只重置 page，游标需自清）。 */
function reloadList(): void {
  offsetCursor = 0;
  total.value = null;
  list.reload();
  window.scrollTo({ top: 0 });
}

function onKindChange(value: string): void {
  const next = value as BookmarkKind;
  if (next === kind.value) return;
  kind.value = next;
  tag.value = null; // 标签属于 kind，切换后回到「全部」
  void loadTags();
  reloadList();
}

function setRest(value: BookmarkRest): void {
  if (value === rest.value) return;
  rest.value = value;
  reloadList();
}

function setTag(value: string | null): void {
  if (value === tag.value) return;
  tag.value = value;
  reloadList();
}

onMounted(() => {
  void loadTags();
  void loadMore();
});

// ===== 取消收藏（乐观移除 + 失败恢复）=====

/** 进行中的删除（非响应式：卡片已乐观移除，仅防同项重复提交）。 */
const removingKeys = new Set<string>();

/** kind 归一：取消收藏走 illust（覆盖插画/漫画/动图）或 novel 端点族。 */
function endpointKind(item: BrowseWorkItem): "illust" | "novel" {
  return item.kind === "novel" ? "novel" : "illust";
}

async function onRemoveBookmark(item: BrowseWorkItem): Promise<void> {
  if (!item.bookmarkId) return; // 无 bookmarkId 的异常项：WorkGrid 已不显示动作，这里兜底
  const key = `${item.kind}:${item.id}`;
  const index = items.value.indexOf(item);
  if (removingKeys.has(key) || index < 0) return;
  // 乐观移除（useInfiniteList 的 shallowRef 整体替换触发更新）
  items.value = items.value.filter((it) => it !== item);
  removingKeys.add(key);
  try {
    await browseBookmarkRemove(endpointKind(item), item.id, item.bookmarkId);
    if (total.value != null) total.value = Math.max(0, total.value - 1);
    notify(t("browse.bookmark.removed"));
    void loadTags(); // 标签计数随删除变化
  } catch (err) {
    // 失败恢复：尽量插回原位置
    const arr = [...items.value];
    arr.splice(Math.min(index, arr.length), 0, item);
    items.value = arr;
    notify(errorMessage(err) || t("browse.bookmark.removeFailed"));
  } finally {
    removingKeys.delete(key);
  }
}

// ===== 卡片跳转：novel → 阅读器，其余走插画/漫画查看器（ugoira V1 显示封面帧）=====

function goWork(item: BrowseWorkItem): void {
  if (item.kind === "novel") void router.push(`/browse/work/novel/${item.id}`);
  else if (item.kind === "manga") void router.push(`/browse/work/manga/${item.id}`);
  else void router.push(`/browse/work/illust/${item.id}`);
}
</script>

<template>
  <div class="page-view bookmark-view">
    <h1 class="page-title">{{ t("nav.browseBookmark") }}</h1>

    <!-- 控制行：类型 tabs（左） + 公开/私密（右） -->
    <div class="bm-controls">
      <SectionTabs class="bm-kind-tabs" :tabs="kindTabs" :value="kind" @change="onKindChange" />
      <div class="rest-group" role="group" :aria-label="t('browse.bookmark.restLabel')">
        <button
          type="button"
          class="rest-btn"
          :class="{ active: rest === 'show' }"
          :aria-pressed="rest === 'show'"
          @click="setRest('show')"
        >
          {{ t("browse.bookmark.restPublic") }}
        </button>
        <button
          type="button"
          class="rest-btn"
          :class="{ active: rest === 'hide' }"
          :aria-pressed="rest === 'hide'"
          @click="setRest('hide')"
        >
          {{ t("browse.bookmark.restPrivate") }}
        </button>
      </div>
    </div>

    <div class="bm-body">
      <!-- 左：标签筛选栏 -->
      <nav class="tag-rail" :aria-label="t('browse.bookmark.tagFilter')">
        <button
          type="button"
          class="tag-item"
          :class="{ active: tag === null }"
          @click="setTag(null)"
        >
          <span class="tag-name">{{ t("browse.bookmark.tagAll") }}</span>
          <span class="tag-count">{{ allCount.toLocaleString() }}</span>
        </button>
        <p v-if="tagsError" class="tag-error">{{ tagsError }}</p>
        <button
          v-for="tg in visibleTags"
          :key="tg.name"
          type="button"
          class="tag-item"
          :class="{ active: tag === tg.name }"
          :title="tagName(tg.name)"
          @click="setTag(tg.name)"
        >
          <span class="tag-name">{{ tagName(tg.name) }}</span>
          <span class="tag-count">{{ tg.count.toLocaleString() }}</span>
        </button>
      </nav>

      <!-- 右：总数 + 网格 -->
      <div class="bm-main">
        <p v-if="total !== null" class="total-line" role="status">
          {{ t("browse.bookmark.total", { count: total.toLocaleString() }) }}
        </p>
        <WorkGrid
          :items="items"
          :loading="loading"
          :error="error"
          :loading-more="loadingMore"
          :has-more="hasMore"
          removable
          @load-more="loadMore"
          @retry="retry"
          @select="goWork"
          @remove-bookmark="onRemoveBookmark"
        />

        <!-- 追加失败的页内重试（WorkGrid 错误态仅在无内容时出现） -->
        <div v-if="error && items.length" class="append-error">
          <span class="append-error-text">{{ error }}</span>
          <md-outlined-button @click="retry">{{ t("common.retry") }}</md-outlined-button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* ===== 控制行 ===== */
.bm-controls {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-md);
  margin-bottom: var(--space-lg);
}

.bm-kind-tabs {
  flex: 1;
  min-width: 0;
}

/* 公开/私密切换：胶囊分段按钮，选中态用 primary-container（与导航 active 同角色） */
.rest-group {
  display: inline-flex;
  flex-shrink: 0;
  padding: 2px;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
}

.rest-btn {
  min-width: 64px;
  padding: var(--space-xs) var(--space-lg);
  border: none;
  border-radius: 999px;
  background: none;
  color: var(--ink-muted);
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
  cursor: pointer;
}

.rest-btn:hover {
  color: var(--ink);
}

.rest-btn:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 1px;
}

.rest-btn.active {
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

/* ===== 双栏主体 ===== */
.bm-body {
  display: flex;
  align-items: flex-start;
  gap: var(--space-lg);
}

.tag-rail {
  display: flex;
  flex-direction: column;
  gap: 2px;
  width: 180px;
  flex-shrink: 0;
  position: sticky;
  top: var(--space-xl);
}

.tag-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-sm);
  min-height: 36px;
  padding: 0 var(--space-md);
  border: none;
  border-radius: 999px;
  background: none;
  color: var(--ink);
  font: inherit;
  font-size: 14px;
  line-height: 1.4;
  text-align: left;
  cursor: pointer;
}

.tag-item:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.tag-item:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.tag-item.active {
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
  font-weight: 600;
}

.tag-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tag-count {
  flex-shrink: 0;
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
}

.tag-item.active .tag-count {
  color: inherit;
}

.tag-error {
  margin: var(--space-xs) 0 0;
  padding: 0 var(--space-md);
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

/* ===== 右侧网格列 ===== */
.bm-main {
  flex: 1;
  min-width: 0;
}

.total-line {
  margin: 0 0 var(--space-md);
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
}

.append-error {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: var(--space-md);
  margin-top: var(--space-lg);
}

.append-error-text {
  color: var(--ink-muted);
  font-size: 13px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

/* ===== 640px：标签栏转为横向换行 chips，随文档流 ===== */
@media (max-width: 640px) {
  .bm-body {
    flex-direction: column;
  }

  .tag-rail {
    position: static;
    width: 100%;
    flex-direction: row;
    flex-wrap: wrap;
    gap: var(--space-xs);
  }

  .tag-item {
    min-height: 32px;
    padding: 0 var(--space-sm);
  }
}
</style>
