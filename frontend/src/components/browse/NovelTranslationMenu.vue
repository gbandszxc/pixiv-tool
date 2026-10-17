<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import type { TranslationMode } from "../../api/translation";
import type { TranslationActive } from "../../stores/translation";
import TranslationProgressBanner from "./TranslationProgressBanner.vue";

const props = defineProps<{
  modelValue: TranslationMode;
  status: string;
  error: boolean;
  busy: boolean;
  disabled: boolean;
  translated: boolean;
  /** 原文已是目标语言、本次未请求模型：只提示，不视为成功或失败。 */
  skipped: boolean;
  /** 当前小说的进行中翻译任务快照；null = 无任务，浮窗内不渲染进度区。 */
  active: TranslationActive | null;
}>();
const emit = defineEmits<{
  "update:modelValue": [mode: TranslationMode];
  translate: [];
}>();
const { t } = useI18n();
const open = ref(false);
const panel = ref<HTMLElement | null>(null);
interface Focusable { focus(): void }
const trigger = ref<Focusable | null>(null);

/**
 * 状态指示器：错误 > 进行中 > 已完成 > 未翻译（含「无需翻译」提示，不算成功也不算失败），
 * 颜色只作补充，状态文字与失败原因始终可读（DESIGN.md Semantic State Rule）。
 * 失败时 hover 指示点/入口按钮可看模型侧报错原文。
 */
const state = computed(() => (props.error ? "error" : props.busy ? "busy" : props.translated ? "done" : "idle"));
const triggerTitle = computed(() => (props.status ? `${t("translation.menu")}：${props.status}` : t("translation.menu")));

function close() { open.value = false; }
/**
 * 左缘钳制：384px 浮窗右缘对齐触发器，左缘可能伸进侧栏区（触发器位于底栏左侧控件组）。
 * 打开时（及窗口变化时）测量侧栏右缘，左缘不足则把浮窗整体右推，保证完整落在主工作区；
 * 层级上浮窗 z-index 恒高于侧栏，此处只解决「横跨边界」的视觉错误。
 */
function clampPanel(): void {
  const el = panel.value;
  if (!el) return;
  const gap = Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--space-sm")) || 8;
  const sidebar = document.querySelector(".sidebar");
  const minLeft = (sidebar?.getBoundingClientRect().right ?? 0) + gap;
  const rect = el.getBoundingClientRect();
  el.style.right = rect.left < minLeft ? `${Math.round(rect.left - minLeft)}px` : "";
}
function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape" && !document.querySelector("dialog[open]")) close();
}
watch(open, (value) => {
  if (value) {
    window.addEventListener("keydown", onKeydown);
    window.addEventListener("resize", clampPanel);
    void nextTick(() => {
      clampPanel();
      panel.value?.focus();
    });
  } else {
    window.removeEventListener("keydown", onKeydown);
    window.removeEventListener("resize", clampPanel);
    trigger.value?.focus();
  }
});
// 挂载即有在途任务（典型：从底栏「翻译中」或侧栏回到该小说）→ 自动展开浮窗展示进度。
// 同页内手动关闭不触发重挂，不会被强制重开；任务已结束（active 为空）不展开。
onMounted(() => {
  if (props.active) open.value = true;
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <div class="translation-menu">
    <span class="translation-trigger-wrap">
      <md-icon-button
        ref="trigger"
        class="translation-trigger"
        :class="{ active: open || busy }"
        :aria-label="t('translation.menu')"
        :title="triggerTitle"
        aria-haspopup="dialog"
        :aria-expanded="open"
        @click="open = !open"
      >
        <!-- Lucide languages：https://lucide.dev/icons/languages -->
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 8 6 6m-7 0 6-6 2-3M2 5h12M7 2h1m14 20-5-10-5 10m2-4h6" /></svg>
      </md-icon-button>
      <!-- 入口状态点：不展开也能看到翻译中/成功/失败；失败原因经 title 悬停查看 -->
      <span v-if="state !== 'idle'" class="translation-indicator" :class="`is-${state}`" :title="props.status" aria-hidden="true"></span>
    </span>
    <div v-if="open" class="translation-backdrop" @click="close"></div>
    <div v-if="open" ref="panel" class="translation-popover" role="dialog" :aria-label="t('translation.menu')" tabindex="-1">
      <span class="translation-title">{{ t('translation.menu') }}</span>
      <span class="translation-status-row">
        <span class="translation-indicator" :class="`is-${state}`" :title="props.status" aria-hidden="true"></span>
        <span class="translation-status" :class="{ 'is-error': error }" role="status" aria-live="polite">{{ status }}</span>
      </span>
      <!-- 进度区：当前小说有翻译任务时，在状态行下方内嵌紧凑进度（阶段/已用/已接收 + 摘要 marquee） -->
      <TranslationProgressBanner v-if="active" :active="active" />
      <!-- 状态/进度与操作分区分隔（既有 outline 35% 1px 配方），仅进度区出现时渲染 -->
      <div v-if="active" class="translation-divider" aria-hidden="true"></div>
      <div class="translation-modes" role="group" :aria-label="t('translation.displayMode')">
        <button v-for="mode in (['original', 'translated', 'bilingual'] as const)" :key="mode" type="button" class="translation-mode" :aria-pressed="modelValue === mode" :class="{ selected: modelValue === mode }" @click="emit('update:modelValue', mode)">{{ t(`translation.${mode}`) }}</button>
      </div>
      <md-outlined-button :disabled="busy || disabled" :aria-busy="busy" @click="emit('translate')">{{ t(skipped ? 'translation.translateAnyway' : translated ? 'translation.retranslate' : 'translation.button') }}</md-outlined-button>
    </div>
  </div>
</template>

<style scoped>
.translation-menu { position: relative; display: inline-flex; }
.translation-trigger-wrap { position: relative; display: inline-flex; }
.translation-trigger svg { width: 20px; height: 20px; }
.translation-trigger.active { --md-icon-button-icon-color: var(--md-sys-color-primary); }
/* 状态点：进行中 primary、成功绿、失败 error；入口点带 surface 描边与图标按钮区隔。 */
.translation-indicator { flex: none; width: 8px; height: 8px; border-radius: 50%; background: var(--ink-subtle); }
.translation-indicator.is-busy { background: var(--md-sys-color-primary); }
.translation-indicator.is-done { background: var(--state-success-ink); }
.translation-indicator.is-error { background: var(--md-sys-color-error); }
.translation-trigger-wrap > .translation-indicator { position: absolute; top: 3px; right: 3px; box-shadow: 0 0 0 2px var(--md-sys-color-surface); }
.translation-backdrop { position: fixed; inset: 0; z-index: 890; }
.translation-popover {
  position: absolute;
  bottom: calc(100% + var(--space-sm));
  right: 0;
  z-index: 900;
  display: flex;
  flex-direction: column;
  gap: var(--space-sm);
  box-sizing: border-box;
  width: calc(var(--space-lg) * 24);
  max-width: calc(100vw - var(--space-lg) * 2);
  padding: var(--space-lg);
  border-radius: var(--radius-control);
  background: var(--md-sys-color-surface-container);
  color: var(--ink);
  box-shadow: 0 4px 12px rgb(0 0 0 / 18%);
}
.translation-popover:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 2px; }
.translation-title { font-size: 14px; font-weight: 600; }
.translation-status-row { display: flex; align-items: flex-start; gap: var(--space-xs); }
.translation-status-row .translation-indicator { margin-top: 5px; }
.translation-status { color: var(--ink-muted); font-size: 12px; overflow-wrap: anywhere; }
.translation-status.is-error { color: var(--md-sys-color-error); }
/* 状态/进度区与操作区分隔：1px outline 35%（与下载面板/设置分组同配方） */
.translation-divider { height: 1px; margin: var(--space-xxs) 0; background: color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.translation-modes { display: flex; gap: var(--space-xxs); }
.translation-mode {
  flex: 1;
  min-width: 0;
  min-height: 40px;
  padding: var(--space-sm) var(--space-sm);
  border: 0;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--ink);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
.translation-mode:hover:not(.selected) { background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent); }
.translation-mode:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 2px; }
.translation-mode.selected { background: var(--md-sys-color-secondary-container); color: var(--md-sys-color-on-secondary-container); }
</style>
