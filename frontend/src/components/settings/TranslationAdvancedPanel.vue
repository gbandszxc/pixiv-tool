<script setup lang="ts">
/**
 * 二级「高级设置」页：分类入口 → 分类详情。
 *
 * 与一级共用 SettingsPanel 的同一份草稿（值由父级持有，这里只发事件），
 * 所以在一级 / 二级之间往返不丢输入。它不是新模态：渲染在设置内容区里，
 * 顶部「返回小说翻译」把视图切回一级。
 *
 * 页面状态（当前分类）在本组件内；父级通过 `locate` 在保存失败时指定
 * 「打开哪个分类、聚焦哪个字段」。
 */
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import HelpTooltip from "../common/HelpTooltip.vue";
import TranslationAgentFields from "./TranslationAgentFields.vue";
import type { Settings } from "../../api/tauri";
import {
  TRANSLATION_AGENT_CATEGORIES,
  agentFieldId,
  countModifiedTranslationAgent,
  resetTranslationAgentCategory,
  resetTranslationAgentSettings,
  type TranslationAgentCategory,
  type TranslationAgentIntKey,
  type TranslationAgentIssueCode,
  type TranslationAgentLocate,
} from "../../stores/settings";

const props = defineProps<{
  /** 草稿（SettingsPanel 的 form），本组件不回写一级字段。 */
  values: Settings;
  /** 高级 JSON 草稿文本与即时错误（由父级持有，二级只编辑）。 */
  extraText: string;
  extraError: string;
  /** 已解析的 JSON 对象；解析失败时为 undefined（计入「已修改」并参与保存校验）。 */
  extraValue: unknown;
  /** 保存失败定位请求；每次失败由父级给出新对象以触发跳转。 */
  locate: TranslationAgentLocate | null;
  errors: Partial<Record<TranslationAgentIntKey, TranslationAgentIssueCode>>;
  /** 本次运行有效的详细诊断（不持久化）。 */
  detailedDiagnostics: boolean;
  /** 诊断开关切换中：禁用控件，避免并发写。 */
  diagnosticsBusy: boolean;
  /** 诊断开关读取 / 写入失败的可读错误。 */
  diagnosticsError: string;
  /** 每次读 / 写后自增：用于把开关控件显式对齐到真实状态（回滚时绑定值可能未变）。 */
  diagnosticsVersion: number;
}>();

const emit = defineEmits<{
  (event: "set-int", key: TranslationAgentIntKey, value: number): void;
  (event: "set-replay", value: boolean): void;
  (event: "set-extra", value: string): void;
  (event: "set-diagnostics", value: boolean): void;
  (event: "reset-all"): void;
  (event: "back"): void;
}>();

const { t } = useI18n();
const openCategory = ref<TranslationAgentCategory | null>(null);
/** 「恢复全部推荐值」的二次确认：就地展开影响说明，不叠新模态。 */
const confirmingAll = ref(false);
const diagnosticsCheckbox = ref<(HTMLElement & { checked: boolean }) | null>(null);

const counts = computed(() => countModifiedTranslationAgent(props.values, props.extraValue));
const totalModified = computed(() =>
  TRANSLATION_AGENT_CATEGORIES.reduce((sum, category) => sum + counts.value[category], 0));

/** 诊断不参与保存（仅本次运行），因此不显示「已修改 N 项」。 */
function categoryMeta(category: TranslationAgentCategory): string {
  if (category === "diagnostics") return t("translation.agent.diagnosticsMeta");
  const count = counts.value[category];
  return count === 0 ? t("translation.agent.recommendedShort") : t("translation.agent.modifiedCount", { count });
}
function categoryModified(): number {
  return openCategory.value ? counts.value[openCategory.value] : 0;
}
function openTarget(category: TranslationAgentCategory): void {
  openCategory.value = category;
  void nextTick(focusFirstControl);
}
function backToIndex(): void { openCategory.value = null; }
function backToService(): void { openCategory.value = null; emit("back"); }
function restoreCategory(): void {
  const category = openCategory.value;
  if (!category || category === "diagnostics") return;
  if (category === "extra") emit("set-extra", "{}");
  else resetTranslationAgentCategory(props.values, category);
}
function restoreAll(): void {
  resetTranslationAgentSettings(props.values);
  emit("set-extra", "{}");
  confirmingAll.value = false;
  emit("reset-all");
}

/** 进入分类后把焦点交给首个控件，键盘用户不必再从页面顶部 Tab 过去。 */
function focusFirstControl(): void {
  const host = document.getElementById("translation-advanced-page");
  host?.querySelector<HTMLElement>("md-outlined-text-field, md-outlined-select, md-checkbox, textarea")?.focus();
}
/**
 * 把焦点交给控件。Material Web 元素在升级完成前 `focus()` 是空操作，
 * 因此先调元素方法，未生效再退到其内部 input；仍未生效则隔一帧重试
 * （最多 8 次，约 130ms），避免元素刚插入时升级时序造成的偶发失焦。
 */
function focusControl(target: HTMLElement | null, attempt = 0): void {
  if (!target) return;
  (target as HTMLElement & { focus?: () => void }).focus?.();
  if (document.activeElement === target || target.matches(":focus-within")) return;
  const inner = target.shadowRoot?.querySelector<HTMLElement>("input, textarea");
  inner?.focus();
  if (inner && document.activeElement === inner) return;
  if (attempt >= 8) return;
  setTimeout(() => { if (document.contains(target)) focusControl(target, attempt + 1); }, 16);
}
/** 保存失败定位：切到目标分类、滚动到字段并聚焦。 */
function focusLocated(locate: TranslationAgentLocate): void {
  openCategory.value = locate.category;
  void nextTick(() => {
    const id = locate.key
      ? agentFieldId(locate.key)
      : locate.category === "extra" ? "translation-agent-extra" : "translation-agent-detailed-diagnostics";
    const target = document.getElementById(id);
    target?.scrollIntoView({ block: "center" });
    focusControl(target);
  });
}
/**
 * 开关状态由父级（后端值）决定：切换失败回滚时绑定值可能仍是 true，Vue 不会重写
 * 自定义元素的 checked；因此每次读 / 写（version 变化）都显式把控件对齐到真实值。
 */
watch(() => [props.detailedDiagnostics, props.diagnosticsVersion] as const, ([value]) => {
  const checkbox = diagnosticsCheckbox.value;
  if (checkbox && checkbox.checked !== value) checkbox.checked = value;
});
watch(() => props.locate, (locate) => { if (locate) focusLocated(locate); });
// 保存失败时父级会同时切到二级并给出定位目标；此时组件刚挂载，watch 不会触发。
onMounted(() => { if (props.locate) focusLocated(props.locate); });

const categoryTitleKey = (category: TranslationAgentCategory) => `translation.agent.categories.${category}.title`;
const categoryHintKey = (category: TranslationAgentCategory) => `translation.agent.categories.${category}.hint`;
</script>

<template>
  <div id="translation-advanced-page" class="translation-advanced">
    <div v-if="!openCategory" class="advanced-head">
      <div class="advanced-head-leading">
        <md-text-button class="advanced-back-button advanced-back-service" @click="backToService">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
          {{ t("translation.agent.backToService") }}
        </md-text-button>
        <div class="advanced-title-group">
          <h3 class="advanced-title">{{ t("translation.agent.advancedSettingsTitle") }}</h3>
        </div>
      </div>
      <span class="advanced-chip advanced-overview" :class="{ 'is-modified': totalModified > 0 }">
        {{ totalModified === 0 ? t("translation.agent.recommendedParams") : t("translation.agent.modifiedCount", { count: totalModified }) }}
      </span>
    </div>

    <!-- 分类首页：先给入口，避免一次铺开 21 个参数 -->
    <template v-if="!openCategory">
      <p class="advanced-intro">{{ t("translation.agent.intro") }}</p>
      <div class="advanced-index">
        <button v-for="category in TRANSLATION_AGENT_CATEGORIES" :key="category" type="button" class="advanced-index-item" @click="openTarget(category)">
          <span class="advanced-index-text">
            <span class="advanced-index-title">{{ t(categoryTitleKey(category)) }}</span>
            <span class="advanced-index-hint">{{ t(categoryHintKey(category)) }}</span>
          </span>
          <span class="advanced-index-meta" :class="{ 'is-modified': counts[category] > 0 }">{{ categoryMeta(category) }}</span>
          <svg class="advanced-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
        </button>
      </div>
      <div class="advanced-actions">
        <template v-if="!confirmingAll">
          <md-outlined-button @click="confirmingAll = true">{{ t("translation.agent.restoreAll") }}</md-outlined-button>
          <span class="field-hint">{{ t("translation.agent.restoreAllHint") }}</span>
        </template>
        <template v-else>
          <span class="advanced-confirm" role="alert">{{ t("translation.agent.restoreAllConfirm") }}</span>
          <md-filled-button @click="restoreAll">{{ t("translation.agent.restoreAllAccept") }}</md-filled-button>
          <md-text-button @click="confirmingAll = false">{{ t("common.cancel") }}</md-text-button>
        </template>
      </div>
    </template>

    <!-- 分类详情 -->
    <template v-else>
      <div class="advanced-detail-head">
        <div class="advanced-head-leading">
          <md-text-button class="advanced-back-button advanced-back-index" @click="backToIndex">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
            {{ t("translation.agent.backToIndex") }}
          </md-text-button>
          <div class="advanced-title-group">
            <h3 class="advanced-title">{{ t(categoryTitleKey(openCategory)) }}</h3>
          </div>
        </div>
        <span class="advanced-chip advanced-category" :class="{ 'is-modified': categoryModified() > 0 }">
          {{ openCategory === 'diagnostics' ? t("translation.agent.diagnosticsMeta") : (categoryModified() === 0 ? t("translation.agent.recommendedShort") : t("translation.agent.modifiedCount", { count: categoryModified() })) }}
        </span>
      </div>
      <p class="advanced-intro">{{ t(categoryHintKey(openCategory)) }}</p>

      <TranslationAgentFields
        v-if="openCategory === 'model' || openCategory === 'concurrency' || openCategory === 'retry' || openCategory === 'chunk'"
        :values="values"
        :category="openCategory"
        :errors="errors"
        @set-int="(key, value) => emit('set-int', key, value)"
        @set-replay="(value) => emit('set-replay', value)"
      />

      <div v-else-if="openCategory === 'extra'" class="m3-field">
        <div class="field-label">
          <label for="translation-agent-extra">{{ t("translation.advanced") }}</label>
          <HelpTooltip :label="t('translation.advanced')" :text="t('translation.jsonHint')" />
        </div>
        <md-outlined-text-field id="translation-agent-extra" type="textarea" rows="6" :value="extraText" :error="Boolean(extraError)" :error-text="extraError" @input="emit('set-extra', ($event.target as HTMLTextAreaElement).value)" />
        <p class="advanced-note">{{ t("translation.agent.extraBudgetHint") }}</p>
      </div>

      <div v-else class="m3-field">
        <div class="field-label">
          <label for="translation-agent-detailed-diagnostics">{{ t("translation.agent.diagnostics.toggle") }}</label>
          <HelpTooltip :label="t('translation.agent.diagnostics.toggle')" :text="t('translation.agent.diagnostics.hint')" />
        </div>
        <label class="m3-choice">
          <md-checkbox ref="diagnosticsCheckbox" id="translation-agent-detailed-diagnostics" :checked="detailedDiagnostics" :disabled="diagnosticsBusy" @change="emit('set-diagnostics', ($event.target as HTMLInputElement).checked)" />
          {{ t("translation.agent.diagnostics.enable") }}
        </label>
        <span v-if="diagnosticsError" class="field-hint diagnostics-error" role="alert">{{ diagnosticsError }}</span>
        <span class="field-hint">{{ t("translation.agent.diagnostics.scope") }}</span>
      </div>

      <div class="advanced-actions">
        <md-outlined-button :disabled="openCategory === 'diagnostics'" @click="restoreCategory">{{ t("translation.agent.restoreCategory") }}</md-outlined-button>
        <span class="field-hint">{{ openCategory === 'diagnostics' ? t("translation.agent.diagnosticsMeta") : t("translation.agent.categoryModified", { count: categoryModified() }) }}</span>
      </div>
    </template>
  </div>
</template>

<style scoped>
/* 二级页：与一级共用设置内容区的直排字段，不加卡片容器，只用既有 token */
.translation-advanced { display: grid; gap: var(--space-lg); }
.advanced-head, .advanced-detail-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-md);
  padding-bottom: var(--space-md);
  border-bottom: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 25%, transparent);
}
.advanced-head-leading { display: flex; align-items: center; gap: var(--space-xs); min-width: 0; }
/* 返回为 md-text-button + 18px chevron（ListRefreshButton 的内联图标配方），-8px 抵消按钮内距与内容区左缘对齐 */
.advanced-back-button { margin-left: -8px; }
.advanced-back-button svg { width: 18px; height: 18px; margin-right: var(--space-xxs); vertical-align: middle; }
.advanced-title-group { display: flex; align-items: center; min-width: 0; }
.advanced-title { margin: 0; font-size: 17px; font-weight: 600; color: var(--ink); line-height: 1.4; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.advanced-chip {
  flex: none;
  display: inline-flex;
  align-items: center;
  height: 24px;
  padding: 0 var(--space-sm);
  border-radius: 999px;
  background: color-mix(in srgb, var(--md-sys-color-surface-container-highest) 70%, transparent);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 25%, transparent);
  color: var(--ink-muted);
  font-size: 12px;
  font-weight: 500;
  white-space: nowrap;
}
.advanced-chip.is-modified {
  background: var(--md-sys-color-primary-container);
  border-color: color-mix(in srgb, var(--md-sys-color-primary) 30%, transparent);
  color: var(--md-sys-color-on-primary-container);
  font-weight: 600;
}
.advanced-intro { margin: 0; color: var(--ink-muted); font-size: 13px; line-height: 1.6; }
.advanced-note { margin: var(--space-sm) 0 0; color: var(--ink-muted); font-size: 12px; line-height: 1.6; }
/* 分类入口：整行可点，hover 用既有 8% primary 状态层 */
.advanced-index { display: grid; border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.advanced-index-item {
  display: flex; align-items: center; gap: var(--space-md); width: 100%; min-height: 56px;
  padding: var(--space-sm) var(--space-xs); color: var(--ink); font: inherit; text-align: left;
  background: transparent; border: 0; border-bottom: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); cursor: pointer;
}
.advanced-index-item:hover { background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }
.advanced-index-item:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 2px; }
.advanced-index-text { display: grid; gap: 2px; flex: 1; min-width: 0; }
.advanced-index-title { font-size: 14px; font-weight: 500; }
.advanced-index-hint { color: var(--ink-muted); font-size: 12px; line-height: 1.5; }
.advanced-index-meta { flex: none; color: var(--ink-muted); font-size: 12px; font-weight: 600; white-space: nowrap; }
.advanced-index-meta.is-modified { color: var(--md-sys-color-primary); }
.advanced-chevron { flex: none; width: 18px; height: 18px; color: var(--ink-muted); }
.advanced-actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-sm) var(--space-md); padding-top: var(--space-md); border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.advanced-confirm { color: var(--md-sys-color-error); font-size: 12px; line-height: 1.5; }
/* 诊断开关读 / 写失败：沿用既有字段错误色，配可读文字（Semantic State Rule） */
.diagnostics-error { color: var(--md-sys-color-error); }
/* 窄窗：计数换到标题下一行，不再挤在右侧 */
@media (max-width: 640px) {
  .advanced-index-item { flex-wrap: wrap; }
  .advanced-index-meta { white-space: normal; }
}
@media (prefers-reduced-motion: reduce) { .advanced-index-item { transition: none; } }
</style>
