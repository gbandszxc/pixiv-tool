<template>
  <n-modal
    :show="show"
    preset="card"
    :title="t('auth.loginDialogTitle')"
    style="width: 480px; max-width: 92vw"
    :bordered="false"
    size="huge"
    @update:show="onUpdateShow"
  >
    <n-tabs type="line" default-value="builtin">
      <!-- 手动 Session 兜底 -->
      <n-tab-pane name="cookie" :tab="t('auth.cookieLoginTab')">
        <n-space vertical :size="12">
          <n-alert type="info" :show-icon="true" :bordered="false">
            <span class="login-hint">{{ t('auth.cookieLoginDesc') }}</span>
          </n-alert>
          <n-text depth="3" class="login-steps">{{ t('auth.cookieLoginSteps') }}</n-text>
          <n-form-item :label="t('auth.phpsessidLabel')" :show-feedback="false">
            <n-input
              v-model:value="phpsessidInput"
              :placeholder="t('auth.phpsessidPlaceholder')"
              :disabled="cookieLoginLoading"
              clearable
              @keyup.enter="handleCookieLogin"
            />
          </n-form-item>
          <n-text depth="3" style="font-size: 12px">{{ t('auth.csrfAutoFillHint') }}</n-text>
          <n-button
            type="primary"
            block
            :loading="cookieLoginLoading"
            :disabled="!phpsessidInput.trim()"
            @click="handleCookieLogin"
          >
            {{ t('auth.importAndLogin') }}
          </n-button>
        </n-space>
      </n-tab-pane>

      <!-- 真实 Chromium 浏览器登录 -->
      <n-tab-pane name="builtin" :tab="t('auth.builtinLoginTab')">
        <n-space vertical :size="12">
          <n-text depth="3">{{ t('auth.builtinLoginDesc') }}</n-text>
          <n-button
            type="primary"
            block
            :loading="authStore.isLoggingIn"
            :disabled="authStore.isLoggingIn"
            @click="handleBuiltinLogin"
          >
            {{ authStore.isLoggingIn ? t('auth.loggingIn') : t('auth.openBuiltinLogin') }}
          </n-button>
        </n-space>
      </n-tab-pane>
    </n-tabs>
  </n-modal>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  NModal,
  NTabs,
  NTabPane,
  NSpace,
  NInput,
  NFormItem,
  NButton,
  NText,
  NAlert,
  useMessage,
} from "naive-ui";
import { useAuthStore } from "../../stores/auth";

// 此组件渲染在 NMessageProvider 子树内,useMessage 可正常工作。
const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{ (e: "update:show", v: boolean): void }>();

const { t } = useI18n();
const message = useMessage();
const authStore = useAuthStore();

const phpsessidInput = ref("");
const cookieLoginLoading = ref(false);

function onUpdateShow(v: boolean) {
  if (!v) {
    // 关闭时不清空输入(用户可能只是瞄一眼另一个 tab),但清错误态
    cookieLoginLoading.value = false;
  }
  emit("update:show", v);
}

async function handleCookieLogin() {
  const value = phpsessidInput.value.trim();
  if (!value) return;
  cookieLoginLoading.value = true;
  try {
    await authStore.loginWithCookie(value);
    // checkStatus 已在 store 内调用,登录态更新后头像会自动显示
    message.success(t("auth.cookieLoginSuccess"));
    phpsessidInput.value = "";
    emit("update:show", false);
  } catch (err: unknown) {
    // 后端 HTTPException 的 detail 在 err.response.data.detail
    const detail =
      (err as { response?: { data?: { detail?: string } } })?.response?.data?.detail ||
      (err as Error)?.message ||
      t("auth.cookieLoginFailed");
    message.error(detail);
  } finally {
    cookieLoginLoading.value = false;
  }
}

async function handleBuiltinLogin() {
  try {
    await authStore.login();
    if (authStore.isLoggedIn) {
      message.success(t("auth.loginSuccess"));
      emit("update:show", false);
    } else {
      message.warning(t("auth.builtinLoginNoResult"));
    }
  } catch (err: unknown) {
    message.error((err as Error)?.message || t("auth.loginFailed"));
  }
}
</script>

<style scoped>
.login-hint {
  font-size: 13px;
  line-height: 1.6;
}

.login-steps {
  font-size: 12px;
  line-height: 1.7;
  white-space: pre-line;
}
</style>
