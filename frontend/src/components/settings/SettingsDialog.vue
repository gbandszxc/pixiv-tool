<template>
  <dialog ref="dialog" class="m3-dialog settings-dialog" :aria-label="t('settings.title')" @cancel="onCancel" @click="onBackdropClick">
    <header class="settings-dialog-header">
      <h2>{{ t("settings.title") }}</h2>
      <md-icon-button :aria-label="t('common.close')" :title="t('common.close')" @click="requestClose()">
        <svg class="bar-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <line x1="6" y1="6" x2="18" y2="18" /><line x1="18" y1="6" x2="6" y2="18" />
        </svg>
      </md-icon-button>
    </header>
    <div class="settings-dialog-main">
      <nav class="settings-nav" :aria-label="t('settings.groupsLabel')">
        <button v-for="item in navItems" :key="item.id" type="button" class="settings-nav-item" :class="{ active: activeSection === item.id }" :aria-current="activeSection === item.id ? 'true' : undefined" @click="selectSection(item.id)">{{ item.label }}</button>
      </nav>
      <div ref="body" class="settings-dialog-body"><SettingsPanel ref="settingsPanel" :section="activeSection" :active="show" /></div>
    </div>
    <footer class="settings-dialog-footer">
      <md-text-button @click="handleCancel">{{ t("common.cancel") }}</md-text-button>
      <md-filled-button @click="handleSave">{{ t("common.save") }}</md-filled-button>
    </footer>
  </dialog>
  <dialog ref="unsavedDialog" class="m3-dialog unsaved-dialog" :aria-label="t('settings.unsavedTitle')" @cancel="onUnsavedCancel" @click="onUnsavedBackdropClick">
    <h2>{{ t("settings.unsavedTitle") }}</h2>
    <p>{{ t("settings.unsavedConfirm") }}</p>
    <div class="m3-row dialog-actions"><md-text-button @click="closeUnsaved">{{ t("settings.keepEditing") }}</md-text-button><md-filled-button @click="discardAndClose">{{ t("settings.discard") }}</md-filled-button></div>
  </dialog>
</template>

<script setup lang="ts">
/**
 * 设置弹窗：账号菜单「设置」入口打开的模态 dialog。左侧为分组栏（切换
 * SettingsPanel 的 section），右侧为当前分组表单，底部为常驻的取消 / 保存
 * （保存为整表一次写入）。✕ / Esc / 点 backdrop 关闭前先问 SettingsPanel
 * 是否脏，有未保存改动则弹第二层确认弹窗；取消 → 回滚并关闭，保存成功 → 关闭。
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import SettingsPanel from "./SettingsPanel.vue";
import { SETTINGS_SECTIONS, type SettingsSection } from "./sections";
import { notify } from "../../ui/notify";

const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{ (e: "update:show", value: boolean): void }>();
const { t } = useI18n();
const dialog = ref<HTMLDialogElement | null>(null);
const unsavedDialog = ref<HTMLDialogElement | null>(null);
const body = ref<HTMLElement | null>(null);
const settingsPanel = ref<InstanceType<typeof SettingsPanel> | null>(null);
const activeSection = ref<SettingsSection>("general");
const navItems = computed(() => SETTINGS_SECTIONS.map(id => ({ id, label: t(`settings.groups.${id}`) })));
watch(() => props.show, (show) => { if (!dialog.value) return; if (show) dialog.value.showModal(); else dialog.value.close(); });
function selectSection(id: SettingsSection) { activeSection.value = id; if (body.value) body.value.scrollTop = 0; }
/** Esc / ✕ / backdrop 的统一关闭请求：有未保存改动先确认，否则直接关。 */
function requestClose(deferModal = false) {
  if (!settingsPanel.value?.hasUnsaved()) { closeDialog(); return; }
  // Esc 走浏览器 close watcher，在其 cancel 处理中同步 showModal 会被拦下（无用户激活），
  // 推迟一个任务再开；✕ / backdrop 是真实点击，直接开。
  if (deferModal) { setTimeout(() => unsavedDialog.value?.showModal(), 0); return; }
  unsavedDialog.value?.showModal();
}
function closeDialog() { emit("update:show", false); }
function handleCancel() { settingsPanel.value?.reset(); closeDialog(); }
async function handleSave() { if (await settingsPanel.value?.save()) closeDialog(); }
function closeUnsaved() { unsavedDialog.value?.close(); }
function discardAndClose() { closeUnsaved(); settingsPanel.value?.reset(); notify(t("settings.unsaved")); closeDialog(); }
/** Esc 触发原生 cancel：拦截后走统一关闭流程，保证脏检查先执行。 */
function onCancel(event: Event) { event.preventDefault(); requestClose(true); }
/** 点 backdrop（原生 dialog 的 backdrop 点击目标即 dialog 本身）关闭。 */
function onBackdropClick(event: MouseEvent) { if (event.target === dialog.value) requestClose(); }
/** 未保存确认弹窗自身不接受 Esc 关闭（等同于「继续编辑」，避免误触丢失输入）。 */
function onUnsavedCancel(event: Event) { event.preventDefault(); closeUnsaved(); }
function onUnsavedBackdropClick(event: MouseEvent) { if (event.target === unsavedDialog.value) closeUnsaved(); }
</script>

<style scoped>
/* 覆盖 .m3-dialog 的 24px 内边距：头部固定、主区（分组栏 + 表单）自身滚动、底部
 * 保存栏常驻（dialog 根元素带父组件作用域属性，故用 dialog.settings-dialog 提高
 * 优先级压过 .m3-dialog 的 padding）。
 * 高度写死（不用 min-/max-height）：各分组字段数不同，由内容撑高会让弹窗随分组跳动；
 * 超过高度的分组在右栏内部滚动（.settings-dialog-body 的 overflow-y）。
 * display:flex 只能写在 [open] 上——直接覆盖会让关闭态的 dialog 失去 UA 的 display:none。 */
dialog.settings-dialog { width: min(840px, 94vw); height: min(680px, 88vh); padding: 0; overflow: hidden; }
dialog.settings-dialog[open] { display: flex; flex-direction: column; }
.settings-dialog-header { display: flex; align-items: center; justify-content: space-between; gap: var(--space-sm); padding: var(--space-md) var(--space-md) var(--space-sm) var(--space-xl); }
.settings-dialog-header h2 { margin: 0; font-size: 18px; font-weight: 700; line-height: 1.4; }
/* 关闭按钮：统一为与其他图标动作同规格的 20px 线性图标（不复用文本 ✕ 字形） */
.settings-dialog-header .bar-icon { width: 20px; height: 20px; stroke-width: 1.8; }
/* 主区：左分组栏 176px + 右表单，两者各自滚动；flex:1 让底栏常驻在弹窗底部。
 * 行高必须钉 minmax(0, 1fr)——auto 行会被内容撑开，列内的 overflow-y 就不会生效。 */
.settings-dialog-main { display: grid; flex: 1; grid-template-columns: 176px minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); min-height: 0; }
.settings-nav { display: flex; flex-direction: column; gap: 2px; min-height: 0; overflow-y: auto; padding: 0 var(--space-sm) var(--space-md) var(--space-md); }
/* 分组项沿用应用侧栏的导航配方（primary-container 选中 / 8% primary hover），按弹窗密度压到 40px 高、20px 胶囊 */
.settings-nav-item { display: flex; align-items: center; width: 100%; min-height: 40px; padding: 0 var(--space-md); color: var(--ink); font: inherit; font-size: 14px; text-align: left; background: transparent; border: 0; border-radius: 20px; cursor: pointer; }
.settings-nav-item:hover { background: color-mix(in srgb, var(--md-sys-color-primary) 8%, transparent); }
.settings-nav-item.active { color: var(--md-sys-color-on-primary-container); font-weight: 600; background: var(--md-sys-color-primary-container); }
.settings-nav-item:focus-visible { outline: 2px solid var(--md-sys-color-primary); outline-offset: 2px; }
.settings-dialog-body { min-height: 0; overflow-y: auto; padding: 0 var(--space-xl) var(--space-xl) var(--space-lg); border-left: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.settings-dialog-footer { display: flex; justify-content: flex-end; gap: var(--space-sm); padding: var(--space-md) var(--space-xl); border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
/* 未保存确认：无标题外的额外层级，沿用 .m3-dialog 配方并收窄 */
.unsaved-dialog { min-width: min(360px, 90vw); width: min(360px, 90vw); }
/* 窄窗口：分组栏改为横排可滚的胶囊行，表单落到下一行 */
@media (max-width: 640px) {
  .settings-dialog-main { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }
  .settings-nav { flex-direction: row; gap: var(--space-xs); padding: 0 var(--space-lg) var(--space-md); overflow-x: auto; }
  .settings-nav-item { width: auto; white-space: nowrap; }
  .settings-dialog-body { padding: 0 var(--space-lg) var(--space-lg); border-left: 0; }
}
</style>
