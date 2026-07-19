<template>
  <div class="crawl-view">
    <h1>抓取</h1>

    <n-card title="新建抓取任务" style="max-width: 600px">
      <n-form label-placement="left" label-width="80">
        <n-form-item label="来源类型">
          <n-radio-group v-model:value="form.sourceType">
            <n-radio value="single">单篇</n-radio>
            <n-radio value="series">系列</n-radio>
            <n-radio value="user">用户</n-radio>
          </n-radio-group>
        </n-form-item>

        <n-form-item label="输入">
          <n-input
            v-model:value="form.sourceId"
            :placeholder="inputPlaceholder"
            @keyup.enter="handleSubmit"
          />
        </n-form-item>

        <n-form-item label="输出格式">
          <n-checkbox-group v-model:value="form.formats">
            <n-checkbox value="txt">TXT</n-checkbox>
            <n-checkbox value="markdown">Markdown</n-checkbox>
          </n-checkbox-group>
        </n-form-item>

        <n-form-item>
          <n-button type="primary" :loading="submitting" @click="handleSubmit">
            开始抓取
          </n-button>
        </n-form-item>
      </n-form>
    </n-card>

    <n-alert v-if="message" :type="messageType" style="margin-top: 16px; max-width: 600px">
      {{ message }}
    </n-alert>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from "vue";
import { NCard, NForm, NFormItem, NInput, NRadioGroup, NRadio, NCheckboxGroup, NCheckbox, NButton, NAlert } from "naive-ui";
import { useTaskStore } from "../stores/tasks";

const taskStore = useTaskStore();

const form = ref({
  sourceType: "single",
  sourceId: "",
  formats: ["txt", "markdown"],
});

const submitting = ref(false);
const message = ref("");
const messageType = ref<"success" | "error" | "info">("info");

const inputPlaceholder = computed(() => {
  const map: Record<string, string> = {
    single: "输入小说 ID 或 URL",
    series: "输入系列 ID 或 URL",
    user: "输入用户 ID 或 URL",
  };
  return map[form.value.sourceType] || "";
});

function extractId(input: string): string {
  // 从 URL 提取 ID: https://www.pixiv.net/novel/show.php?id=12345
  const urlMatch = input.match(/[?&]id=(\d+)/);
  if (urlMatch) return urlMatch[1];
  // /novel/12345
  const pathMatch = input.match(/\/novel\/(\d+)/);
  if (pathMatch) return pathMatch[1];
  // /user/12345
  const userMatch = input.match(/\/user\/(\d+)/);
  if (userMatch) return userMatch[1];
  // /series/12345
  const seriesMatch = input.match(/\/series\/(\d+)/);
  if (seriesMatch) return seriesMatch[1];
  // 纯数字
  return input.trim();
}

async function handleSubmit() {
  const sourceId = extractId(form.value.sourceId);
  if (!sourceId) {
    message.value = "请输入有效的 ID 或 URL";
    messageType.value = "error";
    return;
  }

  submitting.value = true;
  message.value = "";

  try {
    const result = await taskStore.createTask(
      form.value.sourceType,
      sourceId,
      form.value.formats,
    );
    if (result.error) {
      message.value = result.error;
      messageType.value = "error";
    } else {
      message.value = `任务已创建: ${result.task_id}`;
      messageType.value = "success";
      form.value.sourceId = "";
    }
  } catch (err: unknown) {
    const error = err as { response?: { data?: { error?: string } } };
    message.value = error.response?.data?.error || "创建任务失败";
    messageType.value = "error";
  } finally {
    submitting.value = false;
  }
}
</script>

<style scoped>
.crawl-view {
  padding: 0;
}
</style>
