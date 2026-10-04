<template>
  <div class="maintenance-storage">
    <div v-if="infoError || logError" class="maintenance-error" role="alert"><span>{{ infoError || logError }}</span><md-text-button @click="refresh(true)">{{ t('common.retry') }}</md-text-button></div>
    <p v-if="!info && !infoError" class="field-hint" role="status">{{ t('settings.storageLoading') }}</p>
    <section :aria-label="t('settings.cacheLabel')">
      <div class="maintenance-heading"><h3>{{ t('settings.cacheLabel') }}</h3><span>{{ t('settings.totalSize', { size: info ? formatBytes(info.translation.bytes + info.images.bytes) : '—' }) }}</span></div>
      <div v-for="kind in cacheKinds" :key="kind" class="maintenance-resource">
        <div class="maintenance-row">
          <span class="field-label maintenance-name"><strong>{{ t(`settings.cacheKinds.${kind}`) }}</strong><HelpTooltip :label="t(`settings.cacheKinds.${kind}`)" :text="t(`settings.cacheHints.${kind}`)" /></span>
          <span class="maintenance-size">{{ info ? formatBytes(info[kind].bytes) : '—' }}</span>
          <CopyPathButton :path="info?.[kind].path ?? ''" :disabled="!info" />
          <md-text-button :disabled="busy || !info || !info[kind].files" @click="askClear(kind)">{{ t('common.clear') }}</md-text-button>
        </div>
      </div>
    </section>
    <section :aria-label="t('settings.logsLabel')">
      <div class="maintenance-heading">
        <span class="field-label maintenance-name"><h3>{{ t('settings.logsLabel') }}</h3><HelpTooltip :label="t('settings.logsLabel')" :text="t('settings.logsGuide')" /></span>
        <span>{{ t('settings.totalSize', { size: info ? formatBytes(info.logs.bytes) : '—' }) }}</span>
      </div>
      <div class="maintenance-row"><span class="field-hint">{{ t('settings.logsPolling') }}</span><CopyPathButton :path="info?.logs.path ?? ''" :disabled="!info" /><md-text-button :disabled="busy || !info?.logs.bytes" @click="askClear('logs')">{{ t('settings.clearLogs') }}</md-text-button></div>
      <pre ref="logElement" class="maintenance-log" tabindex="0" :aria-label="t('settings.recentLogs')">{{ logs || t('settings.logsEmpty') }}</pre>
    </section>
    <dialog ref="confirmation" class="m3-dialog" :aria-label="t('settings.clearStorage')" @close="action = null">
      <h2>{{ t('settings.clearStorage') }}</h2><p>{{ action === 'logs' ? t('settings.clearLogsConfirm') : action ? t(`settings.clearCacheConfirm.${action}`) : '' }}</p>
      <div class="m3-row dialog-actions"><md-text-button @click="confirmation?.close()">{{ t('common.cancel') }}</md-text-button><md-filled-button @click="confirmClear">{{ t('common.clear') }}</md-filled-button></div>
    </dialog>
  </div>
</template>
<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { clearCache, clearLogs, getMaintenanceInfo, readLogs, type CacheKind, type MaintenanceInfo } from "../../api/maintenance";
import { errorMessage } from "../../api/tauri";
import { notify } from "../../ui/notify";
import HelpTooltip from "../common/HelpTooltip.vue";
import CopyPathButton from "./CopyPathButton.vue";
const { t, locale } = useI18n();
const cacheKinds: CacheKind[] = ['translation', 'images'];
const info = ref<MaintenanceInfo | null>(null);
const logs = ref('');
const logElement = ref<HTMLElement | null>(null);
const infoError = ref('');
const logError = ref('');
const busy = ref(false);
const action = ref<CacheKind | 'logs' | null>(null);
const confirmation = ref<HTMLDialogElement | null>(null);
let stopped = false;
let generation = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let lastInfo = 0;
let refreshing = false;
function formatBytes(bytes: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB'];
  const index = Math.min(3, Math.floor(Math.log(Math.max(1, bytes)) / Math.log(1024)));
  return `${new Intl.NumberFormat(locale.value, { maximumFractionDigits: 1 }).format(bytes / 1024 ** index)} ${units[index]}`;
}
async function refresh(force = false): Promise<void> {
  const version = generation;
  await Promise.all([
    (async () => {
      if (!force && Date.now() - lastInfo < 10000) return;
      try { const result = await getMaintenanceInfo(); if (!stopped && version === generation) { info.value = result; infoError.value = ''; lastInfo = Date.now(); } }
      catch (error) { if (!stopped && version === generation) infoError.value = errorMessage(error); }
    })(),
    (async () => {
      try {
        const result = await readLogs();
        if (!stopped && version === generation) {
          const element = logElement.value;
          const follow = !logs.value || !element || element.scrollTop + element.clientHeight >= element.scrollHeight - 2;
          logs.value = result; logError.value = '';
          if (follow) { await nextTick(); if (!stopped && version === generation && logElement.value) logElement.value.scrollTop = logElement.value.scrollHeight; }
        }
      }
      catch (error) { if (!stopped && version === generation) logError.value = errorMessage(error); }
    })(),
  ]);
}
async function poll(): Promise<void> {
  if (refreshing || stopped) return;
  refreshing = true;
  if (!document.hidden && !busy.value) await refresh();
  refreshing = false;
  if (!stopped) timer = setTimeout(() => { void poll(); }, 2000);
}
function onVisibility(): void { if (!document.hidden) { clearTimeout(timer); void poll(); } }
onMounted(() => { document.addEventListener('visibilitychange', onVisibility); void poll(); });
onBeforeUnmount(() => { stopped = true; generation++; clearTimeout(timer); document.removeEventListener('visibilitychange', onVisibility); });
async function askClear(kind: CacheKind | 'logs'): Promise<void> { action.value = kind; await nextTick(); confirmation.value?.showModal(); }
async function confirmClear(): Promise<void> {
  const kind = action.value; confirmation.value?.close();
  if (!kind || busy.value) return;
  busy.value = true; generation++;
  try { if (kind === 'logs') await clearLogs(); else await clearCache(kind); notify(t(kind === 'logs' ? 'settings.logsCleared' : 'settings.cacheCleared')); }
  catch (error) { notify(errorMessage(error)); }
  finally { await refresh(true); busy.value = false; }
}
</script>
<style scoped>
.maintenance-storage { display: grid; gap: var(--space-xl); min-width: 0; margin-top: var(--space-lg); }
.maintenance-heading, .maintenance-row, .maintenance-error { display: flex; align-items: center; gap: var(--space-sm); flex-wrap: wrap; }
.maintenance-heading { justify-content: space-between; margin-bottom: var(--space-sm); }
.maintenance-heading h3 { margin: 0; font-size: 14px; font-weight: 600; }
.maintenance-heading > span, .maintenance-size, .field-hint { color: var(--ink-muted); font-size: 12px; }
.maintenance-size { margin-left: auto; font-variant-numeric: tabular-nums; }
.maintenance-row strong { font-size: 14px; font-weight: 500; }
.maintenance-row > .field-hint { margin-right: auto; }
.maintenance-resource { padding: var(--space-sm) 0; border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.field-hint { margin: var(--space-xxs) 0; }
.maintenance-log { margin: var(--space-sm) 0 0; padding: var(--space-md); max-height: calc(10 * var(--space-xl)); overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; background: var(--md-sys-color-surface); border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); border-radius: var(--radius-control); color: var(--ink); font-size: 12px; line-height: 1.5; }
.maintenance-log:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 2px; }
.maintenance-error { color: var(--md-sys-color-error); font-size: 12px; }
</style>
