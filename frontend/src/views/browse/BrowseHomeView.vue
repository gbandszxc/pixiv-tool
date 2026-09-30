<script setup lang="ts">
/**
 * 浏览·首页：browse_home_feed 一次性混合推荐流（illust/manga/novel/ugoira）。
 * 「换一批」重新调用并按 kind:id 去重追加；新条目 < 5 视为换不出更多，
 * 停止追加、仅由 WorkGrid 的「没有更多了」收尾提示。
 */
import { onMounted, ref, shallowRef } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { browseHomeFeed, errorMessage, type BrowseWorkItem } from "../../api/browse";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import { notify } from "../../ui/notify";

const { t } = useI18n();
const router = useRouter();

const items = shallowRef<BrowseWorkItem[]>([]);
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
    items.value = (await browseHomeFeed()).items;
  } catch (err) {
    error.value = errorMessage(err);
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
    const seen = new Set(items.value.map((it) => `${it.kind}:${it.id}`));
    const fresh = data.items.filter((it) => !seen.has(`${it.kind}:${it.id}`));
    if (fresh.length >= 5) items.value = items.value.concat(fresh);
    else exhausted.value = true;
  } catch {
    notify(t("common.browseLoadFailed"));
  } finally {
    refreshing.value = false;
  }
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
      <h1 class="page-title">{{ t("nav.browseHome") }}</h1>
      <md-outlined-button :disabled="loading || refreshing || exhausted" @click="shuffle">
        {{ refreshing ? t("browse.home.refreshing") : t("browse.home.refresh") }}
      </md-outlined-button>
    </div>

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
</style>
