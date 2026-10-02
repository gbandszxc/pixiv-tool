<template>
  <div class="account-menu">
    <button
      class="account-chip"
      :class="{ collapsed }"
      :aria-label="t('auth.openAccountMenu')"
      :title="t('auth.openAccountMenu')"
      aria-haspopup="menu"
      :aria-expanded="show"
      @click="toggle()"
    >
      <img v-if="avatarSrc" class="avatar avatar-image" :src="avatarSrc" alt="" @error="avatarFailed = true" />
      <span v-else class="avatar">{{ chipInitial }}</span>
      <span v-if="!collapsed" class="account-name">{{ accountLabel }}</span>
      <svg v-if="!collapsed" class="chip-caret" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m18 15-6-6-6 6" /></svg>
    </button>
    <div v-if="show" class="menu-backdrop" @click="close()" />
    <Transition name="menu">
      <div v-if="show" ref="popover" class="menu-popover" role="menu" :aria-label="t('auth.accounts')" tabindex="-1" @keydown.esc="close()">
        <p v-if="!authStore.accounts.length" class="menu-hint">{{ t('auth.notLoggedInHint') }}</p>
        <button v-for="account in authStore.accounts" :key="account.user_id" role="menuitem" class="menu-item" :disabled="authStore.isSwitching || account.user_id === authStore.activeAccountId" @click="switchAccount(account.user_id)">
          <img v-if="account.avatar_url" class="avatar avatar-image" :src="account.avatar_url" alt="" />
          <span v-else class="avatar">{{ accountInitial(account) }}</span>
          <span class="menu-label">{{ displayName(account) }}</span>
          <span v-if="account.user_id === authStore.activeAccountId" class="menu-check" aria-hidden="true"><svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5" /></svg></span>
        </button>
        <button role="menuitem" class="menu-item" @click="addAccount()">
          <svg class="menu-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12h14" /><path d="M12 5v14" /></svg>
          <span class="menu-label">{{ t('auth.addAccount') }}</span>
        </button>
        <hr class="menu-divider" />
        <button role="menuitem" class="menu-item" @click="openSettings()">
          <svg class="menu-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915" /><circle cx="12" cy="12" r="3" /></svg>
          <span class="menu-label">{{ t('settings.title') }}</span>
          <svg class="menu-caret" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
        </button>
        <button role="menuitem" class="menu-item" :disabled="updateChecking" @click="checkUpdate()">
          <svg class="menu-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" /><path d="M21 3v5h-5" /></svg>
          <span class="menu-label">{{ t('auth.checkUpdate') }}</span>
        </button>
        <hr class="menu-divider" />
        <div class="menu-meta">
          <span class="menu-version">{{ t('auth.versionLabel', { version: appVersion }) }}</span>
          <button class="menu-github" :aria-label="t('auth.githubHome')" :title="t('auth.githubHome')" @click="openGitHub()">
            <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z" /></svg>
          </button>
        </div>
        <button role="menuitem" class="menu-item danger" :disabled="!authStore.isLoggedIn" @click="handleLogout()">
          <svg class="menu-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m16 17 5-5-5-5" /><path d="M21 12H9" /><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" /></svg>
          <span class="menu-label">{{ t('auth.logout') }}</span>
        </button>
      </div>
    </Transition>
    <dialog ref="logoutDialog" class="m3-dialog logout-dialog" @close="logoutConfirmOpen = false">
      <h2>{{ t('auth.logoutConfirmTitle') }}</h2>
      <p>{{ t('auth.logoutConfirmText') }}</p>
      <div class="m3-row dialog-actions">
        <md-text-button @click="cancelLogout">{{ t('common.cancel') }}</md-text-button>
        <md-filled-button @click="confirmLogout">{{ t('auth.logout') }}</md-filled-button>
      </div>
    </dialog>
    <dialog ref="updateDialog" class="m3-dialog update-dialog" @close="updateConfirmOpen = false">
      <h2>{{ t('auth.updateAvailableTitle') }}</h2>
      <p>{{ t('auth.updateAvailableText', { latest: updateInfo.latest_version, current: updateInfo.current_version }) }}</p>
      <div class="m3-row dialog-actions">
        <md-text-button @click="cancelUpdate">{{ t('common.cancel') }}</md-text-button>
        <md-filled-button @click="gotoUpdate">{{ t('auth.gotoUpdate') }}</md-filled-button>
      </div>
    </dialog>
  </div>
</template>

<script setup lang="ts">
/**
 * 账号菜单：侧栏底部头像 chip + 锚定其上方的轻量弹出菜单（无深色 scrim，
 * 透明遮罩点击外部关闭，Esc 关闭）。内容 = 账号列表（当前 ✓，点击切换）/
 * 添加账号（emit 给 LoginDialog）+ 分隔线 + 「设置」入口（emit 出去由 App 打开
 * 模态设置弹窗）+「检查更新」（有新版本弹确认弹窗，无更新/失败静默或提示）
 * + meta 行（版本回显 + GitHub 主页入口）+ 退出登录。
 * 头像/首字母回退逻辑迁自原 AccountMenu 与抽屉。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useAuthStore } from "../../stores/auth";
import { errorMessage, isTauri, type AccountEntry } from "../../api/tauri";
import { checkAppUpdate, type UpdateCheckInfo } from "../../api/appUpdate";
import { notify } from "../../ui/notify";

defineProps<{ collapsed: boolean }>();
const emit = defineEmits<{ (e: "add-account"): void; (e: "open-settings"): void }>();
const { t } = useI18n(); const authStore = useAuthStore();
const show = ref(false); const popover = ref<HTMLElement | null>(null);
const logoutDialog = ref<HTMLDialogElement | null>(null); const logoutConfirmOpen = ref(false);
const updateDialog = ref<HTMLDialogElement | null>(null); const updateConfirmOpen = ref(false);
const updateChecking = ref(false);
/** 弹窗正文始终要渲染版本插值，因此给一个安全的占位初始值而非 null。 */
const updateInfo = ref<UpdateCheckInfo>({ has_update: false, current_version: "--", latest_version: "--", release_url: "" });
const avatarFailed = ref(false); const avatarSrc = computed(() => avatarFailed.value ? "" : authStore.avatarUrl);

const accountLabel = computed(() => authStore.isLoggedIn ? authStore.pixivId || authStore.name : t("auth.accounts"));
const chipInitial = computed(() => accountLabel.value.charAt(0).toUpperCase());
watch(() => authStore.avatarUrl, () => { avatarFailed.value = false; });

const GITHUB_HOME = "https://github.com/gbandszxc/pixiv-tool";
/** 版本回显动态读真实 app 版本；非 Tauri 环境读不到时保持占位符。 */
const appVersion = ref("--");
let startupUpdateTimer: number | undefined;
onMounted(async () => {
  try { appVersion.value = await getVersion(); } catch { /* 非 Tauri 环境 */ }
  // 启动 3s 后静默检查一次更新：仅发现新版本时弹窗，无更新/失败完全静默。
  if (isTauri()) startupUpdateTimer = window.setTimeout(() => {
    checkAppUpdate().then((info) => { if (info.has_update) showUpdateDialog(info); }).catch(() => {});
  }, 3000);
});
onBeforeUnmount(() => window.clearTimeout(startupUpdateTimer));
async function openGitHub() { close(); try { await openUrl(GITHUB_HOME); } catch (error) { notify(errorMessage(error) || t("auth.githubOpenFailed")); } }

/** 有新版本时记录信息并弹确认弹窗（手动检查与启动静默检查共用）。 */
function showUpdateDialog(info: UpdateCheckInfo) { updateInfo.value = info; updateDialog.value?.showModal(); }
/** 手动检查更新：期间菜单项 disabled 防重复点击；结果分支弹窗或提示。 */
async function checkUpdate() {
  if (updateChecking.value) return;
  updateChecking.value = true;
  try {
    const info = await checkAppUpdate();
    if (info.has_update) showUpdateDialog(info);
    else notify(t("auth.updateUpToDate"));
  } catch (error) {
    notify(errorMessage(error) || t("auth.updateCheckFailed"));
  } finally {
    updateChecking.value = false;
  }
}
function cancelUpdate() { updateDialog.value?.close(); }
/** 前往更新：关弹窗并打开发布页；打开失败给出可读提示。 */
async function gotoUpdate() {
  updateDialog.value?.close();
  try { await openUrl(updateInfo.value.release_url); }
  catch (error) { notify(errorMessage(error) || t("auth.updateOpenFailed")); }
}

function toggle() { show.value = !show.value; }
function close() { show.value = false; }
/** Esc 关菜单；设置确认等原生 dialog 打开时让位（Esc 优先关闭最上层 dialog）。 */
function onWindowKeydown(event: KeyboardEvent) { if (event.key === "Escape" && !document.querySelector("dialog[open]")) close(); }
watch(show, (open) => { if (open) { window.addEventListener("keydown", onWindowKeydown); nextTick(() => popover.value?.focus()); } else window.removeEventListener("keydown", onWindowKeydown); });
onBeforeUnmount(() => window.removeEventListener("keydown", onWindowKeydown));

function displayName(account: AccountEntry) { return account.pixiv_id || account.name || account.user_id; }
function accountInitial(account: AccountEntry) { return displayName(account).charAt(0).toUpperCase(); }
async function switchAccount(id: string) { close(); try { await authStore.switchAccount(id); notify(t("auth.switchSuccess", { name: displayName(authStore.accounts.find(account => account.user_id === id)!) })); } catch (error) { notify(errorMessage(error) || t("auth.switchFailed")); } }
function addAccount() { close(); emit("add-account"); }
function openSettings() { close(); emit("open-settings"); }
/** 退出登录先弹确认（原生 dialog），确认后才真正执行。 */
function handleLogout() { logoutDialog.value?.showModal(); }
function cancelLogout() { logoutDialog.value?.close(); }
async function confirmLogout() { logoutDialog.value?.close(); try { await authStore.logout(); close(); } catch (error) { notify(errorMessage(error) || t("auth.logoutFailed")); } }
</script>

<style scoped>
.account-menu { position: relative; width: 100%; }
.account-chip { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 48px; padding: 8px 12px; color: var(--ink); font: inherit; text-align: left; background: transparent; border: 0; border-radius: 24px; cursor: pointer; }
.account-chip:hover { background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }
.account-chip.collapsed { justify-content: center; padding: 8px; }
.avatar { display: grid; place-items: center; flex: none; width: 32px; height: 32px; border-radius: 50%; color: var(--md-sys-color-on-primary-container); font-weight: 600; background: var(--md-sys-color-primary-container); }
.avatar-image { object-fit: cover; }
.account-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.chip-caret { flex: none; width: 16px; height: 16px; color: var(--ink-muted); }
/* 透明遮罩只负责「点击外部关闭」，不做变暗 scrim；popover 锚定 chip 上方弹出 */
.menu-backdrop { position: fixed; inset: 0; z-index: 890; }
.menu-popover { position: absolute; bottom: calc(100% + 8px); left: 0; z-index: 900; width: 264px; padding: var(--space-sm); color: var(--ink); background: var(--md-sys-color-surface-container); border-radius: var(--radius-control); box-shadow: 0 4px 12px rgb(0 0 0 / 18%); outline: none; transform-origin: bottom left; }
.menu-item { display: flex; align-items: center; gap: var(--space-sm); width: 100%; min-height: 40px; padding: 0 var(--space-sm); color: var(--ink); font: inherit; text-align: left; background: transparent; border: 0; border-radius: var(--radius-control); cursor: pointer; }
.menu-item:hover:not(:disabled) { background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }
.menu-item:disabled { opacity: .65; cursor: default; }
.menu-item:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: -2px; }
.menu-icon { flex: none; width: 16px; height: 16px; font-size: 16px; color: var(--ink-muted); }
.menu-label { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.menu-check { flex: none; color: var(--md-sys-color-primary); }
.menu-caret { flex: none; width: 16px; height: 16px; color: var(--ink-muted); }
.menu-divider { margin: var(--space-xs) 0; border: 0; border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.menu-hint { margin: 0; padding: 0 var(--space-sm); color: var(--ink-muted); font-size: 12px; line-height: 40px; }
/* danger 沿用 SettingsPanel .danger-button 的既有规范值 #ba1a1a */
.menu-item.danger .menu-label { color: #ba1a1a; }
/* 底部 meta 行：左对齐版本回显，右对齐 GitHub 主页入口（28px 圆形 hover 状态层） */
.menu-meta { display: flex; align-items: center; justify-content: space-between; gap: var(--space-sm); min-height: 36px; padding: 0 var(--space-sm); }
.menu-version { color: var(--ink-muted); font-size: 12px; }
.menu-github { display: grid; place-items: center; flex: none; width: 28px; height: 28px; padding: 0; color: var(--ink-muted); background: transparent; border: 0; border-radius: 50%; cursor: pointer; }
.menu-github svg { width: 16px; height: 16px; }
.menu-github:hover { color: var(--ink); background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }
.menu-github:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: -2px; }
/* 退出登录确认弹窗：基础 confirm 用更紧凑的 360px 宽（覆盖全局 .m3-dialog 的 min-width）；
 * 标题/正文/按钮行（.dialog-actions 右对齐）均走 main.css 的 .m3-dialog 全局规则 */
dialog.logout-dialog { width: min(360px, 92vw); min-width: 0; }
/* 上浮过渡：时长复用既有 0.15s ease（WorkCard / BookmarkButton 同源）；reduced-motion 由全局兜底压到 0.01ms */
.menu-enter-active, .menu-leave-active { transition: opacity 0.15s ease, transform 0.15s ease; }
.menu-enter-from, .menu-leave-to { opacity: 0; transform: translateY(4px) scale(0.98); }
</style>
