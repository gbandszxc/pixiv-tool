<template>
  <dialog ref="dialog" class="login-dialog" @close="emit('update:show', false)">
    <h2>{{ t('auth.loginDialogTitle') }}</h2>
    <md-tabs @change="onTabChange"><md-primary-tab :active="tab === 'builtin'">{{ t('auth.builtinLoginTab') }}</md-primary-tab><md-primary-tab :active="tab === 'cookie'">{{ t('auth.cookieLoginTab') }}</md-primary-tab></md-tabs>
    <div v-if="tab === 'builtin'" class="dialog-content">
      <div class="m3-row">
        <md-filled-button :disabled="authStore.isLoggingIn" @click="handleBuiltinLogin">{{ authStore.isLoggingIn ? t('auth.loggingIn') : t('auth.openBuiltinLogin') }}</md-filled-button>
        <HelpTooltip :label="t('auth.builtinLoginTab')" :text="t('auth.builtinLoginDesc')" />
      </div>
    </div>
    <div v-else class="dialog-content">
      <div class="field-label"><label for="login-phpsessid">{{ t('auth.phpsessidLabel') }}</label><HelpTooltip :label="t('auth.phpsessidLabel')" :text="`${t('auth.cookieLoginDesc')}\n\n${t('auth.cookieLoginSteps')}\n\n${t('auth.csrfAutoFillHint')}`" /></div>
      <md-outlined-text-field id="login-phpsessid" :aria-label="t('auth.phpsessidLabel')" :value="phpsessidInput" :placeholder="t('auth.phpsessidPlaceholder')" :disabled="cookieLoginLoading" @input="onCookieInput" @keydown.enter="handleCookieLogin" />
      <md-filled-button :disabled="!phpsessidInput.trim() || cookieLoginLoading" @click="handleCookieLogin">{{ t('auth.importAndLogin') }}</md-filled-button>
    </div>
    <div class="dialog-actions"><md-text-button @click="emit('update:show', false)">{{ t('common.cancel') }}</md-text-button></div>
  </dialog>
</template>
<script setup lang="ts">
import HelpTooltip from "../common/HelpTooltip.vue";
import { ref, watch } from "vue"; import { useI18n } from "vue-i18n"; import { useAuthStore, LoginError } from "../../stores/auth"; import { errorMessage } from "../../api/tauri"; import { notify } from "../../ui/notify";
const props = defineProps<{ show: boolean }>(); const emit = defineEmits<{ (e: "update:show", value: boolean): void }>(); const { t } = useI18n(); const authStore = useAuthStore(); const dialog = ref<HTMLDialogElement>(); const tab = ref("builtin"); const phpsessidInput = ref(""); const cookieLoginLoading = ref(false);
watch(() => props.show, (show) => { if (!dialog.value) return; if (show && !dialog.value.open) dialog.value.showModal(); if (!show && dialog.value.open) dialog.value.close(); });
function onTabChange(event: Event) { tab.value = (event.currentTarget as unknown as { activeTabIndex: number }).activeTabIndex === 0 ? "builtin" : "cookie"; }
function onCookieInput(event: Event) { phpsessidInput.value = (event.currentTarget as HTMLInputElement).value; }
async function handleCookieLogin() { const value = phpsessidInput.value.trim(); if (!value) return; cookieLoginLoading.value = true; try { await authStore.loginWithCookie(value); notify(t("auth.cookieLoginSuccess")); phpsessidInput.value = ""; emit("update:show", false); } catch (error) { notify(errorMessage(error) || t("auth.cookieLoginFailed")); } finally { cookieLoginLoading.value = false; } }
async function handleBuiltinLogin() { try { await authStore.login(); notify(t("auth.loginSuccess")); emit("update:show", false); } catch (error) { notify(error instanceof LoginError && error.status === "cancelled" ? t("auth.builtinLoginNoResult") : errorMessage(error) || t("auth.loginFailed")); } }
</script><style scoped>.login-dialog { width:min(520px,92vw); border:0; border-radius:28px; padding:24px; color:var(--ink); background:var(--md-sys-color-surface-container); }.login-dialog::backdrop{background:rgb(0 0 0 / 35%)}.dialog-content{display:grid;gap:16px;padding:20px 0}.dialog-actions{display:flex;justify-content:flex-end}</style>
