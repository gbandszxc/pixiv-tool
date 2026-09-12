<template>
  <div class="page-view">
    <h1 class="page-title">{{ t("settings.title") }}</h1>
    <section class="form-card" :aria-label="t('settings.title')">
      <div class="m3-field">
        <label for="output-dir">{{ t("settings.outputDir") }}</label>
        <div class="settings-path-row">
          <md-outlined-text-field id="output-dir" class="settings-path-input" :value="form.output_dir" :placeholder="t('settings.outputDirPlaceholder')" @input="form.output_dir = ($event.target as HTMLInputElement).value" />
          <md-outlined-button @click="handleBrowse">{{ t("settings.browse") }}</md-outlined-button>
        </div>
      </div>
      <fieldset class="m3-field settings-fieldset">
        <legend>{{ t("settings.outputFormats") }}</legend>
        <div class="m3-row"><label v-for="format in formats" :key="format" class="m3-choice"><md-checkbox :checked="form.output_formats.includes(format)" @change="toggleFormat(format, ($event.target as HTMLInputElement).checked)" />{{ format === "txt" ? "TXT" : "Markdown" }}</label></div>
      </fieldset>
      <div class="m3-field">
        <label for="language">{{ t("settings.language") }}</label>
        <md-outlined-select id="language" :value="form.language" @change="changeLang(($event.target as HTMLSelectElement).value)"><md-select-option v-for="option in langOptions" :key="option.value" :value="option.value">{{ option.label }}</md-select-option></md-outlined-select>
      </div>
      <div class="m3-field">
        <label for="theme">{{ t("settings.theme") }}</label>
        <md-outlined-select id="theme" :value="form.theme" @change="form.theme = ($event.target as HTMLSelectElement).value"><md-select-option v-for="option in themeOptions" :key="option.value" :value="option.value">{{ option.label }}</md-select-option></md-outlined-select>
      </div>
      <div class="m3-field">
        <label for="max-wait">{{ t("settings.maxWait") }}</label>
        <div class="m3-row"><md-outlined-text-field id="max-wait" class="settings-number-input" type="number" min="30" max="86400" step="30" :value="String(form.max_wait_seconds)" @input="form.max_wait_seconds = Number(($event.target as HTMLInputElement).value)" /><span class="field-hint">{{ t("settings.maxWaitHint") }}</span></div>
      </div>
      <div class="m3-row settings-actions"><md-filled-button @click="handleSave">{{ t("common.save") }}</md-filled-button><md-outlined-button @click="openConfirm('logs')">{{ t("settings.clearLogs") }}</md-outlined-button><md-text-button class="danger-button" @click="openConfirm('auth')">{{ t("settings.clearAuth") }}</md-text-button></div>
    </section>
    <div v-if="message" class="m3-alert" :class="messageType" role="status">{{ message }}</div>
    <dialog ref="confirmDialog" class="m3-dialog" @close="confirmAction = null">
      <h2>{{ confirmAction === "logs" ? t("settings.clearLogs") : t("settings.clearAuth") }}</h2>
      <p>{{ confirmAction === "logs" ? t("settings.clearLogsConfirm") : t("settings.clearAuthConfirm") }}</p>
      <div class="m3-row dialog-actions"><md-text-button @click="closeConfirm">{{ t("common.cancel") }}</md-text-button><md-filled-button @click="handleConfirm">{{ confirmAction === "logs" ? t("settings.clearLogs") : t("settings.clearAuth") }}</md-filled-button></div>
    </dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useSettingsStore } from "../stores/settings";
import { useAuthStore } from "../stores/auth";
import { errorMessage } from "../api/tauri";

const { t, locale } = useI18n();
const settingsStore = useSettingsStore();
const authStore = useAuthStore();
const formats = ["txt", "markdown"];
const confirmDialog = ref<HTMLDialogElement | null>(null);
const confirmAction = ref<"logs" | "auth" | null>(null);
const form = ref({ output_dir: "downloads", output_formats: ["txt", "markdown"], language: locale.value, theme: "auto", max_wait_seconds: 180 });
const message = ref("");
const messageType = ref<"success" | "error">("success");
const langOptions = computed(() => [{ label: t("settings.languages.zh-CN"), value: "zh-CN" }, { label: t("settings.languages.en-US"), value: "en-US" }]);
const themeOptions = computed(() => [{ label: t("settings.themes.light"), value: "light" }, { label: t("settings.themes.dark"), value: "dark" }, { label: t("settings.themes.auto"), value: "auto" }]);

function toggleFormat(format: string, checked: boolean) { form.value.output_formats = checked ? [...form.value.output_formats, format] : form.value.output_formats.filter((item) => item !== format); }
function changeLang(lang: string) { locale.value = lang; form.value.language = lang; localStorage.setItem("pixiv-tool-lang", lang); }
onMounted(async () => { await settingsStore.fetchSettings(); if (typeof settingsStore.settings.max_wait_seconds !== "number") settingsStore.settings.max_wait_seconds = 180; form.value = { ...settingsStore.settings }; if (form.value.language && form.value.language !== locale.value) changeLang(form.value.language); });
async function handleBrowse() { try { const path = await settingsStore.selectDirectory(); if (path) form.value.output_dir = path; } catch { message.value = t("settings.pickFailed"); messageType.value = "error"; } }
async function handleSave() { if (!form.value.max_wait_seconds || form.value.max_wait_seconds < 30) { message.value = t("settings.maxWaitInvalid"); messageType.value = "error"; return; } try { await settingsStore.saveSettings(form.value); message.value = t("settings.saved"); messageType.value = "success"; } catch (err) { message.value = errorMessage(err) || t("settings.saveFailed"); messageType.value = "error"; } }
async function openConfirm(action: "logs" | "auth") { confirmAction.value = action; await nextTick(); confirmDialog.value?.showModal(); }
function closeConfirm() { confirmDialog.value?.close(); }
async function handleConfirm() { const action = confirmAction.value; closeConfirm(); if (action === "logs") { await settingsStore.clearLogs(); message.value = t("settings.logsCleared"); } else if (action === "auth") { await authStore.logout(); message.value = t("settings.authCleared"); } messageType.value = "success"; }
</script>

<style scoped>
.settings-path-row { display: flex; gap: var(--space-sm); }
.settings-path-input { flex: 1; min-width: 0; }
.settings-fieldset { min-width: 0; padding: 0; border: 0; }
.settings-fieldset legend { padding: 0; color: var(--ink-muted); font-size: 14px; font-weight: 500; }
.settings-number-input { width: 140px; }
.settings-actions { margin-top: var(--space-xl); }
.field-hint { color: var(--ink-muted); font-size: 12px; }
.danger-button { --md-text-button-label-text-color: #ba1a1a; }
.dialog-actions { justify-content: flex-end; }
@media (max-width: 640px) { .settings-path-row { align-items: stretch; flex-direction: column; } }
</style>
