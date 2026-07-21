<template>
  <div class="page-view">
    <h1 class="page-title">{{ t('crawl.title') }}</h1>

    <n-card :title="t('crawl.newTask')" class="form-card">
      <n-form label-placement="left" label-width="80">
        <n-form-item :label="t('crawl.sourceType')">
          <n-radio-group v-model:value="form.sourceType">
            <n-radio value="single">{{ t('crawl.single') }}</n-radio>
            <n-radio value="series">{{ t('crawl.series') }}</n-radio>
            <n-radio value="user">{{ t('crawl.user') }}</n-radio>
          </n-radio-group>
        </n-form-item>

        <n-form-item :label="t('crawl.input')">
          <n-input
            v-model:value="form.sourceId"
            :placeholder="inputPlaceholder"
            @keyup.enter="handleSubmit"
          />
        </n-form-item>

        <n-form-item :label="t('crawl.outputFormats')">
          <n-checkbox-group v-model:value="form.formats">
            <n-checkbox value="txt">TXT</n-checkbox>
            <n-checkbox value="markdown">Markdown</n-checkbox>
          </n-checkbox-group>
        </n-form-item>

        <n-form-item>
          <n-button type="primary" :loading="submitting" @click="handleSubmit">
            {{ t('crawl.start') }}
          </n-button>
        </n-form-item>
      </n-form>
    </n-card>

    <n-alert v-if="message" :type="messageType" class="page-alert">
      {{ message }}
    </n-alert>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from "vue";
import { NCard, NForm, NFormItem, NInput, NRadioGroup, NRadio, NCheckboxGroup, NCheckbox, NButton, NAlert } from "naive-ui";
import { useI18n } from "vue-i18n";
import { useTaskStore } from "../stores/tasks";

const { t } = useI18n();
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
  const key = form.value.sourceType as "single" | "series" | "user";
  return t(`crawl.inputPlaceholder.${key}`);
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
    message.value = t("crawl.invalidInput");
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
      message.value = t("crawl.taskCreated", { id: result.task_id });
      messageType.value = "success";
      form.value.sourceId = "";
    }
  } catch (err: unknown) {
    const error = err as { response?: { data?: { error?: string } } };
    message.value = error.response?.data?.error || t("crawl.createFailed");
    messageType.value = "error";
  } finally {
    submitting.value = false;
  }
}
</script>
