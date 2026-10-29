<template>
  <section class="settings-panel" :aria-label="t('settings.title')">
    <template v-if="section === 'general'">
      <div class="m3-field">
        <div class="field-label"><label for="startup-page">{{ t('settings.startupPage') }}</label><HelpTooltip :label="t('settings.startupPage')" :text="t('settings.startupPageHint')" /></div>
        <md-outlined-select id="startup-page" :value="form.startup_page" @change="form.startup_page = ($event.target as HTMLSelectElement).value"><md-select-option v-for="option in startupPageOptions" :key="option.value" :value="option.value">{{ option.label }}</md-select-option></md-outlined-select>
      </div>
      <div class="m3-field">
        <label for="language">{{ t("settings.language") }}</label>
        <md-outlined-select id="language" :value="form.language" @change="changeLang(($event.target as HTMLSelectElement).value)"><md-select-option v-for="option in langOptions" :key="option.value" :value="option.value">{{ option.label }}</md-select-option></md-outlined-select>
      </div>
    </template>

    <template v-else-if="section === 'appearance'">
      <div class="m3-field">
        <div class="field-label"><label for="theme">{{ t('settings.theme') }}</label><HelpTooltip :label="t('settings.theme')" :text="t('settings.appearanceHint')" /></div>
        <md-outlined-select id="theme" :value="form.theme" @change="form.theme = ($event.target as HTMLSelectElement).value"><md-select-option v-for="option in themeOptions" :key="option.value" :value="option.value">{{ option.label }}</md-select-option></md-outlined-select>
      </div>
      <fieldset class="m3-field settings-fieldset">
        <legend>{{ t("settings.palette") }}</legend>
        <div class="palette-options"><label v-for="option in paletteOptions" :key="option.value" class="palette-option"><md-radio name="theme-color" :value="option.value" :checked="form.theme_color === option.value" @change="form.theme_color = option.value" /><span class="palette-swatch" :class="`palette-${option.value}`" aria-hidden="true"></span>{{ option.label }}</label></div>
      </fieldset>
    </template>

    <template v-else-if="section === 'download'">
      <div class="m3-field">
        <label for="output-dir">{{ t("settings.outputDir") }}</label>
        <div class="settings-path-row">
          <md-outlined-text-field id="output-dir" class="settings-path-input" :value="form.output_dir" :placeholder="t('settings.outputDirPlaceholder')" :error="Boolean(outputDirError)" :error-text="outputDirError" @input="form.output_dir = ($event.target as HTMLInputElement).value; outputDirError = ''" />
          <md-outlined-button @click="handleBrowse">{{ t("settings.browse") }}</md-outlined-button>
        </div>
      </div>
      <fieldset class="m3-field settings-fieldset">
        <legend>{{ t("settings.outputFormats") }}</legend>
        <div class="m3-row"><label v-for="format in formats" :key="format" class="m3-choice"><md-checkbox :checked="form.output_formats.includes(format)" @change="toggleFormat(format, ($event.target as HTMLInputElement).checked)" />{{ format === "txt" ? "TXT" : "Markdown" }}</label></div>
      </fieldset>
      <div class="m3-field">
        <label for="illust-filename-template">{{ t("settings.filenameTemplateIllust") }}</label>
        <md-outlined-text-field id="illust-filename-template" :value="form.illust_filename_template" :supporting-text="illustTemplateHint" :error="Boolean(templateErrors.illust)" :error-text="templateErrors.illust" @input="form.illust_filename_template = ($event.target as HTMLInputElement).value; templateErrors.illust = ''" />
      </div>
      <div class="m3-field">
        <label for="novel-filename-template">{{ t("settings.filenameTemplateNovel") }}</label>
        <md-outlined-text-field id="novel-filename-template" :value="form.novel_filename_template" :supporting-text="novelTemplateHint" :error="Boolean(templateErrors.novel)" :error-text="templateErrors.novel" @input="form.novel_filename_template = ($event.target as HTMLInputElement).value; templateErrors.novel = ''" />
      </div>
      <div class="m3-field">
        <label for="novel-series-filename-template">{{ t("settings.filenameTemplateSeries") }}</label>
        <md-outlined-text-field id="novel-series-filename-template" :value="form.novel_series_filename_template" :supporting-text="seriesTemplateHint" :error="Boolean(templateErrors.series)" :error-text="templateErrors.series" @input="form.novel_series_filename_template = ($event.target as HTMLInputElement).value; templateErrors.series = ''" />
      </div>
      <div class="m3-field">
        <div class="field-label"><label for="max-wait">{{ t('settings.maxWait') }}</label><HelpTooltip :label="t('settings.maxWait')" :text="t('settings.maxWaitHint')" /></div>
        <div class="m3-row"><md-outlined-text-field id="max-wait" class="settings-number-input" type="number" min="30" max="86400" step="30" :value="maxWaitDisplay" :error="Boolean(maxWaitError)" :error-text="maxWaitError" @input="onMaxWaitInput" /></div>
      </div>
    </template>

    <template v-else-if="section === 'images'">
      <fieldset class="m3-field settings-fieldset">
        <legend><span class="field-label">{{ t("settings.imageQuality") }}<HelpTooltip :label="t('settings.imageQuality')" :text="t('settings.thumbHint')" /></span></legend>
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
      </fieldset>
      <fieldset class="m3-field settings-fieldset">
        <legend><span class="field-label">{{ t("settings.imageCache") }}<HelpTooltip :label="t('settings.imageCache')" :text="t('settings.imageCacheMaxHint')" /></span></legend>
        <div class="m3-field">
          <label for="image-cache-max">{{ t("settings.imageCacheMax") }}</label>
          <md-outlined-select id="image-cache-max" :value="cacheMaxSelectValue" @change="changeCacheMax(($event.target as HTMLSelectElement).value)">
            <md-select-option v-for="preset in cachePresets" :key="preset.value" :value="String(preset.value)">{{ preset.label }}</md-select-option>
            <md-select-option value="custom">{{ t("settings.imageCacheCustom") }}</md-select-option>
          </md-outlined-select>
        </div>
        <div v-if="cacheMaxCustom" class="m3-field">
          <label for="image-cache-max-custom">{{ t("settings.imageCacheCustomLabel") }}</label>
          <md-outlined-text-field id="image-cache-max-custom" class="settings-number-input" type="number" min="256" max="2048" step="64" :value="cacheMaxDisplay" :error="Boolean(cacheMaxError)" :error-text="cacheMaxError" @input="onCacheMaxInput" />
        </div>
      </fieldset>
      <fieldset class="m3-field settings-fieldset">
        <legend><span class="field-label">{{ t("settings.contentDisplay") }}<HelpTooltip :label="t('settings.contentDisplay')" :text="t('settings.showR18Hint')" /></span></legend>
        <div class="m3-row"><label class="m3-choice"><md-checkbox :checked="form.show_r18" @change="form.show_r18 = ($event.target as HTMLInputElement).checked" />{{ t("settings.showR18") }}</label></div>
      </fieldset>
      <div class="m3-field">
        <div class="field-label"><label for="saucenao-api-key">{{ t('settings.saucenaoApiKey') }}</label><HelpTooltip :label="t('settings.saucenaoApiKey')" :text="t('settings.saucenaoApiKeyHint')" /></div>
        <div class="m3-row"><md-outlined-text-field id="saucenao-api-key" type="password" autocomplete="new-password" class="settings-api-key-input" :value="form.saucenao_api_key" @input="form.saucenao_api_key = ($event.target as HTMLInputElement).value" /></div>
      </div>
    </template>

    <template v-else-if="section === 'translation'">
      <!-- 一级：只放连接必需项与常用项；运行参数与原始 JSON 在二级高级设置。 -->
      <template v-if="translationView === 'basic'">
        <fieldset class="m3-field settings-fieldset translation-group" role="group" aria-labelledby="translation-group-service">
          <div id="translation-group-service" class="translation-legend">{{ t("translation.groupService") }}</div>
          <div class="m3-field">
            <div class="field-label"><label for="translation-format">{{ t('translation.apiFormat') }}</label><HelpTooltip :label="t('translation.apiFormat')" :text="t('translation.apiFormatHint')" /></div>
            <md-outlined-select id="translation-format" :value="apiFormat" @change="changeApiFormat(($event.target as HTMLSelectElement).value)">
              <md-select-option v-for="format in API_FORMATS" :key="format" :value="format">{{ t(`translation.apiFormats.${format}`) }}</md-select-option>
            </md-outlined-select>
          </div>
          <div class="m3-field">
            <div class="field-label"><label for="translation-url">{{ t('translation.apiUrl') }}</label><HelpTooltip :label="t('translation.apiUrl')" :text="`${t('translation.settingsHint')}\n\n${t('translation.urlHint', { endpoint: apiFormatEndpoint })}`" /></div>
            <md-outlined-text-field id="translation-url" :value="form.translation_api_url" :placeholder="apiFormatPlaceholder" @input="form.translation_api_url = ($event.target as HTMLInputElement).value" />
          </div>
          <div class="m3-field">
            <div class="field-label">
              <label for="translation-key">API Key</label>
              <HelpTooltip label="API Key" :text="t('translation.keyHint')" />
              <span v-if="form.translation_key_configured" class="translation-key-chip" :class="{ 'is-clearing': clearTranslationKey }">{{ t(clearTranslationKey ? 'translation.keyWillClear' : 'translation.keySavedChip') }}</span>
            </div>
            <div class="translation-key-row">
              <md-outlined-text-field id="translation-key" type="password" autocomplete="new-password" :value="translationKey" :placeholder="t(form.translation_key_configured ? 'translation.keyPlaceholderKeep' : 'translation.keyPlaceholder')" @input="translationKey = ($event.target as HTMLInputElement).value; clearTranslationKey = false" />
              <md-text-button v-if="form.translation_key_configured" class="translation-key-clear" :disabled="clearTranslationKey" @click="clearTranslationKey = true; translationKey = ''">{{ t('translation.clearKey') }}</md-text-button>
            </div>
            <span v-if="form.translation_key_error" class="field-hint credential-error" role="alert">{{ form.translation_key_error }}</span>
          </div>
        </fieldset>

        <fieldset class="m3-field settings-fieldset translation-group" role="group" aria-labelledby="translation-group-model">
          <div id="translation-group-model" class="translation-legend">{{ t("translation.groupModel") }}</div>
          <div class="m3-field">
            <div class="field-label"><label for="translation-model">{{ t('translation.model') }}</label><HelpTooltip :label="t('translation.model')" :text="t('translation.probeHint')" /></div>
            <div class="m3-row translation-model-row">
              <md-outlined-select v-if="availableModels.length && !manualModelMode" id="translation-model" class="translation-model-control" :aria-label="t('translation.model')" :value="form.translation_model" @change="form.translation_model = ($event.target as HTMLSelectElement).value">
                <md-select-option v-for="model in modelOptions" :key="model" :value="model">{{ model }}</md-select-option>
              </md-outlined-select>
              <md-outlined-text-field v-else id="translation-model" class="translation-model-control" :value="form.translation_model" @input="form.translation_model = ($event.target as HTMLInputElement).value" />
              <md-outlined-button :disabled="Boolean(probeBusy) || !canProbe" @click="runProbe('models')">
                {{ t(probeBusy === 'models' ? 'translation.fetchingModels' : (availableModels.length ? 'translation.refreshModels' : 'translation.fetchModels')) }}
              </md-outlined-button>
            </div>
            <div v-if="availableModels.length" class="translation-model-actions">
              <md-text-button v-if="!manualModelMode" class="translation-manual-model" @click="manualModelMode = true">
                {{ t('translation.manualModel') }}
              </md-text-button>
              <md-text-button v-else class="translation-manual-model" @click="manualModelMode = false">
                {{ t('translation.selectModelFromList') }}
              </md-text-button>
            </div>
            <span v-if="modelsError" class="field-hint state-error" role="alert">{{ modelsError }}</span>
          </div>
          <div class="m3-field">
            <div class="field-label"><label for="translation-target-language">{{ t('translation.targetLanguage') }}</label><HelpTooltip :label="t('translation.targetLanguage')" :text="t('translation.targetLanguageHint')" /></div>
            <md-outlined-select id="translation-target-language" :value="canonicalTargetLanguage" @change="form.translation_target_language = ($event.target as HTMLSelectElement).value">
              <md-select-option value="">{{ t('translation.followUiLanguage', { language: t(`settings.languages.${locale}`) }) }}</md-select-option>
              <md-select-option v-for="code in TARGET_LANGUAGE_CODES" :key="code" :value="code">{{ t(`translation.targetLanguages.${code}`) }}</md-select-option>
            </md-outlined-select>
          </div>
        </fieldset>

        <div class="m3-field">
          <div class="translation-choice-row">
            <label class="m3-choice"><md-checkbox id="translation-confirm-before-start" :checked="form.translation_confirm_before_start === true" @change="form.translation_confirm_before_start = ($event.target as HTMLInputElement).checked" />{{ t("translation.confirmBeforeStart") }}</label>
            <HelpTooltip :label="t('translation.confirmBeforeStart')" :text="t('translation.confirmBeforeStartHint')" />
          </div>
        </div>

        <div class="m3-field">
          <div class="translation-probe-row" :aria-busy="Boolean(probeBusy)">
            <md-outlined-button :disabled="Boolean(probeBusy) || !canProbe || !form.translation_model.trim()" @click="runProbe('test')">{{ t(probeBusy === 'test' ? 'translation.testing' : 'translation.testService') }}</md-outlined-button>
            <span v-if="probeBusy === 'test'" class="field-hint" role="status">{{ t('translation.testing') }}</span>
            <span v-else-if="testResult" class="field-hint" :class="testFailed ? 'state-error' : 'state-success'" :role="testFailed ? 'alert' : 'status'">{{ testResult }}</span>
          </div>
        </div>

        <!-- 高级设置入口：显示推荐参数摘要与待修正项数，进入二级页面 -->
        <button type="button" class="translation-advanced-entry" :aria-label="advancedEntryAria" @click="openAdvanced">
          <span class="translation-advanced-text">
            <span class="translation-advanced-title">{{ t("translation.agent.advancedEntry") }}</span>
            <span class="translation-advanced-meta">{{ advancedEntryMeta }}</span>
          </span>
          <span v-if="translationIssueCount" class="translation-advanced-error" role="status">{{ t("translation.agent.entryErrors", { count: translationIssueCount }) }}</span>
          <svg class="translation-advanced-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
        </button>
      </template>

      <TranslationAdvancedPanel
        v-else
        :values="form"
        :extra-text="translationJson"
        :extra-error="translationJsonError"
        :extra-value="translationExtraValue"
        :locate="advancedLocate"
        :errors="agentErrors"
        :detailed-diagnostics="detailedDiagnostics"
        :diagnostics-busy="diagnosticsBusy"
        :diagnostics-error="diagnosticsError"
        :diagnostics-version="diagnosticsVersion"
        @set-int="setAgentInt"
        @set-replay="form.translation_agent_replay_unknown = $event"
        @set-extra="setExtra"
        @set-diagnostics="changeDiagnostic"
        @reset-all="notify(t('translation.agent.restoredAll'))"
        @back="backToBasic"
      />
    </template>

    <template v-else>
      <span class="field-hint">{{ t("settings.maintenanceHint") }}</span>
      <MaintenancePanel v-if="active" />
      <div class="settings-maintenance">
        <div class="settings-maintenance-item">
          <span class="settings-maintenance-text"><span class="field-label"><strong>{{ t("settings.authLabel") }}</strong><HelpTooltip :label="t('settings.authLabel')" :text="t('settings.clearAuthHint')" /></span></span>
          <md-text-button class="danger-button" @click="openConfirm">{{ t("common.clear") }}</md-text-button>
        </div>
      </div>
    </template>

    <dialog ref="confirmDialog" class="m3-dialog" aria-labelledby="clear-auth-dialog-title">
      <h2 id="clear-auth-dialog-title">{{ t("settings.clearAuth") }}</h2>
      <p>{{ t("settings.clearAuthConfirm") }}</p>
      <div class="m3-row dialog-actions"><md-text-button @click="closeConfirm">{{ t("common.cancel") }}</md-text-button><md-filled-button @click="handleConfirm">{{ t("settings.clearAuth") }}</md-filled-button></div>
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
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import HelpTooltip from "../common/HelpTooltip.vue";
import MaintenancePanel from "./MaintenancePanel.vue";
import TranslationAdvancedPanel from "./TranslationAdvancedPanel.vue";
import { fetchTranslationModels, testTranslationService, API_FORMATS, TARGET_LANGUAGE_CODES, canonicalApiFormat, canonicalLanguage, getTranslationDiagnosticStatus, setTranslationDiagnostic, type TranslationProbe } from "../../api/translation";
import {
  countModifiedTranslationAgent,
  normalizeTranslationAgentSettings,
  translationAgentIntRule,
  validateTranslationAgent,
  type TranslationAgentIntKey,
  type TranslationAgentIssueCode,
  type TranslationAgentLocate,
} from "../../stores/settings";
import { useSettingsStore } from "../../stores/settings";
import { groupRoots } from "../../router/navigation";
import { useAuthStore } from "../../stores/auth";
import { errorMessage, setWindowTheme, type Settings } from "../../api/tauri";
import { formatTemplatePlaceholders, ILLUST_TEMPLATE_PLACEHOLDERS, NOVEL_TEMPLATE_PLACEHOLDERS, validateFilenameTemplate, type FilenameTemplateKind } from "../../utils/filenameTemplate";
import { notify } from "../../ui/notify";
import type { SettingsSection } from "./sections";

const props = withDefaults(defineProps<{ section: SettingsSection; active?: boolean }>(), { active: true });

const { t, locale } = useI18n();
const settingsStore = useSettingsStore();
const authStore = useAuthStore();
const formats = ["txt", "markdown"];
const confirmDialog = ref<HTMLDialogElement | null>(null);
const form = ref<Settings>({ ...settingsStore.settings });
const translationKey = ref("");
const clearTranslationKey = ref(false);
const translationJson = ref("{}");
const translationJsonError = ref("");
const availableModels = ref<string[]>([]);
const manualModelMode = ref(false);
const modelsError = ref("");
const probeBusy = ref<"test" | "models" | null>(null);
const testResult = ref("");
const testFailed = ref(false);
/** 一级 / 二级视图：同一内容区切换，不叠加新模态；草稿始终是 form。 */
const translationView = ref<"basic" | "advanced">("basic");
/**
 * 本次运行有效的详细诊断开关：不落 settings.json，不参与「已修改 N 项」与恢复推荐值。
 * 真值以后端会话状态为准（打开设置时读取），切换失败回滚并显示错误。
 */
const detailedDiagnostics = ref(false);
const diagnosticsBusy = ref(false);
const diagnosticsError = ref("");
/** 每次读 / 写诊断状态后自增：让二级开关显式与真实值对齐（失败回滚时绑定值可能没变）。 */
const diagnosticsVersion = ref(0);
/** 保存失败的最后一次定位请求：二级页据此切分类、滚动并聚焦。 */
const advancedLocate = ref<TranslationAgentLocate | null>(null);
let probeVersion = 0;
let probeRequest = 0;
const canProbe = computed(() => Boolean(form.value.translation_api_url.trim() && !clearTranslationKey.value && (translationKey.value.trim() || form.value.translation_key_configured)));
/** 下拉选项始终包含当前值：手动输入的模型可能不在获取到的列表里，切换控件时不丢配置。 */
const modelOptions = computed(() => availableModels.value.includes(form.value.translation_model) || !form.value.translation_model.trim()
  ? availableModels.value
  : [form.value.translation_model, ...availableModels.value]);
/** 目标语言回显：手改 settings.json 写入的语言别名（如 en-US / zh_Hant）也归一化到白名单 code。 */
const canonicalTargetLanguage = computed(() => canonicalLanguage(form.value.translation_target_language));
/** 接口协议回显：手改 settings.json 写入的未知值按默认 Chat Completions 处理（与后端加载回落一致）。 */
const apiFormat = computed(() => canonicalApiFormat(form.value.translation_api_format));
/** URL 帮助与占位随协议变化：端点后缀与常见服务基址各不同。 */
const API_FORMAT_ENDPOINTS: Record<string, { endpoint: string; placeholder: string }> = {
  chat_completions: { endpoint: "/chat/completions", placeholder: "https://api.openai.com/v1" },
  responses: { endpoint: "/responses", placeholder: "https://api.openai.com/v1" },
  anthropic: { endpoint: "/messages", placeholder: "https://api.anthropic.com/v1" },
};
const apiFormatEndpoint = computed(() => API_FORMAT_ENDPOINTS[apiFormat.value].endpoint);
const apiFormatPlaceholder = computed(() => API_FORMAT_ENDPOINTS[apiFormat.value].placeholder);
/** 切换协议只换端点推导与探测上下文，清空模型列表避免跨协议混淆。 */
function changeApiFormat(value: string) {
  form.value.translation_api_format = canonicalApiFormat(value);
  availableModels.value = [];
  form.value.translation_models = [];
  manualModelMode.value = false;
  modelsError.value = "";
}
let lastApiUrl = form.value.translation_api_url;
let lastApiFormat = form.value.translation_api_format;
watch(() => [form.value.translation_api_url, form.value.translation_api_format], ([newUrl, newFormat]) => {
  if (newUrl !== lastApiUrl || newFormat !== lastApiFormat) {
    lastApiUrl = newUrl;
    lastApiFormat = newFormat;
    availableModels.value = [];
    form.value.translation_models = [];
    manualModelMode.value = false;
    modelsError.value = "";
  }
}, { flush: "sync" });
watch(() => [form.value.translation_api_url, form.value.translation_api_format, translationKey.value, clearTranslationKey.value, form.value.translation_model, translationJson.value], () => {
  probeVersion++; testResult.value = ""; modelsError.value = "";
}, { flush: "sync" });
onBeforeUnmount(() => { probeRequest++; });
function parseTranslationJson(): Record<string, unknown> {
  try {
    const extra = JSON.parse(translationJson.value);
    if (!extra || Array.isArray(extra) || typeof extra !== "object") throw new Error();
    translationJsonError.value = "";
    return extra;
  } catch {
    translationJsonError.value = t("translation.jsonInvalid");
    throw new Error(translationJsonError.value);
  }
}
async function runProbe(action: "test" | "models") {
  if (probeBusy.value || !canProbe.value) return;
  const request = ++probeRequest;
  const version = probeVersion;
  modelsError.value = ""; testResult.value = ""; testFailed.value = false;
  try {
    const probe: TranslationProbe = {
      api_url: form.value.translation_api_url, model: action === "test" ? form.value.translation_model : "",
      format: apiFormat.value,
      timeout_seconds: Number(form.value.translation_timeout_seconds) || 600,
      extra: action === "test" ? parseTranslationJson() : {},
      ...(translationKey.value.trim() ? { api_key: translationKey.value } : {}),
    };
    probeBusy.value = action;
    if (action === "models") {
      const models = await fetchTranslationModels(probe);
      if (request === probeRequest && version === probeVersion) {
        availableModels.value = models;
        form.value.translation_models = models;
        manualModelMode.value = false;
        if (!form.value.translation_model.trim() && models.length > 0) {
          form.value.translation_model = models[0];
        }
      }
    } else {
      await testTranslationService(probe);
      if (request === probeRequest && version === probeVersion) testResult.value = t("translation.serviceAvailable");
    }
  } catch (error) {
    if (request !== probeRequest || version !== probeVersion) return;
    if (action === "models") modelsError.value = errorMessage(error) || t("translation.modelsFailed");
    else { testFailed.value = true; testResult.value = errorMessage(error) || t("translation.testFailed"); }
  } finally { if (request === probeRequest) probeBusy.value = null; }
}
/** 收起下拉回到自由文本输入：列表可能不含目标模型，或用户想直接粘贴模型 ID。 */
function useManualModel() { manualModelMode.value = true; }

// ===== 小说翻译高级设置（一级摘要 + 二级页面）=====

/** 高级 JSON 的草稿解析：合法对象返回对象，非法返回 undefined（不写错误状态）。 */
function parseExtraDraft(text: string): Record<string, unknown> | undefined {
  try {
    const parsed = JSON.parse(text);
    return parsed && !Array.isArray(parsed) && typeof parsed === "object"
      ? (parsed as Record<string, unknown>)
      : undefined;
  } catch {
    return undefined;
  }
}
const translationExtraValue = computed(() => parseExtraDraft(translationJson.value));
/** 二级字段的即时错误：草稿变即重算，越界立刻红字。 */
const agentIssues = computed(() => validateTranslationAgent(form.value));
const agentErrors = computed<Partial<Record<TranslationAgentIntKey, TranslationAgentIssueCode>>>(() => {
  const errors: Partial<Record<TranslationAgentIntKey, TranslationAgentIssueCode>> = {};
  for (const issue of agentIssues.value) if (!errors[issue.key]) errors[issue.key] = issue.code;
  return errors;
});
/** 一级入口的错误计数：实际出错字段数（含高级 JSON 解析失败）。 */
const translationIssueCount = computed(() => agentIssues.value.length + (translationExtraValue.value === undefined ? 1 : 0));
const translationModified = computed(() =>
  countModifiedTranslationAgent(form.value, translationExtraValue.value));
const translationModifiedTotal = computed(() =>
  (Object.values(translationModified.value) as number[]).reduce((sum, count) => sum + count, 0));
/** 一级摘要文字：「推荐参数」或「已修改 N 项」，出错时追加待修正提示。 */
const advancedEntryMeta = computed(() => translationModifiedTotal.value === 0
  ? t("translation.agent.recommendedParams")
  : t("translation.agent.modifiedCount", { count: translationModifiedTotal.value }));
const advancedEntryAria = computed(() => `${t("translation.agent.advancedEntry")}，${advancedEntryMeta.value}`);
function openAdvanced() {
  advancedLocate.value = null;
  translationView.value = "advanced";
  void nextTick(() => document.getElementById("translation-advanced-page")?.scrollIntoView({ block: "start" }));
}
function backToBasic() { translationView.value = "basic"; }
/** 二级写回整数参数：未填（空串）记为 NaN，保存时由校验拦截并在原位提示。 */
function setAgentInt(key: TranslationAgentIntKey, value: number) {
  (form.value as unknown as Record<string, number>)[key] = value;
}
/** 二级写回高级 JSON 草稿：编辑即清上一次的解析错误，避免旧红字误导。 */
function setExtra(value: string) {
  translationJson.value = value;
  translationJsonError.value = "";
}
/** 保存失败定位到分类与字段：值是 rule 定义，交叉违规挂在被标红的下游字段上。 */
function locateInvalidAgentField(key: TranslationAgentIntKey) {
  const rule = translationAgentIntRule(key);
  translationView.value = "advanced";
  advancedLocate.value = { category: rule.category, key };
}
function resetTranslationDraft() {
  probeRequest++; probeVersion++; probeBusy.value = null;
  availableModels.value = Array.isArray(form.value.translation_models) ? [...form.value.translation_models] : [];
  manualModelMode.value = false;
  modelsError.value = ""; testResult.value = "";
  translationKey.value = "";
  clearTranslationKey.value = false;
  translationJson.value = JSON.stringify(form.value.translation_extra, null, 2);
  translationJsonError.value = "";
  translationView.value = "basic";
  advancedLocate.value = null;
}
/**
 * 读取会话级诊断状态：打开设置时以后端为准回显。
 * 诊断开关不是设置草稿（取消不撤销、保存不写入），所以这里只同步显示值。
 */
async function loadDiagnosticStatus() {
  try {
    const status = await getTranslationDiagnosticStatus();
    detailedDiagnostics.value = status === true;
    diagnosticsError.value = "";
    diagnosticsVersion.value += 1;
  } catch (err) {
    diagnosticsError.value = errorMessage(err) || t("translation.agent.diagnostics.readFailed");
  }
}
/** 诊断开关：立即写后端；失败回滚到切换前的值并给出可读错误。 */
async function changeDiagnostic(enabled: boolean) {
  if (diagnosticsBusy.value) return;
  const previous = detailedDiagnostics.value;
  diagnosticsBusy.value = true;
  diagnosticsError.value = "";
  try {
    detailedDiagnostics.value = (await setTranslationDiagnostic(enabled)) === true;
  } catch (err) {
    detailedDiagnostics.value = previous;
    diagnosticsError.value = errorMessage(err) || t("translation.agent.diagnostics.writeFailed");
  } finally {
    diagnosticsBusy.value = false;
    diagnosticsVersion.value += 1;
  }
}
const startupPageOptions = computed(() => Object.entries(groupRoots).map(([group, value]) => ({ value, label: t(`workspace.${group}`) })));
const savedSnapshot = ref("");
const langOptions = computed(() => ["zh-CN", "en-US", "ja-JP"].map((value) => ({ label: t(`settings.languages.${value}`), value })));
const themeOptions = computed(() => [{ label: t("settings.themes.light"), value: "light" }, { label: t("settings.themes.dark"), value: "dark" }, { label: t("settings.themes.auto"), value: "auto" }]);
const paletteOptions = computed(() => ["pixiv", "indigo", "jade", "violet", "amber"].map(value => ({ value, label: t(`settings.palettes.${value}`) })));
// 缩略图档位取值集合与后端白名单一致（src-tauri/src/settings.rs 的 THUMB_*_TIERS）。
// 详情页 medium 是「接口 regular 原样」（最长边 1200），故文案与列表档的「中 · 540px」分开。
const gridTierOptions = computed(() => ["small", "medium", "large"].map(value => ({ value, label: t(`settings.thumbTiers.${value}`) })));
const detailTierOptions = computed(() => [{ value: "medium", label: t("settings.thumbDetailTiers.medium") }, { value: "large", label: t("settings.thumbDetailTiers.large") }, { value: "original", label: t("settings.thumbTiers.original") }]);
const fullscreenTierOptions = computed(() => [{ value: "large", label: t("settings.thumbTiers.large") }, { value: "original", label: t("settings.thumbTiers.original") }]);
/** 图片缓存上限预设档（MiB），与后端合法区间 256~2048 对齐；区间外的现值走「自定义」。 */
const cachePresets = [
  { value: 256, label: "256 MiB" },
  { value: 512, label: "512 MiB" },
  { value: 1024, label: "1 GiB" },
  { value: 2048, label: "2 GiB" },
];
const cacheMaxCustom = computed(() => !cachePresets.some((preset) => preset.value === form.value.image_cache_max_mib));
const cacheMaxSelectValue = computed(() => (cacheMaxCustom.value ? "custom" : String(form.value.image_cache_max_mib)));
/** 选「自定义」时保持当前数值原样，由自定义输入框接管；选预设直接落值。 */
function changeCacheMax(value: string) {
  if (value !== "custom") { form.value.image_cache_max_mib = Number(value); cacheMaxError.value = ""; }
}

// ===== 数字字段草稿（下载等待上限 / 缓存上限自定义值）=====
// 草稿态允许 NaN：清空输入不伪造 0（0 是会被 save 拦下的非法值，回写会打断输入），
// 显示回落空串（同 TranslationAgentFields 的范式）；越界不在输入时拦，保存时字段级标红。

/** 保存前校验错误（字段内 error-text 展示，输入 / 选预设即清）。 */
const maxWaitError = ref("");
const cacheMaxError = ref("");
/** 清空 / 非数字显示为空串，避免「NaN」或回写的 0 出现在输入框。 */
const maxWaitDisplay = computed(() => Number.isFinite(form.value.max_wait_seconds) ? String(form.value.max_wait_seconds) : "");
const cacheMaxDisplay = computed(() => Number.isFinite(form.value.image_cache_max_mib) ? String(form.value.image_cache_max_mib) : "");
function onMaxWaitInput(event: Event) {
  const raw = (event.target as HTMLInputElement).value.trim();
  form.value.max_wait_seconds = raw === "" ? Number.NaN : Number(raw);
  maxWaitError.value = "";
}
function onCacheMaxInput(event: Event) {
  const raw = (event.target as HTMLInputElement).value.trim();
  form.value.image_cache_max_mib = raw === "" ? Number.NaN : Number(raw);
  cacheMaxError.value = "";
}

function toggleFormat(format: string, checked: boolean) { form.value.output_formats = checked ? [...form.value.output_formats, format] : form.value.output_formats.filter((item) => item !== format); }
/** 输出目录的保存前校验错误（字段内 error-text 展示，输入 / 浏览回填即清）。 */
const outputDirError = ref("");
/** Windows 路径非法字符，与后端 validate_output_dir 的名单一致（本项目主目标平台，统一按 Windows 规则拦下）。 */
const OUTPUT_DIR_ILLEGAL_CHARS = "*?\"<>|";
/**
 * 收集输出目录中的非法字符：按出现顺序去重，控制字符改写成 U+00XX
 * （原字符不可见，直接插进提示里等于没有信息）。
 */
function outputDirIllegalChars(value: string): string {
  const seen = new Set<string>();
  const found: string[] = [];
  for (const char of value) {
    const code = char.codePointAt(0) ?? 0;
    const shown = code < 32
      ? `U+${code.toString(16).toUpperCase().padStart(4, "0")}`
      : OUTPUT_DIR_ILLEGAL_CHARS.includes(char) ? char : "";
    if (shown && !seen.has(shown)) { seen.add(shown); found.push(shown); }
  }
  return found.join(" ");
}
/** 下载文件名模板的保存前校验错误（字段内 error-text 展示，输入即清；键对应三个模板输入框）。 */
const templateErrors = ref({ illust: "", novel: "", series: "" });
/** supporting text 列出可用占位符：清单与校验白名单同源（utils/filenameTemplate.ts），不会漂移。 */
const illustTemplateHint = computed(() => t("settings.filenameTemplateHint", { list: formatTemplatePlaceholders(ILLUST_TEMPLATE_PLACEHOLDERS) }));
const novelTemplateHint = computed(() => t("settings.filenameTemplateHint", { list: formatTemplatePlaceholders(NOVEL_TEMPLATE_PLACEHOLDERS) }));
const seriesTemplateHint = computed(() => t("settings.filenameTemplateSeriesHint", { list: formatTemplatePlaceholders(NOVEL_TEMPLATE_PLACEHOLDERS), required: "{order}" }));
/** 语言与主题同理是即时预览：立即切 i18n / 写 localStorage，落盘仍等保存。 */
function applyLanguage(lang: string) { locale.value = lang; localStorage.setItem("pixiv-tool-lang", lang); }
function changeLang(lang: string) { applyLanguage(lang); form.value.language = lang; }
function snapshot() { return JSON.stringify([form.value, translationJson.value]); }
function applyPreview() { const dark = form.value.theme === "dark" || (form.value.theme === "auto" && window.matchMedia("(prefers-color-scheme: dark)").matches); document.documentElement.classList.toggle("dark", dark); document.documentElement.dataset.palette = form.value.theme_color || "pixiv"; setWindowTheme(dark ? "dark" : "light").catch(() => {}); }
/** 离开「小说翻译」组时回到一级，避免下次进入落在二级分类页。 */
watch(() => props.section, (section) => {
  if (section !== "translation") {
    translationView.value = "basic";
    advancedLocate.value = null;
  }
});
watch(() => [form.value.theme, form.value.theme_color], applyPreview);
onMounted(async () => { await settingsStore.fetchSettings(); if (typeof settingsStore.settings.max_wait_seconds !== "number") settingsStore.settings.max_wait_seconds = 180;
  // 老配置 / mock 缺运行参数键时补后端默认值，避免草稿里出现 undefined 被当成越界
  form.value = normalizeTranslationAgentSettings({ ...settingsStore.settings });
  outputDirError.value = "";
  // 老配置缺模板键时回落默认值（与 stores/settings.ts 初值一致），避免空值卡住保存校验
  if (!form.value.illust_filename_template) form.value.illust_filename_template = "{title}_{pid}_p{page}";
  if (!form.value.novel_filename_template) form.value.novel_filename_template = "{title}_{nid}";
  if (!form.value.novel_series_filename_template) form.value.novel_series_filename_template = "{order}_{title}";
  resetTranslationDraft(); savedSnapshot.value = snapshot(); if (form.value.language && form.value.language !== locale.value) applyLanguage(form.value.language);
  // 诊断是会话级状态，不属于设置草稿：每次打开设置都以后端为准回显。
  if (props.active) void loadDiagnosticStatus(); });
/** 设置弹窗每次打开时重新读取诊断开关（可能有别处改过，或应用重启后已复位）。 */
watch(() => props.active, (active) => { if (active) void loadDiagnosticStatus(); });
/** 有未保存改动：弹窗据此决定关闭前是否提示。 */
function hasUnsaved() { return Boolean(translationKey.value) || clearTranslationKey.value || Boolean(savedSnapshot.value) && savedSnapshot.value !== snapshot(); }
/** 回滚到上次保存值（取消 / 确认放弃修改）。主题预览随表单回到已保存值。 */
function reset() {
  form.value = normalizeTranslationAgentSettings({ ...settingsStore.settings });
  templateErrors.value = { illust: "", novel: "", series: "" };
  outputDirError.value = "";
  maxWaitError.value = "";
  cacheMaxError.value = "";
  translationJsonError.value = "";
  resetTranslationDraft(); savedSnapshot.value = snapshot();
  if (form.value.language && form.value.language !== locale.value) applyLanguage(form.value.language);
}
defineExpose({ save, reset, hasUnsaved });
async function save(): Promise<boolean> {
  // 数字字段保存前预校验：空 / NaN / 越界在字段下方直接标红并阻止整表写入（同 output-dir 配方）。
  if (!Number.isFinite(form.value.max_wait_seconds) || form.value.max_wait_seconds < 30) {
    maxWaitError.value = t("settings.maxWaitInvalid");
    notify(maxWaitError.value);
    return false;
  }
  const cacheMax = form.value.image_cache_max_mib;
  if (!Number.isInteger(cacheMax) || cacheMax < 256 || cacheMax > 2048) {
    cacheMaxError.value = t("settings.imageCacheMaxInvalid");
    notify(cacheMaxError.value);
    return false;
  }
  // 运行参数（含二级隐藏分类）统一校验：失败时切到二级并把焦点落到出错字段，
  // 一级入口同时显示错误计数，用户不必自己翻分类找问题。
  const agentIssues = validateTranslationAgent(form.value);
  if (agentIssues.length) {
    locateInvalidAgentField(agentIssues[0].key);
    notify(t("translation.agent.errors.saveBlocked", { count: agentIssues.length }));
    return false;
  }
  // 输出目录保存前预校验：空值 / 非法字符在字段下方直接标红并阻止整表写入；
  // 目录是否可创建只能由后端判定，交给 saveSettings reject 后的回填。
  if (!form.value.output_dir.trim()) {
    outputDirError.value = t("settings.outputDirEmpty");
    notify(outputDirError.value);
    return false;
  }
  const illegalChars = outputDirIllegalChars(form.value.output_dir);
  if (illegalChars) {
    outputDirError.value = t("settings.outputDirInvalidChars", { chars: illegalChars });
    notify(outputDirError.value);
    return false;
  }
  outputDirError.value = "";
  // 文件名模板保存前预校验：非法字段标红（error-text 替换 supporting text）并阻止整表写入
  const templateChecks: Array<["illust" | "novel" | "series", FilenameTemplateKind, string]> = [
    ["illust", "illust", form.value.illust_filename_template],
    ["novel", "novel", form.value.novel_filename_template],
    ["series", "novelSeries", form.value.novel_series_filename_template],
  ];
  let templateInvalid = false;
  for (const [field, kind, value] of templateChecks) {
    const issue = validateFilenameTemplate(kind, value);
    templateErrors.value[field] = issue ? t(issue.key, issue.params) : "";
    templateInvalid = templateInvalid || issue !== null;
  }
  if (templateInvalid) return false;
  let extra: Record<string, unknown>;
  try { extra = parseTranslationJson(); }
  catch {
    // 原始 JSON 校验失败同样定位到二级「请求扩展」分类的文本框
    translationView.value = "advanced";
    advancedLocate.value = { category: "extra", key: null };
    notify(translationJsonError.value);
    return false;
  }
  try {
    await settingsStore.saveSettings({ ...form.value, translation_extra: extra,
      ...(clearTranslationKey.value ? { translation_api_key: "" } : translationKey.value.trim() ? { translation_api_key: translationKey.value } : {}) });
    form.value = normalizeTranslationAgentSettings({ ...settingsStore.settings });
    resetTranslationDraft(); savedSnapshot.value = snapshot(); notify(t("settings.saved")); return true;
  } catch (err) {
    const message = errorMessage(err) || t("settings.saveFailed");
    // 后端 validate_output_dir 的中文文案都含「输出目录」：落到字段下方，同时保留通知
    if (message.includes("输出目录")) outputDirError.value = message;
    notify(message);
    return false;
  }
}
async function handleBrowse() {
  try {
    // 文本框已有路径时以其为初始目录；空值 / 不存在 / 含非法字符时由对话框插件回落默认位置。
    const path = await settingsStore.selectDirectory(form.value.output_dir.trim() || undefined);
    if (path) { form.value.output_dir = path; outputDirError.value = ""; }
  } catch { notify(t("settings.pickFailed")); }
}
async function openConfirm() { await nextTick(); confirmDialog.value?.showModal(); }
function closeConfirm() { confirmDialog.value?.close(); }
async function handleConfirm() { closeConfirm(); try { await authStore.logout(); notify(t("settings.authCleared")); } catch (err) { notify(errorMessage(err) || t("settings.saveFailed")); } }
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
/* 状态色只做补充，状态本身始终有可读文字（DESIGN.md Semantic State Rule）。 */
.credential-error, .state-error { color: var(--md-sys-color-error); }
.state-success { color: var(--state-success-ink); }
.translation-model-control { flex: 1; min-width: 0; }
.translation-model-row > md-outlined-button { flex: none; }
/* 小说翻译：三组子分区（服务连接 / 模型与语言 / 高级）。组标题为 12px/600 辅助层级，
 * 与 14px/500 字段标签形成层级差；组间用既有 35% outline 分隔线分段，不新增容器或 token。
 * 用 role="group" + aria-labelledby 给 fieldset 命名，不用 legend——legend 在带 border 的
 * fieldset 里会被画进 border 缺口（压住分隔线），浮动渲染在各浏览器下又会挤开组内首字段。 */
fieldset.translation-group { display: block; }
fieldset.translation-group + fieldset.translation-group { margin-top: var(--space-xl); padding-top: var(--space-lg); border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.translation-legend { margin-bottom: var(--space-md); color: var(--ink-muted); font-size: 12px; font-weight: 600; }
fieldset.translation-group > .m3-field:last-child { margin-bottom: 0; }
.translation-key-row { display: flex; align-items: center; gap: var(--space-sm); }
.translation-key-row > md-outlined-text-field { flex: 1; min-width: 0; }
.translation-key-clear { flex: none; }
/* 凭据状态胶囊：中性描边胶囊（与频道筛选 chips 同配方）；待清除时文案与描边切 error 角色 */
.translation-key-chip { display: inline-flex; align-items: center; min-height: 20px; margin-left: var(--space-xxs); padding: 0 var(--space-sm); border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 60%, transparent); border-radius: 999px; color: var(--ink-muted); font-size: 12px; font-weight: 600; }
.translation-key-chip.is-clearing { color: var(--md-sys-color-error); border-color: color-mix(in srgb, var(--md-sys-color-error) 45%, transparent); }
/* 低强调动作靠左对齐，不随 grid 拉伸成整行宽的胶囊 */
.translation-model-actions { display: flex; align-items: center; gap: var(--space-xs); margin-top: 2px; }
.translation-manual-model { justify-self: start; }
.translation-probe-row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-md); }
/* 「翻译前确认术语与风格」：勾选行与帮助图标同排，窄窗自然换行 */
.translation-choice-row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-sm); }
/* 高级设置入口：整行可点的次级动作行（非卡片），标题 + 参数摘要 + 可选错误计数；
 * hover 用既有 8% primary 状态层，focus-visible 为 primary 2px 外环，不新增 token。 */
.translation-advanced-entry {
  display: flex; align-items: center; gap: var(--space-md); width: 100%; min-height: 56px;
  margin-top: var(--space-xl); padding: var(--space-sm) var(--space-xs); color: var(--ink);
  font: inherit; text-align: left; background: transparent; border: 0;
  border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent);
  border-bottom: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent);
  cursor: pointer;
}
.translation-advanced-entry:hover { background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }
.translation-advanced-entry:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 2px; }
.translation-advanced-text { display: grid; gap: 2px; flex: 1; min-width: 0; }
.translation-advanced-title { font-size: 14px; font-weight: 500; }
.translation-advanced-meta { color: var(--ink-muted); font-size: 12px; }
.translation-advanced-error { flex: none; color: var(--md-sys-color-error); font-size: 12px; font-weight: 600; }
.translation-advanced-chevron { flex: none; width: 18px; height: 18px; color: var(--ink-muted); }
.danger-button { --md-text-button-label-text-color: var(--md-sys-color-error); }
.palette-options { display: flex; flex-wrap: wrap; gap: var(--space-sm) var(--space-lg); }.palette-option { display: inline-flex; align-items: center; gap: var(--space-xs); min-height: 40px; }.palette-swatch { width: 18px; height: 18px; border: 1px solid var(--md-sys-color-outline); border-radius: 50%; }/* 色块取各色板 primary 的规范值，改色板时必须与 main.css 的 [data-palette] 定义、.impeccable/design.json 的 extensions.palettes 同步 */.palette-pixiv { background: #006eaf; }.palette-indigo { background: #445e91; }.palette-jade { background: #006c4d; }.palette-violet { background: #76547b; }.palette-amber { background: #8b5000; }
/* 维护组：名称与帮助图标在左、动作按钮在右，行间以 divider 分隔 */
.settings-maintenance { display: grid; margin-top: var(--space-md); border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.settings-maintenance-item { display: flex; align-items: center; justify-content: space-between; gap: var(--space-lg); padding: var(--space-md) 0; border-bottom: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.settings-maintenance-text { display: grid; gap: var(--space-xxs); min-width: 0; }
.settings-maintenance-text strong { color: var(--ink); font-size: 14px; font-weight: 500; }
@media (max-width: 640px) { .settings-path-row, .translation-model-row, .translation-key-row { align-items: stretch; flex-direction: column; } .translation-key-row > .translation-key-clear { align-self: flex-start; } }
</style>
