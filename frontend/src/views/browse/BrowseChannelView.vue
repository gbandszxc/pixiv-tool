<script setup lang="ts">
/**
 * 浏览·频道页（插画/漫画/小说三路由共用，props.kind 区分）：
 * browse_channel 一次性快照 → 纵向分区「已关注的新作 / 为你推荐 / 每日排行 / 最新投稿」
 * + 底部热门标签。各板块 WorkGrid 只展示前 12 条；排行榜板块附 ranking_date，
 * 「查看完整榜单」跳 /browse/ranking 并预选对应类型；标签点击 → 搜索页预填关键词。
 */
import { computed, onMounted, ref, shallowRef, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import {
  browseChannel,
  errorMessage,
  type BrowseChannel as ChannelSnapshot,
  type BrowseWorkItem,
} from "../../api/browse";
import R18FilterBar from "../../components/browse/R18FilterBar.vue";
import WorkGrid from "../../components/browse/WorkGrid.vue";
import { filterByR18, useChannelR18Filter } from "../../components/browse/r18Filter";

const props = defineProps<{ kind: "illustration" | "manga" | "novel" }>();

const { t } = useI18n();
const router = useRouter();

/** 频道档 R-18 过滤：默认跟随全局，手动切换只在当前频道页生效（按 kind 分档，见 r18Filter.ts）。 */
const { filter, setFilter } = useChannelR18Filter(() => props.kind);

const data = shallowRef<ChannelSnapshot | null>(null);
const loading = ref(false);
const error = ref("");

async function load(): Promise<void> {
  loading.value = true;
  error.value = "";
  try {
    data.value = await browseChannel(props.kind);
  } catch (err) {
    error.value = errorMessage(err);
  } finally {
    loading.value = false;
  }
}

/** 三条路由复用同一组件实例（仅 props.kind 变化），需重载。 */
watch(
  () => props.kind,
  () => {
    data.value = null;
    void load();
  }
);

onMounted(load);

const title = computed(() => {
  if (props.kind === "illustration") return t("nav.browseIllustration");
  if (props.kind === "manga") return t("nav.browseManga");
  return t("nav.browseNovel");
});

/** 每板块最多展示 12 条（完整流量交给各独立页面）；先按档位过滤再截断。 */
const SECTION_LIMIT = 12;

const followItems = computed<BrowseWorkItem[]>(() =>
  filterByR18(data.value?.follow.items ?? [], filter.value).slice(0, SECTION_LIMIT)
);
const recommendItems = computed<BrowseWorkItem[]>(() =>
  filterByR18(data.value?.recommend.items ?? [], filter.value).slice(0, SECTION_LIMIT)
);
const rankingItems = computed<BrowseWorkItem[]>(() =>
  filterByR18(data.value?.ranking.items ?? [], filter.value).slice(0, SECTION_LIMIT)
);
const newPostItems = computed<BrowseWorkItem[]>(() =>
  filterByR18(data.value?.new_post.items ?? [], filter.value).slice(0, SECTION_LIMIT)
);
const trendingTags = computed(() => data.value?.trending_tags ?? []);

/** 当前档位下四板块合计隐藏的 R-18 条数（按快照全量计，不受 12 条展示上限影响）。 */
const hiddenCount = computed(() => {
  const snapshot = data.value;
  if (!snapshot) return 0;
  return [snapshot.follow, snapshot.recommend, snapshot.ranking, snapshot.new_post].reduce(
    (sum, list) => sum + list.items.length - filterByR18(list.items, filter.value).length,
    0
  );
});
/** 榜单板块日期（YYYYMMDD → YYYY-MM-DD，缺失为空）。 */
const rankingDate = computed(() => formatYmd(data.value?.ranking_date));
/** 已关注板块仅在「已加载且为空」时整体隐藏；加载中仍显示骨架。 */
const showFollow = computed(() => loading.value || followItems.value.length > 0);

/** YYYYMMDD → YYYY-MM-DD（非法/缺失返回空串）。 */
function formatYmd(ymd?: string | null): string {
  if (!ymd || !/^\d{8}$/.test(ymd)) return "";
  return `${ymd.slice(0, 4)}-${ymd.slice(4, 6)}-${ymd.slice(6, 8)}`;
}

/** 卡片跳转：novel → 小说阅读器；其余 → 作品查看器（ugoira 按 illust 详情拉取）。 */
function openWork(item: BrowseWorkItem): void {
  const kindPath = item.kind === "novel" ? "novel" : item.kind === "manga" ? "manga" : "illust";
  void router.push(`/browse/work/${kindPath}/${item.id}`);
}

/** 完整榜单：预选对应类型（频道 illustration 对应 ranking 的 illust）。 */
function openRanking(): void {
  const kindParam = props.kind === "illustration" ? "illust" : props.kind;
  void router.push({ path: "/browse/ranking", query: { kind: kindParam } });
}

/** 热门标签 → 搜索页并预填关键词（query.word）。 */
function openTag(name: string): void {
  void router.push({ path: "/browse/search", query: { word: name } });
}
</script>

<template>
  <div class="page-view browse-channel">
    <h1 class="page-title">{{ title }}</h1>

    <!-- 档位切换为纯 computed：不重新请求快照；计数提示统一在筛选条右侧 -->
    <R18FilterBar :model-value="filter" :hidden-count="hiddenCount" @update:model-value="setFilter" />

    <!-- 整页错误（快照尚未到手）→ 文案 + 重试 -->
    <div v-if="error && !data" class="channel-state" role="alert">
      <p class="state-text">{{ error }}</p>
      <md-outlined-button @click="load">{{ t("common.retry") }}</md-outlined-button>
    </div>

    <template v-else>
      <section v-if="showFollow" class="channel-section">
        <div class="section-head">
          <h2 class="section-title">{{ t("browse.channel.followNew") }}</h2>
        </div>
        <WorkGrid :items="followItems" :loading="loading" hooks hide-r18-hint @select="openWork" />
      </section>

      <section class="channel-section">
        <div class="section-head">
          <h2 class="section-title">{{ t("browse.channel.recommend") }}</h2>
        </div>
        <WorkGrid :items="recommendItems" :loading="loading" hooks hide-r18-hint @select="openWork" />
      </section>

      <section class="channel-section">
        <div class="section-head">
          <h2 class="section-title">{{ t("browse.channel.dailyRanking") }}</h2>
          <div class="section-head-side">
            <span v-if="rankingDate" class="ranking-date">{{ rankingDate }}</span>
            <md-text-button @click="openRanking">{{ t("browse.channel.viewFullRanking") }}</md-text-button>
          </div>
        </div>
        <WorkGrid :items="rankingItems" :loading="loading" hooks hide-r18-hint @select="openWork" />
      </section>

      <section class="channel-section">
        <div class="section-head">
          <h2 class="section-title">{{ t("browse.channel.newPost") }}</h2>
        </div>
        <WorkGrid :items="newPostItems" :loading="loading" hooks hide-r18-hint @select="openWork" />
      </section>

      <section v-if="trendingTags.length" class="channel-section">
        <div class="section-head">
          <h2 class="section-title">{{ t("browse.channel.trendingTags") }}</h2>
        </div>
        <div class="tag-row">
          <button
            v-for="tag in trendingTags"
            :key="tag.name"
            type="button"
            class="tag-chip"
            :title="tag.translated_name"
            @click="openTag(tag.name)"
          >
            {{ tag.name }}
          </button>
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.channel-section + .channel-section {
  margin-top: var(--space-xl);
}

.section-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-xxs) var(--space-md);
  margin-bottom: var(--space-md);
}

/* 区标题：16px/600，on-surface-variant（DESIGN.md 正文/辅助层级） */
.section-title {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  line-height: 1.4;
  color: var(--ink-muted);
}

.section-head-side {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-sm);
}

.ranking-date {
  color: var(--ink-subtle);
  font-size: 12px;
  font-weight: 600;
}

.tag-row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-sm);
}

/* 标签胶囊：primary-container 底 + on-primary-container 文字（DESIGN.md 分类 label 惯例） */
.tag-chip {
  padding: var(--space-xxs) var(--space-md);
  border: 0;
  border-radius: 999px;
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.6;
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.tag-chip:hover {
  background: color-mix(in srgb, var(--md-sys-color-on-primary-container) 10%, var(--md-sys-color-primary-container));
}

.tag-chip:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.channel-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-sm);
  padding: var(--space-xl) var(--space-lg);
  text-align: center;
}

.state-text {
  margin: 0;
  max-width: 480px;
  color: var(--ink-muted);
  font-size: 14px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}
</style>
