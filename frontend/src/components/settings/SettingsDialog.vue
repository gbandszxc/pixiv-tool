<template>
  <dialog ref="dialog" class="m3-dialog settings-dialog" :aria-label="t('settings.title')" @cancel="onCancel" @click="onBackdropClick">
    <header class="settings-dialog-header">
      <h2>{{ t("settings.title") }}</h2>
      <md-icon-button :aria-label="t('common.close')" :title="t('common.close')" @click="close()">
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <line x1="6" y1="6" x2="18" y2="18" /><line x1="18" y1="6" x2="6" y2="18" />
        </svg>
      </md-icon-button>
    </header>
    <div class="settings-dialog-body"><SettingsPanel ref="settingsPanel" /></div>
  </dialog>
</template>

<script setup lang="ts">
/**
 * 设置弹窗：账号菜单「设置」入口打开的模态 dialog，承载 SettingsPanel（原
 * SettingsView 完整表单）。设置项后续增多时在弹窗内分组扩展。✕ / Esc / 点
 * backdrop 关闭，关闭前经 SettingsPanel.beforeClose() 恢复未保存的主题预览。
 */
import { ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import SettingsPanel from "./SettingsPanel.vue";

const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{ (e: "update:show", value: boolean): void }>();
const { t } = useI18n();
const dialog = ref<HTMLDialogElement | null>(null);
const settingsPanel = ref<InstanceType<typeof SettingsPanel> | null>(null);
watch(() => props.show, (show) => { if (!dialog.value) return; if (show) dialog.value.showModal(); else dialog.value.close(); });
function close() { settingsPanel.value?.beforeClose(); emit("update:show", false); }
/** Esc 触发原生 cancel：拦截后走统一关闭流程，保证 beforeClose 先执行。 */
function onCancel(event: Event) { event.preventDefault(); close(); }
/** 点 backdrop（原生 dialog 的 backdrop 点击目标即 dialog 本身）关闭。 */
function onBackdropClick(event: MouseEvent) { if (event.target === dialog.value) close(); }
</script>

<style scoped>
/* 覆盖 .m3-dialog 的 24px 内边距：头部固定、表单区自身滚动（dialog 根元素带父组件
 * 作用域属性，故用 dialog.settings-dialog 提高优先级压过 .m3-dialog 的 padding）。
 * display:flex 只能写在 [open] 上——直接覆盖会让关闭态的 dialog 失去 UA 的 display:none。 */
dialog.settings-dialog { width: min(600px, 92vw); max-height: min(84vh, 100%); padding: 0; overflow: hidden; }
dialog.settings-dialog[open] { display: flex; flex-direction: column; }
.settings-dialog-header { display: flex; align-items: center; justify-content: space-between; gap: var(--space-sm); padding: var(--space-md) var(--space-md) var(--space-sm) var(--space-xl); }
.settings-dialog-header h2 { margin: 0; font-size: 18px; font-weight: 700; line-height: 1.4; }
/* 关闭按钮：统一为与其他图标动作同规格的 20px 线性图标（不复用文本 ✕ 字形） */
.settings-dialog-header .bar-icon { width: 20px; height: 20px; stroke-width: 1.8; }
.settings-dialog-body { min-height: 0; overflow-y: auto; padding: 0 var(--space-xl) var(--space-xl); }
</style>
