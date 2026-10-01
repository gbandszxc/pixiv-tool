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
      <svg v-if="!collapsed" class="chip-caret" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 14 6-6 6 6" /></svg>
    </button>
    <div v-if="show" class="menu-backdrop" @click="close()" />
    <Transition name="menu">
      <div v-if="show" ref="popover" class="menu-popover" role="menu" :aria-label="t('auth.accounts')" tabindex="-1" @keydown.esc="close()">
        <p v-if="!authStore.accounts.length" class="menu-hint">{{ t('auth.notLoggedInHint') }}</p>
        <button v-for="account in authStore.accounts" :key="account.user_id" role="menuitem" class="menu-item" :disabled="authStore.isSwitching || account.user_id === authStore.activeAccountId" @click="switchAccount(account.user_id)">
          <img v-if="account.avatar_url" class="avatar avatar-image" :src="account.avatar_url" alt="" />
          <span v-else class="avatar">{{ accountInitial(account) }}</span>
          <span class="menu-label">{{ displayName(account) }}</span>
          <span v-if="account.user_id === authStore.activeAccountId" class="menu-check" aria-hidden="true">✓</span>
        </button>
        <button role="menuitem" class="menu-item" @click="addAccount()">
          <svg class="menu-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 5v14" /><path d="M5 12h14" /></svg>
          <span class="menu-label">{{ t('auth.addAccount') }}</span>
        </button>
        <hr class="menu-divider" />
        <button role="menuitem" class="menu-item" @click="openSettings()">
          <SidebarIcon class="menu-icon" name="settings" />
          <span class="menu-label">{{ t('settings.title') }}</span>
          <svg class="menu-caret" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m10 6 6 6-6 6" /></svg>
        </button>
        <button role="menuitem" class="menu-item danger" :disabled="!authStore.isLoggedIn" @click="handleLogout()">
          <svg class="menu-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 4H5v16h4" /><path d="m14 8-4 4 4 4" /><path d="M10 12h9" /></svg>
          <span class="menu-label">{{ t('auth.logout') }}</span>
        </button>
      </div>
    </Transition>
    <dialog ref="logoutDialog" class="m3-dialog" @close="logoutConfirmOpen = false">
      <h2>{{ t('auth.logoutConfirmTitle') }}</h2>
      <p class="logout-confirm-text">{{ t('auth.logoutConfirmText') }}</p>
      <div class="m3-row logout-confirm-actions">
        <md-text-button @click="cancelLogout">{{ t('common.cancel') }}</md-text-button>
        <md-filled-button @click="confirmLogout">{{ t('auth.logout') }}</md-filled-button>
      </div>
    </dialog>
  </div>
</template>

<script setup lang="ts">
/**
 * 账号菜单：侧栏底部头像 chip + 锚定其上方的轻量弹出菜单（无深色 scrim，
 * 透明遮罩点击外部关闭，Esc 关闭）。内容 = 账号列表（当前 ✓，点击切换）/
 * 添加账号（emit 给 LoginDialog）+ 分隔线 + 「设置」入口（emit 出去由 App 打开
 * 模态设置弹窗）+ 退出登录。头像/首字母回退逻辑迁自原 AccountMenu 与抽屉。
 */
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import SidebarIcon from "../navigation/SidebarIcon.vue";
import { useAuthStore } from "../../stores/auth";
import { errorMessage, type AccountEntry } from "../../api/tauri";
import { notify } from "../../ui/notify";

defineProps<{ collapsed: boolean }>();
const emit = defineEmits<{ (e: "add-account"): void; (e: "open-settings"): void }>();
const { t } = useI18n(); const authStore = useAuthStore();
const show = ref(false); const popover = ref<HTMLElement | null>(null);
const logoutDialog = ref<HTMLDialogElement | null>(null); const logoutConfirmOpen = ref(false);
const avatarFailed = ref(false); const avatarSrc = computed(() => avatarFailed.value ? "" : authStore.avatarUrl);

const accountLabel = computed(() => authStore.isLoggedIn ? authStore.pixivId || authStore.name : t("auth.accounts"));
const chipInitial = computed(() => accountLabel.value.charAt(0).toUpperCase());
watch(() => authStore.avatarUrl, () => { avatarFailed.value = false; });

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
.menu-icon { flex: none; width: 20px; height: 20px; font-size: 20px; color: var(--ink-muted); }
.menu-label { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.menu-check { flex: none; color: var(--md-sys-color-primary); }
.menu-caret { flex: none; width: 16px; height: 16px; color: var(--ink-muted); }
.menu-divider { margin: var(--space-xs) 0; border: 0; border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.menu-hint { margin: 0; padding: 0 var(--space-sm); color: var(--ink-muted); font-size: 12px; line-height: 40px; }
/* danger 沿用 SettingsPanel .danger-button 的既有规范值 #ba1a1a */
.menu-item.danger .menu-label { color: #ba1a1a; }
/* 退出登录确认弹窗（容器样式来自 main.css 的全局 .m3-dialog） */
.logout-confirm-text { margin: var(--space-sm) 0 0; color: var(--ink-muted); line-height: 1.6; }
.logout-confirm-actions { justify-content: flex-end; margin-top: var(--space-lg); }
/* 上浮过渡：时长复用既有 0.15s ease（WorkCard / BookmarkButton 同源）；reduced-motion 由全局兜底压到 0.01ms */
.menu-enter-active, .menu-leave-active { transition: opacity 0.15s ease, transform 0.15s ease; }
.menu-enter-from, .menu-leave-to { opacity: 0; transform: translateY(4px) scale(0.98); }
</style>
