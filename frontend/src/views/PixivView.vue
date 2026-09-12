<template>
  <div class="pixiv-view">
    <div class="browse-toolbar">
      <div class="toolbar-nav">
        <md-icon-button :aria-label="t('pixiv.back')" :title="t('pixiv.back')" @click="handleBack"><svg class="toolbar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="15 18 9 12 15 6" /></svg></md-icon-button>
        <md-icon-button :aria-label="t('pixiv.home')" :title="t('pixiv.home')" @click="handleHome"><svg class="toolbar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m3 9 9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /><polyline points="9 22 9 12 15 12 15 22" /></svg></md-icon-button>
        <md-icon-button :aria-label="t('pixiv.reload')" :title="t('pixiv.reload')" @click="handleReload"><svg class="toolbar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21.5 2v6h-6M2.5 22v-6h6M2 11.5a10 10 0 0 1 18.8-4.3M22 12.5a10 10 0 0 1-18.8 4.2" /></svg></md-icon-button>
      </div>
      <div class="toolbar-url" :title="currentUrl || BROWSE_HOME"><span class="url-text">{{ currentUrl || BROWSE_HOME }}</span></div>
      <div class="toolbar-actions"><md-outlined-button :disabled="syncingLogin" :aria-busy="syncingLogin" @click="handleSyncLogin">{{ t('pixiv.syncLogin') }}</md-outlined-button></div>
    </div>
    <div v-if="page" class="browse-crawl-bar">
      <div class="crawl-bar-info">
        <span class="detected-badge"><svg class="badge-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" /><polyline points="22 4 12 14.01 9 11.01" /></svg>{{ detectedLabel }}</span>
        <div v-if="page.kind === 'novel-single' || page.kind === 'novel-series'" class="crawl-bar-formats"><label v-for="format in formats" :key="format" class="m3-choice"><md-checkbox :checked="selectedFormats.includes(format)" @change="toggleFormat(format, ($event.target as HTMLInputElement).checked)" />{{ format === 'txt' ? 'TXT' : 'Markdown' }}</label></div>
      </div>
      <div class="crawl-bar-actions">
        <template v-if="page.kind === 'novel-single' || page.kind === 'novel-series'"><md-filled-button :disabled="submitting" :aria-busy="submitting" @click="handleCrawlNovel">{{ t('pixiv.crawlNow') }}</md-filled-button><md-outlined-button @click="handleFillForm">{{ t('pixiv.fillForm') }}</md-outlined-button></template>
        <template v-else-if="page.kind === 'user'"><md-filled-button :disabled="submitting" :aria-busy="submitting" @click="handleCrawlUserNovels">{{ t('pixiv.crawlUserNovels') }}</md-filled-button><md-filled-button :disabled="submitting" :aria-busy="submitting" @click="handleCrawlUserIllustrations">{{ t('pixiv.crawlUserIllustrations') }}</md-filled-button><md-outlined-button @click="handleFillForm">{{ t('pixiv.fillForm') }}</md-outlined-button></template>
        <template v-else-if="page.kind === 'illustration'"><md-filled-button :disabled="submitting" :aria-busy="submitting" @click="handleCrawlIllustration">{{ t('pixiv.crawlNow') }}</md-filled-button><md-outlined-button @click="handleFillIllustrationForm">{{ t('pixiv.fillIllustForm') }}</md-outlined-button></template>
      </div>
    </div>
    <div v-if="alertMessage" class="browse-alert-container"><div class="m3-alert" :class="alertType" role="alert"><span>{{ alertMessage }}</span><md-icon-button aria-label="Close" @click="alertMessage = ''"><svg class="close-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg></md-icon-button></div></div>
    <div ref="hostEl" class="browse-host"></div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { invoke, listen, isTauri, errorMessage, type BrowseSyncLoginResponse, type UnlistenFn } from "../api/tauri";
import { useAuthStore } from "../stores/auth";
import { useTaskStore } from "../stores/tasks";
import { parsePixivUrl } from "../utils/pixivUrl";
const BROWSE_HOME = "https://www.pixiv.net/";
const formats = ["txt", "markdown"];
const { t } = useI18n(); const router = useRouter(); const authStore = useAuthStore(); const taskStore = useTaskStore();
const hostEl = ref<HTMLElement | null>(null); const currentUrl = ref(BROWSE_HOME); const page = computed(() => parsePixivUrl(currentUrl.value));
const detectedLabel = computed(() => { if (!page.value) return ""; switch (page.value.kind) { case "novel-single": return t("pixiv.detected.novelSingle", { id: page.value.id }); case "novel-series": return t("pixiv.detected.novelSeries", { id: page.value.id }); case "user": return t("pixiv.detected.user", { id: page.value.id }); case "illustration": return t("pixiv.detected.illustration", { id: page.value.id }); } });
const selectedFormats = ref(["txt", "markdown"]); const submitting = ref(false); const syncingLogin = ref(false); const alertMessage = ref(""); const alertType = ref<"success" | "error" | "info" | "warning">("info"); let unlisten: UnlistenFn | null = null; let resizeObserver: ResizeObserver | null = null;
function toggleFormat(format: string, checked: boolean) { selectedFormats.value = checked ? [...selectedFormats.value, format] : selectedFormats.value.filter((item) => item !== format); }
function syncBounds() { if (!hostEl.value) return; const rect = hostEl.value.getBoundingClientRect(); const x = rect.left >= 0 ? rect.left : 180; const y = rect.top >= 0 ? rect.top : 42; const w = rect.width > 0 ? rect.width : window.innerWidth - x; const h = rect.height > 0 ? rect.height : window.innerHeight - y; if (w > 0 && h > 0) invoke("browse_set_bounds", { x, y, w, h }).catch((err) => console.error("browse_set_bounds error:", err)); }
function handleHome() { invoke("browse_navigate", { url: BROWSE_HOME }).catch(() => {}); } function handleReload() { invoke("browse_navigate", { url: currentUrl.value || BROWSE_HOME }).catch(() => {}); } function handleBack() { invoke("browse_go_back").catch(() => {}); }
async function handleSyncLogin() { syncingLogin.value = true; alertMessage.value = ""; try { const res = await invoke<BrowseSyncLoginResponse>("browse_sync_login"); if (res.status === "success") { await authStore.checkStatus(); alertMessage.value = t("pixiv.syncLoginSuccess"); alertType.value = "success"; } else if (res.status === "injected") { alertMessage.value = t("pixiv.syncLoginInjected"); alertType.value = "success"; } else if (res.status === "no_session") { alertMessage.value = t("pixiv.syncLoginNoSession"); alertType.value = "warning"; } else { alertMessage.value = res.message || (res.status === "invalid" ? t("pixiv.syncLoginInvalid") : t("pixiv.syncLoginError")); alertType.value = "error"; } } catch (err) { alertMessage.value = errorMessage(err) || t("pixiv.syncLoginError"); alertType.value = "error"; } finally { syncingLogin.value = false; } }
async function executeCreateTask(sourceType: string, sourceId: string, taskFormats: string[], category: "novel" | "illustration") { submitting.value = true; alertMessage.value = ""; try { const result = await taskStore.createTask(sourceType, sourceId, taskFormats, category); alertMessage.value = result.error || t("pixiv.taskCreated", { id: result.task_id }); alertType.value = result.error ? "error" : "success"; } catch (err) { alertMessage.value = errorMessage(err) || t("pixiv.createFailed"); alertType.value = "error"; } finally { submitting.value = false; } }
async function handleCrawlNovel() { if (page.value) await executeCreateTask(page.value.kind === "novel-single" ? "single" : "series", page.value.id, selectedFormats.value, "novel"); } async function handleCrawlUserNovels() { if (page.value) await executeCreateTask("user", page.value.id, ["txt", "markdown"], "novel"); } async function handleCrawlUserIllustrations() { if (page.value) await executeCreateTask("user", page.value.id, [], "illustration"); } async function handleCrawlIllustration() { if (page.value) await executeCreateTask("single", page.value.id, [], "illustration"); }
function handleFillForm() { if (!page.value) return; router.push({ path: "/", query: { sourceType: page.value.kind === "novel-single" ? "single" : page.value.kind === "novel-series" ? "series" : "user", sourceId: page.value.id } }); } function handleFillIllustrationForm() { if (page.value) router.push({ path: "/illustration", query: { sourceType: page.value.kind === "user" ? "user" : "single", sourceId: page.value.id } }); }
watch([page, alertMessage], async () => { await nextTick(); syncBounds(); }); onMounted(async () => { const rect = hostEl.value?.getBoundingClientRect(); await invoke("browse_open", { x: rect?.left || 180, y: rect?.top || 42, w: rect?.width || window.innerWidth - 180, h: rect?.height || window.innerHeight - 42 }).catch(() => {}); await nextTick(); syncBounds(); if (isTauri()) unlisten = await listen<{ url: string }>("browse://url-changed", (event) => { currentUrl.value = event.payload.url; }); if (hostEl.value) { resizeObserver = new ResizeObserver(syncBounds); resizeObserver.observe(hostEl.value); } window.addEventListener("resize", syncBounds); }); onBeforeUnmount(() => { unlisten?.(); resizeObserver?.disconnect(); window.removeEventListener("resize", syncBounds); invoke("browse_deactivate").catch(() => {}); });
</script>

<style scoped>
.pixiv-view { display: flex; width: 100%; height: 100%; flex-direction: column; overflow: hidden; background: var(--surface); }
.browse-toolbar, .browse-crawl-bar { display: flex; align-items: center; gap: var(--space-sm); border-bottom: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 30%, transparent); background: var(--surface); flex-shrink: 0; }
.browse-toolbar { height: 56px; padding: var(--space-xs) var(--space-md); }.toolbar-nav, .toolbar-actions, .crawl-bar-info, .crawl-bar-actions, .crawl-bar-formats { display: flex; align-items: center; gap: var(--space-sm); }.toolbar-icon { width: 20px; height: 20px; stroke-width: 1.8; }
.toolbar-url { flex: 1; min-width: 0; padding: 8px 12px; border-radius: 8px; background: var(--md-sys-color-surface-container); color: var(--ink-muted); font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.url-text { user-select: text; }
.browse-crawl-bar { justify-content: space-between; padding: var(--space-sm) var(--space-lg); flex-wrap: wrap; }.detected-badge { display: inline-flex; align-items: center; gap: var(--space-xs); color: var(--ink); font-size: 13px; font-weight: 500; }.badge-icon { width: 16px; height: 16px; stroke: #146c2e; stroke-width: 2.2; }
.browse-alert-container { padding: var(--space-sm) var(--space-lg) 0; background: var(--surface); flex-shrink: 0; }.browse-alert-container .m3-alert { display: flex; align-items: center; justify-content: space-between; margin-top: 0; }.close-icon { width: 18px; height: 18px; stroke-width: 2; }.browse-host { position: relative; width: 100%; min-height: 0; height: 100%; flex: 1; }
@media (max-width: 640px) { .browse-toolbar { height: auto; flex-wrap: wrap; }.toolbar-url { order: 3; flex-basis: 100%; }.browse-crawl-bar { align-items: flex-start; flex-direction: column; }.crawl-bar-actions { flex-wrap: wrap; } }
</style>
