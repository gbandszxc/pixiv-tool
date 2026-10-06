<template>
  <div class="app-shell" :class="{ collapsed: siderCollapsed, fullscreen: imageFullscreen }">
    <aside class="sidebar">
      <header class="sider-header">
        <template v-if="!siderCollapsed"><img src="./assets/icon.png" alt="" /><span class="sider-title">pixiv-tool</span></template>
        <md-icon-button class="collapse" :aria-label="siderCollapsed ? t('nav.expandSidebar') : t('nav.collapseSidebar')" :title="siderCollapsed ? t('nav.expandSidebar') : t('nav.collapseSidebar')" @click="siderCollapsed = !siderCollapsed"><svg v-if="siderCollapsed" class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="18" height="18" x="3" y="3" rx="2" /><path d="M9 3v18" /><path d="m14 9 3 3-3 3" /></svg><svg v-else class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="18" height="18" x="3" y="3" rx="2" /><path d="M9 3v18" /><path d="m16 15-3-3 3-3" /></svg></md-icon-button>
      </header>
      <nav aria-label="Primary">
        <button v-for="item in menuItems" :key="item.path" class="nav-item" :class="{ active: activeGroup === item.group }" :aria-label="item.label" :title="siderCollapsed ? item.label : undefined" :aria-current="activeGroup === item.group ? 'page' : undefined" @click="navigate(item.path)"><SidebarIcon :name="item.icon" /><span v-if="!siderCollapsed" class="nav-label">{{ item.label }}</span><span v-if="item.group === 'downloads' && taskStore.activeTasks.length" class="task-count">{{ taskStore.activeTasks.length }}</span></button>
      </nav>
      <footer class="sider-footer">
        <AccountMenu :collapsed="siderCollapsed" @add-account="showLoginDialog = true" @open-settings="openSettings" />
      </footer>
    </aside>
    <section ref="workspaceEl" class="workspace" :class="{ 'panel-open': panel.visible }">
    <div class="workspace-body">
    <div class="main-pane">
    <main ref="contentEl" class="app-content" :class="{ 'detail-page': route.path.startsWith('/browse/work/'), 'series-page': route.name === 'browse-series' }" tabindex="-1">
      <router-view v-slot="{ Component, route: pageRoute }">
        <KeepAlive :key="browseSession" :include="cachedBrowseViews" :max="20">
          <component :is="Component" :key="pageRoute.path.startsWith('/browse/') ? pageRoute.path : undefined" />
        </KeepAlive>
      </router-view>
    </main>
    <md-filled-tonal-icon-button v-if="route.path !== '/browse/search' && !route.path.startsWith('/browse/work/novel/')" class="search-fab" aria-keyshortcuts="Control+K Meta+K" :aria-label="t('common.search')" :title="t('workspace.searchShortcut')" @click="openSearch"><SidebarIcon name="search" /></md-filled-tonal-icon-button>
    </div>
    <DownloadPanel />
    </div>
    <DownloadStatusBar />
    </section>
  </div>
  <dialog ref="exitDialog" class="m3-dialog exit-dialog" @close="showExitConfirm = false"><h2>{{ t('app.exitConfirmTitle') }}</h2><p>{{ t('app.exitConfirmBody') }}</p><div class="m3-row dialog-actions"><md-text-button @click="showExitConfirm = false">{{ t('common.cancel') }}</md-text-button><md-filled-button @click="invoke('app_exit').catch(() => {})">{{ t('app.exit') }}</md-filled-button></div></dialog>
  <LoginDialog v-model:show="showLoginDialog" />
  <SettingsDialog v-model:show="showSettings" />
  <div v-if="notification" class="m3-snackbar" role="status">{{ notification }}</div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter, useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { listen } from "@tauri-apps/api/event";
import LoginDialog from "./components/auth/LoginDialog.vue";
import AccountMenu from "./components/auth/AccountMenu.vue";
import SettingsDialog from "./components/settings/SettingsDialog.vue";
import SidebarIcon, { type SidebarIconName } from "./components/navigation/SidebarIcon.vue";
import { useAuthStore } from "./stores/auth";
import { useSettingsStore } from "./stores/settings";
import { invoke, isTauri, setWindowTheme } from "./api/tauri";
import { OPEN_LOGIN_EVENT } from "./api/browse";
import { OPEN_SETTINGS_EVENT } from "./api/saucenao";
import DownloadPanel from "./components/download/DownloadPanel.vue";
import DownloadStatusBar from "./components/download/DownloadStatusBar.vue";
import { useDownloadPanelStore } from "./stores/downloadPanel";
import { useTaskStore } from "./stores/tasks";
import { groupRoots, navigationGroup, type NavigationGroup } from "./router/navigation";
import { usePageScroll } from "./composables/usePageScroll";
import { resetChannelManual } from "./components/browse/r18Filter";

const router = useRouter(); const route = useRoute(); const { t } = useI18n();
const authStore = useAuthStore(); const settingsStore = useSettingsStore();
const authReady = authStore.checkStatus();
const contentEl = ref<HTMLElement>(); const workspaceEl = ref<HTMLElement>();
usePageScroll(contentEl);
const panel = useDownloadPanelStore(); const taskStore = useTaskStore();
const imageFullscreen = ref(false);
const activeGroup = ref<NavigationGroup>(navigationGroup(route.path) ?? "discover");
let stopTasks: (() => void) | undefined;
function navigate(path: string) { if (route.path !== path) void router.push(path); }
async function openSearch() {
  if (document.querySelector('dialog[open]') || imageFullscreen.value) return;
  if (route.path !== '/browse/search') await router.push('/browse/search');
  await focusSearch();
}
async function focusSearch() {
  await nextTick();
  const field = contentEl.value?.querySelector<HTMLElement & { updateComplete?: Promise<boolean> }>('.search-field');
  await field?.updateComplete;
  if (route.path === '/browse/search') field?.focus();
}
function onWorkspaceKey(event: KeyboardEvent) {
  if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 'k') { event.preventDefault(); if (!event.repeat) void openSearch(); }
  if (event.key === 'Escape' && panel.visible && !document.querySelector('dialog[open]')) { event.preventDefault(); panel.close(); }
}
function onImageFullscreen(event: Event) { imageFullscreen.value = (event as CustomEvent<boolean>).detail; }
// 布局伸缩以当前可见作品卡为锚，避免网格换列后跳到别的作品。
let layoutAnchor: { element: HTMLElement; offset: number } | undefined;
let layoutTop = 0;
function captureLayout() {
  const main = contentEl.value; if (!main) return;
  layoutTop = main.scrollTop;
  const top = main.getBoundingClientRect().top;
  layoutAnchor = undefined;
  for (const card of main.querySelectorAll<HTMLElement>('.work-card')) {
    const rect = card.getBoundingClientRect();
    if (rect.bottom > top) { layoutAnchor = { element: card, offset: rect.top - top }; break; }
  }
}
function restoreLayout() {
  const main = contentEl.value; if (!main) return;
  if (layoutAnchor?.element.isConnected) main.scrollTop += layoutAnchor.element.getBoundingClientRect().top - main.getBoundingClientRect().top - layoutAnchor.offset;
  else main.scrollTop = layoutTop;
}

const cachedBrowseViews = [
  "BrowseHomeView", "BrowseChannelView", "BrowseDiscoverView", "BrowseFeedView",
  "BrowseWatchlistView", "BrowseSearchView", "BrowseRankingView", "BrowseBookmarkView",
  "BrowseHistoryView", "BrowseMeView", "BrowseAuthorView", "BrowseSeriesView", "BrowseWorkView",
];
const browseSession = computed(() => `${authStore.isLoggedIn}:${authStore.userId}`);
// 与 KeepAlive 相同的 20 页 LRU 边界；主内容滚动不在 window 上。
const browseScroll = new Map<string, number>();
/** 缓存集内的页面返回时按 path 恢复主内容滚动。插画/漫画详情（BrowseWorkView）同样入缓存：
 * 作者页与相关推荐的往返复用已加载的舞台，不再重新加载；小说阅读器自持阅读位置与翻译状态，
 * 暂不纳入。 */
function isCachedBrowsePage(path: string): boolean {
  if (path.startsWith("/browse/work/")) return /^\/browse\/work\/(illust|manga)\//.test(path);
  return path.startsWith("/browse/");
}
const removeBeforeEach = router.beforeEach((to, from) => {
  if (to.path !== from.path && isCachedBrowsePage(from.path)) {
    browseScroll.set(from.path, contentEl.value?.scrollTop ?? 0);
  }
});
const removeAfterEach = router.afterEach(async (to, from, failure) => {
  if (failure) return;
  const entryGroup = router.options.history.state.navigationGroup;
  const inherited = typeof entryGroup === 'string' && entryGroup in groupRoots ? entryGroup as NavigationGroup : activeGroup.value;
  activeGroup.value = navigationGroup(to.path) ?? inherited;
  router.options.history.replace(to.fullPath, { navigationGroup: activeGroup.value });
  if (to.fullPath !== from.fullPath && !from.query.downloadForm) panel.close(false);
  if (to.query.downloadForm === 'novel' || to.query.downloadForm === 'illustration') {
    // 首次凭据探测会清空会话草稿；等探测完成后再消费旧链接，避免返填被抹掉。
    await authReady;
    await nextTick();
    if (route.fullPath !== to.fullPath) return;
    const sourceType = typeof to.query.sourceType === 'string' && ['single','series','user'].includes(to.query.sourceType) ? to.query.sourceType as 'single' | 'series' | 'user' : 'single';
    panel.open({ form: to.query.downloadForm, sourceType, sourceId: typeof to.query.sourceId === 'string' ? to.query.sourceId : '' });
    const query = { ...to.query }; delete query.downloadForm; delete query.sourceType; delete query.sourceId;
    await router.replace({ path: to.path, query, hash: to.hash });
  }
  if (to.path === from.path) return;
  const top = browseScroll.get(to.path) ?? 0;
  if (isCachedBrowsePage(to.path)) {
    browseScroll.delete(to.path);
    browseScroll.set(to.path, top);
    if (browseScroll.size > 20) browseScroll.delete(browseScroll.keys().next().value!);
  }
  await nextTick();
  if (route.path === to.path) contentEl.value?.scrollTo({ top, behavior: "instant" });
  if (to.path === '/browse/search') await focusSearch();
});
watch(browseSession, () => {
  browseScroll.clear(); panel.reset(); resetChannelManual();
  if (isCachedBrowsePage(route.path)) browseScroll.set(route.path, 0);
  contentEl.value?.scrollTo({ top: 0, behavior: "instant" });
});
onBeforeUnmount(() => { removeBeforeEach(); removeAfterEach(); });
const showLoginDialog = ref(false); const showSettings = ref(false); const showExitConfirm = ref(false); const exitDialog = ref<HTMLDialogElement>();
const siderCollapsed = ref(false); const notification = ref(""); let toastTimer: number | undefined; let unlistenExit: (() => void) | undefined;
let disposed = false;
interface MenuItem { path: string; label: string; icon: SidebarIconName; group: NavigationGroup }
const menuItems = computed<MenuItem[]>(() => [
  { path: groupRoots.discover, label: t("workspace.discover"), icon: "discover", group: "discover" },
  { path: groupRoots.following, label: t("workspace.following"), icon: "feed", group: "following" },
  { path: groupRoots.library, label: t("workspace.library"), icon: "bookmark", group: "library" },
  { path: groupRoots.downloads, label: t("workspace.downloads"), icon: "tasks", group: "downloads" },
]);
watch([() => panel.visible, siderCollapsed], async () => { captureLayout(); await nextTick(); restoreLayout(); }, { flush: "pre" });
let resizeObserver: ResizeObserver | undefined;
let lastWidth = 0;
watch(showExitConfirm, (show) => { if (!exitDialog.value) return; if (show) exitDialog.value.showModal(); else exitDialog.value.close(); });
const systemDark = ref(window.matchMedia("(prefers-color-scheme: dark)").matches); const media = window.matchMedia("(prefers-color-scheme: dark)"); const onMediaChange = (e: MediaQueryListEvent) => systemDark.value = e.matches;
const isDark = computed(() => settingsStore.settings.theme === "dark" || (settingsStore.settings.theme === "auto" && systemDark.value));
watch(isDark, (dark) => { document.documentElement.classList.toggle("dark", dark); setWindowTheme(dark ? "dark" : "light").catch(() => {}); }, { immediate: true });
watch(() => settingsStore.settings.theme_color, (palette) => { document.documentElement.dataset.palette = palette || "pixiv"; }, { immediate: true });
function onNotification(event: Event) { notification.value = (event as CustomEvent<string>).detail; clearTimeout(toastTimer); toastTimer = window.setTimeout(() => notification.value = "", 3200); }
/** 浏览接口报未登录（api/browse.ts 派发）→ 复用现有登录弹窗。 */
function onOpenLogin() { showLoginDialog.value = true; }
/** 以图识图页「打开设置」引导（api/saucenao.ts 派发）→ 复用设置弹窗。 */
function onOpenSettings() { showSettings.value = true; }
/** 账号菜单的「设置」入口：开模态设置弹窗（菜单自行关闭）。 */
function openSettings() { showSettings.value = true; }
onMounted(() => {
  stopTasks = taskStore.startMonitoring();
  window.addEventListener('keydown', onWorkspaceKey);
  window.addEventListener('pixiv-tool:image-fullscreen', onImageFullscreen);
  resizeObserver = new ResizeObserver(entries => { const width = entries[0]?.contentRect.width ?? 0; if (lastWidth && width !== lastWidth) restoreLayout(); lastWidth = width; captureLayout(); });
  if (workspaceEl.value) resizeObserver.observe(workspaceEl.value);
  contentEl.value?.addEventListener('scroll', captureLayout, { passive: true });
  media.addEventListener("change", onMediaChange); authStore.fetchAccounts(); if (isTauri()) void listen("app://confirm-exit", () => { if (!disposed) showExitConfirm.value = true; }).then(fn => { if (disposed) fn(); else unlistenExit = fn; }).catch(() => {}); window.addEventListener("pixiv-tool:notify", onNotification); window.addEventListener(OPEN_LOGIN_EVENT, onOpenLogin); window.addEventListener(OPEN_SETTINGS_EVENT, onOpenSettings); });
onBeforeUnmount(() => { disposed = true; clearTimeout(toastTimer); layoutAnchor = undefined; stopTasks?.(); resizeObserver?.disconnect(); window.removeEventListener('keydown', onWorkspaceKey); window.removeEventListener('pixiv-tool:image-fullscreen', onImageFullscreen); contentEl.value?.removeEventListener('scroll', captureLayout); media.removeEventListener("change", onMediaChange); unlistenExit?.(); window.removeEventListener("pixiv-tool:notify", onNotification); window.removeEventListener(OPEN_LOGIN_EVENT, onOpenLogin); window.removeEventListener(OPEN_SETTINGS_EVENT, onOpenSettings); });
</script>

<style scoped>
/* 外壳恒为视口高：行高钉 100vh + 两列 min-height:0，右侧 app-content 是唯一滚动容器，
 * 长内容不再把侧栏拉长（头像恒在侧栏底部）。 */
.app-shell { display:grid; grid-template-columns:256px minmax(0,1fr); grid-template-rows:100vh; height:100vh; overflow:hidden; background:var(--surface); }.app-shell.collapsed { grid-template-columns:72px minmax(0,1fr); }.sidebar { display:flex; flex-direction:column; min-height:0; background:var(--md-sys-color-surface-container); border-right:1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }.sider-header { display:flex; align-items:center; justify-content:space-between; gap:12px; height:72px; padding:0 12px 0 20px; font-size:18px; font-weight:700; }.app-shell.collapsed .sider-header { justify-content:center; padding:0; }.sider-header img { flex:none; width:32px; height:32px; }.sider-title { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }.sidebar nav { flex:1; min-height:0; overflow-y:auto; padding-bottom:var(--space-sm); }.nav-item { display:flex; align-items:center; gap:16px; width:calc(100% - 24px); min-height:48px; margin:2px 12px; padding:0 16px; color:var(--ink); font:inherit; text-align:left; background:transparent; border:0; border-radius:24px; cursor:pointer; }.app-shell.collapsed .nav-item { justify-content:center; padding:0; }.nav-item:hover { background:color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }.nav-item.active { color:var(--md-sys-color-on-primary-container); font-weight:600; background:var(--md-sys-color-primary-container); }.nav-label { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }.nav-divider { height:0; margin:var(--space-sm) 12px; border-top:1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }.sider-footer { padding:12px; }/* 唯一滚动容器：接管整页滚动，长内容不再撑破 100vh 外壳（配合 .app-shell 的行高钉死与 overflow:hidden） */.app-content { min-height:0; overflow-y:auto; }/* 退出确认弹窗：容器配方走 main.css 的 .m3-dialog 通用层，这里只收窄宽度 */.exit-dialog { min-width:0; width:min(360px, 90vw); }/* 图标动作统一 20px 线性图标（stroke 2，lucide.dev 官方路径内联），点击域由 md-icon-button 承载 */.bar-icon { width:20px; height:20px; stroke-width:2; }
.workspace { min-width:0; min-height:0; display:grid; grid-template-rows:minmax(0,1fr) auto; container:workspace / inline-size; }
.workspace-body { min-width:0; min-height:0; display:grid; grid-template-rows:minmax(0,1fr); }
.workspace.panel-open .workspace-body { grid-template-rows:minmax(0,1fr) min(45%,320px); }
.main-pane { min-width:0; min-height:0; position:relative; display:flex; flex-direction:column; container:main-pane / size; }
.app-content { flex:1; min-width:0; padding-bottom:calc(var(--space-xl) * 3); }
.app-content.detail-page { padding-bottom:var(--space-xl); }
/* 系列分集页：底栏翻页器是 sticky 尾行，必须贴到滚动区真实底边。sticky 的吸附矩形会被滚动
 * 容器的底部内边距内缩（元素停在「视口底 − padding」处、下方漏出滚动内容），故该页把滚动末端
 * 余量从容器内边距改为内容自持——见 BrowseSeriesView 的 .series-body。 */
.app-content.series-page { padding-bottom:0; }
@media (max-width:640px) { .app-content.detail-page { padding-bottom:var(--space-lg); } }
.download-panel { border-top:1px solid color-mix(in srgb,var(--md-sys-color-outline) 35%,transparent); }
.search-fab { position:absolute; right:var(--space-xl); bottom:calc(var(--space-xl) * 3); width:calc(2 * var(--space-xl) + var(--space-sm)); height:calc(2 * var(--space-xl) + var(--space-sm)); --md-filled-tonal-icon-button-container-width:calc(2 * var(--space-xl) + var(--space-sm)); --md-filled-tonal-icon-button-container-height:calc(2 * var(--space-xl) + var(--space-sm)); --md-filled-tonal-icon-button-container-shape:50%; }
.search-fab :deep(svg) { width:var(--space-xl); height:var(--space-xl); fill:none; stroke:currentColor; }
/* 预留 56px 按钮 + 16px 间距，分页在任意滚动位置都不进入悬浮按钮区域。 */
.main-pane:has(> .search-fab) :deep(.app-pagination) { margin-inline-end:calc(3 * var(--space-xl)); }
.task-count { font-size:12px; margin-left:auto; }
.nav-item:focus-visible { outline:2px solid var(--md-sys-color-primary); outline-offset:2px; }
.app-shell.collapsed .task-count { margin-left:0; }
.fullscreen .download-status,.fullscreen .search-fab { display:none; }
@container workspace (min-width:1120px) {
  .workspace.panel-open .workspace-body { grid-template-columns:minmax(0,1fr) calc(var(--space-xl) * 20); grid-template-rows:minmax(0,1fr); }
  .download-panel { border-top:0; border-left:1px solid color-mix(in srgb,var(--md-sys-color-outline) 35%,transparent); }
}
</style>
