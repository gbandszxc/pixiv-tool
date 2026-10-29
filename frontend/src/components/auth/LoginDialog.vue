<template>
  <dialog ref="dialog" class="m3-dialog login-dialog" aria-labelledby="login-title" @close="emit('update:show', false)">
    <h2 id="login-title">{{ t('auth.loginDialogTitle') }}</h2>
    <md-tabs @change="onTabChange"><md-primary-tab :active="tab === 'builtin'">{{ t('auth.builtinLoginTab') }}</md-primary-tab><md-primary-tab :active="tab === 'cookie'">{{ t('auth.cookieLoginTab') }}</md-primary-tab></md-tabs>
    <div v-if="tab === 'builtin'" class="dialog-content">
      <div class="m3-row">
        <md-filled-button :disabled="authStore.isLoggingIn" :aria-busy="authStore.isLoggingIn" @click="handleBuiltinLogin">{{ authStore.isLoggingIn ? t('auth.loggingIn') : t('auth.openBuiltinLogin') }}</md-filled-button>
        <HelpTooltip :label="t('auth.builtinLoginTab')" :text="t('auth.builtinLoginDesc')" />
      </div>
    </div>
    <div v-else class="dialog-content">
      <div class="field-label"><label for="login-phpsessid">{{ t('auth.phpsessidLabel') }}</label><HelpTooltip :label="t('auth.phpsessidLabel')" :text="`${t('auth.cookieLoginDesc')}\n\n${t('auth.cookieLoginSteps')}\n\n${t('auth.csrfAutoFillHint')}`" /></div>
      <md-outlined-text-field id="login-phpsessid" type="password" autocomplete="new-password" :aria-label="t('auth.phpsessidLabel')" :value="phpsessidInput" :placeholder="t('auth.phpsessidPlaceholder')" :disabled="cookieLoginLoading" @input="onCookieInput" @keydown.enter="handleCookieLogin" />
      <md-filled-button :disabled="!phpsessidInput.trim() || cookieLoginLoading" :aria-busy="cookieLoginLoading" @click="handleCookieLogin">{{ t('auth.importAndLogin') }}</md-filled-button>
    </div>
    <!-- 登录失败就地展示（两条路径共用）：不依赖右上角 Snackbar，读屏经 role=alert 播报 -->
    <p v-if="inlineError" class="m3-alert error" role="alert">{{ inlineError }}</p>
    <div class="dialog-actions"><md-text-button @click="emit('update:show', false)">{{ t('common.cancel') }}</md-text-button></div>
  </dialog>
</template>
<script setup lang="ts">
import HelpTooltip from "../common/HelpTooltip.vue";
import { ref, watch } from "vue"; import { useI18n } from "vue-i18n"; import { useAuthStore, LoginError } from "../../stores/auth"; import { errorMessage } from "../../api/tauri"; import { notify } from "../../ui/notify";
const props = defineProps<{ show: boolean }>(); const emit = defineEmits<{ (e: "update:show", value: boolean): void }>(); const { t } = useI18n(); const authStore = useAuthStore(); const dialog = ref<HTMLDialogElement>(); const tab = ref("builtin"); const phpsessidInput = ref(""); const cookieLoginLoading = ref(false); const inlineError = ref("");
watch(() => props.show, (show) => { inlineError.value = ""; if (!show) phpsessidInput.value = ""; if (!dialog.value) return; if (show && !dialog.value.open) dialog.value.showModal(); if (!show && dialog.value.open) dialog.value.close(); });
/** 切页签清空 Cookie 草稿前先看加载态：登录在途时草稿是失败重试的唯一凭据，不能清。 */
function onTabChange(event: Event) { tab.value = (event.currentTarget as unknown as { activeTabIndex: number }).activeTabIndex === 0 ? "builtin" : "cookie"; inlineError.value = ""; if (tab.value === "builtin" && !cookieLoginLoading.value) phpsessidInput.value = ""; }
function onCookieInput(event: Event) { phpsessidInput.value = (event.currentTarget as HTMLInputElement).value; }
async function handleCookieLogin() { const value = phpsessidInput.value.trim(); if (!value || cookieLoginLoading.value) return; cookieLoginLoading.value = true; inlineError.value = ""; try { await authStore.loginWithCookie(value); notify(t("auth.cookieLoginSuccess")); phpsessidInput.value = ""; emit("update:show", false); } catch (error) { inlineError.value = errorMessage(error) || t("auth.cookieLoginFailed"); } finally { cookieLoginLoading.value = false; } }
async function handleBuiltinLogin() { inlineError.value = ""; try { await authStore.login(); notify(t("auth.loginSuccess")); emit("update:show", false); } catch (error) { inlineError.value = error instanceof LoginError && error.status === "cancelled" ? t("auth.builtinLoginNoResult") : errorMessage(error) || t("auth.loginFailed"); } }
</script><style scoped>.login-dialog { width:min(520px,92vw); border:0; border-radius:28px; padding:24px; color:var(--ink); background:var(--md-sys-color-surface-container); }.login-dialog::backdrop{background:rgb(0 0 0 / 35%)}.dialog-content{display:grid;gap:16px;padding:20px 0 0}.dialog-actions{display:flex;justify-content:flex-end}</style>
