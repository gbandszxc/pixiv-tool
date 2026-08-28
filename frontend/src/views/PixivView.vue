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
        <!-- Phase 3 接同步逻辑，本阶段 disabled 占位 -->
        <n-button size="small" disabled>
          {{ t('pixiv.syncLogin') }}
        </n-button>
      </div>
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
import { ref, onMounted, onBeforeUnmount, nextTick } from "vue";
import { NButton, NTooltip, NSpin } from "naive-ui";
import { useI18n } from "vue-i18n";
import { invoke, listen, isTauri, type UnlistenFn } from "../api/tauri";

const BROWSE_HOME = "https://www.pixiv.net/";

const { t } = useI18n();

const hostEl = ref<HTMLElement | null>(null);
const currentUrl = ref("");

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
