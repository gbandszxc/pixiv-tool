<template>
  <Transition name="drawer">
    <div v-if="show" class="drawer-root">
      <div class="drawer-scrim" @click="close()"></div>
      <aside ref="panel" class="drawer-panel" role="dialog" aria-modal="true" :aria-label="t('auth.drawerTitle')" tabindex="-1">
        <header class="drawer-header">
          <h2>{{ t('auth.drawerTitle') }}</h2>
          <md-icon-button :aria-label="t('common.close')" :title="t('common.close')" @click="close()">✕</md-icon-button>
        </header>
        <div class="drawer-body">
          <section class="drawer-section" :aria-label="t('auth.accounts')">
            <h3>{{ t('auth.accounts') }}</h3>
            <p v-if="!authStore.accounts.length" class="account-empty">{{ t('auth.notLoggedInHint') }}</p>
            <div v-else class="account-list">
              <button v-for="account in authStore.accounts" :key="account.user_id" class="account-option" :disabled="authStore.isSwitching || account.user_id === authStore.activeAccountId" @click="switchAccount(account.user_id)">
                <img v-if="account.avatar_url" class="avatar avatar-image" :src="account.avatar_url" alt="" />
                <span v-else class="avatar">{{ accountInitial(account) }}</span>
                <span class="account-name">{{ displayName(account) }}</span>
                <span v-if="account.user_id === authStore.activeAccountId" class="account-check" aria-hidden="true">✓</span>
              </button>
            </div>
            <div class="m3-row account-actions">
              <md-outlined-button @click="emit('add-account')">{{ t('auth.addAccount') }}</md-outlined-button>
              <md-text-button :disabled="!authStore.isLoggedIn" @click="handleLogout">{{ t('auth.logout') }}</md-text-button>
            </div>
          </section>
          <hr class="drawer-divider" />
          <button class="drawer-menu-item" @click="emit('open-settings')">
            <SidebarIcon name="settings" />
            <span>{{ t('settings.title') }}</span>
            <svg class="menu-caret" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m10 6 6 6-6 6" /></svg>
          </button>
        </div>
      </aside>
    </div>
  </Transition>
</template>

<script setup lang="ts">
/**
 * 账号抽屉：侧栏底部头像 chip 的展开物（M3 modal drawer 形态）。
 * 账号区迁移自原 AccountMenu（列表切换 / 添加账号 emit 给 LoginDialog / 退出登录）；
 * 「设置」只留入口，点击 emit open-settings（由 App 关抽屉并打开模态设置弹窗
 * SettingsDialog）。Esc / 点 scrim / 标题栏 ✕ 均可关闭。
 */
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import SidebarIcon from "../navigation/SidebarIcon.vue";
import { useAuthStore } from "../../stores/auth";
import { errorMessage, type AccountEntry } from "../../api/tauri";
import { notify } from "../../ui/notify";

const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{ (e: "update:show", value: boolean): void; (e: "add-account"): void; (e: "open-settings"): void }>();
const { t } = useI18n(); const authStore = useAuthStore();
const panel = ref<HTMLElement | null>(null);

function close() { emit("update:show", false); }
/** Esc 关闭抽屉（window 级监听）；设置确认等原生 dialog 打开时让位（Esc 优先关闭最上层 dialog）。 */
function onWindowKeydown(event: KeyboardEvent) { if (event.key === "Escape" && !document.querySelector("dialog[open]")) close(); }
watch(() => props.show, (show) => { if (show) { window.addEventListener("keydown", onWindowKeydown); nextTick(() => panel.value?.focus()); } else window.removeEventListener("keydown", onWindowKeydown); });
onBeforeUnmount(() => window.removeEventListener("keydown", onWindowKeydown));

function displayName(account: AccountEntry) { return account.pixiv_id || account.name || account.user_id; }
function accountInitial(account: AccountEntry) { return displayName(account).charAt(0).toUpperCase(); }
async function switchAccount(id: string) { try { await authStore.switchAccount(id); notify(t("auth.switchSuccess", { name: displayName(authStore.accounts.find(account => account.user_id === id)!) })); } catch (error) { notify(errorMessage(error) || t("auth.switchFailed")); } }
async function handleLogout() { try { await authStore.logout(); } catch (error) { notify(errorMessage(error) || t("auth.logoutFailed")); } }
</script>

<style scoped>
.drawer-root { position: fixed; inset: 0; z-index: 900; }
.drawer-scrim { position: absolute; inset: 0; background: rgb(0 0 0 / 35%); }
.drawer-panel { position: absolute; top: 0; bottom: 0; left: 0; display: flex; flex-direction: column; width: min(420px, 92vw); color: var(--ink); background: var(--md-sys-color-surface-container); border-radius: 0 16px 16px 0; outline: none; }
.drawer-header { display: flex; align-items: center; justify-content: space-between; gap: var(--space-sm); padding: var(--space-md) var(--space-md) var(--space-md) var(--space-xl); }
.drawer-header h2 { margin: 0; font-size: 16px; font-weight: 600; line-height: 1.4; }
.drawer-body { flex: 1; min-height: 0; overflow-y: auto; padding: 0 var(--space-xl) var(--space-xl); }
.drawer-section h3 { margin: 0 0 var(--space-md); color: var(--ink-muted); font-size: 12px; font-weight: 600; line-height: 20px; }
.drawer-divider { margin: var(--space-lg) 0; border: 0; border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.account-list { display: grid; gap: var(--space-xxs); margin-bottom: var(--space-md); }
.account-option { display: flex; align-items: center; gap: var(--space-sm); width: 100%; min-height: 44px; padding: var(--space-xxs) var(--space-sm); color: var(--ink); font: inherit; text-align: left; background: transparent; border: 0; border-radius: var(--radius-control); cursor: pointer; }
.account-option:hover { background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }
.account-option:disabled { opacity: .65; cursor: default; }
.account-option:disabled:hover { background: transparent; }
.account-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.account-check { flex: none; color: var(--md-sys-color-primary); }
.account-empty { margin: 0 0 var(--space-md); color: var(--ink-muted); font-size: 12px; }
.account-actions { margin-bottom: var(--space-sm); }
.drawer-menu-item { display: flex; align-items: center; gap: var(--space-sm); width: 100%; min-height: 44px; padding: var(--space-xxs) var(--space-sm); color: var(--ink); font: inherit; text-align: left; background: transparent; border: 0; border-radius: var(--radius-control); cursor: pointer; }
.drawer-menu-item:hover { background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }
.menu-caret { flex: none; width: 16px; height: 16px; margin-left: auto; color: var(--ink-muted); }
.avatar { display: grid; place-items: center; flex: none; width: 32px; height: 32px; border-radius: 50%; color: var(--md-sys-color-on-primary-container); font-weight: 600; background: var(--md-sys-color-primary-container); }
.avatar-image { object-fit: cover; }
/* 滑入过渡：时长复用既有 0.15s ease（WorkCard / BookmarkButton 同源）；reduced-motion 由全局兜底压到 0.01ms */
.drawer-enter-active .drawer-panel, .drawer-leave-active .drawer-panel { transition: transform 0.15s ease; }
.drawer-enter-from .drawer-panel, .drawer-leave-to .drawer-panel { transform: translateX(-100%); }
.drawer-enter-active .drawer-scrim, .drawer-leave-active .drawer-scrim { transition: opacity 0.15s ease; }
.drawer-enter-from .drawer-scrim, .drawer-leave-to .drawer-scrim { opacity: 0; }
</style>
