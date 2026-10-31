<template>
  <dialog ref="dialog" class="m3-dialog update-dialog" tabindex="-1" aria-labelledby="update-title" @cancel.prevent="requestClose">
    <h2 id="update-title" aria-live="polite">{{ t(`update.titles.${phase}`) }}</h2>
    <p v-if="phase === 'available'">{{ t('auth.updateAvailableText', { latest: info.latest_version, current: info.current_version }) }}</p>

    <template v-if="phase === 'downloading'">
      <p class="update-version">{{ t('update.version', { version: info.latest_version }) }}</p>
      <p class="update-file">{{ progress.file_name || t('update.preparing') }}</p>
      <md-linear-progress :value="fraction" :indeterminate="progress.total === 0" :aria-label="t('update.progress')" />
      <div class="download-stats" aria-live="off">
        <span>{{ formatBytes(progress.downloaded) }} / {{ progress.total ? formatBytes(progress.total) : '--' }}</span>
        <span>{{ Math.floor(fraction * 100) }}% · {{ formatBytes(progress.bytes_per_second) }}/s</span>
      </div>
    </template>

    <p v-if="error" class="m3-alert error" role="alert">{{ error }}</p>
    <p v-if="phase === 'cancelled'" class="update-note" role="status">{{ t('update.cancelled') }}</p>

    <template v-if="result && (phase === 'ready' || phase === 'confirm' || phase === 'opening')">
      <p role="status">{{ t('update.ready') }}</p>
      <p class="update-note">{{ t(`update.install.${platformKey}`) }}</p>
      <p v-if="phase === 'confirm'" class="m3-alert error update-warning" role="alert">{{ t('update.activeTasks') }}</p>
      <div class="update-path-row">
        <p class="update-path">{{ result.path }}</p>
        <button class="update-folder" type="button" :aria-label="t('update.openDirectory')" :title="t('update.openDirectory')" @click="openDirectory">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2" /></svg>
        </button>
      </div>
      <md-linear-progress v-if="phase === 'opening'" indeterminate :aria-label="t('update.titles.opening')" />
    </template>

    <div class="m3-row dialog-actions update-actions">
      <md-text-button v-if="phase === 'downloading'" :disabled="cancelling" @click="requestClose">{{ t('update.continueUsing') }}</md-text-button>
      <md-text-button v-if="phase === 'downloading'" :disabled="cancelling" @click="cancel">{{ t(cancelling ? 'update.cancelling' : 'update.cancelDownload') }}</md-text-button>

      <md-text-button v-if="!busy" class="update-aux" @click="requestClose">{{ t(result ? 'update.later' : 'common.cancel') }}</md-text-button>
      <md-text-button v-if="phase === 'error'" class="update-aux" @click="openRelease">{{ t('update.openRelease') }}</md-text-button>

      <md-filled-button v-if="phase === 'available' || phase === 'error' || phase === 'cancelled'" @click="download">{{ t(phase === 'available' ? 'update.download' : 'update.retry') }}</md-filled-button>
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
/** 开发预览（仅账号菜单的 DEV 入口传入）：不调用任何真实下载 / 安装命令。 */
const preview = ref(false);
let previewTimer: number | undefined;
const busy = computed(() => phase.value === "downloading" || phase.value === "opening");
const fraction = computed(() => progress.value.total ? Math.min(1, progress.value.downloaded / progress.value.total) : 0);
const platformKey = computed(() => info.value.platform === "macos" ? "macos" : info.value.platform === "linux" ? "linux" : "windows");
watch(phase, async () => {
  await nextTick();
  if (!dialog.value?.open) return;
  // 状态切换后焦点留在模态容器：不抢按钮焦点，鼠标操作不会出现 focus-visible 描边；
  // 键盘用户按一次 Tab 即进入动作行。
  dialog.value.focus();
});
function formatBytes(bytes: number) {
  if (bytes < 1000) return `${Math.round(bytes)} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1000, index = 0;
  while (value >= 1000 && index < units.length - 1) { value /= 1000; index++; }
  return `${value.toFixed(1)} ${units[index]}`;
}
function clearPreviewTimer() {
  window.clearInterval(previewTimer);
  window.clearTimeout(previewTimer);
  previewTimer = undefined;
}
function open(value: UpdateCheckInfo, options?: { preview?: boolean }) {
  if (dialog.value?.open) return;
  preview.value = options?.preview === true;
  if (preview.value) {
    clearPreviewTimer();
    info.value = { ...value };
    phase.value = "available";
    result.value = undefined;
    error.value = "";
    progress.value = emptyProgress();
  } else if (!busy.value && info.value.latest_version !== value.latest_version) {
    info.value = { ...value }; phase.value = "available"; result.value = undefined; error.value = "";
  } else if (!busy.value && phase.value === "confirm") {
    phase.value = "ready";
  }
  dialog.value?.showModal();
  // showModal 默认聚焦首个可聚焦后代（按钮），会带出 focus-visible 描边；
  // 统一把初始焦点收到容器，Tab 再进入动作行。
  dialog.value?.focus();
}
defineExpose({ open });
function requestClose() {
  if (phase.value !== "opening") { clearPreviewTimer(); dialog.value?.close(); }
}
async function cancel() {
  if (preview.value) {
    clearPreviewTimer();
    phase.value = "cancelled";
    progress.value = emptyProgress();
    return;
  }
  if (cancelling.value) return;
  cancelling.value = true;
  try { await cancelAppUpdate(); }
  catch (value) { cancelling.value = false; notify(errorMessage(value)); }
}
/** 预览下载：固定 1.8s 走完，展示进度条与统计行；不写盘、不请求网络。 */
function previewDownload() {
  phase.value = "downloading"; cancelling.value = false; error.value = ""; result.value = undefined;
  const total = 23_540_000;
  const fileExt = info.value.platform === "macos" ? "dmg" : info.value.platform === "linux" ? "AppImage" : "exe";
  const fileName = `Pixiv Tool_${info.value.latest_version}_x64.${fileExt}`;
  progress.value = { phase: "downloading", file_name: fileName, downloaded: 0, total, bytes_per_second: 0 };
  const started = performance.now();
  clearPreviewTimer();
  previewTimer = window.setInterval(() => {
    const ratio = Math.min(1, (performance.now() - started) / 1_800);
    progress.value = {
      phase: "downloading",
      file_name: fileName,
      downloaded: Math.round(total * ratio),
      total,
      bytes_per_second: Math.round(total / 1.8),
    };
    if (ratio < 1) return;
    clearPreviewTimer();
    const directory = info.value.platform === "windows" ? "C:\\Users\\demo\\Downloads" : "/Users/demo/Downloads";
    const separator = info.value.platform === "windows" ? "\\" : "/";
    result.value = { path: `${directory}${separator}${fileName}`, package_type: info.value.package_type ?? "" };
    phase.value = "ready";
  }, 120);
}
async function download() {
  if (busy.value) return;
  if (preview.value) { previewDownload(); return; }
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
  if (preview.value) {
    // 预览完整走一遍「检测到活跃任务 → 确认 → 正在安装」的交互，不执行真实交接。
    if (!confirmed) { phase.value = "confirm"; return; }
    phase.value = "opening";
    previewTimer = window.setTimeout(() => {
      previewTimer = undefined;
      dialog.value?.close();
      phase.value = "ready";
      notify(t('update.previewDone'));
    }, 1_200);
    return;
  }
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
async function openDirectory() {
  if (preview.value) return;
  try { await openUpdateDirectory(); } catch (value) { notify(errorMessage(value)); }
}
onBeforeUnmount(() => {
  clearPreviewTimer();
  if (phase.value === "downloading" && !preview.value) void cancelAppUpdate().catch(() => {});
});
</script>

<style scoped>
/* 结构：标题 → 状态说明（muted）→ 平台提示（muted）→ 路径块 → 动作行。
 * 主操作是唯一 filled；其余为 text 且走 on-surface-variant，避免按钮抢焦点。
 * 动作行保持单行（480px 内三枚文字按钮可容纳），窄窗才换行。 */
.update-dialog { width: min(480px, 90vw); box-sizing: border-box; max-height: 88vh; overflow-y: auto; }
/* 焦点停在对话框容器（非可操作控件）时不画描边；Tab 进入动作行后由按钮自身显示焦点。 */
.update-dialog:focus,
.update-dialog:focus-visible { outline: none; }
.update-version { margin: var(--space-md) 0 0; }
.update-file { margin: var(--space-xxs) 0 0; color: var(--ink-muted); font-size: 12px; line-height: 1.5; overflow-wrap: anywhere; }
.update-dialog md-linear-progress { margin-top: var(--space-md); }
.download-stats { display: flex; flex-wrap: wrap; justify-content: space-between; gap: var(--space-sm); margin-top: var(--space-sm); color: var(--ink-muted); font-size: 12px; font-variant-numeric: tabular-nums; }
.update-note { margin: var(--space-md) 0 0; color: var(--ink-muted); font-size: 13px; line-height: 1.6; }
.update-note-tight { margin-top: var(--space-xxs); }
.update-warning { margin-top: var(--space-md); }
.update-path-row { display: flex; align-items: center; gap: var(--space-sm); margin-top: var(--space-lg); padding: var(--space-xs) var(--space-xs) var(--space-xs) var(--space-md); background: var(--md-sys-color-surface-container-highest); border-radius: var(--radius-control); }
.update-path { flex: 1; min-width: 0; margin: 0; color: var(--ink-muted); font-size: 12px; line-height: 1.5; overflow-wrap: anywhere; }
.update-folder { display: grid; place-items: center; flex: none; width: 28px; height: 28px; padding: 0; color: var(--ink-muted); background: transparent; border: 0; border-radius: 50%; cursor: pointer; }
.update-folder svg { width: 16px; height: 16px; }
.update-folder:hover { color: var(--ink); background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }
.update-folder:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: -2px; }
.update-actions { flex-wrap: nowrap; justify-content: flex-end; gap: var(--space-sm); }
.update-aux { --md-text-button-label-text-color: var(--md-sys-color-on-surface-variant); }
.error { overflow-wrap: anywhere; }
@media (max-width: 520px) {
  .update-actions { flex-wrap: wrap; }
}
</style>
