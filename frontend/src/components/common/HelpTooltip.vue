<script lang="ts">
let nextTooltipId = 0;
</script>

<script setup lang="ts">
import { onBeforeUnmount, ref } from "vue";
import { useI18n } from "vue-i18n";

defineProps<{ label: string; text: string }>();
const { t } = useI18n();
const id = `help-tooltip-${++nextTooltipId}`;
const root = ref<HTMLElement | null>(null);
const trigger = ref<HTMLButtonElement | null>(null);
const tooltip = ref<HTMLElement | null>(null);
let pinned = false;
let closeTimer: ReturnType<typeof setTimeout> | undefined;

function cancelClose() { clearTimeout(closeTimer); }
function close() {
  cancelClose();
  pinned = false;
  if (tooltip.value?.matches(":popover-open")) tooltip.value.hidePopover();
  window.removeEventListener("keydown", onKeydown, true);
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("resize", close);
  window.removeEventListener("scroll", onScroll, true);
}
function show() {
  cancelClose();
  const tip = tooltip.value;
  const button = trigger.value;
  if (!tip || !button || tip.matches(":popover-open")) return;
  // 原生 top layer 避免设置 dialog 的滚动区裁切说明。
  tip.showPopover();
  const anchor = button.getBoundingClientRect();
  const gap = parseFloat(getComputedStyle(tip).getPropertyValue("--space-sm")) || 8;
  const margin = gap * 2;
  const below = anchor.bottom + gap;
  tip.style.left = `${Math.max(margin, Math.min(anchor.left, innerWidth - tip.offsetWidth - margin))}px`;
  tip.style.top = `${Math.max(margin, below + tip.offsetHeight <= innerHeight - margin ? below : anchor.top - gap - tip.offsetHeight)}px`;
  window.addEventListener("keydown", onKeydown, true);
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("resize", close);
  window.addEventListener("scroll", onScroll, true);
}
function scheduleClose() {
  if (!pinned && document.activeElement !== trigger.value) {
    // 留出跨过图标与浮层间隙的时间；移入浮层后继续阅读。
    closeTimer = setTimeout(close, 150);
  }
}
function toggle() {
  if (pinned) close();
  else { show(); pinned = true; }
}
function onOutside(event: Event) {
  if (!root.value?.contains(event.target as Node)) close();
}
function onScroll(event: Event) {
  if (event.target instanceof Node && tooltip.value?.contains(event.target)) return;
  close();
}
function onKeydown(event: KeyboardEvent) {
  if (event.key !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  close();
}
onBeforeUnmount(close);
</script>

<template>
  <span ref="root" class="help-tooltip">
    <button ref="trigger" type="button" class="help-trigger" :aria-label="t('common.helpFor', { label })" :aria-describedby="id" @pointerenter="show" @pointerleave="scheduleClose" @focus="show" @blur="close" @click="toggle">
      <!-- Lucide circle-question-mark -->
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="10" /><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3m.08 4h.01" /></svg>
    </button>
    <span :id="id" ref="tooltip" class="help-content" popover="manual" role="tooltip" @pointerenter="cancelClose" @pointerleave="scheduleClose">{{ text }}</span>
  </span>
</template>

<style scoped>
.help-tooltip { display: inline-flex; flex: none; vertical-align: middle; }
.help-trigger { display: inline-flex; align-items: center; justify-content: center; width: calc(var(--space-lg) * 2); height: calc(var(--space-lg) * 2); padding: var(--space-sm); border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink-muted); cursor: help; }
.help-trigger svg { width: 16px; height: 16px; flex: none; }
.help-trigger:hover { background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent); }
.help-trigger:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 2px; }
.help-content { position: fixed; inset: auto; margin: 0; box-sizing: border-box; width: max-content; max-width: min(calc(var(--space-xl) * 15), calc(100vw - var(--space-lg) * 2)); max-height: calc(100vh - var(--space-lg) * 2); overflow: auto; padding: var(--space-md); border: 0; border-radius: var(--radius-control); background: var(--md-sys-color-surface-container); color: var(--ink); font: 13px/1.6 var(--font-app); font-weight: 400; white-space: pre-line; overflow-wrap: anywhere; box-shadow: 0 4px 12px rgb(0 0 0 / 18%); }
.help-content:not(:popover-open) { display: none; }
</style>
