<script setup lang="ts">
/**
 * 用户搜索卡片：头像 + 昵称 + @handle · ID + 自我介绍 + 最近作品缩略行。
 * 整卡可点击（emit click → 作者页）；works 缩略仅展示不可点（嵌套交互 V1 不做）。
 * 无阴影，hover 用 8% primary 状态层（DESIGN.md 卡片惯例，与 WorkCard 同源）。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { thumbSrc, type BrowseUserItem } from "../../api/browse";
import { useThumbTier } from "../../composables/useThumbTier";

const props = defineProps<{
  item: BrowseUserItem;
  disabled?: boolean;
  /** 首屏卡片立即请求缩略，其余沿用浏览器懒加载。 */
  priority?: boolean;
}>();

const emit = defineEmits<{
  (e: "click", item: BrowseUserItem): void;
}>();

const { t } = useI18n();

/** 网格封面档位（thumb_quality_grid，与 WorkCard 同一设置键）。 */
const gridTier = useThumbTier("thumb_quality_grid");

/** 头像加载失败回落占位图标；条目复用切 id 时重置。 */
const avatarBroken = ref(false);
watch(
  () => props.item.avatar,
  () => {
    avatarBroken.value = false;
  }
);

/** 辅助行：@handle 与用户 ID 拼接（handle 缺失时只留 ID）。 */
const handleLine = computed(() => {
  const parts: string[] = [];
  if (props.item.pixiv_id) parts.push(`@${props.item.pixiv_id}`);
  parts.push(`ID ${props.item.id}`);
  return parts.join(" · ");
});

function handleClick(): void {
  if (props.disabled) return;
  emit("click", props.item);
}
</script>

<template>
  <div
    class="user-card"
    role="button"
    :tabindex="disabled ? -1 : 0"
    :aria-disabled="disabled || undefined"
    :aria-label="item.name"
    :class="{ disabled }"
    @click="handleClick"
    @keydown.enter.prevent="handleClick"
    @keydown.space.prevent="handleClick"
  >
    <div class="avatar">
      <img
        v-if="item.avatar && !avatarBroken"
        :src="thumbSrc(item.avatar, gridTier)"
        alt=""
        :loading="priority ? 'eager' : 'lazy'"
        decoding="async"
        @error="avatarBroken = true"
      />
      <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <!-- lucide circle-user-round -->
        <path d="M18 20a6 6 0 0 0-12 0" />
        <circle cx="12" cy="10" r="4" />
        <circle cx="12" cy="12" r="10" />
      </svg>
    </div>
    <div class="main">
      <p class="name-row">
        <span class="name" :title="item.name">{{ item.name }}</span>
        <span v-if="item.is_followed" class="followed">{{ t("browse.userSearch.following") }}</span>
      </p>
      <p class="handle">{{ handleLine }}</p>
      <p v-if="item.comment" class="comment" :title="item.comment">{{ item.comment }}</p>
    </div>
    <div v-if="item.works.length" class="works" aria-hidden="true">
      <img
        v-for="work in item.works.slice(0, 4)"
        :key="`${work.kind}:${work.id}`"
        class="work-thumb"
        :src="thumbSrc(work.cover, gridTier)"
        alt=""
        :loading="priority ? 'eager' : 'lazy'"
        decoding="async"
      />
    </div>
  </div>
</template>

<style scoped>
/* 行卡：surface-container 底、control 圆角；hover 8% primary 状态层 */
.user-card {
  display: flex;
  align-items: center;
  gap: var(--space-md);
  padding: var(--space-sm) var(--space-md);
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
  cursor: pointer;
  transition: background-color 0.15s ease;
}

.user-card:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.user-card:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.user-card.disabled {
  cursor: default;
}

/* 头像：56px 圆角方（与作者页头像同配方），加载失败回落占位图标 */
.avatar {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 56px;
  height: 56px;
  overflow: hidden;
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container-high);
  color: var(--ink-subtle);
}

.avatar img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.avatar svg {
  width: 28px;
  height: 28px;
}

/* 主信息：昵称（加粗，含已关注胶囊）+ 辅助行 + 简介两行省略 */
.main {
  display: flex;
  flex: 1;
  min-width: 0;
  flex-direction: column;
  gap: 2px;
}

.name-row {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: var(--space-sm);
  margin: 0;
}

.name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
  color: var(--ink);
}

/* 已关注徽标：中性描边胶囊（与搜索历史类型徽标同配方） */
.followed {
  display: inline-flex;
  flex: none;
  align-items: center;
  min-height: 20px;
  padding: 0 var(--space-sm);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 60%, transparent);
  border-radius: 999px;
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
}

.handle {
  margin: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--ink-muted);
  font-variant-numeric: tabular-nums;
}

.comment {
  display: -webkit-box;
  margin: 0;
  overflow: hidden;
  font-size: 12px;
  color: var(--ink-subtle);
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}

/* 最近作品缩略行：方形 56px 一排（≤4 张），窄窗收缩到可换行 */
.works {
  display: flex;
  flex: none;
  gap: var(--space-xs);
}

.work-thumb {
  width: 56px;
  height: 56px;
  object-fit: cover;
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container-high);
}

/* 窄窗：缩略行换行到主信息下方 */
@media (max-width: 560px) {
  .user-card {
    flex-wrap: wrap;
  }

  .works {
    flex-basis: 100%;
    padding-left: calc(56px + var(--space-md));
  }
}
</style>
