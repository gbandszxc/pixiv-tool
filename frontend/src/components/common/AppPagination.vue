<script setup lang="ts">
/**
 * 公共分页组件（Material 3，跨域 common/）：对齐 Ant Design Pagination 的
 * 常用能力——总数回显 / 每页容量下拉 / prev-next / 页码按钮 + 省略号 /
 * 快捷跳页输入组（showQuickJumper）/ 响应式收缩。
 *
 * 数据模式：
 * - `total` 或 `totalPages` 二选一传入 → 内部换算 pageCount（已知总页数）；
 *   两者同时给时 totalPages 优先。
 * - 两者皆缺省 → 「未知总页数」简单模式：无页码窗口、恒显「第 N 页」，
 *   next 禁用跟随 `hasNext`（默认 true），对齐排行榜 next_page 链场景。
 *
 * 受控组件：不持有页码状态，页码 / 容量变化经 update:currentPage /
 * update:pageSize / change 上抛（父级可直接 v-model:currentPage /
 * v-model:pageSize）；容量切换后回第 1 页由父级处理，本组件只 emit。
 *
 * variant="reader"：吸底居中紧凑形态，复用小说阅读器翻页器 recipe
 * （sticky 底部、surface 底 + 上缘 divider、md-icon-button + 跳页 select），
 * 不显示页码窗口与总数区，供系列页 / 阅读器迁移。
 */
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";

const props = withDefaults(
  defineProps<{
    /** 当前页码，1 起 */
    currentPage: number;
    /** 总条数（与 totalPages 二选一；配合 pageSize 换算 pageCount） */
    total?: number;
    /** 总页数（与 total 二选一；同时给时优先生效） */
    totalPages?: number;
    /** 未知总页数模式下 next 是否可用（默认 true） */
    hasNext?: boolean;
    /** 每页容量（与 pageSizeOptions 搭配；换算 pageCount 的除数） */
    pageSize?: number;
    /** 传入才渲染每页容量下拉；切换后回第 1 页由父级处理 */
    pageSizeOptions?: number[];
    /** 整体禁用（含 prev/next、页码、容量下拉） */
    disabled?: boolean;
    /** default：流内分页行；reader：吸底居中紧凑形态 */
    variant?: "default" | "reader";
  }>(),
  {
    total: undefined,
    totalPages: undefined,
    hasNext: true,
    pageSize: undefined,
    pageSizeOptions: undefined,
    disabled: false,
    variant: "default",
  },
);

const emit = defineEmits<{
  (e: "update:currentPage", page: number): void;
  (e: "update:pageSize", size: number): void;
  (e: "change", payload: { page: number; pageSize: number | undefined }): void;
}>();

defineSlots<{
  /** 覆盖左侧总数区（如排行榜「第 from-to 名」）；无插槽且有 total 时默认渲染「共 {total} 项」 */
  start?: () => unknown;
  /** reader 变体左侧插槽（如小说阅读器字号缩放控件）；无插槽时翻页组仍居中 */
  leading?: () => unknown;
}>();

const { t } = useI18n();

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** pageCount：totalPages 优先，其次 total ÷ pageSize；皆缺省 = 未知（null） */
const pageCount = computed<number | null>(() => {
  if (props.totalPages !== undefined) return Math.max(1, Math.floor(props.totalPages));
  if (props.total !== undefined) {
    const size = props.pageSize && props.pageSize > 0 ? props.pageSize : Math.max(1, props.total);
    return Math.max(1, Math.ceil(props.total / size));
  }
  return null;
});

/** 展示用当前页：已知总页数时钳制在 [1, pageCount]，防父级异步回写期越界渲染 */
const currentView = computed(() => {
  const count = pageCount.value;
  const page = Math.max(1, Math.floor(props.currentPage) || 1);
  return count === null ? page : clamp(page, 1, count);
});

type PageItem = { kind: "page"; value: number } | { kind: "ellipsis" };

/** 页码窗口：pageCount ≤ 7 全显；否则首尾恒显 + current±2 窗口 + 省略号（纯文本，不做跳页按钮） */
const pageWindow = computed<PageItem[]>(() => {
  const count = pageCount.value ?? 0;
  if (count <= 7) {
    return Array.from({ length: count }, (_, index) => ({ kind: "page" as const, value: index + 1 }));
  }
  const current = currentView.value;
  let start = Math.max(1, Math.min(current - 2, count - 4));
  const end = Math.min(count, start + 4);
  start = Math.max(1, end - 4);
  const items: PageItem[] = [{ kind: "page", value: 1 }];
  if (start > 2) items.push({ kind: "ellipsis" });
  for (let page = Math.max(2, start); page <= Math.min(count - 1, end); page += 1) {
    items.push({ kind: "page", value: page });
  }
  if (end < count - 1) items.push({ kind: "ellipsis" });
  items.push({ kind: "page", value: count });
  return items;
});

const prevDisabled = computed(() => currentView.value <= 1);
const nextDisabled = computed(() => {
  const count = pageCount.value;
  // 未知总页数：next 禁用跟随 hasNext；已知总页数：末页禁用
  return count === null ? !props.hasNext : currentView.value >= count;
});

function goTo(page: number): void {
  if (props.disabled) return;
  const count = pageCount.value;
  const target = count === null ? Math.max(1, page) : clamp(page, 1, count);
  if (target === props.currentPage) return;
  emit("update:currentPage", target);
  emit("change", { page: target, pageSize: props.pageSize });
}

function onSizeChange(event: Event): void {
  if (props.disabled) return;
  const size = Number((event.target as HTMLSelectElement).value);
  if (!Number.isFinite(size) || size === props.pageSize) return;
  // 容量切换后回第 1 页由父级处理：这里只上报新容量与当时页码
  emit("update:pageSize", size);
  emit("change", { page: props.currentPage, pageSize: size });
}

function onReaderSelect(event: Event): void {
  goTo(Number((event.target as HTMLSelectElement).value));
}

/** 跳页输入框草稿（非受控：仅 Enter / blur 时解析，解析后无论是否导航都清空） */
const jumpInput = ref("");

/** 快捷跳页：parseInt 落在 1..pageCount 才 goTo；越界 / 非数字只清空不导航 */
function onJump(): void {
  const raw = jumpInput.value.trim();
  jumpInput.value = "";
  if (props.disabled || raw === "") return;
  const count = pageCount.value;
  if (count === null) return;
  const page = Number.parseInt(raw, 10);
  if (!Number.isFinite(page) || page < 1 || page > count) return;
  goTo(page);
}
</script>

<template>
  <!-- reader 变体：吸底居中紧凑形态（小说阅读器翻页器 recipe）；
       三列 grid：左 leading（可选，如字号缩放）/ 中翻页组恒居中 / 右留空平衡 -->
  <nav v-if="variant === 'reader'" class="app-pagination is-reader" :aria-label="t('common.pagination.navLabel')">
    <div v-if="$slots.leading" class="reader-leading"><slot name="leading" /></div>
    <div class="reader-inner">
      <md-icon-button
        :aria-label="t('common.pagination.prevPage')"
        :title="t('common.pagination.prevPage')"
        :disabled="disabled || prevDisabled"
        @click="goTo(currentView - 1)"
      >
        <svg class="pager-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="15 18 9 12 15 6" /></svg>
      </md-icon-button>
      <select
        v-if="pageCount !== null"
        class="page-select"
        :value="currentView"
        :disabled="disabled"
        :aria-label="t('browse.novel.pageSelect')"
        @change="onReaderSelect"
      >
        <option v-for="n in pageCount" :key="n" :value="n">
          {{ t("browse.novel.pageInfo", { current: n, total: pageCount }) }}
        </option>
      </select>
      <span v-else class="reader-label">{{ t("common.pagination.currentPage", { n: currentView }) }}</span>
      <md-icon-button
        :aria-label="t('common.pagination.nextPage')"
        :title="t('common.pagination.nextPage')"
        :disabled="disabled || nextDisabled"
        @click="goTo(currentView + 1)"
      >
        <svg class="pager-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="9 18 15 12 9 6" /></svg>
      </md-icon-button>
    </div>
  </nav>

  <!-- default 变体：流内分页行四段式——左总数区（或 #start）→ prev+页码窗口+next → 跳页输入组 → 最右容量下拉 -->
  <nav v-else class="app-pagination" :aria-label="t('common.pagination.navLabel')">
    <span v-if="$slots.start" class="pg-start"><slot name="start" /></span>
    <span v-else-if="total !== undefined" class="pg-total">{{ t("common.pagination.total", { count: total }) }}</span>
    <button
      type="button"
      class="pg-nav"
      :aria-label="t('common.pagination.prevPage')"
      :title="t('common.pagination.prevPage')"
      :disabled="disabled || prevDisabled"
      @click="goTo(currentView - 1)"
    >
      <svg class="pager-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="15 18 9 12 15 6" /></svg>
    </button>

    <template v-if="pageCount !== null">
      <span class="pg-window">
        <template v-for="(item, index) in pageWindow" :key="index">
          <span v-if="item.kind === 'ellipsis'" class="pg-ellipsis" aria-hidden="true">…</span>
          <button
            v-else
            type="button"
            class="pg-page"
            :class="{ 'is-active': item.value === currentView }"
            :aria-label="t('common.pagination.currentPage', { n: item.value })"
            :aria-current="item.value === currentView ? 'page' : undefined"
            :disabled="disabled"
            @click="goTo(item.value)"
          >{{ item.value }}</button>
        </template>
      </span>
      <!-- compact 断点以下替代页码窗口 -->
      <span class="pg-compact-label">{{ t("common.pagination.pageOf", { current: currentView, total: pageCount }) }}</span>
    </template>
    <!-- 未知总页数模式：无页码窗口，恒显「第 N 页」 -->
    <span v-else class="pg-unknown-label">{{ t("common.pagination.currentPage", { n: currentView }) }}</span>

    <button
      type="button"
      class="pg-nav"
      :aria-label="t('common.pagination.nextPage')"
      :title="t('common.pagination.nextPage')"
      :disabled="disabled || nextDisabled"
      @click="goTo(currentView + 1)"
    >
      <svg class="pager-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="9 18 15 12 9 6" /></svg>
    </button>
    <!-- 快捷跳页输入组（对齐 antd showQuickJumper）：已知总页数且 >1 才渲染，恒在容量下拉左侧 -->
    <label
      v-if="pageCount !== null && pageCount > 1"
      class="pg-jump"
      :class="{ 'is-disabled': disabled }"
    >
      <span class="pg-jump-text">{{ t("common.pagination.jumpTo") }}</span>
      <input
        v-model="jumpInput"
        type="text"
        inputmode="numeric"
        class="pg-jump-input"
        :disabled="disabled"
        :aria-label="t('common.pagination.jumpToLabel')"
        @keydown.enter.prevent="onJump"
        @blur="onJump"
      />
      <span class="pg-jump-text">{{ t("common.pagination.pageUnit") }}</span>
    </label>
    <!-- 容量下拉恒居翻页组右侧（margin-left auto 推至行尾）；自绘原生小号 select 保证 32px 控制高度 -->
    <select
      v-if="pageSizeOptions"
      class="pg-size"
      :value="String(pageSize ?? pageSizeOptions[0])"
      :disabled="disabled"
      :aria-label="t('common.pagination.pageSizeLabel')"
      @change="onSizeChange"
    >
      <option v-for="size in pageSizeOptions" :key="size" :value="String(size)">
        {{ t("common.pagination.pageSizeOption", { n: size }) }}
      </option>
    </select>
  </nav>
</template>

<style scoped>
/* 全 token 视觉：M3 颜色角色 + DESIGN.md 间距/圆角/动效刻度，零字面值 */
.app-pagination {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-sm);
  color: var(--ink);
}

/* 行内文本区与控件统一 32px 控制高度（--space-lg × 2，DESIGN.md AppPagination 条目），flex 垂直居中 */
.pg-start,
.pg-total {
  display: inline-flex;
  align-items: center;
  min-height: calc(var(--space-lg) * 2);
  color: var(--ink-muted);
  font-size: 14px;
  font-weight: 500;
  white-space: nowrap;
}

.pg-window {
  display: flex;
  align-items: center;
  gap: var(--space-xxs);
}

.pg-compact-label {
  display: none;
}

/* 自绘 prev/next：32px 胶囊 icon button，与页码钮同高；hover 8% primary 状态层 */
.pg-nav {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: calc(var(--space-lg) * 2);
  height: calc(var(--space-lg) * 2);
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: var(--ink);
  cursor: pointer;
  transition: background-color 0.15s ease, color 0.15s ease;
}

.pg-nav:hover:not(:disabled) {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.pg-nav:disabled {
  color: var(--ink-subtle);
  cursor: default;
}

.pg-nav:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

/* 自绘页码按钮：32px 方域（--space-lg×2）+ 999px 胶囊 */
.pg-page {
  min-width: calc(var(--space-lg) * 2);
  height: calc(var(--space-lg) * 2);
  padding: 0 var(--space-sm);
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: var(--ink);
  font-size: 14px;
  font-weight: 500;
  line-height: 1;
  cursor: pointer;
  transition: background-color 0.15s ease, color 0.15s ease;
}

.pg-page:hover:not(:disabled):not(.is-active) {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent);
}

.pg-page.is-active {
  background: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

.pg-page:disabled {
  color: var(--ink-subtle);
  cursor: default;
}

.pg-page:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.pg-ellipsis {
  min-width: calc(var(--space-lg) * 2);
  color: var(--ink-subtle);
  font-size: 14px;
  text-align: center;
}

/* 容量下拉：原生小号 select（32px 控制高度），风格与 reader 变体跳页 select 统一；margin-left auto 恒居行尾 */
.pg-size {
  height: calc(var(--space-lg) * 2);
  margin-left: auto;
  padding: 0 var(--space-md) 0 var(--space-sm);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 45%, transparent);
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  font-size: 13px;
  cursor: pointer;
}

.pg-size:disabled {
  color: var(--ink-subtle);
  cursor: default;
}

.pg-size:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

/* 快捷跳页输入组：文案 + 输入框 + 「页」，行尾段（margin-left auto 与容量下拉成对）；
   jump 存在时由它接管行尾推挤，紧邻的容量下拉取消 auto 保持贴合 */
.pg-jump {
  display: inline-flex;
  align-items: center;
  gap: var(--space-xs);
  min-height: calc(var(--space-lg) * 2);
  margin-left: auto;
  color: var(--ink-muted);
  font-size: 13px;
  white-space: nowrap;
}

.pg-jump + .pg-size {
  margin-left: 0;
}

.pg-jump.is-disabled {
  color: var(--md-sys-color-outline);
}

.pg-jump-text {
  display: inline-flex;
  align-items: center;
}

.pg-jump-input {
  box-sizing: border-box;
  width: calc(var(--space-xl) * 2);
  height: calc(var(--space-lg) * 2);
  padding: 0 var(--space-xxs);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 45%, transparent);
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  font-size: 13px;
  text-align: center;
}

.pg-jump-input:disabled {
  color: var(--ink-subtle);
  cursor: default;
}

.pg-jump-input:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.pg-unknown-label {
  display: inline-flex;
  align-items: center;
  min-height: calc(var(--space-lg) * 2);
  font-size: 14px;
  font-weight: 500;
  white-space: nowrap;
}

/* reader 变体：与小说阅读器翻页器同 recipe（sticky 底部 + surface 底 + 上缘 divider）；
   三列 grid 让翻页组恒居中：左右 1fr 等宽、中间 auto；无 leading 时视觉与纯居中一致（BrowseSeriesView 回归点） */
.app-pagination.is-reader {
  position: sticky;
  bottom: 0;
  z-index: 10;
  display: grid;
  grid-template-columns: 1fr auto 1fr;
  align-items: center;
  padding: var(--space-sm) var(--space-md);
  background: var(--surface);
  border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 30%, transparent);
}

/* 显式落第 2 列：无 leading 插槽时首项默认落第 1 列会左移 */
.reader-inner {
  grid-column: 2;
  display: flex;
  align-items: center;
  gap: var(--space-sm);
}

.reader-leading {
  grid-column: 1;
  justify-self: start;
}

.page-select {
  max-width: 200px;
  padding: var(--space-xs) var(--space-lg) var(--space-xs) var(--space-sm);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 45%, transparent);
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  font-size: 13px;
  text-align: center;
  cursor: pointer;
}

.page-select:disabled {
  cursor: default;
}

.page-select:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.reader-label {
  color: var(--ink);
  font-size: 13px;
  white-space: nowrap;
}

.pager-icon {
  width: 20px;
  height: 20px;
  stroke-width: 1.8;
}

/* compact 断点：隐藏页码窗口，显示「current / pageCount」；prev/next、跳页输入组与容量下拉保留（随 wrap 自然换行），32px 高度同规 */
@media (max-width: 640px) {
  .pg-window {
    display: none;
  }

  .pg-compact-label {
    display: inline-flex;
    align-items: center;
    min-height: calc(var(--space-lg) * 2);
    font-size: 14px;
    font-weight: 500;
    white-space: nowrap;
  }
}
</style>
