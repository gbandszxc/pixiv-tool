<template>
  <div class="page-view">
    <h1 class="page-title">{{ t('illust.title') }}</h1>

    <n-card :title="t('illust.newTask')" class="form-card">
      <n-form label-placement="left" label-width="80">
        <n-form-item :label="t('illust.sourceType')">
          <n-radio-group v-model:value="form.sourceType">
            <n-radio value="single">{{ t('illust.single') }}</n-radio>
            <n-radio value="user">{{ t('illust.user') }}</n-radio>
          </n-radio-group>
        </n-form-item>

        <n-form-item :label="t('illust.input')">
          <n-input
            v-model:value="form.sourceId"
            :placeholder="inputPlaceholder"
            @keyup.enter="handleSubmit"
          />
        </n-form-item>

        <n-form-item>
          <n-button type="primary" :loading="submitting" @click="handleSubmit">
            {{ t('illust.start') }}
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
import { NCard, NForm, NFormItem, NInput, NRadioGroup, NRadio, NButton, NAlert } from "naive-ui";
import { useI18n } from "vue-i18n";
import { useTaskStore } from "../stores/tasks";

const { t } = useI18n();
const taskStore = useTaskStore();

const form = ref({
  sourceType: "single",
  sourceId: "",
});

const submitting = ref(false);
const message = ref("");
const messageType = ref<"success" | "error" | "info">("info");

const inputPlaceholder = computed(() => {
  const key = form.value.sourceType as "single" | "user";
  return t(`illust.inputPlaceholder.${key}`);
});

function extractId(input: string): string {
  // 从 URL 提取 ID: https://www.pixiv.net/artworks/12345
  const artworkMatch = input.match(/\/artworks\/(\d+)/);
  if (artworkMatch) return artworkMatch[1];
  // https://www.pixiv.net/users/12345
  const userMatch = input.match(/\/users\/(\d+)/);
  if (userMatch) return userMatch[1];
  // /user/12345
  const userAltMatch = input.match(/\/user\/(\d+)/);
  if (userAltMatch) return userAltMatch[1];
  // 纯数字
  return input.trim();
}

async function handleSubmit() {
  const sourceId = extractId(form.value.sourceId);
  if (!sourceId) {
    message.value = t("illust.invalidInput");
    messageType.value = "error";
    return;
  }

  submitting.value = true;
  message.value = "";

  try {
    const result = await taskStore.createTask(
      form.value.sourceType,
      sourceId,
      [],
      "illustration",
    );
    if (result.error) {
      message.value = result.error;
      messageType.value = "error";
    } else {
      message.value = t("illust.taskCreated", { id: result.task_id });
      messageType.value = "success";
      form.value.sourceId = "";
    }
  } catch (err: unknown) {
    const error = err as { response?: { data?: { error?: string } } };
    message.value = error.response?.data?.error || t("illust.createFailed");
    messageType.value = "error";
  } finally {
    submitting.value = false;
  }
}
</script>
