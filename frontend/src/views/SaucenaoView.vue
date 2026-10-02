<script setup lang="ts">
import PageBackButton from "../components/navigation/PageBackButton.vue";
/**
 * 以图识图页（SauceNAO 反向图片搜索）。
 *
 * - 输入三通道：拖拽文件（Tauri onDragDropEvent）、系统文件选择对话框、公网图片 URL；
 *   搜索源优先级 = 已选本地文件 > URL 输入（SauceNAO 免费档 4 次/30s，选定/拖入
 *   不自动发请求，统一由「搜索」触发，避免误拖烧配额）。
 * - 状态机 idle | loading | done | empty | error；key 未配置时显示引导条
 *   （不打请求），「打开设置」派发 OPEN_SETTINGS_EVENT 由 App.vue 弹设置窗。
 * - 结果排序：pixiv（index 5/6）恒最前（组内 similarity 降序），其余 similarity 降序
 *   （后端按 SauceNAO 原序返回，排序在本页做）。
 * - 缩略图为 saucenao 签名临时 URL，原样直连（不走 pixiv-img），no-referrer +
 *   加载失败占位块；错误条文案 = 后端 Err 原样（errorMessage 归一化）。
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  OPEN_SETTINGS_EVENT,
  errorMessage,
  isTauri,
  searchSaucenao,
  type SaucenaoQuota,
  type SaucenaoResult,
} from "../api/saucenao";
import { useSettingsStore } from "../stores/settings";
import { openInBrowser } from "../utils/pixivHooks";
import { notify } from "../ui/notify";

const { t } = useI18n();
const router = useRouter();
const settingsStore = useSettingsStore();

// ===== 输入源 =====

/** 本地图片绝对路径（拖入 / 文件对话框；浏览器 mock 下文件无真实路径，仅记文件名）。 */
const filePath = ref<string | null>(null);
const fileName = ref("");
const urlInput = ref("");
const dragHover = ref(false);

/** Tauri 环境用 asset 协议生成本地预览地址；浏览器环境无路径，预览跳过。 */
const previewSrc = computed(() =>
  filePath.value && isTauri() ? convertFileSrc(filePath.value) : ""
);

function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** 采纳一张本地图片（拖入或选择）：覆盖旧图并回到初始态。 */
function adoptFile(path: string): void {
  filePath.value = path;
  fileName.value = baseName(path);
  resetResult();
}

function removeImage(): void {
  filePath.value = null;
  fileName.value = "";
  resetResult();
}

async function chooseFile(): Promise<void> {
  if (phase.value === "loading") return;
  try {
    const selected = await openDialog({
      multiple: false,
      filters: [
        { name: t("saucenao.fileFilterName"), extensions: ["png", "jpg", "jpeg", "gif", "webp"] },
      ],
    });
    if (typeof selected === "string") adoptFile(selected);
  } catch {
    // 用户取消或对话框不可用：静默
  }
}

// ===== 拖拽（仅 Tauri；浏览器无此 API，静默降级为仅文件选择 / URL）=====

let unlistenDragDrop: (() => void) | undefined;

onMounted(async () => {
  // 直跳本页时确保 settings 已就绪（App.vue 挂载时也拉过一次，幂等）
  void settingsStore.fetchSettings().catch(() => {});
  if (!isTauri()) return;
  try {
    unlistenDragDrop = await getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "enter") {
        dragHover.value = true;
      } else if (event.payload.type === "leave") {
        dragHover.value = false;
      } else if (event.payload.type === "drop") {
        dragHover.value = false;
        const path = event.payload.paths[0];
        if (path) adoptFile(path);
      }
    });
  } catch {
    // 拖拽事件不可用时静默降级（选择文件 / URL 通道不受影响）
  }
});

onBeforeUnmount(() => {
  unlistenDragDrop?.();
  unlistenDragDrop = undefined;
});

// ===== 搜索状态机 =====

type Phase = "idle" | "loading" | "done" | "empty" | "error";
const phase = ref<Phase>("idle");
const results = ref<SaucenaoResult[]>([]);
const quota = ref<SaucenaoQuota | null>(null);
const errorText = ref("");
/** 缩略图加载失败的条目下标（→ 占位块）。 */
const brokenThumbs = ref<Set<number>>(new Set());

const hasKey = computed(() => settingsStore.settings.saucenao_api_key.trim().length > 0);
const canSearch = computed(() => phase.value !== "loading" && (filePath.value != null || urlInput.value.trim().length > 0));

function resetResult(): void {
  phase.value = "idle";
  results.value = [];
  quota.value = null;
  errorText.value = "";
  brokenThumbs.value = new Set();
}

/** pixiv 恒最前（组内 similarity 降序），其余 similarity 降序。 */
function sortResults(items: SaucenaoResult[]): SaucenaoResult[] {
  return [...items].sort((a, b) =>
    a.is_pixiv === b.is_pixiv ? b.similarity - a.similarity : a.is_pixiv ? -1 : 1
  );
}

async function doSearch(): Promise<void> {
  if (!canSearch.value) return;
  // key 为空：不打请求（引导条已可见）；后端也有同款防御
  if (!hasKey.value) return;
  let sourceType: "file" | "url";
  let source: string;
  if (filePath.value) {
    sourceType = "file";
    source = filePath.value;
  } else {
    const url = urlInput.value.trim();
    // 明显非法的输入直接拦下，不浪费配额（后端错误文案面向真实网络错误）
    if (!/^https?:\/\/\S+$/i.test(url)) {
      errorText.value = t("saucenao.invalidUrl");
      phase.value = "error";
      return;
    }
    sourceType = "url";
    source = url;
  }
  phase.value = "loading";
  errorText.value = "";
  try {
    const data = await searchSaucenao(sourceType, source);
    results.value = sortResults(data.results);
    quota.value = data.quota;
    brokenThumbs.value = new Set();
    phase.value = results.value.length ? "done" : "empty";
  } catch (err) {
    errorText.value = errorMessage(err);
    phase.value = "error";
  }
}

// ===== 结果展示辅助 =====

/** 来源名：pixiv 条目显示固定名；其余剥离 `Index #N: ` 前缀。 */
function sourceName(item: SaucenaoResult): string {
  if (item.is_pixiv) return t("saucenao.sourcePixiv");
  return item.index_name.replace(/^Index #\d+:\s*/, "") || item.index_name;
}

function displayTitle(item: SaucenaoResult): string {
  const title = item.title?.trim();
  return title || t("saucenao.noTitle");
}

/** 作者行：member_name · 作品 {pixiv_id}（缺哪段省哪段）。 */
function authorLine(item: SaucenaoResult): string {
  const parts: string[] = [];
  if (item.member_name) parts.push(item.member_name);
  if (item.pixiv_id != null) parts.push(t("saucenao.workId", { id: item.pixiv_id }));
  return parts.join(" · ");
}

function quotaLine(quota: SaucenaoQuota): string {
  return t("saucenao.quotaLine", {
    shortRemaining: quota.short_remaining ?? "–",
    shortLimit: quota.short_limit,
    longRemaining: quota.long_remaining ?? "–",
    longLimit: quota.long_limit,
  });
}

function openResult(item: SaucenaoResult): void {
  if (!item.ext_url) return;
  void openInBrowser(item.ext_url).catch(() => notify(t("saucenao.openFailed")));
}

function openWork(item: SaucenaoResult): void {
  if (item.pixiv_id == null) return;
  void router.push(`/browse/work/illust/${item.pixiv_id}`);
}

function openSettingsDialog(): void {
  window.dispatchEvent(new CustomEvent(OPEN_SETTINGS_EVENT));
}

function onThumbError(index: number): void {
  brokenThumbs.value.add(index);
}
</script>

<template>
  <div class="page-view">
    <div class="page-heading saucenao-heading"><PageBackButton /><h1 class="page-title">{{ t("saucenao.title") }}</h1></div>
    <p class="page-subtitle">{{ t("saucenao.subtitle") }}</p>

    <!-- 输入卡片：左预览 + 右操作 -->
    <div class="input-card" :class="{ dragging: dragHover }">
      <div class="preview">
        <img v-if="previewSrc" :src="previewSrc" class="preview-img" :alt="fileName" />
        <template v-else-if="fileName">
          <!-- 浏览器 mock 环境：本地文件无可预览路径，仅显示文件名 -->
          <span class="preview-filename" :title="fileName">{{ fileName }}</span>
        </template>
        <template v-else>
          <svg class="preview-placeholder-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <!-- lucide image -->
            <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
            <circle cx="9" cy="9" r="2" />
            <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />
          </svg>
          <span class="preview-placeholder-text">{{ t("saucenao.previewEmpty") }}</span>
        </template>
        <md-icon-button
          v-if="filePath || fileName"
          class="preview-remove"
          :aria-label="t('saucenao.removeImage')"
          :title="t('saucenao.removeImage')"
          @click="removeImage"
        >
          <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <!-- lucide x -->
            <path d="M18 6 6 18" />
            <path d="m6 6 12 12" />
          </svg>
        </md-icon-button>
      </div>

      <div class="input-body">
        <p class="drag-hint">{{ t("saucenao.dragHint") }}</p>
        <div>
          <md-outlined-button :disabled="phase === 'loading'" @click="chooseFile">
            {{ t("saucenao.chooseFile") }}
          </md-outlined-button>
        </div>
        <div class="or-divider" aria-hidden="true"><span>{{ t("saucenao.or") }}</span></div>
        <div class="url-row">
          <md-outlined-text-field
            class="url-field"
            :value="urlInput"
            :label="t('saucenao.urlLabel')"
            :placeholder="t('saucenao.urlPlaceholder')"
            :aria-label="t('saucenao.urlLabel')"
            :disabled="phase === 'loading'"
            @input="urlInput = ($event.target as HTMLInputElement).value"
            @keydown.enter="doSearch"
          />
          <md-filled-button class="search-btn" :disabled="!canSearch" @click="doSearch">
            {{ t("common.search") }}
          </md-filled-button>
        </div>
      </div>
    </div>

    <!-- key 未配置引导条（不打请求） -->
    <div v-if="!hasKey" class="m3-alert warning guide-bar">
      <span class="guide-text">{{ t("saucenao.needKey") }}</span>
      <md-text-button @click="openSettingsDialog">{{ t("saucenao.openSettings") }}</md-text-button>
    </div>

    <!-- loading 态 -->
    <div v-if="phase === 'loading'" class="loading-block" role="status">
      <md-linear-progress class="loading-bar" :indeterminate="true" :aria-label="t('saucenao.searching')" />
      <span class="loading-text">{{ t("saucenao.searching") }}</span>
    </div>

    <!-- error 态：后端 Err 文案原样 -->
    <div v-if="phase === 'error'" class="m3-alert error" role="alert">{{ errorText }}</div>

    <!-- empty 终态 -->
    <p v-if="phase === 'empty'" class="empty-state" role="status">{{ t("saucenao.noMatch") }}</p>

    <!-- idle 引导文案 -->
    <p v-if="phase === 'idle'" class="idle-hint">{{ t("saucenao.idleHint") }}</p>

    <!-- 结果区 -->
    <section v-if="phase === 'done'" class="result-section">
      <div class="result-head">
        <span class="result-count" role="status">{{ t("saucenao.resultCount", { count: results.length }) }}</span>
        <span v-if="quota" class="quota-line">{{ quotaLine(quota) }}</span>
      </div>

      <div v-for="(item, i) in results" :key="`${i}-${item.index_id}`" class="result-item">
        <div class="thumb">
          <img
            v-if="!brokenThumbs.has(i) && item.thumbnail"
            class="thumb-img"
            :src="item.thumbnail"
            loading="lazy"
            referrerpolicy="no-referrer"
            :alt="displayTitle(item)"
            @error="onThumbError(i)"
          />
          <div v-else class="thumb-placeholder" :title="t('saucenao.thumbUnavailable')">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <!-- lucide image -->
              <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
              <circle cx="9" cy="9" r="2" />
              <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />
            </svg>
          </div>
        </div>

        <div class="item-main">
          <div class="item-top">
            <span class="sim-badge">{{ item.similarity.toFixed(1) }}%</span>
            <span v-if="item.is_pixiv" class="source-chip">{{ sourceName(item) }}</span>
            <span v-else class="source-name" :title="item.index_name">{{ sourceName(item) }}</span>
          </div>
          <p class="item-title" :title="displayTitle(item)">{{ displayTitle(item) }}</p>
          <p v-if="authorLine(item)" class="item-author" :title="authorLine(item)">{{ authorLine(item) }}</p>
        </div>

        <div class="item-actions">
          <md-filled-tonal-button v-if="item.is_pixiv && item.pixiv_id != null" @click="openWork(item)">
            {{ t("saucenao.inApp") }}
          </md-filled-tonal-button>
          <md-icon-button
            v-if="item.ext_url"
            :aria-label="t('saucenao.openOriginal')"
            :title="t('saucenao.openOriginal')"
            @click="openResult(item)"
          >
            <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <!-- lucide square-arrow-out-up-right -->
              <path d="M21 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h6" />
              <path d="m21 3-9 9" />
              <path d="M15 3h6v6" />
            </svg>
          </md-icon-button>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.saucenao-heading { margin-bottom: var(--space-lg); }
.page-subtitle {
  margin: calc(-1 * var(--space-sm)) 0 var(--space-lg);
  color: var(--ink-muted);
  font-size: 13px;
}

/* ===== 输入卡片 ===== */

.input-card {
  display: flex;
  gap: var(--space-xl);
  padding: var(--space-xl);
  /* 16px = DESIGN.md rounded.card（与 .form-card / .m3-card 同规格） */
  border-radius: 16px;
  background: var(--md-sys-color-surface-container);
  /* outline 常驻占位（透明），拖入高亮时不产生布局位移 */
  outline: 2px solid transparent;
}

.input-card.dragging {
  outline-color: var(--md-sys-color-primary);
}

.preview {
  position: relative;
  flex: none;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-xs);
  width: 160px;
  height: 160px;
  border-radius: 12px;
  overflow: hidden;
  background: color-mix(in srgb, var(--md-sys-color-outline) 10%, transparent);
  color: var(--ink-subtle);
}

.preview-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.preview-filename {
  max-width: 90%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--ink-muted);
}

.preview-placeholder-icon {
  width: 40px;
  height: 40px;
  stroke-width: 2;
}

.preview-placeholder-text {
  font-size: 12px;
  color: var(--ink-subtle);
}

.preview-remove {
  position: absolute;
  top: 0;
  right: 0;
}

.input-body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: var(--space-md);
}

.drag-hint {
  margin: 0;
  color: var(--ink-muted);
}

.or-divider {
  display: flex;
  align-items: center;
  gap: var(--space-md);
  color: var(--ink-subtle);
  font-size: 12px;
}

.or-divider::before,
.or-divider::after {
  content: "";
  flex: 1;
  border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 30%, transparent);
}

.url-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-md);
}

.url-field {
  flex: 1;
  min-width: 220px;
}

.search-btn {
  flex: none;
}

/* ===== 引导条 / 状态条（warning / error 配色走全局 .m3-alert.*） ===== */

.guide-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-xs) var(--space-md);
}

.guide-text {
  flex: 1;
  min-width: 200px;
}

.loading-block {
  display: flex;
  flex-direction: column;
  gap: var(--space-xs);
  margin-top: var(--space-lg);
}

.loading-text {
  font-size: 12px;
  color: var(--ink-muted);
}

.empty-state,
.idle-hint {
  margin: var(--space-lg) 0 0;
  color: var(--ink-muted);
}

.idle-hint {
  font-size: 12px;
  color: var(--ink-subtle);
}

/* ===== 结果区 ===== */

.result-section {
  margin-top: var(--space-lg);
}

.result-head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--space-xs) var(--space-md);
  margin-bottom: var(--space-sm);
}

.result-count {
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-muted);
}

.quota-line {
  font-size: 12px;
  color: var(--ink-subtle);
}

.result-item {
  display: flex;
  align-items: center;
  gap: var(--space-md);
  margin-top: var(--space-sm);
  padding: var(--space-md) var(--space-lg);
  border-radius: 16px;
  background: var(--md-sys-color-surface-container);
}

.result-item:hover {
  background: color-mix(in srgb, var(--md-sys-color-outline) 8%, var(--md-sys-color-surface-container));
}

.thumb {
  flex: none;
  width: 96px;
  height: 96px;
  border-radius: 12px;
  overflow: hidden;
}

.thumb-img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.thumb-placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  background: color-mix(in srgb, var(--md-sys-color-outline) 12%, var(--md-sys-color-surface-container));
  color: var(--ink-subtle);
}

.thumb-placeholder svg {
  width: 32px;
  height: 32px;
  stroke-width: 2;
}

.item-main {
  flex: 1;
  min-width: 0;
}

.item-top {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-xs);
}

.sim-badge {
  flex: none;
  padding: 1px 10px;
  border-radius: 999px;
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
  font-size: 12px;
  font-weight: 600;
}

.source-chip {
  padding: 1px 10px;
  border-radius: 999px;
  background: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
  font-size: 12px;
  font-weight: 600;
}

.source-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--ink-muted);
}

.item-title {
  margin: var(--space-xxs) 0 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}

.item-author {
  margin: var(--space-xxs) 0 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--ink-muted);
}

.item-actions {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--space-xs);
}

/* 图标动作统一 20px 线性图标（stroke 2，lucide 官方路径），与仓库其他视图一致 */
.bar-icon {
  width: 20px;
  height: 20px;
  stroke-width: 2;
}

@media (max-width: 640px) {
  /* 窄窗口：预览与操作区纵向堆叠 */
  .input-card {
    flex-direction: column;
    align-items: center;
  }

  .input-body {
    width: 100%;
  }

  /* 结果条目退化为两行：首行缩略图 + 信息占满剩余宽度，动作按钮整行右对齐换到第二行 */
  .result-item {
    flex-wrap: wrap;
  }

  .item-actions {
    flex-basis: 100%;
    justify-content: flex-end;
  }
}
</style>
