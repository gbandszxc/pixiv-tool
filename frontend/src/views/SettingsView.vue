<template>
  <div class="page-view">
    <h1 class="page-title">{{ t('settings.title') }}</h1>

    <n-card class="form-card">
      <n-form label-placement="left" label-width="100">
        <n-form-item :label="t('settings.outputDir')">
          <n-input-group>
            <n-input v-model:value="form.output_dir" :placeholder="t('settings.outputDirPlaceholder')" />
            <n-button @click="handleBrowse">{{ t('settings.browse') }}</n-button>
          </n-input-group>
        </n-form-item>

        <n-form-item :label="t('settings.outputFormats')">
          <n-checkbox-group v-model:value="form.output_formats">
            <n-checkbox value="txt">TXT</n-checkbox>
            <n-checkbox value="markdown">Markdown</n-checkbox>
          </n-checkbox-group>
        </n-form-item>

        <n-form-item :label="t('settings.language')">
          <n-select
            :value="form.language"
            :options="langOptions"
            @update:value="changeLang"
          />
        </n-form-item>

        <n-form-item :label="t('settings.theme')">
          <n-select v-model:value="form.theme" :options="themeOptions" />
        </n-form-item>

        <n-form-item :label="t('settings.maxWait')">
          <n-input-number v-model:value="form.max_wait_seconds" :min="30" :max="86400" :step="30" style="width: 140px" />
          <span class="field-hint">{{ t('settings.maxWaitHint') }}</span>
        </n-form-item>

        <n-form-item>
          <n-space>
            <n-button type="primary" @click="handleSave">{{ t('common.save') }}</n-button>
            <n-popconfirm @positive-click="handleClearLogs">
              <template #trigger>
                <n-button>{{ t('settings.clearLogs') }}</n-button>
              </template>
              {{ t('settings.clearLogsConfirm') }}
            </n-popconfirm>
            <n-popconfirm @positive-click="handleLogout">
              <template #trigger>
                <n-button type="error">{{ t('settings.clearAuth') }}</n-button>
              </template>
              {{ t('settings.clearAuthConfirm') }}
            </n-popconfirm>
          </n-space>
        </n-form-item>
      </n-form>
    </n-card>

    <n-alert v-if="message" :type="messageType" class="page-alert">
      {{ message }}
    </n-alert>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { NCard, NForm, NFormItem, NInput, NInputGroup, NInputNumber, NSelect, NCheckboxGroup, NCheckbox, NButton, NSpace, NAlert, NPopconfirm, useMessage } from "naive-ui";
import { useI18n } from "vue-i18n";
import { useSettingsStore } from "../stores/settings";
import { useAuthStore } from "../stores/auth";
import { errorMessage } from "../api/tauri";

const { t, locale } = useI18n();
const settingsStore = useSettingsStore();
const authStore = useAuthStore();

const form = ref({
  output_dir: "downloads",
  output_formats: ["txt", "markdown"],
  language: locale.value,
  theme: "auto",
  max_wait_seconds: 180,
});

const message = ref("");
const messageType = ref<"success" | "error">("success");
const browseMessage = useMessage();

// 切换语言时整个 UI 立即更新（locale 是响应式 ref，computed 自动追踪）
const langOptions = computed(() => [
  { label: t("settings.languages.zh-CN"), value: "zh-CN" },
  { label: t("settings.languages.en-US"), value: "en-US" },
]);

const themeOptions = computed(() => [
  { label: t("settings.themes.light"), value: "light" },
  { label: t("settings.themes.dark"), value: "dark" },
  { label: t("settings.themes.auto"), value: "auto" },
]);

function changeLang(lang: string) {
  locale.value = lang;
  form.value.language = lang;
  localStorage.setItem("pixiv-tool-lang", lang);
}

onMounted(async () => {
  await settingsStore.fetchSettings();
  // 旧设置文件没有 max_wait_seconds 时回填默认值，避免保存时覆盖为空
  if (typeof settingsStore.settings.max_wait_seconds !== "number") {
    settingsStore.settings.max_wait_seconds = 180;
  }
  form.value = { ...settingsStore.settings };
  // 以 settings 中保存的语言为准（若已持久化）
  if (form.value.language && form.value.language !== locale.value) {
    locale.value = form.value.language;
    localStorage.setItem("pixiv-tool-lang", form.value.language);
  }
});

async function handleBrowse() {
  try {
    const p = await settingsStore.selectDirectory();
    if (p) form.value.output_dir = p;
  } catch {
    browseMessage.error(t("settings.pickFailed"));
  }
}

async function handleSave() {
  if (!form.value.max_wait_seconds || form.value.max_wait_seconds < 30) {
    message.value = t("settings.maxWaitInvalid");
    messageType.value = "error";
    return;
  }
  try {
    await settingsStore.saveSettings(form.value);
    message.value = t("settings.saved");
    messageType.value = "success";
  } catch (err) {
    const detail = errorMessage(err);
    message.value = detail || t("settings.saveFailed");
    messageType.value = "error";
  }
}

async function handleClearLogs() {
  await settingsStore.clearLogs();
  message.value = t("settings.logsCleared");
  messageType.value = "success";
}

async function handleLogout() {
  await authStore.logout();
  message.value = t("settings.authCleared");
  messageType.value = "success";
}
</script>

<style scoped>
.field-hint {
  margin-left: 8px;
  color: var(--ink-muted);
  font-size: 12px;
}
</style>
