<template>
  <div class="app-shell" :class="{ collapsed: siderCollapsed }">
    <aside class="sidebar">
      <div class="sider-title"><img src="./assets/icon.png" alt="" /><span v-if="!siderCollapsed">pixiv-tool</span></div>
      <nav aria-label="Primary">
        <template v-for="(group, groupIndex) in menuGroups" :key="group.label">
          <div v-if="!siderCollapsed" class="nav-group-title">{{ group.label }}</div>
          <div v-else-if="groupIndex > 0" class="nav-group-divider" aria-hidden="true"></div>
          <button v-for="item in group.items" :key="item.path" class="nav-item" :class="{ active: route.path === item.path }" @click="router.push(item.path)"><SidebarIcon :name="item.icon" /><span v-if="!siderCollapsed">{{ item.label }}</span></button>
        </template>
      </nav>
      <div class="sider-footer">
        <AccountMenu v-if="authStore.isLoggedIn || authStore.accounts.length" :collapsed="siderCollapsed" @visible="syncBrowseVisibility" @add-account="showLoginDialog = true" />
        <md-outlined-button v-else @click="showLoginDialog = true">{{ t('auth.login') }}</md-outlined-button>
        <md-icon-button class="collapse" :aria-label="siderCollapsed ? 'Expand navigation' : 'Collapse navigation'" @click="siderCollapsed = !siderCollapsed">{{ siderCollapsed ? '›' : '‹' }}</md-icon-button>
      </div>
    </aside>
    <main class="app-content" :class="{ 'is-pixiv-route': route.path === '/pixiv' }"><router-view /></main>
  </div>
  <dialog ref="exitDialog" class="m3-dialog" @close="showExitConfirm = false"><h2>{{ t('app.exitConfirmTitle') }}</h2><div class="m3-row"><md-text-button @click="showExitConfirm = false">{{ t('common.cancel') }}</md-text-button><md-filled-button @click="invoke('app_exit').catch(() => {})">{{ t('app.exit') }}</md-filled-button></div></dialog>
  <LoginDialog v-model:show="showLoginDialog" />
  <div v-if="notification" class="m3-snackbar" role="status">{{ notification }}</div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter, useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { listen } from "@tauri-apps/api/event";
import LoginDialog from "./components/auth/LoginDialog.vue";
import AccountMenu from "./components/auth/AccountMenu.vue";
import SidebarIcon, { type SidebarIconName } from "./components/navigation/SidebarIcon.vue";
import { useAuthStore } from "./stores/auth";
import { useSettingsStore } from "./stores/settings";
import { invoke, setWindowTheme } from "./api/tauri";
import { OPEN_LOGIN_EVENT } from "./api/browse";

const router = useRouter(); const route = useRoute(); const { t } = useI18n();
const authStore = useAuthStore(); const settingsStore = useSettingsStore();
const showLoginDialog = ref(false); const showExitConfirm = ref(false); const exitDialog = ref<HTMLDialogElement>();
const siderCollapsed = ref(false); const notification = ref(""); let toastTimer: number | undefined; let unlistenExit: (() => void) | undefined;
interface MenuItem { path: string; label: string; icon: SidebarIconName }
interface MenuGroup { label: string; items: MenuItem[] }
const menuGroups = computed<MenuGroup[]>(() => [
  {
    label: t("nav.groupTools"),
    items: [
      { path: "/pixiv", label: t("nav.pixiv"), icon: "pixiv" },
      { path: "/", label: t("nav.crawlNovel"), icon: "crawl" },
      { path: "/illustration", label: t("nav.crawlIllustration"), icon: "crawl" },
      { path: "/tasks", label: t("nav.tasks"), icon: "tasks" },
      { path: "/history", label: t("nav.history"), icon: "history" },
      { path: "/settings", label: t("nav.settings"), icon: "settings" },
    ],
  },
  {
    label: t("nav.groupBrowse"),
    items: [
      { path: "/browse/home", label: t("nav.browseHome"), icon: "home" },
      { path: "/browse/illustration", label: t("nav.browseIllustration"), icon: "image" },
      { path: "/browse/manga", label: t("nav.browseManga"), icon: "manga" },
      { path: "/browse/novel", label: t("nav.browseNovel"), icon: "novel" },
      { path: "/browse/discover", label: t("nav.browseDiscover"), icon: "discover" },
      { path: "/browse/feed", label: t("nav.browseFeed"), icon: "feed" },
      { path: "/browse/search", label: t("nav.browseSearch"), icon: "search" },
      { path: "/browse/ranking", label: t("nav.browseRanking"), icon: "ranking" },
    ],
  },
]);
function syncBrowseVisibility(hidden: boolean) { if (hidden) invoke("browse_hide").catch(() => {}); else if (route.path === "/pixiv") invoke("browse_show").catch(() => {}); }
watch([showLoginDialog, showExitConfirm], ([login, exit]) => syncBrowseVisibility(login || exit));
watch(showExitConfirm, (show) => { if (!exitDialog.value) return; if (show) exitDialog.value.showModal(); else exitDialog.value.close(); });
const systemDark = ref(window.matchMedia("(prefers-color-scheme: dark)").matches); const media = window.matchMedia("(prefers-color-scheme: dark)"); const onMediaChange = (e: MediaQueryListEvent) => systemDark.value = e.matches;
const isDark = computed(() => settingsStore.settings.theme === "dark" || (settingsStore.settings.theme === "auto" && systemDark.value));
watch(isDark, (dark) => { document.documentElement.classList.toggle("dark", dark); setWindowTheme(dark ? "dark" : "light").catch(() => {}); }, { immediate: true });
watch(() => settingsStore.settings.theme_color, (palette) => { document.documentElement.dataset.palette = palette || "pixiv"; }, { immediate: true });
function onNotification(event: Event) { notification.value = (event as CustomEvent<string>).detail; clearTimeout(toastTimer); toastTimer = window.setTimeout(() => notification.value = "", 3200); }
/** 浏览接口报未登录（api/browse.ts 派发）→ 复用现有登录弹窗。 */
function onOpenLogin() { showLoginDialog.value = true; }
/** Alt 松开唤起菜单栏（Windows 默认隐藏，见 src-tauri/src/menu_bar.rs；其它平台 no-op）。用 keyup 以免 Alt+Tab 等组合键误触发。 */
function onMenuKeyUp(event: KeyboardEvent) { if (event.key === "Alt") invoke("app_menu_show").catch(() => {}); }
onMounted(() => { media.addEventListener("change", onMediaChange); settingsStore.fetchSettings().catch(() => {}); authStore.checkStatus(); authStore.fetchAccounts(); listen("app://confirm-exit", () => showExitConfirm.value = true).then(fn => unlistenExit = fn); window.addEventListener("pixiv-tool:notify", onNotification); window.addEventListener(OPEN_LOGIN_EVENT, onOpenLogin); window.addEventListener("keyup", onMenuKeyUp); });
onBeforeUnmount(() => { media.removeEventListener("change", onMediaChange); unlistenExit?.(); window.removeEventListener("pixiv-tool:notify", onNotification); window.removeEventListener(OPEN_LOGIN_EVENT, onOpenLogin); window.removeEventListener("keyup", onMenuKeyUp); });
</script>

<style scoped>
.app-shell { display:grid; grid-template-columns:256px 1fr; height:100vh; background:var(--surface); }.app-shell.collapsed { grid-template-columns:72px 1fr; }.sidebar { display:flex; flex-direction:column; min-width:0; background:var(--md-sys-color-surface-container); border-right:1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }.sider-title { display:flex; align-items:center; gap:12px; height:72px; padding:0 20px; font-size:18px; font-weight:700; }.sider-title img { width:32px; height:32px; }.nav-item { display:flex; align-items:center; gap:16px; width:calc(100% - 24px); min-height:48px; margin:2px 12px; padding:0 16px; color:var(--ink); font:inherit; text-align:left; background:transparent; border:0; border-radius:24px; cursor:pointer; }.nav-item:hover { background:color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }.nav-item.active { color:var(--md-sys-color-on-primary-container); font-weight:600; background:var(--md-sys-color-primary-container); }.nav-group-title { margin:var(--space-sm) 12px 0; padding:0 16px; color:var(--md-sys-color-on-surface-variant); font-size:12px; font-weight:600; line-height:20px; }.nav-group-divider { height:0; margin:var(--space-sm) 12px; border-top:1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }.sider-footer { margin-top:auto; padding:12px; display:grid; gap:8px; }.collapse { justify-self:center; }.m3-dialog { min-width:min(420px, 90vw); border:0; border-radius:28px; padding:24px; color:var(--ink); background:var(--md-sys-color-surface-container); }.m3-dialog::backdrop { background:rgb(0 0 0 / 35%); }.m3-dialog .m3-row { justify-content:flex-end; }.app-content.is-pixiv-route { padding:0; height:100%; overflow:hidden; }
</style>
