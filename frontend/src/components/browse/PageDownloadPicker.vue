<script setup lang="ts">
/**
 * 详情页·多 P 作品按页选择下载（browse-work-pages-v1）。
 *
 * - 方形缩略图网格（自适应列宽，56px 起），序号与「已下载」用角标表达；
 * - 选中态不只靠颜色：2px primary 外描边 + 右上角实心圆对勾，且按钮 aria-pressed 同步；
 * - 已下载页来自 illustSavedPages（onMounted + watch id），请求失败静默降级为「全部未下载」；
 * - 下载走 tasks.createTask("single", id, [], "illustration", 页码升序数组)，busy 期间禁止重复提交；
 * - 任务完成后重新拉取已下载页：task://done 事件不带作品 id，故按「本组件刚创建那次任务的第一条
 *   done」收敛（一次性订阅，触发即注销）；页面级轮询兜底由 tasks store 负责，这里只补「已下载」标记；
 * - ugoira 作品整块不渲染（动图没有按页选择语义）；
 * - 组件自身无卡片外框，纵向 space-md、行内 space-sm，宽度自适应右侧信息列不横向溢出。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { errorMessage, illustSavedPages, thumbSrc } from "../../api/browse";
import { listen, type UnlistenFn } from "../../api/tauri";
import { useTaskStore } from "../../stores/tasks";
import { notify } from "../../ui/notify";
import { useThumbTier } from "../../composables/useThumbTier";

const props = defineProps<{
  /** 作品 id（下载任务来源 id） */
  id: number;
  /** 详情响应的 pages（small/medium 可能缺失，original 必有） */
  pages: { small?: string; medium?: string; original: string; width?: number; height?: number }[];
  /** 动图作品：整块不渲染 */
  ugoira?: boolean;
  /** 外部禁用（例如详情仍在刷新） */
  disabled?: boolean;
}>();

const { t } = useI18n();
const tasks = useTaskStore();
const gridTier = useThumbTier("thumb_quality_grid");

/** 已下载页（0 起）；读取失败保持空集，不影响手动选择 */
const savedPages = ref<Set<number>>(new Set());
/** 选中页（0 起）；整集替换以保证响应式 */
const selected = ref<Set<number>>(new Set());
const busy = ref(false);
/** 作品切换竞态：只接受最后一次请求的结果 */
let savedToken = 0;
/** 本次下载注册的 task://done 一次性订阅；重复下载前与卸载时注销 */
let stopDone: UnlistenFn | null = null;
/** 订阅代次：注销时自增，令在途注册的 then/catch 与旧回调失效 */
let doneToken = 0;

/** 注销 task://done 订阅（幂等；同时作废在途注册） */
function clearDoneListener(): void {
  doneToken += 1;
  stopDone?.();
  stopDone = null;
}

/**
 * 等本组件刚创建的任务完成：事件不带作品 id，取第一条 done 即收敛。
 * 注册失败静默忽略——下载本身已成功，已下载标记留给下次挂载 / 切作品时刷新。
 */
function watchTaskDone(): void {
  clearDoneListener();
  const token = doneToken;
  try {
    void listen("task://done", () => {
      if (token !== doneToken) return;
      clearDoneListener();
      void loadSaved();
    }).then((unlisten) => {
      // 注册完成时已被作废（重复下载 / 卸载）→ 立即退订，不覆盖新订阅
      if (token === doneToken) stopDone = unlisten;
      else unlisten();
    }).catch(() => {
      if (token === doneToken) stopDone = null;
    });
  } catch {
    // 非 Tauri 环境或订阅同步失败：静默，不影响下载结果
    if (token === doneToken) stopDone = null;
  }
}

const total = computed(() => props.pages.length);
const selectedPages = computed(() => [...selected.value].sort((a, b) => a - b));
const selectedCount = computed(() => selectedPages.value.length);
/** 全部页都已落盘（无页时不算） */
const allSaved = computed(() => total.value > 0 && props.pages.every((_, i) => savedPages.value.has(i)));
const downloadDisabled = computed(() => Boolean(props.disabled) || busy.value || selectedCount.value === 0);

function isSelected(index: number): boolean {
  return selected.value.has(index);
}

function toggle(index: number): void {
  const next = new Set(selected.value);
  if (next.has(index)) next.delete(index);
  else next.add(index);
  selected.value = next;
}

/** 全选未下载页；若全部已下载则退化为全选全部 */
function selectAll(): void {
  const all = props.pages.map((_, i) => i);
  const unsaved = all.filter((i) => !savedPages.value.has(i));
  selected.value = new Set(unsaved.length ? unsaved : all);
}

function clearAll(): void {
  selected.value = new Set();
}

async function loadSaved(): Promise<void> {
  const token = ++savedToken;
  try {
    const result = await illustSavedPages(props.id);
    if (token !== savedToken) return;
    savedPages.value = new Set(result.pages);
  } catch {
    // 静默：查不到已下载页时按「全部未下载」处理，不打扰手动选择
    if (token === savedToken) savedPages.value = new Set();
  }
}

async function download(): Promise<void> {
  if (downloadDisabled.value) return;
  if (allSaved.value) {
    notify(t("browse.work.pageAllSaved"));
    return;
  }
  const pages = selectedPages.value;
  busy.value = true;
  try {
    const result = await tasks.createTask("single", String(props.id), [], "illustration", pages);
    if (result.error) {
      notify(result.error);
      return;
    }
    notify(t("browse.work.pageDownloadStarted", { count: pages.length }));
    await tasks.fetchTasks().catch(() => {});
    selected.value = new Set();
    // 任务已在后端排队：等它完成后再刷新「已下载」角标（一次性订阅）
    watchTaskDone();
  } catch (err) {
    notify(errorMessage(err) || t("browse.work.pageDownloadFailed"));
  } finally {
    busy.value = false;
  }
}

onMounted(loadSaved);
onBeforeUnmount(clearDoneListener);
watch(() => props.id, () => {
  // 换作品：旧任务的完成事件不再与本作品相关
  clearDoneListener();
  selected.value = new Set();
  savedPages.value = new Set();
  void loadSaved();
});
</script>

<template>
  <section v-if="!ugoira" class="page-picker">
    <h3 class="picker-title">{{ t("browse.work.pagePickerTitle") }}</h3>

    <div class="picker-actions">
      <div class="picker-links">
        <button
          type="button"
          class="picker-link"
          :title="t('browse.work.pageSelectAll')"
          @click="selectAll"
        >
          {{ t("browse.work.pageSelectAll") }}
        </button>
        <button
          type="button"
          class="picker-link"
          :title="t('browse.work.pageSelectNone')"
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

    <div class="picker-footer">
      <md-filled-button
        :disabled="downloadDisabled"
        :aria-busy="busy ? 'true' : undefined"
        @click="download"
      >
        {{ t("browse.work.pageDownloadSelected", { count: selectedCount }) }}
      </md-filled-button>
    </div>
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

/* 320px 信息列内不允许主按钮把容器撑出横向滚动 */
.picker-footer md-filled-button {
  max-width: 100%;
}

@media (prefers-reduced-motion: reduce) {
  .picker-link,
  .page-cell {
    transition: none;
  }
}
</style>
