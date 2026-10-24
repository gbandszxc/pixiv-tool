<script setup lang="ts">
/**
 * 二级高级设置：单个分类的整数运行参数（重试分类额外带「未知结果重放」开关）。
 *
 * 只负责渲染与即时校验提示：值由父级草稿（SettingsPanel 的 form）持有，写入走
 * `set-int` / `set-replay`。越界不拦输入（允许一边改一边看提示），保存前由父级
 * 统一拦截并定位。区间、默认值与步进来自 stores/settings.ts 的后端镜像表。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import HelpTooltip from "../common/HelpTooltip.vue";
import type { Settings } from "../../api/tauri";
import {
  TRANSLATION_AGENT_INT_RULES,
  agentFieldId,
  type TranslationAgentCategory,
  type TranslationAgentIntKey,
  type TranslationAgentIssueCode,
  type TranslationAgentIntRule,
} from "../../stores/settings";

type IntCategory = Exclude<TranslationAgentCategory, "extra" | "diagnostics">;

const props = defineProps<{
  values: Settings;
  category: IntCategory;
  errors: Partial<Record<TranslationAgentIntKey, TranslationAgentIssueCode>>;
}>();
const emit = defineEmits<{
  (event: "set-int", key: TranslationAgentIntKey, value: number): void;
  (event: "set-replay", value: boolean): void;
}>();

const { t, locale } = useI18n();
const rules = computed(() => TRANSLATION_AGENT_INT_RULES.filter((rule) => rule.category === props.category));

/** 千分位展示区间与推荐值，避免 32768 这类长数字被误读。 */
const format = (value: number) => value.toLocaleString(locale.value);

/** 草稿里缺失或脏值一律显示为空，不伪造 0（0 对多个参数是合法值）。 */
function valueOf(key: TranslationAgentIntKey): number {
  const value = props.values[key];
  return typeof value === "number" ? value : Number.NaN;
}
function display(rule: TranslationAgentIntRule): string {
  const value = valueOf(rule.key);
  return Number.isFinite(value) ? String(value) : "";
}
function unitLabel(rule: TranslationAgentIntRule): string {
  return t(`translation.agent.units.${rule.unit}`);
}
/** 说明 + 单位 + 合法区间 + 推荐值：用户不必翻文档就能判断该填什么。 */
function hint(rule: TranslationAgentIntRule): string {
  return `${t(rule.hintKey)}\n\n${t("translation.agent.rangeHint", {
    unit: unitLabel(rule),
    min: format(rule.min),
    max: format(rule.max),
    recommended: format(rule.default),
  })}`;
}
function errorText(rule: TranslationAgentIntRule): string {
  const code = props.errors[rule.key];
  if (!code) return "";
  if (code === "range") {
    return t("translation.agent.errors.range", {
      min: format(rule.min),
      max: format(rule.max),
      recommended: format(rule.default),
    });
  }
  if (code === "chunkTargetOverMax") {
    return t("translation.agent.errors.chunkTargetOverMax", {
      max: format(valueOf("translation_agent_chunk_max_chars")),
    });
  }
  if (code === "outputCapOverWindow") {
    return t("translation.agent.errors.outputCapOverWindow", {
      window: format(valueOf("translation_agent_context_window")),
    });
  }
  return t("translation.agent.errors.historyPlusSummaryOverWindow", {
    window: format(valueOf("translation_agent_context_window")),
  });
}
function onInput(rule: TranslationAgentIntRule, event: Event): void {
  const raw = (event.target as HTMLInputElement).value;
  emit("set-int", rule.key, raw.trim() === "" ? Number.NaN : Number(raw));
}
const replayLabelKey = "translation.agent.fields.translation_agent_replay_unknown.label";
const replayHintKey = "translation.agent.fields.translation_agent_replay_unknown.hint";
</script>

<template>
  <div class="agent-fields">
    <div v-for="rule in rules" :key="rule.key" class="m3-field">
      <div class="field-label">
        <label :for="agentFieldId(rule.key)">{{ t(rule.labelKey) }}</label>
        <HelpTooltip :label="t(rule.labelKey)" :text="hint(rule)" />
      </div>
      <div class="agent-input-row">
        <md-outlined-text-field
          :id="agentFieldId(rule.key)"
          class="agent-number"
          type="number"
          :min="String(rule.min)"
          :max="String(rule.max)"
          :step="String(rule.step)"
          :value="display(rule)"
          :error="Boolean(errors[rule.key])"
          :error-text="errorText(rule)"
          @input="onInput(rule, $event)"
        />
        <span class="agent-unit">{{ unitLabel(rule) }}</span>
      </div>
      <span v-if="rule.key === 'translation_call_budget' && !errors[rule.key]" class="field-hint">
        {{ valueOf(rule.key) === 0 ? t("translation.agent.budgetAuto") : t("translation.agent.budgetManual") }}
      </span>
    </div>

    <div v-if="category === 'retry'" class="m3-field">
      <div class="field-label">
        <label for="translation-agent-replay-unknown">{{ t(replayLabelKey) }}</label>
        <HelpTooltip :label="t(replayLabelKey)" :text="t(replayHintKey)" />
      </div>
      <label class="m3-choice">
        <md-checkbox
          id="translation-agent-replay-unknown"
          :checked="values.translation_agent_replay_unknown === true"
          @change="emit('set-replay', ($event.target as HTMLInputElement).checked)"
        />
        {{ t("translation.agent.replayEnable") }}
      </label>
    </div>
  </div>
</template>

<style scoped>
.agent-fields { display: grid; }
.agent-input-row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-sm); }
.agent-number { width: 180px; max-width: 100%; }
.agent-unit { color: var(--ink-muted); font-size: 12px; }
@media (max-width: 640px) { .agent-number { width: 100%; } }
</style>
