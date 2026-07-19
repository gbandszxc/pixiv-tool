<template>
  <div class="settings-view">
    <h1>设置</h1>

    <n-card style="max-width: 600px">
      <n-form label-placement="left" label-width="100">
        <n-form-item label="输出目录">
          <n-input v-model:value="form.output_dir" placeholder="downloads" />
        </n-form-item>

        <n-form-item label="输出格式">
          <n-checkbox-group v-model:value="form.output_formats">
            <n-checkbox value="txt">TXT</n-checkbox>
            <n-checkbox value="markdown">Markdown</n-checkbox>
          </n-checkbox-group>
        </n-form-item>

        <n-form-item label="语言">
          <n-select v-model:value="form.language" :options="langOptions" />
        </n-form-item>

        <n-form-item label="主题">
          <n-select v-model:value="form.theme" :options="themeOptions" />
        </n-form-item>

        <n-form-item>
          <n-space>
            <n-button type="primary" @click="handleSave">保存</n-button>
            <n-popconfirm @positive-click="handleClearLogs">
              <template #trigger>
                <n-button>清除日志</n-button>
              </template>
              确定清除日志文件？
            </n-popconfirm>
            <n-popconfirm @positive-click="handleLogout">
              <template #trigger>
                <n-button type="error">清除登录</n-button>
              </template>
              确定清除登录态？需要重新登录。
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
import { ref, onMounted } from "vue";
import { NCard, NForm, NFormItem, NInput, NSelect, NCheckboxGroup, NCheckbox, NButton, NSpace, NAlert, NPopconfirm } from "naive-ui";
import { useSettingsStore } from "../stores/settings";
import { useAuthStore } from "../stores/auth";

const settingsStore = useSettingsStore();
const authStore = useAuthStore();

const form = ref({
  output_dir: "downloads",
  output_formats: ["txt", "markdown"],
  language: "zh-CN",
  theme: "auto",
});

const message = ref("");
const messageType = ref<"success" | "error">("success");

const langOptions = [
  { label: "简体中文", value: "zh-CN" },
  { label: "English", value: "en-US" },
];

const themeOptions = [
  { label: "浅色", value: "light" },
  { label: "深色", value: "dark" },
  { label: "跟随系统", value: "auto" },
];

onMounted(async () => {
  await settingsStore.fetchSettings();
  form.value = { ...settingsStore.settings };
});

async function handleSave() {
  try {
    await settingsStore.saveSettings(form.value);
    message.value = "设置已保存";
    messageType.value = "success";
  } catch {
    message.value = "保存失败";
    messageType.value = "error";
  }
}

async function handleClearLogs() {
  await settingsStore.clearLogs();
  message.value = "日志已清除";
  messageType.value = "success";
}

async function handleLogout() {
  await authStore.logout();
  message.value = "已清除登录态";
  messageType.value = "success";
}
</script>
