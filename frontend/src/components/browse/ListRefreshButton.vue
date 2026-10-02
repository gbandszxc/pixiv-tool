<script setup lang="ts">
import { onActivated, onBeforeUnmount, onDeactivated, onMounted } from "vue";
import { useI18n } from "vue-i18n";

const props = defineProps<{ busy?: boolean; disabled?: boolean }>();
const emit = defineEmits<{ refresh: [] }>();
const { t } = useI18n();

function refresh(): void {
  if (props.busy || props.disabled) return;
  document.querySelector(".app-content")?.scrollTo({ top: 0, behavior: "instant" });
  emit("refresh");
}

function onKeydown(event: KeyboardEvent): void {
  if (event.defaultPrevented || event.altKey || !(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "r") return;
  // 在列表内拦截 WebView 整页重载；模态窗口打开时不刷新背后的列表。
  event.preventDefault();
  if (event.repeat || document.querySelector("dialog[open]")) return;
  refresh();
}

function activate(): void { window.addEventListener("keydown", onKeydown); }
function deactivate(): void { window.removeEventListener("keydown", onKeydown); }
onMounted(activate);
onActivated(activate);
onDeactivated(deactivate);
onBeforeUnmount(deactivate);
</script>

<template>
  <md-text-button class="list-refresh" :disabled="busy || disabled" :aria-busy="!!busy" aria-keyshortcuts="Control+R Meta+R" :title="t('browse.refreshShortcut')" @click="refresh">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
      <path d="M21 3v5h-5" />
    </svg>
    {{ busy ? t('browse.home.refreshing') : t('browse.refresh') }}
  </md-text-button>
</template>

<style scoped>
.list-refresh { flex: none; margin-left: auto; }
svg { width: 18px; height: 18px; margin-right: var(--space-xxs); vertical-align: middle; }
</style>
