<template>
  <div class="pixiv-view">
    <!-- 顶部工具栏 -->
    <div class="browse-toolbar">
      <div class="toolbar-nav">
        <n-tooltip trigger="hover">
          <template #trigger>
            <n-button
              quaternary
              circle
              size="small"
              aria-label="Home"
              @click="handleHome"
            >
              <template #icon>
                <svg
                  class="toolbar-icon"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  aria-hidden="true"
                >
                  <path d="m3 9 9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
                  <polyline points="9 22 9 12 15 12 15 22" />
                </svg>
              </template>
            </n-button>
          </template>
          {{ t('pixiv.home') }}
        </n-tooltip>

        <n-tooltip trigger="hover">
          <template #trigger>
            <n-button
              quaternary
              circle
              size="small"
              aria-label="Reload"
              @click="handleReload"
            >
              <template #icon>
                <svg
                  class="toolbar-icon"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  aria-hidden="true"
                >
                  <path d="M21.5 2v6h-6M2.5 22v-6h6M2 11.5a10 10 0 0 1 18.8-4.3M22 12.5a10 10 0 0 1-18.8 4.2" />
                </svg>
              </template>
            </n-button>
          </template>
          {{ t('pixiv.reload') }}
        </n-tooltip>
      </div>

      <div class="toolbar-url" :title="currentUrl || BROWSE_HOME">
        <span class="url-text">{{ currentUrl || BROWSE_HOME }}</span>
      </div>

      <div class="toolbar-actions">
        <n-button size="small" :loading="syncingLogin" @click="handleSyncLogin">
          {{ t('pixiv.syncLogin') }}
        </n-button>
      </div>
    </div>

    <!-- 抓取联动栏（识别到小说/插画/用户主页时展示） -->
    <div v-if="page" class="browse-crawl-bar">
      <div class="crawl-bar-info">
        <span class="detected-badge">
          <svg
            class="badge-icon"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
            <polyline points="22 4 12 14.01 9 11.01" />
          </svg>
          {{ detectedLabel }}
        </span>

        <!-- 小说单篇 / 系列：格式选择器 -->
        <n-checkbox-group
          v-if="page.kind === 'novel-single' || page.kind === 'novel-series'"
          v-model:value="selectedFormats"
          class="crawl-bar-formats"
        >
          <n-checkbox value="txt" size="small">TXT</n-checkbox>
          <n-checkbox value="markdown" size="small">Markdown</n-checkbox>
        </n-checkbox-group>
      </div>

      <div class="crawl-bar-actions">
        <!-- 小说单篇 / 系列 -->
        <template v-if="page.kind === 'novel-single' || page.kind === 'novel-series'">
          <n-button
            type="primary"
            size="small"
            :loading="submitting"
            @click="handleCrawlNovel"
          >
            {{ t('pixiv.crawlNow') }}
          </n-button>
          <n-button size="small" @click="handleFillForm">
            {{ t('pixiv.fillForm') }}
          </n-button>
        </template>

        <!-- 用户主页 -->
        <template v-else-if="page.kind === 'user'">
          <n-button
            type="primary"
            size="small"
            :loading="submitting"
            @click="handleCrawlUserNovels"
          >
            {{ t('pixiv.crawlUserNovels') }}
          </n-button>
          <n-button
            type="primary"
            size="small"
            :loading="submitting"
            @click="handleCrawlUserIllustrations"
          >
            {{ t('pixiv.crawlUserIllustrations') }}
          </n-button>
          <n-button size="small" @click="handleFillForm">
            {{ t('pixiv.fillForm') }}
          </n-button>
        </template>

        <!-- 插画作品 -->
        <template v-else-if="page.kind === 'illustration'">
          <n-button
            type="primary"
            size="small"
            :loading="submitting"
            @click="handleCrawlIllustration"
          >
            {{ t('pixiv.crawlNow') }}
          </n-button>
          <n-button size="small" @click="handleFillIllustrationForm">
            {{ t('pixiv.fillIllustForm') }}
          </n-button>
        </template>
      </div>
    </div>

    <!-- 结果反馈提示（页内局部反馈，不使用全局 n-message，避免被原生 webview 遮挡） -->
    <div v-if="alertMessage" class="browse-alert-container">
      <n-alert
        :type="alertType"
        closable
        size="small"
        @close="alertMessage = ''"
      >
        {{ alertMessage }}
      </n-alert>
    </div>

    <!-- 原生子 Webview 宿主占位容器 -->
    <div ref="hostEl" class="browse-host">
      <div v-if="!currentUrl" class="loading-state">
        <n-spin size="medium" />
        <span class="loading-text">{{ t('pixiv.loading') }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from "vue";
import { useRouter } from "vue-router";
import { NButton, NTooltip, NSpin, NCheckboxGroup, NCheckbox, NAlert } from "naive-ui";
import { useI18n } from "vue-i18n";
import {
  invoke,
  listen,
  isTauri,
  errorMessage,
  type UnlistenFn,
  type BrowseSyncLoginResponse,
} from "../api/tauri";
import { useAuthStore } from "../stores/auth";
import { useTaskStore } from "../stores/tasks";
import { parsePixivUrl } from "../utils/pixivUrl";

const BROWSE_HOME = "https://www.pixiv.net/";

const { t } = useI18n();
const router = useRouter();
const authStore = useAuthStore();
const taskStore = useTaskStore();
const hostEl = ref<HTMLElement | null>(null);
const currentUrl = ref("");

// 页面场景识别
const page = computed(() => parsePixivUrl(currentUrl.value));

// 识别展示文案
const detectedLabel = computed(() => {
  if (!page.value) return "";
  switch (page.value.kind) {
    case "novel-single":
      return t("pixiv.detected.novelSingle", { id: page.value.id });
    case "novel-series":
      return t("pixiv.detected.novelSeries", { id: page.value.id });
    case "user":
      return t("pixiv.detected.user", { id: page.value.id });
    case "illustration":
      return t("pixiv.detected.illustration", { id: page.value.id });
  }
});

// 格式选择与操作状态
const selectedFormats = ref(["txt", "markdown"]);
const submitting = ref(false);
const syncingLogin = ref(false);
const alertMessage = ref("");
const alertType = ref<"success" | "error" | "info" | "warning">("info");
let unlisten: UnlistenFn | null = null;
let resizeObserver: ResizeObserver | null = null;

function syncBounds() {
  if (!hostEl.value) return;
  const r = hostEl.value.getBoundingClientRect();
  if (r.width <= 0 || r.height <= 0) return;
  invoke("browse_set_bounds", {
    x: r.left,
    y: r.top,
    w: r.width,
    h: r.height,
  }).catch(() => {});
}

function handleHome() {
  invoke("browse_navigate", { url: BROWSE_HOME }).catch(() => {});
}

function handleReload() {
  invoke("browse_navigate", { url: currentUrl.value || BROWSE_HOME }).catch(() => {});
}


async function handleSyncLogin() {
  syncingLogin.value = true;
  alertMessage.value = "";
  try {
    const res = await invoke<BrowseSyncLoginResponse>("browse_sync_login");
    if (res.status === "success") {
      await authStore.checkStatus();
      alertMessage.value = t("pixiv.syncLoginSuccess");
      alertType.value = "success";
    } else if (res.status === "no_session") {
      alertMessage.value = t("pixiv.syncLoginNoSession");
      alertType.value = "warning";
    } else if (res.status === "invalid") {
      alertMessage.value = res.message || t("pixiv.syncLoginInvalid");
      alertType.value = "error";
    } else {
      alertMessage.value = res.message || t("pixiv.syncLoginError");
      alertType.value = "error";
    }
  } catch (err: unknown) {
    alertMessage.value = errorMessage(err) || t("pixiv.syncLoginError");
    alertType.value = "error";
  } finally {
    syncingLogin.value = false;
  }
}
async function executeCreateTask(
  sourceType: string,
  sourceId: string,
  formats: string[],
  category: "novel" | "illustration"
) {
  submitting.value = true;
  alertMessage.value = "";
  try {
    const result = await taskStore.createTask(sourceType, sourceId, formats, category);
    if (result.error) {
      alertMessage.value = result.error;
      alertType.value = "error";
    } else {
      alertMessage.value = t("pixiv.taskCreated", { id: result.task_id });
      alertType.value = "success";
    }
  } catch (err: unknown) {
    alertMessage.value = errorMessage(err) || t("pixiv.createFailed");
    alertType.value = "error";
  } finally {
    submitting.value = false;
  }
}

async function handleCrawlNovel() {
  if (!page.value) return;
  const sourceType = page.value.kind === "novel-single" ? "single" : "series";
  await executeCreateTask(sourceType, page.value.id, selectedFormats.value, "novel");
}

async function handleCrawlUserNovels() {
  if (!page.value) return;
  await executeCreateTask("user", page.value.id, ["txt", "markdown"], "novel");
}

async function handleCrawlUserIllustrations() {
  if (!page.value) return;
  await executeCreateTask("user", page.value.id, [], "illustration");
}

async function handleCrawlIllustration() {
  if (!page.value) return;
  await executeCreateTask("single", page.value.id, [], "illustration");
}

function handleFillForm() {
  if (!page.value) return;
  const sourceType =
    page.value.kind === "novel-single"
      ? "single"
      : page.value.kind === "novel-series"
      ? "series"
      : "user";
  router.push({
    path: "/",
    query: { sourceType, sourceId: page.value.id },
  });
}

function handleFillIllustrationForm() {
  if (!page.value) return;
  const sourceType = page.value.kind === "user" ? "user" : "single";
  router.push({
    path: "/illustration",
    query: { sourceType, sourceId: page.value.id },
  });
}

// 联动栏或提示框出现/消失时同步子 webview 边界
watch([page, alertMessage], async () => {
  await nextTick();
  syncBounds();
});

onMounted(async () => {
  await invoke("browse_open").catch(() => {});
  await nextTick();
  syncBounds();

  if (isTauri()) {
    unlisten = await listen<{ url: string }>("browse://url-changed", (e) => {
      currentUrl.value = e.payload.url;
    });
  }

  if (hostEl.value) {
    resizeObserver = new ResizeObserver(() => {
      syncBounds();
    });
    resizeObserver.observe(hostEl.value);
  }
  window.addEventListener("resize", syncBounds);
});

onBeforeUnmount(() => {
  if (unlisten) {
    unlisten();
    unlisten = null;
  }
  if (resizeObserver) {
    resizeObserver.disconnect();
    resizeObserver = null;
  }
  window.removeEventListener("resize", syncBounds);
  invoke("browse_hide").catch(() => {});
});
</script>

<style scoped>
.pixiv-view {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.browse-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 16px;
  border-bottom: 1px solid rgba(128, 128, 128, 0.15);
  background: var(--n-color, #ffffff);
  flex-shrink: 0;
}

.toolbar-nav {
  display: flex;
  align-items: center;
  gap: 4px;
}

.toolbar-icon {
  width: 16px;
  height: 16px;
  stroke-width: 1.8;
}

.toolbar-url {
  flex: 1;
  min-width: 0;
  background: rgba(128, 128, 128, 0.08);
  border-radius: 4px;
  padding: 4px 10px;
  font-size: 13px;
  color: #666;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.url-text {
  user-select: text;
}

.toolbar-actions {
  display: flex;
  align-items: center;
  flex-shrink: 0;
}

.browse-crawl-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 6px 16px;
  background: var(--n-color, #ffffff);
  border-bottom: 1px solid rgba(128, 128, 128, 0.15);
  font-size: 13px;
  flex-shrink: 0;
  flex-wrap: wrap;
}

.crawl-bar-info {
  display: flex;
  align-items: center;
  gap: 12px;
}

.detected-badge {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-weight: 500;
  font-size: 13px;
  color: var(--n-text-color, #333333);
}

.badge-icon {
  width: 14px;
  height: 14px;
  stroke: #18a058;
  stroke-width: 2.2;
}

.crawl-bar-formats {
  display: inline-flex;
  align-items: center;
}

.crawl-bar-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.browse-alert-container {
  padding: 6px 16px;
  background: var(--n-color, #ffffff);
  border-bottom: 1px solid rgba(128, 128, 128, 0.15);
  flex-shrink: 0;
}

.browse-host {
  flex: 1;
  min-height: 0;
  position: relative;
  width: 100%;
}

.loading-state {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: #888;
  font-size: 14px;
}
</style>
