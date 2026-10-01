<template>
  <div class="app-shell" :class="{ collapsed: siderCollapsed }">
    <aside class="sidebar">
      <header class="sider-header">
        <template v-if="!siderCollapsed"><img src="./assets/icon.png" alt="" /><span class="sider-title">pixiv-tool</span></template>
        <md-icon-button class="collapse" :aria-label="siderCollapsed ? t('nav.expandSidebar') : t('nav.collapseSidebar')" :title="siderCollapsed ? t('nav.expandSidebar') : t('nav.collapseSidebar')" @click="siderCollapsed = !siderCollapsed">{{ siderCollapsed ? '›' : '‹' }}</md-icon-button>
      </header>
      <nav aria-label="Primary">
        <button v-for="item in menuItems" :key="item.path" class="nav-item" :class="{ active: route.path === item.path }" @click="router.push(item.path)"><SidebarIcon :name="item.icon" /><span v-if="!siderCollapsed" class="nav-label">{{ item.label }}</span></button>
        <div class="nav-divider" aria-hidden="true"></div>
        <button class="nav-item" :class="{ active: route.path.startsWith('/tools') }" @click="router.push('/tools')"><SidebarIcon name="tasks" /><span v-if="!siderCollapsed">{{ t('nav.tools') }}</span></button>
      </nav>
      <footer class="sider-footer">
        <button class="account-chip" :class="{ collapsed: siderCollapsed }" :aria-label="t('auth.openAccountDrawer')" :title="t('auth.openAccountDrawer')" @click="showDrawer = true">
          <img v-if="avatarSrc" class="avatar avatar-image" :src="avatarSrc" alt="" @error="avatarFailed = true" />
          <span v-else class="avatar">{{ accountInitial }}</span>
          <span v-if="!siderCollapsed" class="account-name">{{ accountLabel }}</span>
          <svg v-if="!siderCollapsed" class="chip-caret" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 14 6-6 6 6" /></svg>
        </button>
      </footer>
    </aside>
    <main class="app-content"><router-view /></main>
  </div>
  <dialog ref="exitDialog" class="m3-dialog" @close="showExitConfirm = false"><h2>{{ t('app.exitConfirmTitle') }}</h2><div class="m3-row"><md-text-button @click="showExitConfirm = false">{{ t('common.cancel') }}</md-text-button><md-filled-button @click="invoke('app_exit').catch(() => {})">{{ t('app.exit') }}</md-filled-button></div></dialog>
  <LoginDialog v-model:show="showLoginDialog" />
  <AccountDrawer v-model:show="showDrawer" @add-account="showLoginDialog = true" />
  <div v-if="notification" class="m3-snackbar" role="status">{{ notification }}</div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRouter, useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { listen } from "@tauri-apps/api/event";
import LoginDialog from "./components/auth/LoginDialog.vue";
import AccountDrawer from "./components/auth/AccountDrawer.vue";
import SidebarIcon, { type SidebarIconName } from "./components/navigation/SidebarIcon.vue";
import { useAuthStore } from "./stores/auth";
import { useSettingsStore } from "./stores/settings";
import { invoke, setWindowTheme } from "./api/tauri";
import { OPEN_LOGIN_EVENT } from "./api/browse";

const router = useRouter(); const route = useRoute(); const { t } = useI18n();
const authStore = useAuthStore(); const settingsStore = useSettingsStore();
const showLoginDialog = ref(false); const showDrawer = ref(false); const showExitConfirm = ref(false); const exitDialog = ref<HTMLDialogElement>();
const siderCollapsed = ref(false); const notification = ref(""); let toastTimer: number | undefined; let unlistenExit: (() => void) | undefined;
interface MenuItem { path: string; label: string; icon: SidebarIconName }
/** 扁平侧栏：浏览区在上（精确匹配高亮），其后分隔线 + 单一「工具」项（/tools* 前缀高亮）。 */
const menuItems = computed<MenuItem[]>(() => [
  { path: "/browse/home", label: t("nav.browseHome"), icon: "home" },
  { path: "/browse/illustration", label: t("nav.browseIllustration"), icon: "image" },
  { path: "/browse/manga", label: t("nav.browseManga"), icon: "manga" },
  { path: "/browse/novel", label: t("nav.browseNovel"), icon: "novel" },
  { path: "/browse/discover", label: t("nav.browseDiscover"), icon: "discover" },
  { path: "/browse/feed", label: t("nav.browseFeed"), icon: "feed" },
  { path: "/browse/search", label: t("nav.browseSearch"), icon: "search" },
  { path: "/browse/ranking", label: t("nav.browseRanking"), icon: "ranking" },
  { path: "/browse/bookmark", label: t("nav.browseBookmark"), icon: "bookmark" },
]);
// 头像 chip（账号/设置抽屉触发器）：文案与首字母回退逻辑迁自原 AccountMenu
const avatarFailed = ref(false); const avatarSrc = computed(() => avatarFailed.value ? "" : authStore.avatarUrl);
const accountLabel = computed(() => authStore.isLoggedIn ? authStore.pixivId || authStore.name : t("auth.accounts"));
const accountInitial = computed(() => accountLabel.value.charAt(0).toUpperCase());
watch(() => authStore.avatarUrl, () => { avatarFailed.value = false; });
watch(showExitConfirm, (show) => { if (!exitDialog.value) return; if (show) exitDialog.value.showModal(); else exitDialog.value.close(); });
const systemDark = ref(window.matchMedia("(prefers-color-scheme: dark)").matches); const media = window.matchMedia("(prefers-color-scheme: dark)"); const onMediaChange = (e: MediaQueryListEvent) => systemDark.value = e.matches;
const isDark = computed(() => settingsStore.settings.theme === "dark" || (settingsStore.settings.theme === "auto" && systemDark.value));
watch(isDark, (dark) => { document.documentElement.classList.toggle("dark", dark); setWindowTheme(dark ? "dark" : "light").catch(() => {}); }, { immediate: true });
watch(() => settingsStore.settings.theme_color, (palette) => { document.documentElement.dataset.palette = palette || "pixiv"; }, { immediate: true });
function onNotification(event: Event) { notification.value = (event as CustomEvent<string>).detail; clearTimeout(toastTimer); toastTimer = window.setTimeout(() => notification.value = "", 3200); }
/** 浏览接口报未登录（api/browse.ts 派发）→ 复用现有登录弹窗。 */
function onOpenLogin() { showLoginDialog.value = true; }
onMounted(() => { media.addEventListener("change", onMediaChange); settingsStore.fetchSettings().catch(() => {}); authStore.checkStatus(); authStore.fetchAccounts(); listen("app://confirm-exit", () => showExitConfirm.value = true).then(fn => unlistenExit = fn); window.addEventListener("pixiv-tool:notify", onNotification); window.addEventListener(OPEN_LOGIN_EVENT, onOpenLogin); });
onBeforeUnmount(() => { media.removeEventListener("change", onMediaChange); unlistenExit?.(); window.removeEventListener("pixiv-tool:notify", onNotification); window.removeEventListener(OPEN_LOGIN_EVENT, onOpenLogin); });
</script>

<style scoped>
/* 外壳恒为视口高：行高钉 100vh + 两列 min-height:0，右侧 app-content 是唯一滚动容器，
 * 长内容不再把侧栏拉长（头像恒在侧栏底部）。 */
.app-shell { display:grid; grid-template-columns:256px 1fr; grid-template-rows:100vh; height:100vh; overflow:hidden; background:var(--surface); }.app-shell.collapsed { grid-template-columns:72px 1fr; }.sidebar { display:flex; flex-direction:column; min-height:0; background:var(--md-sys-color-surface-container); border-right:1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }.sider-header { display:flex; align-items:center; justify-content:space-between; gap:12px; height:72px; padding:0 12px 0 20px; font-size:18px; font-weight:700; }.app-shell.collapsed .sider-header { justify-content:center; padding:0; }.sider-header img { flex:none; width:32px; height:32px; }.sider-title { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }.sidebar nav { flex:1; min-height:0; overflow-y:auto; padding-bottom:var(--space-sm); }.nav-item { display:flex; align-items:center; gap:16px; width:calc(100% - 24px); min-height:48px; margin:2px 12px; padding:0 16px; color:var(--ink); font:inherit; text-align:left; background:transparent; border:0; border-radius:24px; cursor:pointer; }.app-shell.collapsed .nav-item { justify-content:center; padding:0; }.nav-item:hover { background:color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }.nav-item.active { color:var(--md-sys-color-on-primary-container); font-weight:600; background:var(--md-sys-color-primary-container); }.nav-label { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }.nav-divider { height:0; margin:var(--space-sm) 12px; border-top:1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }.sider-footer { padding:12px; }.account-chip { display:flex; align-items:center; gap:8px; width:100%; min-height:48px; padding:8px 12px; color:var(--ink); font:inherit; text-align:left; background:transparent; border:0; border-radius:24px; cursor:pointer; }.account-chip:hover { background:color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }.account-chip.collapsed { justify-content:center; padding:8px; }.avatar { display:grid; place-items:center; flex:none; width:32px; height:32px; border-radius:50%; color:var(--md-sys-color-on-primary-container); font-weight:600; background:var(--md-sys-color-primary-container); }.avatar-image { object-fit:cover; }.account-name { flex:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }.chip-caret { flex:none; width:16px; height:16px; color:var(--ink-muted); }/* 唯一滚动容器：接管整页滚动，长内容不再撑破 100vh 外壳（配合 .app-shell 的行高钉死与 overflow:hidden） */.app-content { min-height:0; overflow-y:auto; }.m3-dialog { min-width:min(420px, 90vw); border:0; border-radius:28px; padding:24px; color:var(--ink); background:var(--md-sys-color-surface-container); }.m3-dialog::backdrop { background:rgb(0 0 0 / 35%); }.m3-dialog .m3-row { justify-content:flex-end; } </style>
