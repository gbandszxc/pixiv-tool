<template>
  <div class="settings-view">
    <h1>{{ t('settings.title') }}</h1>

    <n-card style="max-width: 600px">
      <n-form label-placement="left" label-width="100">
        <n-form-item :label="t('settings.outputDir')">
          <n-input v-model:value="form.output_dir" placeholder="downloads" />
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

    <n-alert v-if="message" :type="messageType" style="margin-top: 16px; max-width: 600px">
      {{ message }}
    </n-alert>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { NCard, NForm, NFormItem, NInput, NSelect, NCheckboxGroup, NCheckbox, NButton, NSpace, NAlert, NPopconfirm } from "naive-ui";
import { useI18n } from "vue-i18n";
import { useSettingsStore } from "../stores/settings";
import { useAuthStore } from "../stores/auth";

const { t, locale } = useI18n();
const settingsStore = useSettingsStore();
const authStore = useAuthStore();

const form = ref({
  output_dir: "downloads",
  output_formats: ["txt", "markdown"],
  language: locale.value,
  theme: "auto",
});

const message = ref("");
const messageType = ref<"success" | "error">("success");

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
  form.value = { ...settingsStore.settings };
  // 以 settings 中保存的语言为准（若已持久化）
  if (form.value.language && form.value.language !== locale.value) {
    locale.value = form.value.language;
    localStorage.setItem("pixiv-tool-lang", form.value.language);
  }
});

async function handleSave() {
  try {
    await settingsStore.saveSettings(form.value);
    message.value = t("settings.saved");
    messageType.value = "success";
  } catch {
    message.value = t("settings.saveFailed");
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
