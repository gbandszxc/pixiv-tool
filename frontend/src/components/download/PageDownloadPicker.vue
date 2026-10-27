<script setup lang="ts">
/**
 * 按页选择下载（受控展示组件，browse-work-pages-v1）。
 *
 * - 方形缩略图网格（自适应列宽，56px 起），序号与「已下载」用角标表达；
 * - 选中态不只靠颜色：2px primary 外描边 + 右上角实心圆对勾，且按钮 aria-pressed 同步；
 * - 自身不查已下载、不建任务：`pages`/`saved` 由父级提供，选中集合经 v-model 双向绑定；
 * - 只负责选择：选中集合经 v-model 交给父级，由下载侧栏的下载主按钮统一下发任务；
 * - ugoira 由父级直接不渲染（动图没有按页选择语义），这里只按页数兜底；
 * - 组件自身无卡片外框，纵向 space-md、行内 space-sm，宽度自适应容器不横向溢出。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { thumbSrc, type IllustPage } from "../../api/browse";
import { useThumbTier } from "../../composables/useThumbTier";

const props = withDefaults(
  defineProps<{
    /** 作品页面（small/medium 可能缺失，original 必有） */
    pages?: IllustPage[];
    /** 已下载页码（0 起） */
    saved?: number[];
    /** 选中页码（0 起）；父级持有的唯一数据源 */
    modelValue?: number[];
    /** 外部禁用（例如表单提交中） */
    disabled?: boolean;
  }>(),
  { pages: () => [], saved: () => [], modelValue: () => [], disabled: false }
);

const emit = defineEmits<{
  (e: "update:modelValue", pages: number[]): void;
}>();

const { t } = useI18n();
const gridTier = useThumbTier("thumb_quality_grid");

const total = computed(() => props.pages.length);
const savedPages = computed(() => new Set(props.saved));
/**
 * 本地选中集：props 更新要等父级重渲染，连续快速点选会读到同一份旧值而丢步，
 * 因此组件自持一份同步副本；外部重置（换作品 / 全选）经 watch 覆盖回来。
 */
const selection = ref<number[]>([...props.modelValue]);
/** 已 emit、尚未回写到 props 的选中集（逗号串）：props 回写比连续点击慢一拍，靠它区分「自己的回写」与「外部重置」 */
let pendingEchoes: string[] = [];
watch(() => props.modelValue, (value) => {
  const key = value.join(",");
  const echo = pendingEchoes.indexOf(key);
  if (echo >= 0) {
    // 父级把本次（或更早一次）的选择写回来了：丢弃该批回写，不覆盖本地最新状态
    pendingEchoes = pendingEchoes.slice(echo + 1);
    return;
  }
  // 真正的外部重置（换作品 / 默认全选 / 清空）：以 props 为准
  pendingEchoes = [];
  if (key !== selection.value.join(",")) selection.value = [...value];
});
const selected = computed(() => new Set(selection.value));
const selectedCount = computed(() => selected.value.size);
/**
 * 全选按钮的禁用判定：selectAll 的目标集（未下载页；全部已下载则退化为全部页）
 * 已与当前选中集一致时，点击不会有任何变化。
 */
const selectDisabled = computed(() => {
  if (total.value === 0) return true;
  const unsaved = props.pages.map((_, i) => i).filter((i) => !savedPages.value.has(i));
  const goal = unsaved.length ? unsaved : props.pages.map((_, i) => i);
  return goal.length === selection.value.length && goal.every((i) => selected.value.has(i));
});
/** 清空按钮的禁用判定：已经一页未选时无可清空。 */
const clearDisabled = computed(() => selectedCount.value === 0);

function isSelected(index: number): boolean {
  return selected.value.has(index);
}

/** 提交升序页码，父级无需再排序 */
function commit(pages: Iterable<number>): void {
  const next = [...pages].sort((a, b) => a - b);
  selection.value = next;
  pendingEchoes.push(next.join(","));
  emit("update:modelValue", next);
}

function toggle(index: number): void {
  const next = new Set(selected.value);
  if (next.has(index)) next.delete(index);
  else next.add(index);
  commit(next);
}

/** 全选未下载页；若全部已下载则退化为全选全部 */
function selectAll(): void {
  const all = props.pages.map((_, i) => i);
  const unsaved = all.filter((i) => !savedPages.value.has(i));
  commit(unsaved.length ? unsaved : all);
}

function clearAll(): void {
  commit([]);
}
</script>

<template>
  <section class="page-picker">
    <h3 class="picker-title">{{ t("browse.work.pagePickerTitle") }}</h3>

    <template v-if="total > 0">
      <div class="picker-actions">
        <div class="picker-links">
          <button
            type="button"
            class="picker-link"
            :title="t('browse.work.pageSelectAll')"
            :disabled="props.disabled || selectDisabled"
            @click="selectAll"
          >
            {{ t("browse.work.pageSelectAll") }}
          </button>
          <button
            type="button"
            class="picker-link"
            :title="t('browse.work.pageSelectNone')"
            :disabled="props.disabled || clearDisabled"
            @click="clearAll"
          >
            {{ t("browse.work.pageSelectNone") }}
          </button>
        </div>
        <span class="picker-hint">{{ t("browse.work.pagePickerHint", { count: selectedCount, total }) }}</span>
      </div>

      <div class="picker-grid">
        <button
          v-for="(page, i) in pages"
          :key="i"
          type="button"
          class="page-cell"
          :class="{ selected: isSelected(i) }"
          :aria-pressed="isSelected(i)"
          :aria-label="t('browse.work.pageSelectLabel', { page: i + 1 })"
          :title="t('browse.work.pageSelectLabel', { page: i + 1 })"
          @click="toggle(i)"
        >
          <img
            :src="thumbSrc(page.small ?? page.medium ?? page.original, gridTier)"
            loading="lazy"
            decoding="async"
            alt=""
          />
          <span class="page-badges">
            <span class="page-index">{{ i + 1 }}</span>
            <span v-if="savedPages.has(i)" class="page-saved">{{ t("browse.work.pageAlreadySaved") }}</span>
          </span>
          <span v-if="isSelected(i)" class="page-check" aria-hidden="true">
            <!-- lucide check -->
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M20 6 9 17l-5-5" />
            </svg>
          </span>
        </button>
      </div>
    </template>

    <!-- 保底空态：父级通常直接不渲染本组件 -->
    <p v-else class="picker-empty">{{ t("browse.work.pagePickerEmpty") }}</p>
  </section>
</template>

<style scoped>
/* 组件无外框：直接标题 + 网格 + 动作行，纵向 space-md 分隔 */
.page-picker {
  display: flex;
  flex-direction: column;
  gap: var(--space-md);
  min-width: 0;
}

.picker-title {
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  line-height: 1.4;
  color: var(--ink);
}

.picker-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--space-sm);
  min-width: 0;
}

.picker-links {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
}

/* 文本按钮：12px/600 ink-muted，与 md-text-button 观感一致但不带 40px 最小高度 */
.picker-link {
  padding: 0;
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 600;
  line-height: 1.4;
  cursor: pointer;
  transition: color 0.15s ease;
}

.picker-link:hover {
  color: var(--ink);
}

/* 无动作可做（已全选 / 已清空）或外部禁用时：降透明度、不变色、不可点 */
.picker-link:disabled {
  cursor: default;
  opacity: 0.45;
}

.picker-link:disabled:hover {
  color: var(--ink-muted);
}

.picker-link:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.picker-hint {
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.4;
  font-variant-numeric: tabular-nums;
}

.picker-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(56px, 1fr));
  gap: var(--space-xs);
  min-width: 0;
}

.page-cell {
  position: relative;
  aspect-ratio: 1;
  min-width: 0;
  padding: 0;
  border: 0;
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
  overflow: hidden;
  cursor: pointer;
  outline: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 45%, transparent);
  outline-offset: 0;
  transition: outline-color 0.15s ease;
}

/* 选中态：描边 + 右上角对勾两重表达，不依赖颜色单通道 */
.page-cell.selected {
  outline: 2px solid var(--md-sys-color-primary);
}

.page-cell:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.page-cell img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.page-badges {
  position: absolute;
  top: var(--space-xxs);
  left: var(--space-xxs);
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-xxs);
  max-width: calc(100% - var(--space-xxs) * 2);
  pointer-events: none;
}

.page-index,
.page-saved {
  padding: 1px 6px;
  border-radius: 999px;
  font-size: 11px;
  line-height: 1.4;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.page-index {
  background: color-mix(in srgb, var(--md-sys-color-surface) 82%, transparent);
  color: var(--ink);
}

.page-saved {
  background: var(--md-sys-color-surface-container);
  color: var(--ink-muted);
}

.page-check {
  position: absolute;
  top: var(--space-xxs);
  right: var(--space-xxs);
  display: grid;
  place-items: center;
  width: 16px;
  height: 16px;
  border-radius: 999px;
  background: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
  pointer-events: none;
}

.page-check svg {
  width: 10px;
  height: 10px;
}

.picker-footer {
  display: flex;
  justify-content: flex-start;
  min-width: 0;
}

/* 窄容器内不允许主按钮把容器撑出横向滚动 */
.picker-footer md-filled-button {
  max-width: 100%;
}

.picker-empty {
  margin: 0;
  color: var(--ink-muted);
  font-size: 12px;
  line-height: 1.4;
}

@media (prefers-reduced-motion: reduce) {
  .picker-link,
  .page-cell {
    transition: none;
  }
}
</style>
