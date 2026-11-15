<script setup lang="ts">
import HelpTooltip from "../components/common/HelpTooltip.vue";
import PageBackButton from "../components/navigation/PageBackButton.vue";
/**
 * 以图识图页（SauceNAO 反向图片搜索）。
 *
 * - 输入区 v2：输入卡片本体 = 拖拽热区（Tauri onDragDropEvent，任何展示模式下
 *   都可用且优先于 URL 输入），内部按展示层模式切换三态：空态（image-up 主视觉 +
 *   「选择文件」filled 主按钮 + 「使用图片 URL」小字链接）、URL 次级模式
 *   （输入框 + 搜索同行 + 「使用本地文件」小字链接切回）、已采纳态（大图预览 +
 *   48px filled「搜索」大号主 CTA）。搜索源优先级 = 已选本地文件 > URL 输入
 *   （SauceNAO 免费档 4 次/30s，选定/拖入不自动发请求，统一由「搜索」触发，
 *   避免误拖烧配额）。
 * - 本地预览走仓库自建 pixiv-download 协议（默认 asset 协议未启用、CSP 不放行；
 *   协议按扩展名白名单 jpg/jpeg/png/webp/gif 读本地图片，与本页文件过滤器一致，
 *   注册见 src-tauri/src/lib.rs）；浏览器 mock 无真实路径，仅显示文件名。
 * - 状态机 idle | loading | done | empty | error；key 未配置时显示引导条
 *   （不打请求），「打开设置」派发 OPEN_SETTINGS_EVENT（携带 images 分区与 Key
 *   字段 id）由 App.vue 直达设置弹窗并聚焦，「获取 API Key」走系统浏览器外链。
 * - 搜索历史：输入卡片之下、结果区之上（空列表不渲染；最新在前 ≤200 条，后端
 *   在 saucenao_search 成功后权威记录，本页只在挂载与每次成功搜索后刷新）。
 *   区标题在限高容器外，列表本体 max-height min(60vh, 320px) 内部滚动。
 *   点击整行 = 采纳该源并立即搜索；hover 单条删除；file 条目缩略图走
 *   pixiv-download 协议、url 条目直连（no-referrer + 失败占位块，同结果缩略图）。
 * - 结果排序：pixiv（index 5/6）恒最前（组内 similarity 降序），其余 similarity 降序
 *   （后端按 SauceNAO 原序返回，排序在本页做）。
 * - 缩略图为 saucenao 签名临时 URL，原样直连（不走 pixiv-img），no-referrer +
 *   加载失败占位块；相似度 = 13px 数值为准 + primary 进度条作补充（aria-hidden）；
 *   错误条文案 = 后端 Err 原样（errorMessage 归一化）。
 */
import { computed, onActivated, onBeforeUnmount, onDeactivated, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  OPEN_SETTINGS_EVENT,
  SAUCENAO_KEY_FIELD_ID,
  deleteSaucenaoHistory,
  errorMessage,
  isTauri,
  listSaucenaoHistory,
  searchSaucenao,
  type OpenSettingsDetail,
  type SaucenaoHistoryItem,
  type SaucenaoQuota,
  type SaucenaoResult,
} from "../api/saucenao";
import { useSettingsStore } from "../stores/settings";
import { openInBrowser } from "../utils/pixivHooks";
import { relativeTime as formatRelativeTime } from "../utils/relativeTime";
import { notify } from "../ui/notify";

const { t, locale } = useI18n();
const router = useRouter();
const settingsStore = useSettingsStore();

// ===== 输入源 =====

/** 本地图片绝对路径（拖入 / 文件对话框；浏览器 mock 下文件无真实路径，仅记文件名）。 */
const filePath = ref<string | null>(null);
const fileName = ref("");
const urlInput = ref("");
const dragHover = ref(false);
/** 输入卡片展示层模式：file = 本地主通道（空态热区），url = URL 次级入口。 */
const inputMode = ref<"file" | "url">("file");

/** 已采纳图片（Tauri 有真实路径；浏览器 mock 只记文件名）。 */
const adopted = computed(() => filePath.value != null || fileName.value.length > 0);

/** 空态热区（file 主通道 + 尚无图片）：整卡可点 = 选择文件。 */
const showEmptyZone = computed(() => inputMode.value === "file" && !adopted.value);

/** URL 次级模式（尚无图片时才展示；拖入图片会自动切回 file 主通道）。 */
const showUrlMode = computed(() => inputMode.value === "url" && !adopted.value);

/**
 * Tauri 环境用仓库自建 pixiv-download 协议生成本地预览地址（默认 asset 协议未启用，
 * CSP img-src 也不放行，直用必裂；该协议按扩展名白名单读任意本地图片）；浏览器环境
 * 无路径，预览跳过。
 */
const previewSrc = computed(() =>
  filePath.value && isTauri() ? convertFileSrc(filePath.value, "pixiv-download") : ""
);

function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** 采纳一张本地图片（拖入或选择）：覆盖旧图、切回 file 主通道并回到初始态。 */
function adoptFile(path: string): void {
  filePath.value = path;
  fileName.value = baseName(path);
  inputMode.value = "file";
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

/** 空态整卡可点（等价「选择文件」）；URL 模式与已采纳态卡片本体不承载点击。 */
function onCardClick(): void {
  if (showEmptyZone.value) chooseFile();
}

// ===== 拖拽（仅 Tauri；浏览器无此 API，静默降级为仅文件选择 / URL）=====
//
// 本页入 KeepAlive 缓存（App.vue cachedViews）：失活即摘除监听，避免在别的页面
// 拖入图片被本页收下；激活（含首次挂载）时重新挂载并刷新一次历史。

let unlistenDragDrop: (() => void) | undefined;
let attachSeq = 0;
let disposed = false;

async function attachDrag(): Promise<void> {
  if (!isTauri()) return;
  const seq = ++attachSeq;
  try {
    const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      if (disposed || seq !== attachSeq) return;
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
    if (disposed || seq !== attachSeq) {
      unlisten();
      return;
    }
    unlistenDragDrop = unlisten;
  } catch {
    // 拖拽事件不可用时静默降级（选择文件 / URL 通道不受影响）
  }
}

function detachDrag(): void {
  attachSeq += 1; // 作废在途注册，避免异步回写竞态
  unlistenDragDrop?.();
  unlistenDragDrop = undefined;
  dragHover.value = false;
}

onMounted(() => {
  // 直跳本页时确保 settings 已就绪（App.vue 挂载时也拉过一次，幂等）
  void settingsStore.fetchSettings().catch(() => {});
});

onActivated(() => {
  void attachDrag();
  void refreshHistory();
});

onDeactivated(detachDrag);

onBeforeUnmount(() => {
  disposed = true;
  detachDrag();
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
// 无 Key 时按钮直接禁用（引导条仍在，信息不丢），不再留一个点了没反应的可点状态。
const canSearch = computed(() => hasKey.value && phase.value !== "loading" && (filePath.value != null || urlInput.value.trim().length > 0));

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
  // key 为空不发生（按钮已禁用）；此处仅作防御，后端也有同款防御
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
    // 本页搜索成功即刷新历史（后端在 saucenao_search 成功路径内已权威记录）
    void refreshHistory();
  } catch (err) {
    errorText.value = errorMessage(err);
    phase.value = "error";
  }
}

// ===== 搜索历史（后端权威记录，本页只读 + 删；空列表不渲染整个区块）=====

const historyItems = ref<SaucenaoHistoryItem[]>([]);
/** 缩略图加载失败的条目 id（file 路径被移动 / url 签名失效 → 占位块）。 */
const brokenHistoryThumbs = ref<Set<number>>(new Set());

async function refreshHistory(): Promise<void> {
  try {
    historyItems.value = await listSaucenaoHistory();
    brokenHistoryThumbs.value = new Set();
  } catch {
    // 历史是辅助信息，读取失败静默保留旧列表，不打断搜索主流程
  }
}

/** file 条目经 pixiv-download 协议现场取（同本页预览），url 条目直连。 */
function historyThumbSrc(item: SaucenaoHistoryItem): string {
  if (item.source_type === "file") return isTauri() ? convertFileSrc(item.source, "pixiv-download") : "";
  return item.source;
}

function onHistoryThumbError(id: number): void {
  brokenHistoryThumbs.value.add(id);
}

/** 元信息行：命中数 · 相对时间（分钟/小时/天，超 30 天回落短日期）。 */
function historyMeta(item: SaucenaoHistoryItem): string {
  // {n} 命名插值：英文键用管道复数（1 result / N results），中文日文无分支直接插值
  return `${t("saucenao.historyResults", { n: item.result_count })} · ${relativeTime(item.searched_at)}`;
}

/** 相对时间：分桶逻辑在 utils/relativeTime.ts（与浏览搜索历史共用），此处只解析标签。 */
function relativeTime(searchedAt: string): string {
  return formatRelativeTime(searchedAt, locale.value, {
    justNow: t("saucenao.historyJustNow"),
    minutesAgo: (n) => t("saucenao.historyMinutesAgo", { n }),
    hoursAgo: (n) => t("saucenao.historyHoursAgo", { n }),
    daysAgo: (n) => t("saucenao.historyDaysAgo", { n }),
  });
}

/** 点击历史条目 = 采纳该源并立即搜索（loading 期按钮已禁用，此处再防御）。 */
async function adoptHistory(item: SaucenaoHistoryItem): Promise<void> {
  if (phase.value === "loading") return;
  if (item.source_type === "file") {
    adoptFile(item.source);
  } else {
    // file 通道优先级高于 URL：采纳 url 历史前先清掉已选文件，避免填了不生效；
    // 同时切到 URL 模式，让源在界面上保持可见
    removeImage();
    inputMode.value = "url";
    urlInput.value = item.source;
  }
  await doSearch();
}

async function deleteHistory(item: SaucenaoHistoryItem): Promise<void> {
  try {
    await deleteSaucenaoHistory(item.id);
    historyItems.value = historyItems.value.filter((entry) => entry.id !== item.id);
  } catch (err) {
    notify(errorMessage(err));
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

/** 相似度进度条宽度：0-100 钳制（后端 parseFloat，防御越界值）。 */
function simPercent(value: number): number {
  return Math.min(100, Math.max(0, value));
}

function openResult(item: SaucenaoResult): void {
  if (!item.ext_url) return;
  void openInBrowser(item.ext_url).catch(() => notify(t("saucenao.openFailed")));
}

function openWork(item: SaucenaoResult): void {
  if (item.pixiv_id == null) return;
  void router.push(`/browse/work/illust/${item.pixiv_id}`);
}

/** 打开设置并直达「浏览与图片」分区、聚焦高亮 SauceNAO Key 字段。 */
function openSettingsDialog(): void {
  const detail: OpenSettingsDetail = { section: "images", focusField: SAUCENAO_KEY_FIELD_ID };
  window.dispatchEvent(new CustomEvent<OpenSettingsDetail>(OPEN_SETTINGS_EVENT, { detail }));
}

/** 系统浏览器打开 SauceNAO 获取 Key 页面。 */
function openGetApiKey(): void {
  void openInBrowser("https://saucenao.com/user.php?page=search-api").catch(() => notify(t("saucenao.openFailed")));
}

function onThumbError(index: number): void {
  brokenThumbs.value.add(index);
}
</script>

<template>
  <div class="page-view">
    <div class="page-heading saucenao-heading"><PageBackButton /><h1 class="page-title">{{ t("saucenao.title") }}</h1><HelpTooltip :label="t('saucenao.title')" :text="`${t('saucenao.subtitle')}\n\n${t('saucenao.idleHint')}`" /></div>

    <!-- 输入卡片 v2：卡片本体 = 拖拽热区（任何模式拖拽可用且优先），内部三态切换：
         空态（file 主通道主视觉）/ URL 次级模式 / 已采纳态（预览 + 搜索主 CTA） -->
    <div
      class="input-card"
      :class="{ 'is-empty': showEmptyZone, 'is-url': showUrlMode, 'is-adopted': adopted, 'drag-over': dragHover }"
      @click="onCardClick"
    >
      <!-- 空态：image-up 主视觉 + 选择文件主按钮 + URL 次级入口小字链接 -->
      <template v-if="showEmptyZone">
        <svg class="zone-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <!-- lucide image-up -->
          <path d="M10.3 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v10l-3.1-3.1a2 2 0 0 0-2.814.014L6 21" />
          <path d="m14 19.5 3-3 3 3" />
          <path d="M17 22v-5.5" />
          <circle cx="9" cy="9" r="2" />
        </svg>
        <span class="zone-hint">{{ t("saucenao.dragHint") }}</span>
        <md-filled-button class="zone-choose" :disabled="phase === 'loading'" @click.stop="chooseFile">
          {{ t("saucenao.chooseFile") }}
        </md-filled-button>
        <button type="button" class="mode-link" :disabled="phase === 'loading'" @click.stop="inputMode = 'url'">
          {{ t("saucenao.useImageUrl") }}
        </button>
      </template>

      <!-- URL 次级模式：输入框 + 搜索同行（回车触发），下方切回链接 + 拖拽提示 -->
      <template v-else-if="showUrlMode">
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
        <div class="url-mode-foot">
          <button type="button" class="mode-link" :disabled="phase === 'loading'" @click.stop="inputMode = 'file'">
            {{ t("saucenao.useLocalFile") }}
          </button>
          <span class="mode-hint">{{ t("saucenao.dropAlsoWorks") }}</span>
        </div>
      </template>

      <!-- 已采纳态：大图预览（clamp 高、contain）+ 移除钮右上常驻 + 48px 搜索主 CTA -->
      <template v-else>
        <div class="zone-preview">
          <img v-if="previewSrc" :src="previewSrc" class="zone-preview-img" :alt="fileName" />
          <!-- 浏览器 mock 环境：本地文件无可预览路径，仅显示文件名 -->
          <span v-else class="zone-filename" :title="fileName">{{ fileName }}</span>
          <button
            type="button"
            class="zone-remove"
            :aria-label="t('saucenao.removeImage')"
            :title="t('saucenao.removeImage')"
            :disabled="phase === 'loading'"
            @click.stop="removeImage"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <!-- lucide x -->
              <path d="M18 6 6 18" />
              <path d="m6 6 12 12" />
            </svg>
          </button>
        </div>
        <md-filled-button class="search-cta" :disabled="!canSearch" @click.stop="doSearch">
          <svg slot="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <!-- lucide search -->
            <circle cx="11" cy="11" r="8" />
            <path d="m21 21-4.3-4.3" />
          </svg>
          {{ t("common.search") }}
        </md-filled-button>
        <span class="mode-hint">{{ t("saucenao.dropToReplace") }}</span>
      </template>
    </div>

    <!-- 搜索历史：输入卡片之下、结果区之上；空列表不渲染整个区块。
         区标题留在限高容器外，滚动只卷列表本体 -->
    <section v-if="historyItems.length" class="history-section" :aria-label="t('saucenao.historyTitle')">
      <span class="history-title">{{ t("saucenao.historyTitle") }}</span>
      <div class="history-list">
        <div v-for="item in historyItems" :key="item.id" class="history-row">
          <button type="button" class="history-item" :disabled="phase === 'loading'" :title="item.label" @click="adoptHistory(item)">
            <span class="history-thumb" aria-hidden="true">
              <img
                v-if="!brokenHistoryThumbs.has(item.id) && historyThumbSrc(item)"
                class="history-thumb-img"
                :src="historyThumbSrc(item)"
                loading="lazy"
                referrerpolicy="no-referrer"
                alt=""
                @error="onHistoryThumbError(item.id)"
              />
              <span v-else class="history-thumb-placeholder">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <!-- lucide image -->
                  <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
                  <circle cx="9" cy="9" r="2" />
                  <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />
                </svg>
              </span>
            </span>
            <span class="history-main">
              <span class="history-label">{{ item.label }}</span>
              <span class="history-meta">{{ historyMeta(item) }}</span>
            </span>
          </button>
          <button
            type="button"
            class="history-remove"
            :aria-label="t('saucenao.historyDelete')"
            :title="t('saucenao.historyDelete')"
            :disabled="phase === 'loading'"
            @click.stop="deleteHistory(item)"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <!-- lucide x -->
              <path d="M18 6 6 18" />
              <path d="m6 6 12 12" />
            </svg>
          </button>
        </div>
      </div>
    </section>

    <!-- key 未配置引导条（不打请求） -->
    <div v-if="!hasKey" class="m3-alert warning guide-bar">
      <span class="guide-text">{{ t("saucenao.needKey") }}</span>
      <md-text-button @click="openGetApiKey">{{ t("saucenao.getApiKey") }}</md-text-button>
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
            <span class="sim-value">{{ item.similarity.toFixed(1) }}%</span>
            <span v-if="item.is_pixiv" class="source-chip">{{ sourceName(item) }}</span>
            <span v-else class="source-name" :title="item.index_name">{{ sourceName(item) }}</span>
          </div>
          <!-- 相似度可视化：数值为准、条作补充，对读屏隐藏 -->
          <div class="sim-track" aria-hidden="true">
            <div class="sim-fill" :style="{ width: `${simPercent(item.similarity)}%` }"></div>
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
.saucenao-heading { margin-bottom: var(--space-md); }

/* ===== 输入卡片 v2：卡片本体 = 拖拽热区，内部三态（空态 / URL 次级 / 已采纳）===== */

.input-card {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-sm);
  padding: var(--space-xl);
  /* 16px = DESIGN.md rounded.card（与 .form-card / .m3-card 同规格） */
  border-radius: 16px;
  background: var(--md-sys-color-surface-container);
  /* outline 常驻占位（透明），拖入高亮时不产生布局位移 */
  outline: 2px solid transparent;
  transition: outline-color 0.15s ease, background-color 0.15s ease;
}

/* 空态热区：整卡虚线边传达可拖放（outline 45% 虚线配方同小说正文未收录图），整卡可点 = 选择文件 */
.input-card.is-empty {
  cursor: pointer;
  border: 1px dashed color-mix(in srgb, var(--md-sys-color-outline) 45%, transparent);
}

.input-card.is-empty:hover {
  background: color-mix(in srgb, var(--md-sys-color-outline) 10%, var(--md-sys-color-surface-container));
}

/* 拖拽悬停（任何模式可用且优先）：primary 2px 描边 + 8% primary 淡染，整卡即视觉落点 */
.input-card.drag-over {
  outline-color: var(--md-sys-color-primary);
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

/* URL 次级模式：输入行 + 脚注占满卡片宽，左对齐 */
.input-card.is-url {
  align-items: stretch;
  gap: var(--space-md);
}

.zone-icon {
  width: 40px;
  height: 40px;
  color: var(--ink-subtle);
}

.zone-hint {
  font-size: 14px;
  color: var(--ink-muted);
}

/* 已采纳态预览容器：高 = clamp(240px, 40vh, 480px)（240px = 10 × space-xl，
   与旧热区等高；480px = 20 × space-xl），大图 contain 居中，移除钮浮于右上 */
.zone-preview {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: clamp(240px, 40vh, 480px);
}

.zone-preview-img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.zone-filename {
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
  color: var(--ink-muted);
}

/* 移除按钮：28px 快捷动作配方（与作品卡快捷动作同款，浮于预览之上） */
.zone-remove {
  position: absolute;
  top: var(--space-sm);
  right: var(--space-sm);
  display: flex;
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
  transition: background-color 0.15s ease;
}

.zone-remove:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.zone-remove:active {
  background: color-mix(in srgb, var(--md-sys-color-primary) 12%, var(--md-sys-color-surface-container));
}

.zone-remove:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.zone-remove:disabled {
  cursor: default;
}

.zone-remove svg {
  width: 16px;
  height: 16px;
}

/* 搜索主 CTA：48px（3 × space-lg）大号 filled（Material 官方容器高 token），
   预览正下方一眼可见；带 20px lucide search 图标 */
.search-cta {
  --md-filled-button-container-height: 48px;
  height: 48px;
  padding: 0 var(--space-xl);
}

.search-cta svg {
  width: 20px;
  height: 20px;
}

/* 模式切换小字链接：12px ink-muted 下划线，hover 提升为正文色 */
.mode-link {
  padding: 0;
  border: none;
  background: transparent;
  font: inherit;
  font-size: 12px;
  color: var(--ink-muted);
  text-decoration: underline;
  text-underline-offset: 2px;
  cursor: pointer;
  transition: color 0.15s ease;
}

.mode-link:hover {
  color: var(--ink);
}

.mode-link:disabled {
  cursor: default;
}

.mode-link:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

/* 非交互提示小字：12px ink-subtle（URL 模式拖拽提示 / 已采纳替换提示） */
.mode-hint {
  font-size: 12px;
  color: var(--ink-subtle);
}

/* URL 次级模式脚注：切回链接 + 拖拽提示横排 */
.url-mode-foot {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-xs) var(--space-md);
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

/* ===== 搜索历史：surface-container 圆角行（结果行同配方），缩略图更紧凑（48px = 4 × space-lg） ===== */

.history-section {
  margin-top: var(--space-lg);
}

/* 区标题与结果计数同层级：12px/600 ink-muted */
.history-title {
  display: block;
  margin-bottom: var(--space-sm);
  font-size: 12px;
  font-weight: 600;
  color: var(--ink-muted);
}

/* 列表限高内部滚动（与搜索页历史下拉同款额度）：后端历史上限 200 条，
   长列表不再把页面无限撑高；区标题留在容器外，滚动只卷列表本体 */
.history-list {
  max-height: min(60vh, 320px);
  overflow-y: auto;
}

.history-row {
  display: flex;
  align-items: center;
  gap: var(--space-xs);
  margin-top: var(--space-sm);
  padding: var(--space-xs) var(--space-md);
  border-radius: 16px;
  background: var(--md-sys-color-surface-container);
  transition: background-color 0.15s ease;
}

.history-row:hover,
.history-row:focus-within {
  background: color-mix(in srgb, var(--md-sys-color-outline) 8%, var(--md-sys-color-surface-container));
}

/* 行主体（缩略图 + 两行信息）是可点按钮；删除 X 为兄弟节点，不嵌套 */
.history-item {
  display: flex;
  flex: 1;
  align-items: center;
  gap: var(--space-md);
  min-width: 0;
  padding: var(--space-xs) 0;
  font: inherit;
  text-align: left;
  background: transparent;
  border: 0;
  border-radius: 12px;
  cursor: pointer;
}

.history-item:disabled {
  cursor: default;
}

.history-item:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

/* 缩略图：48px 方形（control 圆角 + outline 12% 衬底），衬底兼作透明 PNG 底色与失败占位 */
.history-thumb {
  display: flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 48px;
  height: 48px;
  border-radius: 12px;
  overflow: hidden;
  background: color-mix(in srgb, var(--md-sys-color-outline) 12%, var(--md-sys-color-surface-container));
}

.history-thumb-img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.history-thumb-placeholder {
  color: var(--ink-subtle);
}

.history-thumb-placeholder svg {
  width: 20px;
  height: 20px;
  stroke-width: 2;
}

.history-main {
  display: grid;
  flex: 1;
  gap: 2px;
  min-width: 0;
}

.history-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
}

.history-meta {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  color: var(--ink-muted);
}

/* 单条删除：28px 快捷动作配方（同拖拽热区移除钮），hover / focus-within 才显现 */
.history-remove {
  display: flex;
  flex: none;
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
  opacity: 0;
  transition: background-color 0.15s ease, opacity 0.15s ease;
}

.history-row:hover .history-remove,
.history-row:focus-within .history-remove {
  opacity: 1;
}

/* 触摸设备没有 hover：删除按钮常驻（同全屏看图控件惯例） */
@media (hover: none) {
  .history-remove {
    opacity: 1;
  }
}

.history-remove:hover {
  background: color-mix(in srgb, var(--md-sys-color-primary) 8%, var(--md-sys-color-surface-container));
}

.history-remove:active {
  background: color-mix(in srgb, var(--md-sys-color-primary) 12%, var(--md-sys-color-surface-container));
}

.history-remove:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: 2px;
}

.history-remove:disabled {
  cursor: default;
}

.history-remove svg {
  width: 16px;
  height: 16px;
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

.empty-state {
  margin: var(--space-lg) 0 0;
  color: var(--ink-muted);
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
  transition: background-color 0.15s ease;
}

.result-item:hover,
.result-item:focus-within {
  background: color-mix(in srgb, var(--md-sys-color-outline) 8%, var(--md-sys-color-surface-container));
}

/* 统一 96px（8 × space-lg）方形缩略图；衬底兼作透明 PNG 底色，与失败占位块同配方 */
.thumb {
  flex: none;
  width: 96px;
  height: 96px;
  border-radius: 12px;
  overflow: hidden;
  background: color-mix(in srgb, var(--md-sys-color-outline) 12%, var(--md-sys-color-surface-container));
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
  gap: var(--space-sm);
}

/* 相似度数值：13px/700 等宽数字（识别 badge 字号），为相似度的权威表达 */
.sim-value {
  flex: none;
  font-size: 13px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: var(--ink);
}

/* pixiv 来源徽标：primary-container 胶囊（与排行前三徽标同级强调），本行唯一胶囊 */
.source-chip {
  flex: none;
  padding: 1px 10px;
  border-radius: 999px;
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
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

/* 相似度进度条：6px（= space-xs）轨道 + primary 填充，轨道同滑杆 inactive 配方 */
.sim-track {
  height: 6px;
  margin-top: var(--space-xs);
  border-radius: 999px;
  overflow: hidden;
  background: color-mix(in srgb, var(--md-sys-color-outline) 30%, transparent);
}

.sim-fill {
  height: 100%;
  border-radius: 999px;
  background: var(--md-sys-color-primary);
}

.item-title {
  margin: var(--space-sm) 0 0;
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
  /* 窄窗口：卡片本体即热区天然纵向，搜索主 CTA 全宽易点；URL 行沿用既有 wrap */
  .search-cta {
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
