<template>
  <dialog ref="dialog" class="m3-dialog update-dialog" tabindex="-1" aria-labelledby="update-title" @cancel.prevent="requestClose">
    <h2 id="update-title" aria-live="polite">{{ t(`update.titles.${phase}`) }}</h2>
    <p v-if="phase === 'available'">{{ t('auth.updateAvailableText', { latest: info.latest_version, current: info.current_version }) }}</p>
    <template v-if="phase === 'downloading'">
      <p>{{ t('update.version', { version: info.latest_version }) }}</p>
      <p class="file-name">{{ progress.file_name || t('update.preparing') }}</p>
      <md-linear-progress :value="fraction" :indeterminate="progress.total === 0" :aria-label="t('update.progress')" />
      <div class="download-stats" aria-live="off">
        <span>{{ formatBytes(progress.downloaded) }} / {{ progress.total ? formatBytes(progress.total) : '--' }}</span>
        <span>{{ Math.floor(fraction * 100) }}% · {{ formatBytes(progress.bytes_per_second) }}/s</span>
      </div>
    </template>
    <p v-if="error" class="m3-alert error" role="alert">{{ error }}</p>
    <p v-if="phase === 'cancelled'" role="status">{{ t('update.cancelled') }}</p>
    <template v-if="result && (phase === 'ready' || phase === 'confirm' || phase === 'opening')">
      <p role="status">{{ t('update.ready') }}</p>
      <p>{{ t(`update.install.${info.platform === 'macos' ? 'macos' : info.platform === 'linux' ? 'linux' : 'windows'}`) }}</p>
      <p>{{ t('update.restart') }}</p>
      <p v-if="phase === 'confirm'" class="m3-alert error" role="alert">{{ t('update.activeTasks') }}</p>
      <p class="file-name">{{ result.path }}</p>
      <md-linear-progress v-if="phase === 'opening'" indeterminate :aria-label="t('update.titles.opening')" />
    </template>
    <div class="m3-row dialog-actions">
      <md-text-button v-if="!busy" @click="requestClose">{{ t(result ? 'update.later' : 'common.cancel') }}</md-text-button>
      <md-text-button v-if="phase === 'downloading'" :disabled="cancelling" @click="requestClose">{{ t('update.continueUsing') }}</md-text-button>
      <md-text-button v-if="phase === 'downloading'" :disabled="cancelling" @click="cancel">{{ t(cancelling ? 'update.cancelling' : 'update.cancelDownload') }}</md-text-button>
      <md-outlined-button v-if="phase === 'error'" @click="openRelease">{{ t('update.openRelease') }}</md-outlined-button>
      <md-filled-button v-if="phase === 'available' || phase === 'error' || phase === 'cancelled'" @click="download">{{ t(phase === 'available' ? 'update.download' : 'update.retry') }}</md-filled-button>
      <md-outlined-button v-if="result && !busy" @click="openDirectory">{{ t('update.openDirectory') }}</md-outlined-button>
      <md-text-button v-if="result && !busy" @click="download">{{ t('update.retry') }}</md-text-button>
      <md-filled-button v-if="phase === 'ready' || phase === 'confirm'" @click="install">{{ t(phase === 'confirm' ? 'update.confirmInstall' : 'update.installNow') }}</md-filled-button>
    </div>
  </dialog>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cancelAppUpdate, downloadAppUpdate, installAppUpdate, openUpdateDirectory, type UpdateCheckInfo, type UpdateDownloadResult, type UpdateProgress } from "../../api/appUpdate";
import { errorMessage } from "../../api/tauri";
import { isActiveJobState, listTranslationJobs } from "../../api/translation";
import { useTaskStore } from "../../stores/tasks";
import { useTranslationStore } from "../../stores/translation";
import { notify } from "../../ui/notify";

const { t } = useI18n();
const tasks = useTaskStore();
const translations = useTranslationStore();
const dialog = ref<HTMLDialogElement>();
const phase = ref<"available" | "downloading" | "opening" | "error" | "cancelled" | "ready" | "confirm">("available");
const info = ref<UpdateCheckInfo>({ has_update: false, current_version: "--", latest_version: "--", release_url: "", platform: "", package_type: null });
const emptyProgress = (): UpdateProgress => ({ phase: "downloading", file_name: "", downloaded: 0, total: 0, bytes_per_second: 0 });
const progress = ref(emptyProgress());
const result = ref<UpdateDownloadResult>();
const error = ref("");
const cancelling = ref(false);
const busy = computed(() => phase.value === "downloading" || phase.value === "opening");
const fraction = computed(() => progress.value.total ? Math.min(1, progress.value.downloaded / progress.value.total) : 0);
watch(phase, async () => {
  await nextTick();
  if (!dialog.value?.open) return;
  // 状态切换会移除原焦点按钮，保持键盘焦点在模态内。
  const action = dialog.value.querySelector<HTMLElement>('md-filled-button, md-outlined-button, md-text-button');
  (action ?? dialog.value).focus();
});
function formatBytes(bytes: number) {
  if (bytes < 1000) return `${Math.round(bytes)} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1000, index = 0;
  while (value >= 1000 && index < units.length - 1) { value /= 1000; index++; }
  return `${value.toFixed(1)} ${units[index]}`;
}
function open(value: UpdateCheckInfo) {
  if (dialog.value?.open) return;
  if (!busy.value && info.value.latest_version !== value.latest_version) {
    info.value = { ...value }; phase.value = "available"; result.value = undefined; error.value = "";
  } else if (!busy.value && phase.value === "confirm") {
    phase.value = "ready";
  }
  dialog.value?.showModal();
}
defineExpose({ open });
function requestClose() {
  if (phase.value !== "opening") dialog.value?.close();
}
async function cancel() {
  if (cancelling.value) return;
  cancelling.value = true;
  try { await cancelAppUpdate(); }
  catch (value) { cancelling.value = false; notify(errorMessage(value)); }
}
async function download() {
  if (busy.value) return;
  phase.value = "downloading"; cancelling.value = false; error.value = ""; result.value = undefined; progress.value = emptyProgress();
  let acceptingProgress = true;
  try {
    result.value = await downloadAppUpdate(info.value.latest_version, value => {
      if (acceptingProgress) progress.value = value;
    });
    phase.value = "ready";
    if (!dialog.value?.open) notify(t('update.downloadReady'));
  } catch (value) {
    error.value = errorMessage(value);
    phase.value = error.value === "更新下载已取消" ? "cancelled" : "error";
    if (phase.value === "cancelled") error.value = "";
    if (!dialog.value?.open) notify(error.value || t('update.cancelled'));
  } finally { acceptingProgress = false; cancelling.value = false; }
}
async function install() {
  if (busy.value || !result.value) return;
  const confirmed = phase.value === "confirm";
  phase.value = "opening"; error.value = "";
  if (!confirmed) {
    let activeTranslation = translations.activeJobs.length > 0;
    try {
      const snapshot = await listTranslationJobs();
      activeTranslation ||= snapshot.jobs.some(job => isActiveJobState(job.state));
    } catch { activeTranslation = true; }
    if (tasks.activeTasks.length > 0 || tasks.syncFailed || activeTranslation) {
      phase.value = "confirm";
      return;
    }
  }
  try { await installAppUpdate(); }
  catch (value) { error.value = errorMessage(value); phase.value = "ready"; }
}
async function openRelease() { try { await openUrl(info.value.release_url); } catch (value) { notify(errorMessage(value)); } }
async function openDirectory() { try { await openUpdateDirectory(); } catch (value) { notify(errorMessage(value)); } }
onBeforeUnmount(() => { if (phase.value === "downloading") void cancelAppUpdate().catch(() => {}); });
</script>

<style scoped>
.update-dialog { width: min(480px, 90vw); box-sizing: border-box; max-height: 88vh; overflow-y: auto; }
.file-name { overflow-wrap: anywhere; color: var(--ink-muted); margin-block: var(--space-lg); }
.download-stats { display: flex; flex-wrap: wrap; justify-content: space-between; gap: var(--space-sm); margin-top: var(--space-sm); font-variant-numeric: tabular-nums; }
.error { overflow-wrap: anywhere; }
</style>
