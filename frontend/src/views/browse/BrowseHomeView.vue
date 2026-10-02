<script setup lang="ts">
import PageBackButton from "../../components/navigation/PageBackButton.vue";
import ListRefreshButton from "../../components/browse/ListRefreshButton.vue";
/**
 * 浏览·首页：browse_home_feed 一次性混合推荐流（illust/manga/novel/ugoira）。
 * 「换一批」重新调用并按 kind:id 去重追加；新条目 < 5 视为换不出更多，
 * 停止追加、仅由 WorkGrid 的「没有更多了」收尾提示。
 */
import { onBeforeUnmount, onMounted, ref, shallowRef } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { browseHomeFeed, errorMessage, type BrowseWorkItem } from "../../api/browse";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import { notify } from "../../ui/notify";
import { useAuthStore } from "../../stores/auth";
import { readHomeCache, saveHomeCache } from "../../utils/homeCache";

const { t } = useI18n();
const router = useRouter();

const auth = useAuthStore();
const cacheUserId = auth.isLoggedIn ? auth.userId : "";
const items = shallowRef<BrowseWorkItem[]>(readHomeCache(cacheUserId));
let disposed = false;
onBeforeUnmount(() => { disposed = true; });
const loading = ref(false);
const refreshing = ref(false);
const error = ref("");
/** 换一批再也换不出足够的新条目（< 5）→ 到底 */
const exhausted = ref(false);

/** 首屏加载（骨架屏 / 空态 / 错误重试由 WorkGrid 承担）。 */
async function initialLoad(): Promise<void> {
  if (loading.value) return;
  loading.value = true;
  error.value = "";
  try {
    const data = await browseHomeFeed();
    if (disposed) return;
    items.value = data.items;
    saveHomeCache(cacheUserId, items.value);
  } catch (err) {
    if (!disposed) error.value = errorMessage(err);
  } finally {
    loading.value = false;
  }
}

/** 换一批：重复调用 + 按 id 去重追加；本轮新条目不足 5 条则判定到底（不再追加）。 */
async function shuffle(): Promise<void> {
  if (loading.value || refreshing.value || exhausted.value) return;
  refreshing.value = true;
  try {
    const data = await browseHomeFeed();
    if (disposed) return;
    const seen = new Set(items.value.map((it) => `${it.kind}:${it.id}`));
    const fresh = data.items.filter((it) => !seen.has(`${it.kind}:${it.id}`));
    if (fresh.length >= 5) {
      items.value = items.value.concat(fresh);
      saveHomeCache(cacheUserId, items.value);
    } else exhausted.value = true;
  } catch {
    if (!disposed) notify(t("common.browseLoadFailed"));
  } finally {
    refreshing.value = false;
  }
}

function refresh(): void {
  exhausted.value = false;
  void initialLoad();
}

/** 卡片跳转：novel → 小说阅读器；illust/manga → 作品查看器（ugoira 后端按 illust 详情 + illustType=2 处理）。 */
function openWork(item: BrowseWorkItem): void {
  const kindPath = item.kind === "novel" ? "novel" : item.kind === "manga" ? "manga" : "illust";
  void router.push(`/browse/work/${kindPath}/${item.id}`);
}

onMounted(initialLoad);
</script>

<template>
  <div class="page-view browse-home">
    <div class="home-header">
      <div class="page-heading"><PageBackButton /><h1 class="page-title">{{ t("nav.browseHome") }}</h1></div>
      <ListRefreshButton :busy="loading || refreshing" @refresh="refresh" />
      <!-- 换一批降为文字按钮 + 线性刷新图标，弱化头部主次层级（图标风格对齐 HistoryView .row-actions） -->
      <md-text-button :disabled="loading || refreshing || exhausted" @click="shuffle">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <!-- lucide refresh-cw -->
          <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
          <path d="M21 3v5h-5" />
          <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
          <path d="M8 16H3v5" />
        </svg>
        {{ refreshing ? t("browse.home.refreshing") : t("browse.home.refresh") }}
      </md-text-button>
    </div>
    <p v-if="error && items.length" role="alert">{{ error }}</p>

    <WorkGrid
      :items="items"
      :loading="loading"
      :error="error"
      :has-more="!exhausted"
      @retry="initialLoad"
      @select="openWork"
    />
  </div>
</template>

<style scoped>
.home-header {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-sm) var(--space-md);
  margin-bottom: var(--space-lg);
}

.home-header .page-title {
  margin: 0;
}

/* 刷新图标：线性描边（fill:none / stroke:currentColor / stroke-width:2），尺寸对齐 HistoryView .row-actions svg */
.home-header md-text-button svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-linecap: round;
  stroke-linejoin: round;
  stroke-width: 2;
  vertical-align: middle;
  margin-right: var(--space-xxs);
}
</style>
