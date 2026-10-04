<script lang="ts">
let nextTooltipId = 0;
</script>

<script setup lang="ts">
import { onBeforeUnmount, ref } from "vue";
import { useI18n } from "vue-i18n";
import { notify } from "../../ui/notify";

const props = defineProps<{ path: string; disabled?: boolean }>();
const { t } = useI18n();
const id = `copy-path-tooltip-${++nextTooltipId}`;
const root = ref<HTMLElement | null>(null);
const trigger = ref<HTMLButtonElement | null>(null);
const tooltip = ref<HTMLElement | null>(null);
const copied = ref(false);
let pinned = false;
let closeTimer: ReturnType<typeof setTimeout> | undefined;
let copiedTimer: ReturnType<typeof setTimeout> | undefined;

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
  if (props.disabled) return;
  cancelClose();
  const tip = tooltip.value;
  const button = trigger.value;
  if (!tip || !button || tip.matches(":popover-open")) return;
  // 原生 top layer 避免设置 dialog 的滚动区裁切，与 HelpTooltip 同一规则。
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
  if (!pinned && document.activeElement !== trigger.value) closeTimer = setTimeout(close, 150);
}
async function copy(): Promise<void> {
  if (props.disabled) return;
  try {
    await navigator.clipboard.writeText(props.path);
    notify(t("settings.pathCopied"));
    copied.value = true;
    clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => { copied.value = false; }, 1600);
  }
  catch { notify(t("settings.copyFailed")); }
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
onBeforeUnmount(() => { close(); clearTimeout(copiedTimer); });
</script>

<template>
  <span ref="root" class="copy-path">
    <button ref="trigger" type="button" class="copy-trigger" :disabled="disabled" :aria-label="t('settings.copyPath')" :aria-describedby="id" @pointerenter="show" @pointerleave="scheduleClose" @focus="show" @blur="close" @click="copy">
      <!-- Lucide check -->
      <svg v-if="copied" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5" /></svg>
      <!-- Lucide copy -->
      <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="14" height="14" x="8" y="8" rx="2" ry="2" /><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" /></svg>
    </button>
    <span :id="id" ref="tooltip" class="copy-tooltip" popover="manual" role="tooltip" @pointerenter="cancelClose" @pointerleave="scheduleClose"><code>{{ path }}</code></span>
  </span>
</template>

<style scoped>
.copy-path { display: inline-flex; flex: none; vertical-align: middle; }
.copy-trigger { display: inline-flex; align-items: center; justify-content: center; width: calc(var(--space-lg) * 2); height: calc(var(--space-lg) * 2); padding: var(--space-sm); border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink-muted); cursor: pointer; }
.copy-trigger svg { width: 16px; height: 16px; flex: none; }
.copy-trigger:hover:not(:disabled) { background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent); }
.copy-trigger:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 2px; }
.copy-trigger:disabled { color: color-mix(in srgb, var(--ink-muted) 55%, transparent); cursor: default; }
.copy-tooltip { position: fixed; inset: auto; margin: 0; box-sizing: border-box; width: max-content; max-width: min(calc(var(--space-xl) * 15), calc(100vw - var(--space-lg) * 2)); overflow: auto; padding: var(--space-sm) var(--space-md); border: 0; border-radius: var(--radius-control); background: var(--md-sys-color-surface-container); color: var(--ink); font: 13px/1.6 var(--font-app); font-weight: 400; white-space: pre-wrap; overflow-wrap: anywhere; box-shadow: 0 4px 12px rgb(0 0 0 / 18%); }
.copy-tooltip:not(:popover-open) { display: none; }
.copy-tooltip code { font-size: 12px; }
</style>
