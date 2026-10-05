<script setup lang="ts">
/**
 * 作品卡片：封面 + 标题（两行省略）+ 作者行 + 左上角徽标（页数 + R-18/R-18G，各自独立并列）。
 * 整卡可点击（emit click），键盘可聚焦；无阴影，hover 用 8% primary 状态层（DESIGN.md）。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { pxSrc, thumbSrc, type BrowseWorkItem } from "../../api/browse";
import { useThumbTier } from "../../composables/useThumbTier";
import { notify } from "../../ui/notify";
import { fillDownloadForm, openInBrowser, workDownloadTarget } from "../../utils/pixivHooks";
import { pixivWorkUrl } from "../../utils/pixivUrl";

const props = defineProps<{
  item: BrowseWorkItem;
  disabled?: boolean;
  /** 首屏卡片立即请求小图，其余沿用浏览器懒加载。 */
  priority?: boolean;
  /** 封面快捷动作（打开原页 / 返填表单）：仅浏览频道页显式开启 */
  hooks?: boolean;
  /** 显示「取消收藏」快捷动作（收藏页卡片；无 bookmarkId 的条目由父级控制不传） */
  removable?: boolean;
}>();

const emit = defineEmits<{
  (e: "click", item: BrowseWorkItem): void;
  (e: "remove-bookmark"): void;
}>();

const { t } = useI18n();

/** 返填按钮文案：小说 → 小说抓取页，其余 → 插画抓取页 */
const fillLabel = computed(() =>
  props.item.kind === "novel" ? t("browse.hooks.fillNovelForm") : t("browse.hooks.fillIllustForm")
);

/** 小说封面为 1:1.4 竖版，其余按 1:1 方形展示。 */
const portrait = computed(() => props.item.kind === "novel");

/** 页数徽标：多图（>1）恒显示，插画与漫画同口径，不因 R-18 让位。 */
const pageBadge = computed(() => (props.item.page_count > 1 ? `${props.item.page_count}P` : ""));

/** R-18 / R-18G 徽标：与页数各自独立，同时命中时并排显示。 */
const restrictBadge = computed(() => {
  if (props.item.x_restrict === 1) return t("common.browseR18");
  if (props.item.x_restrict === 2) return t("common.browseR18G");
  return "";
});

/** 网格封面档位（thumb_quality_grid）。 */
const gridTier = useThumbTier("thumb_quality_grid");

/**
 * 改写后地址（含 novel-cover 等路径）一旦 404，回落到接口给的原始 URL 重试一次；
 * 小图先显示，设定档位就绪后覆盖；高清失败保留小图并尝试原始 URL。
 * 封面或档位变化时重置所有状态。
 */
const previewFailed = ref(false);
const coverFailed = ref(false);
const previewLoaded = ref(false);
const coverLoaded = ref(false);
const coverSrc = computed(() => coverFailed.value ? pxSrc(props.item.cover) : thumbSrc(props.item.cover, gridTier.value));
const previewSrc = computed(() => previewFailed.value ? coverSrc.value : thumbSrc(props.item.cover, "small"));

function onPreviewError(): void {
  if (previewFailed.value) coverFailed.value = true;
  else previewFailed.value = true;
}

watch(
  () => [props.item.cover, gridTier.value],
  () => {
    previewFailed.value = false;
    coverFailed.value = false;
    previewLoaded.value = false;
    coverLoaded.value = false;
  }
);

function handleClick(): void {
  if (props.disabled) return;
  emit("click", props.item);
}

function handleOpenInBrowser(): void {
  void openInBrowser(pixivWorkUrl(props.item.kind, props.item.id)).catch(() =>
    notify(t("browse.hooks.openFailed"))
  );
}

function handleFillForm(): void {
  fillDownloadForm(workDownloadTarget(props.item));
}
</script>

<template>
  <div class="work-card-wrap">
    <div
      class="work-card"
      role="button"
      :tabindex="disabled ? -1 : 0"
      :aria-disabled="disabled || undefined"
      :class="{ disabled }"
      @click="handleClick"
      @keydown.enter.prevent="handleClick"
      @keydown.space.prevent="handleClick"
    >
      <div class="cover" :class="{ portrait }">
        <img
          v-if="item.cover && !coverLoaded"
          :key="previewSrc"
          :src="previewSrc"
          alt=""
          :loading="priority ? 'eager' : 'lazy'"
          :fetchpriority="priority ? 'high' : 'auto'"
          decoding="async"
          @load="previewLoaded = true"
          @error="onPreviewError"
        />
        <img
          v-if="previewLoaded && coverSrc !== previewSrc"
          v-show="coverLoaded"
          :key="coverSrc"
          class="cover-upgrade"
          :src="coverSrc"
          alt=""
          fetchpriority="low"
          decoding="async"
          @load="coverLoaded = true"
          @error="coverFailed = true"
        />
        <span v-if="pageBadge || restrictBadge" class="badges">
          <span v-if="pageBadge" class="badge">{{ pageBadge }}</span>
          <span v-if="restrictBadge" class="badge restricted">{{ restrictBadge }}</span>
        </span>
        <span v-if="item.kind === 'novel'" class="kind-mark" :title="t('nav.browseNovel')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <!-- lucide book -->
            <path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H19a1 1 0 0 1 1 1v18a1 1 0 0 1-1 1H6.5a1 1 0 0 1 0-5H20" />
          </svg>
        </span>
      </div>
      <p class="title" :title="item.title">{{ item.title }}</p>
      <p class="author" :title="item.author_name">{{ item.author_name }}</p>
    </div>
    <!-- 快捷动作与 role="button" 卡片为兄弟节点：嵌套会被 ARIA children-presentational 从无障碍树抹掉 -->
    <div v-if="(hooks && !disabled) || removable" class="cover-actions">
      <template v-if="hooks && !disabled">
        <button
          class="cover-action"
          type="button"
          :aria-label="t('browse.hooks.openInBrowser')"
          :title="t('browse.hooks.openInBrowser')"
          @click="handleOpenInBrowser"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <!-- lucide square-arrow-out-up-right -->
            <path d="M21 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h6" />
            <path d="m21 3-9 9" />
            <path d="M15 3h6v6" />
          </svg>
        </button>
        <button
          class="cover-action"
          type="button"
          :aria-label="fillLabel"
          :title="fillLabel"
          @click="handleFillForm"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <!-- lucide download -->
            <path d="M12 15V3" />
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <path d="m7 10 5 5 5-5" />
          </svg>
        </button>
      </template>
      <!-- 取消收藏（收藏页）：同规格 28px 胶囊动作，心形图标 -->
      <button
        v-if="removable"
        class="cover-action"
        type="button"
        :aria-label="t('browse.bookmark.removeBookmark')"
        :title="t('browse.bookmark.removeBookmark')"
        @click="emit('remove-bookmark')"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <!-- lucide heart -->
          <path d="M2 9.5a5.5 5.5 0 0 1 9.591-3.676.56.56 0 0 0 .818 0A5.49 5.49 0 0 1 22 9.5c0 2.29-1.5 4-3 5.5l-5.492 5.313a2 2 0 0 1-3 .019L5 15c-1.5-1.5-3-3.2-3-5.5" />
        </svg>
      </button>
    </div>
  </div>
</template>

<style scoped>
.work-card-wrap {
  position: relative;
}

/* 快捷动作：默认隐形，hover / 键盘 focus-within 显现（封面右上角，位置 = 卡片 padding + 封面内缩） */
.cover-actions {
  position: absolute;
  top: calc(var(--space-xs) * 2);
  right: calc(var(--space-xs) * 2);
  display: flex;
  gap: var(--space-xs);
  opacity: 0;
  transition: opacity 0.15s ease;
}

.work-card-wrap:hover .cover-actions,
.work-card-wrap:focus-within .cover-actions {
  opacity: 1;
}

.cover-action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: none;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  cursor: pointer;
}

.cover-action:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.cover-action:active {
  background: color-mix(in srgb, var(--md-sys-color-primary) 12%, var(--md-sys-color-surface-container));
}

.cover-action:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.cover-action svg {
  width: 16px;
  height: 16px;
  stroke-width: 2;
}

@media (prefers-reduced-motion: reduce) {
  .cover-actions {
    transition: none;
  }
}

.work-card {
  display: block;
  padding: var(--space-xs);
  text-align: left;
  border-radius: var(--radius-control);
  cursor: pointer;
  outline: none;
  transition: background-color 0.15s ease;
}

.work-card:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.work-card:active {
  background: color-mix(in srgb, var(--md-sys-color-primary) 12%, transparent);
}

.work-card:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.work-card.disabled {
  cursor: default;
  opacity: 0.5;
  pointer-events: none;
}

.cover {
  position: relative;
  aspect-ratio: 1 / 1;
  border-radius: var(--radius-control);
  overflow: hidden;
  /* 封面重渲染不影响卡片外布局（长列表滚动性能） */
  contain: content;
  /* 主色底占位：封面未加载时可见（DESIGN.md token 派生） */
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.cover.portrait {
  aspect-ratio: 1 / 1.4;
}

.cover img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.cover .cover-upgrade {
  position: absolute;
  inset: 0;
}

/* 徽标组：左上角，页数与 R-18 各自独立胶囊（窄卡时允许换行） */
.badges {
  position: absolute;
  top: var(--space-xs);
  left: var(--space-xs);
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-xxs);
  max-width: calc(100% - 2 * var(--space-xs));
}

.badge {
  padding: 1px var(--space-sm);
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.badge.restricted {
  background: var(--ink);
  color: var(--md-sys-color-surface);
}

.kind-mark {
  position: absolute;
  right: var(--space-xs);
  bottom: var(--space-xs);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border-radius: 999px;
  background: var(--md-sys-color-surface-container);
  color: var(--ink-muted);
}

.kind-mark svg {
  width: 13px;
  height: 13px;
  stroke-width: 2;
}

.title,
.author {
  margin: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.title {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  margin-top: var(--space-sm);
  font-size: 14px;
  font-weight: 600;
  line-height: 1.4;
  color: var(--ink);
}

.author {
  margin-top: var(--space-xxs);
  font-size: 12px;
  line-height: 1.4;
  color: var(--ink-muted);
  white-space: nowrap;
}
</style>
