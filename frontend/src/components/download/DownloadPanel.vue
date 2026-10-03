<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useDownloadPanelStore, type DownloadFormKind } from "../../stores/downloadPanel";
import { useTaskStore } from "../../stores/tasks";
import { parsePixivUrl } from "../../utils/pixivUrl";
import { errorMessage } from "../../api/tauri";
import { notify } from "../../ui/notify";
import SectionTabs from "../browse/SectionTabs.vue";

const panel = useDownloadPanelStore(); const tasks = useTaskStore(); const { t } = useI18n();
const root = ref<HTMLElement>(); const confirm = ref<HTMLDialogElement>();
const draft = computed(() => panel.drafts[panel.kind]);
const prefix = computed(() => panel.kind === "novel" ? "crawl" : "illust");
const tabs = computed(() => [{ value: "novel", label: t("nav.crawlNovel") }, { value: "illustration", label: t("nav.crawlIllustration") }]);
const sources = computed(() => panel.kind === "novel" ? ["single", "series", "user"] : ["single", "user"]);
function switchKind(value: string) { if (!panel.submitting) { panel.kind = value as DownloadFormKind; panel.error = ""; } }
function editSource(value: string) { draft.value.sourceId = value; draft.value.edited = true; panel.error = ""; }
function editType(value: string) { draft.value.sourceType = value; draft.value.edited = true; panel.error = ""; }
function formatChange(value: string, checked: boolean) { draft.value.formats = checked ? [...draft.value.formats, value] : draft.value.formats.filter(format => format !== value); }
watch(() => panel.visible, async (visible) => { if (visible) { await nextTick(); root.value?.querySelector<HTMLElement>("md-outlined-text-field")?.focus(); } });
watch(() => panel.pendingTarget, async (target) => { await nextTick(); if (target) confirm.value?.showModal(); else confirm.value?.close(); });
async function submit() {
  if (panel.submitting) return;
  const kind = panel.kind;
  const session = panel.session;
  const form = panel.drafts[kind];
  const parsed = parsePixivUrl(form.sourceId);
  const supported = kind === "novel" ? ["novel-single", "novel-series", "user"] : ["illustration", "user"];
  const id = parsed && supported.includes(parsed.kind) ? parsed.id : form.sourceId.trim();
  if (!id || (kind === "novel" && !form.formats.length)) { panel.error = t(`${prefix.value}.invalidInput`); return; }
  panel.submitting = true; panel.error = "";
  try {
    const result = await tasks.createTask(form.sourceType, id, kind === "novel" ? [...form.formats] : [], kind);
    if (session !== panel.session) return;
    if (result.error) { panel.error = result.error; return; }
    form.sourceId = ""; form.edited = false;
    panel.close();
    notify(t(`${kind === "novel" ? "crawl" : "illust"}.taskCreated`, { id: result.task_id }));
    await tasks.fetchTasks().catch(() => {});
  } catch (error) { if (session === panel.session) panel.error = errorMessage(error) || t(`${prefix.value}.createFailed`); }
  finally { panel.submitting = false; }
}
</script>

<template>
  <aside v-show="panel.visible" ref="root" class="download-panel" :aria-label="t('workspace.newDownload')">
    <header><h2>{{ t('workspace.newDownload') }}</h2><md-icon-button :aria-label="t('common.close')" @click="panel.close()"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m18 6-12 12M6 6l12 12" /></svg></md-icon-button></header>
    <div class="panel-body">
      <SectionTabs :tabs="tabs" :value="panel.kind" :disabled="panel.submitting" @change="switchKind" />
      <form :aria-busy="panel.submitting" @submit.prevent="submit">
        <fieldset :disabled="panel.submitting">
          <legend>{{ t(`${prefix}.sourceType`) }}</legend>
          <div class="m3-row"><label v-for="source in sources" :key="source" class="m3-choice"><md-radio name="download-source" :disabled="panel.submitting" :checked="draft.sourceType === source" @change="editType(source)" />{{ t(`${prefix}.${source}`) }}</label></div>
          <md-outlined-text-field :disabled="panel.submitting" :label="t(`${prefix}.input`)" :placeholder="t(`${prefix}.inputPlaceholder.${draft.sourceType}`)" :value="draft.sourceId" @input="editSource(($event.target as HTMLInputElement).value)" @keydown.enter.prevent="submit" />
          <div v-if="panel.kind === 'novel'" class="m3-field"><label>{{ t('crawl.outputFormats') }}</label><div class="m3-row"><label v-for="format in ['txt', 'markdown']" :key="format" class="m3-choice"><md-checkbox :disabled="panel.submitting" :checked="draft.formats.includes(format)" @change="formatChange(format, ($event.target as HTMLInputElement).checked)" />{{ format === 'txt' ? 'TXT' : 'Markdown' }}</label></div></div>
        </fieldset>
        <p v-if="panel.error" class="m3-alert error" role="alert">{{ panel.error }}</p>
        <md-filled-button :disabled="panel.submitting" @click="submit">{{ panel.submitting ? t('common.loading') : t(`${prefix}.start`) }}</md-filled-button>
      </form>
    </div>
    <dialog ref="confirm" class="m3-dialog" @cancel="panel.pendingTarget = null" @close="panel.pendingTarget = null"><h2>{{ t('workspace.replaceSource') }}</h2><p>{{ t('workspace.replaceSourceHint') }}</p><div class="dialog-actions m3-row"><md-text-button @click="panel.pendingTarget = null">{{ t('common.cancel') }}</md-text-button><md-filled-button @click="panel.confirmTarget()">{{ t('common.confirm') }}</md-filled-button></div></dialog>
  </aside>
</template>

<style scoped>
.download-panel { min-width:0; min-height:0; display:flex; flex-direction:column; background:var(--md-sys-color-surface-container); }
header { display:flex; align-items:center; justify-content:space-between; padding:var(--space-sm) var(--space-xl); flex:none; }
h2 { font-size:18px; margin:0; }
svg { width:20px; height:20px; }
.panel-body { min-height:0; overflow:auto; padding:0 var(--space-xl) var(--space-xl); }
.panel-body :deep(.section-tabs) { --md-secondary-tab-container-color:var(--md-sys-color-surface-container); }
form { display:flex; flex-direction:column; gap:var(--space-lg); margin-top:var(--space-lg); }
fieldset { display:flex; flex-direction:column; gap:var(--space-lg); margin:0; padding:0; border:0; min-width:0; }
legend { padding:0; margin-bottom:var(--space-sm); }
.m3-row { flex-wrap:wrap; }
md-outlined-text-field { width:100%; }
md-filled-button { align-self:flex-start; }
.m3-alert { margin:0; overflow-wrap:anywhere; }
</style>
