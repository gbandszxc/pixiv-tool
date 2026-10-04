<template>
  <section class="settings-panel" :aria-label="t('settings.title')">
    <template v-if="section === 'general'">
      <div class="m3-field">
        <label for="startup-page">{{ t("settings.startupPage") }}</label>
        <md-outlined-select id="startup-page" :value="form.startup_page" @change="form.startup_page = ($event.target as HTMLSelectElement).value"><md-select-option v-for="option in startupPageOptions" :key="option.value" :value="option.value">{{ option.label }}</md-select-option></md-outlined-select>
        <span class="field-hint">{{ t("settings.startupPageHint") }}</span>
      </div>
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
    </template>

    <template v-else-if="section === 'appearance'">
      <div class="m3-field">
        <label for="theme">{{ t("settings.theme") }}</label>
        <md-outlined-select id="theme" :value="form.theme" @change="form.theme = ($event.target as HTMLSelectElement).value"><md-select-option v-for="option in themeOptions" :key="option.value" :value="option.value">{{ option.label }}</md-select-option></md-outlined-select>
      </div>
      <fieldset class="m3-field settings-fieldset">
        <legend>{{ t("settings.palette") }}</legend>
        <div class="palette-options"><label v-for="option in paletteOptions" :key="option.value" class="palette-option"><md-radio name="theme-color" :value="option.value" :checked="form.theme_color === option.value" @change="form.theme_color = option.value" /><span class="palette-swatch" :class="`palette-${option.value}`" aria-hidden="true"></span>{{ option.label }}</label></div>
      </fieldset>
      <span class="field-hint">{{ t("settings.appearanceHint") }}</span>
    </template>

    <template v-else-if="section === 'images'">
      <fieldset class="m3-field settings-fieldset">
        <legend>{{ t("settings.imageQuality") }}</legend>
        <div class="m3-field">
          <label for="thumb-grid">{{ t("settings.thumbGrid") }}</label>
          <md-outlined-select id="thumb-grid" :value="form.thumb_quality_grid" @change="form.thumb_quality_grid = ($event.target as HTMLSelectElement).value"><md-select-option v-for="option in gridTierOptions" :key="option.value" :value="option.value">{{ option.label }}</md-select-option></md-outlined-select>
        </div>
        <div class="m3-field">
          <label for="thumb-detail">{{ t("settings.thumbDetail") }}</label>
          <md-outlined-select id="thumb-detail" :value="form.thumb_quality_detail" @change="form.thumb_quality_detail = ($event.target as HTMLSelectElement).value"><md-select-option v-for="option in detailTierOptions" :key="option.value" :value="option.value">{{ option.label }}</md-select-option></md-outlined-select>
        </div>
        <div class="m3-field">
          <label for="thumb-fullscreen">{{ t("settings.thumbFullscreen") }}</label>
          <md-outlined-select id="thumb-fullscreen" :value="form.thumb_quality_fullscreen" @change="form.thumb_quality_fullscreen = ($event.target as HTMLSelectElement).value"><md-select-option v-for="option in fullscreenTierOptions" :key="option.value" :value="option.value">{{ option.label }}</md-select-option></md-outlined-select>
        </div>
        <span class="field-hint">{{ t("settings.thumbHint") }}</span>
      </fieldset>
      <fieldset class="m3-field settings-fieldset">
        <legend>{{ t("settings.contentDisplay") }}</legend>
        <div class="m3-row"><label class="m3-choice"><md-checkbox :checked="form.show_r18" @change="form.show_r18 = ($event.target as HTMLInputElement).checked" />{{ t("settings.showR18") }}</label></div>
        <span class="field-hint">{{ t("settings.showR18Hint") }}</span>
      </fieldset>
    </template>

    <template v-else-if="section === 'translation'">
      <span class="field-hint">{{ t('translation.settingsHint') }}</span>
      <div class="m3-field">
        <label for="translation-url">{{ t('translation.apiUrl') }}</label>
        <md-outlined-text-field id="translation-url" :value="form.translation_api_url" placeholder="https://api.openai.com/v1" @input="form.translation_api_url = ($event.target as HTMLInputElement).value" />
        <span class="field-hint">{{ t('translation.urlHint') }}</span>
      </div>
      <div class="m3-field">
        <label for="translation-key">API Key</label>
        <md-outlined-text-field id="translation-key" type="password" autocomplete="new-password" :value="translationKey" :placeholder="t(form.translation_key_configured ? 'translation.keySaved' : 'translation.keyPlaceholder')" @input="translationKey = ($event.target as HTMLInputElement).value; clearTranslationKey = false" />
        <span class="field-hint">{{ t('translation.keyHint') }}</span>
        <span v-if="form.translation_key_error" class="field-hint credential-error" role="alert">{{ form.translation_key_error }}</span>
        <md-text-button v-if="form.translation_key_configured" :disabled="clearTranslationKey" @click="clearTranslationKey = true; translationKey = ''">{{ t(clearTranslationKey ? 'translation.keyWillClear' : 'translation.clearKey') }}</md-text-button>
      </div>
      <div class="m3-field">
        <label for="translation-model">{{ t('translation.model') }}</label>
        <md-outlined-text-field id="translation-model" :value="form.translation_model" @input="form.translation_model = ($event.target as HTMLInputElement).value" />
      </div>
      <div class="m3-field">
        <label for="translation-json">{{ t('translation.advanced') }}</label>
        <md-outlined-text-field id="translation-json" type="textarea" rows="5" :value="translationJson" :error="Boolean(translationJsonError)" :error-text="translationJsonError" @input="translationJson = ($event.target as HTMLTextAreaElement).value; translationJsonError = ''" />
        <span class="field-hint">{{ t('translation.jsonHint') }}</span>
      </div>
    </template>

    <template v-else-if="section === 'advanced'">
      <div class="m3-field">
        <label for="saucenao-api-key">{{ t("settings.saucenaoApiKey") }}</label>
        <div class="m3-row"><md-outlined-text-field id="saucenao-api-key" class="settings-api-key-input" :value="form.saucenao_api_key" @input="form.saucenao_api_key = ($event.target as HTMLInputElement).value" /><span class="field-hint">{{ t("settings.saucenaoApiKeyHint") }}</span></div>
      </div>
      <div class="m3-field">
        <label for="max-wait">{{ t("settings.maxWait") }}</label>
        <div class="m3-row"><md-outlined-text-field id="max-wait" class="settings-number-input" type="number" min="30" max="86400" step="30" :value="String(form.max_wait_seconds)" @input="form.max_wait_seconds = Number(($event.target as HTMLInputElement).value)" /><span class="field-hint">{{ t("settings.maxWaitHint") }}</span></div>
      </div>
    </template>

    <template v-else>
      <span class="field-hint">{{ t("settings.maintenanceHint") }}</span>
      <div class="settings-maintenance">
        <div class="settings-maintenance-item">
          <span class="settings-maintenance-text"><strong>{{ t("settings.logsLabel") }}</strong><span class="field-hint">{{ t("settings.clearLogsHint") }}</span></span>
          <md-outlined-button @click="openConfirm('logs')">{{ t("common.clear") }}</md-outlined-button>
        </div>
        <div class="settings-maintenance-item">
          <span class="settings-maintenance-text"><strong>{{ t("settings.authLabel") }}</strong><span class="field-hint">{{ t("settings.clearAuthHint") }}</span></span>
          <md-text-button class="danger-button" @click="openConfirm('auth')">{{ t("common.clear") }}</md-text-button>
        </div>
      </div>
    </template>

    <dialog ref="confirmDialog" class="m3-dialog" @close="confirmAction = null">
      <h2>{{ confirmAction === "logs" ? t("settings.clearLogs") : t("settings.clearAuth") }}</h2>
      <p>{{ confirmAction === "logs" ? t("settings.clearLogsConfirm") : t("settings.clearAuthConfirm") }}</p>
      <div class="m3-row dialog-actions"><md-text-button @click="closeConfirm">{{ t("common.cancel") }}</md-text-button><md-filled-button @click="handleConfirm">{{ confirmAction === "logs" ? t("settings.clearLogs") : t("settings.clearAuth") }}</md-filled-button></div>
    </dialog>
  </section>
</template>

<script setup lang="ts">
/**
 * 设置面板：承载全部设置字段，按 `section` 分组渲染（分组切换由 SettingsDialog
 * 左栏驱动），同一时刻只渲染当前分组。表单状态与「已保存快照」都在此处，
 * 弹窗底部的常驻保存/取消按钮通过 defineExpose 的 save()/reset()/hasUnsaved()
 * 调用——保存是整表一次写入，取消回滚到上次保存值并恢复主题预览。
 * 主题与配色仍为即时预览（只写 DOM，不落盘），落盘时机由保存按钮决定。
 */
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useSettingsStore } from "../../stores/settings";
import { groupRoots } from "../../router/navigation";
import { useAuthStore } from "../../stores/auth";
import { errorMessage, setWindowTheme, type Settings } from "../../api/tauri";
import { notify } from "../../ui/notify";
import type { SettingsSection } from "./sections";

defineProps<{ section: SettingsSection }>();

const { t, locale } = useI18n();
const settingsStore = useSettingsStore();
const authStore = useAuthStore();
const formats = ["txt", "markdown"];
const confirmDialog = ref<HTMLDialogElement | null>(null);
const confirmAction = ref<"logs" | "auth" | null>(null);
const form = ref<Settings>({ ...settingsStore.settings });
const translationKey = ref("");
const clearTranslationKey = ref(false);
const translationJson = ref("{}");
const translationJsonError = ref("");
function resetTranslationDraft() {
  translationKey.value = "";
  clearTranslationKey.value = false;
  translationJson.value = JSON.stringify(form.value.translation_extra, null, 2);
  translationJsonError.value = "";
}
const startupPageOptions = computed(() => Object.entries(groupRoots).map(([group, value]) => ({ value, label: t(`workspace.${group}`) })));
const savedSnapshot = ref("");
const langOptions = computed(() => [{ label: t("settings.languages.zh-CN"), value: "zh-CN" }, { label: t("settings.languages.en-US"), value: "en-US" }]);
const themeOptions = computed(() => [{ label: t("settings.themes.light"), value: "light" }, { label: t("settings.themes.dark"), value: "dark" }, { label: t("settings.themes.auto"), value: "auto" }]);
const paletteOptions = computed(() => ["pixiv", "indigo", "jade", "violet", "amber"].map(value => ({ value, label: t(`settings.palettes.${value}`) })));
// 缩略图档位取值集合与后端白名单一致（src-tauri/src/settings.rs 的 THUMB_*_TIERS）。
// 详情页 medium 是「接口 regular 原样」（最长边 1200），故文案与列表档的「中 · 540px」分开。
const gridTierOptions = computed(() => ["small", "medium", "large"].map(value => ({ value, label: t(`settings.thumbTiers.${value}`) })));
const detailTierOptions = computed(() => [{ value: "medium", label: t("settings.thumbDetailTiers.medium") }, { value: "large", label: t("settings.thumbDetailTiers.large") }, { value: "original", label: t("settings.thumbTiers.original") }]);
const fullscreenTierOptions = computed(() => [{ value: "large", label: t("settings.thumbTiers.large") }, { value: "original", label: t("settings.thumbTiers.original") }]);

function toggleFormat(format: string, checked: boolean) { form.value.output_formats = checked ? [...form.value.output_formats, format] : form.value.output_formats.filter((item) => item !== format); }
/** 语言与主题同理是即时预览：立即切 i18n / 写 localStorage，落盘仍等保存。 */
function applyLanguage(lang: string) { locale.value = lang; localStorage.setItem("pixiv-tool-lang", lang); }
function changeLang(lang: string) { applyLanguage(lang); form.value.language = lang; }
function snapshot() { return JSON.stringify([form.value, translationJson.value]); }
function applyPreview() { const dark = form.value.theme === "dark" || (form.value.theme === "auto" && window.matchMedia("(prefers-color-scheme: dark)").matches); document.documentElement.classList.toggle("dark", dark); document.documentElement.dataset.palette = form.value.theme_color || "pixiv"; setWindowTheme(dark ? "dark" : "light").catch(() => {}); }
watch(() => [form.value.theme, form.value.theme_color], applyPreview);
onMounted(async () => { await settingsStore.fetchSettings(); if (typeof settingsStore.settings.max_wait_seconds !== "number") settingsStore.settings.max_wait_seconds = 180; form.value = { ...settingsStore.settings }; resetTranslationDraft(); savedSnapshot.value = snapshot(); if (form.value.language && form.value.language !== locale.value) applyLanguage(form.value.language); });
/** 有未保存改动：弹窗据此决定关闭前是否提示。 */
function hasUnsaved() { return Boolean(translationKey.value) || clearTranslationKey.value || Boolean(savedSnapshot.value) && savedSnapshot.value !== snapshot(); }
/** 回滚到上次保存值（取消 / 确认放弃修改）。主题预览随表单回到已保存值。 */
function reset() { form.value = { ...settingsStore.settings }; resetTranslationDraft(); savedSnapshot.value = snapshot(); if (form.value.language && form.value.language !== locale.value) applyLanguage(form.value.language); }
defineExpose({ save, reset, hasUnsaved });
async function save(): Promise<boolean> {
  if (!form.value.max_wait_seconds || form.value.max_wait_seconds < 30) { notify(t("settings.maxWaitInvalid")); return false; }
  let extra: Record<string, unknown>;
  try {
    extra = JSON.parse(translationJson.value);
    if (!extra || Array.isArray(extra) || typeof extra !== "object") throw new Error();
  } catch { translationJsonError.value = t("translation.jsonInvalid"); notify(translationJsonError.value); return false; }
  try {
    await settingsStore.saveSettings({ ...form.value, translation_extra: extra,
      ...(clearTranslationKey.value ? { translation_api_key: "" } : translationKey.value.trim() ? { translation_api_key: translationKey.value } : {}) });
    form.value = { ...settingsStore.settings };
    resetTranslationDraft(); savedSnapshot.value = snapshot(); notify(t("settings.saved")); return true;
  } catch (err) { notify(errorMessage(err) || t("settings.saveFailed")); return false; }
}
async function handleBrowse() { try { const path = await settingsStore.selectDirectory(); if (path) form.value.output_dir = path; } catch { notify(t("settings.pickFailed")); } }
async function openConfirm(action: "logs" | "auth") { confirmAction.value = action; await nextTick(); confirmDialog.value?.showModal(); }
function closeConfirm() { confirmDialog.value?.close(); }
async function handleConfirm() { const action = confirmAction.value; closeConfirm(); try { if (action === "logs") { await settingsStore.clearLogs(); notify(t("settings.logsCleared")); } else if (action === "auth") { await authStore.logout(); notify(t("settings.authCleared")); } } catch (err) { notify(errorMessage(err) || t("settings.saveFailed")); } }
</script>

<style scoped>
/* 密度层下字段与按钮同为 40px：居中对齐，按钮不再随文本框拉伸（窄屏 column 时由下方媒体查询改回 stretch 铺满行宽） */
.settings-path-row { display: flex; align-items: center; gap: var(--space-sm); }
.settings-path-input { flex: 1; min-width: 0; }
.settings-fieldset { min-width: 0; padding: 0; border: 0; }
.settings-fieldset legend { padding: 0; color: var(--ink-muted); font-size: 14px; font-weight: 500; }
.settings-number-input { width: 140px; }
.settings-api-key-input { flex: 1; min-width: 0; }
.field-hint { color: var(--ink-muted); font-size: 12px; }
.credential-error { color: var(--md-sys-color-error); }
.danger-button { --md-text-button-label-text-color: #ba1a1a; }
.palette-options { display: flex; flex-wrap: wrap; gap: var(--space-sm) var(--space-lg); }.palette-option { display: inline-flex; align-items: center; gap: var(--space-xs); min-height: 40px; }.palette-swatch { width: 18px; height: 18px; border: 1px solid var(--md-sys-color-outline); border-radius: 50%; }/* 色块取各色板 primary 的规范值，改色板时必须与 main.css 的 [data-palette] 定义、.impeccable/design.json 的 extensions.palettes 同步 */.palette-pixiv { background: #006eaf; }.palette-indigo { background: #445e91; }.palette-jade { background: #006c4d; }.palette-violet { background: #76547b; }.palette-amber { background: #8b5000; }
/* 维护组：说明在左（标题 + 提示两行）、动作按钮在右，行间以 divider 分隔 */
.settings-maintenance { display: grid; margin-top: var(--space-md); border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.settings-maintenance-item { display: flex; align-items: center; justify-content: space-between; gap: var(--space-lg); padding: var(--space-md) 0; border-bottom: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.settings-maintenance-text { display: grid; gap: var(--space-xxs); min-width: 0; }
.settings-maintenance-text strong { color: var(--ink); font-size: 14px; font-weight: 500; }
@media (max-width: 640px) { .settings-path-row { align-items: stretch; flex-direction: column; } }
</style>
