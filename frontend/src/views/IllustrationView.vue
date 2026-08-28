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
import { ref, computed, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { NCard, NForm, NFormItem, NInput, NRadioGroup, NRadio, NButton, NAlert } from "naive-ui";
import { useI18n } from "vue-i18n";
import { useTaskStore } from "../stores/tasks";
import { errorMessage } from "../api/tauri";
import { parsePixivUrl } from "../utils/pixivUrl";

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
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
  const parsed = parsePixivUrl(input);
  if (parsed && (parsed.kind === "illustration" || parsed.kind === "user")) {
    return parsed.id;
  }
  return input.trim();
}

onMounted(() => {
  if (typeof route.query.sourceType === "string" && ["single", "user"].includes(route.query.sourceType)) {
    form.value.sourceType = route.query.sourceType;
  }
  if (typeof route.query.sourceId === "string" && route.query.sourceId) {
    form.value.sourceId = route.query.sourceId;
  }
  if (route.query.sourceType || route.query.sourceId) {
    router.replace({ query: {} });
  }
});

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
    message.value = errorMessage(err) || t("illust.createFailed");
    messageType.value = "error";
  } finally {
    submitting.value = false;
  }
}
</script>
