<script setup lang="ts">
/**
 * 浏览·关注动态页（F2）。
 *
 * - 两个 SectionTabs：作品类型（插画|小说 → kind illust|novel）与
 *   范围（全部|R-18 → mode all|r18）；
 * - `browseFollowLatest(kind, mode, page)` 分页加载（后端 next_page = isLastPage ? null : p+1，
 *   useInfiniteList 按 next_page 判定收尾）；
 * - 切换任一 tab 重置列表并回到页面顶部。
 */
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { browseFollowLatest, type BrowseWorkItem, type FeedKind, type FeedMode } from "../../api/browse";
import SectionTabs from "../../components/browse/SectionTabs.vue";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import { useInfiniteList } from "../../composables/useInfiniteList";

const { t } = useI18n();
const router = useRouter();

// ===== 过滤状态 =====

const kind = ref<FeedKind>("illust");
const mode = ref<FeedMode>("all");

const kindTabs = computed(() => [
  { value: "illust", label: t("browse.feed.kindIllust") },
  { value: "novel", label: t("browse.feed.kindNovel") },
]);
const modeTabs = computed(() => [
  { value: "all", label: t("browse.feed.scopeAll") },
  { value: "r18", label: t("browse.feed.scopeR18") },
]);

// ===== 分页列表 =====

const list = useInfiniteList<BrowseWorkItem>((page) =>
  browseFollowLatest(kind.value, mode.value, page)
);
const { items, loading, loadingMore, error, hasMore, loadMore, retry } = list;

onMounted(() => {
  void loadMore();
});

function scrollToTop(): void {
  window.scrollTo({ top: 0 });
}

function onKindChange(value: string): void {
  kind.value = value as FeedKind;
  list.reload();
  scrollToTop();
}

function onModeChange(value: string): void {
  mode.value = value as FeedMode;
  list.reload();
  scrollToTop();
}

// ===== 卡片跳转 =====

/** 作品详情路由：novel → 阅读器，其余走插画/漫画查看器（ugoira V1 显示封面帧）。 */
function goWork(item: BrowseWorkItem): void {
  if (item.kind === "novel") void router.push(`/browse/work/novel/${item.id}`);
  else if (item.kind === "manga") void router.push(`/browse/work/manga/${item.id}`);
  else void router.push(`/browse/work/illust/${item.id}`);
}
</script>

<template>
  <div class="page-view">
    <h1 class="page-title">{{ t("nav.browseFeed") }}</h1>

    <div class="feed-controls">
      <div
        class="control-row"
        role="group"
        :aria-label="t('browse.feed.kindLabel')"
      >
        <span class="control-label">{{ t("browse.feed.kindLabel") }}</span>
        <SectionTabs :tabs="kindTabs" :value="kind" @change="onKindChange" />
      </div>
      <div
        class="control-row"
        role="group"
        :aria-label="t('browse.feed.scopeLabel')"
      >
        <span class="control-label">{{ t("browse.feed.scopeLabel") }}</span>
        <SectionTabs :tabs="modeTabs" :value="mode" @change="onModeChange" />
      </div>
    </div>

    <WorkGrid
      :items="items"
      :loading="loading"
      :error="error"
      :loading-more="loadingMore"
      :has-more="hasMore"
      @load-more="loadMore"
      @retry="retry"
      @select="goWork"
    />

    <!-- 追加失败的页内重试（WorkGrid 错误态仅在无内容时出现） -->
    <div v-if="error && items.length" class="append-error">
      <span>{{ error }}</span>
      <md-outlined-button @click="retry">{{ t("common.retry") }}</md-outlined-button>
    </div>
  </div>
</template>

<style scoped>
.feed-controls {
  display: flex;
  flex-direction: column;
  gap: var(--space-sm);
  margin-bottom: var(--space-lg);
}

.control-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-md);
}

.control-label {
  min-width: 56px;
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-muted);
}

.append-error {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: var(--space-md);
  margin-top: var(--space-lg);
  color: var(--ink-muted);
  font-size: 13px;
}

@media (max-width: 640px) {
  .control-row {
    flex-direction: column;
    align-items: stretch;
  }

  .control-label {
    min-width: 0;
  }
}
</style>
