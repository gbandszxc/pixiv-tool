<template>
  <div id="sync-page" class="sync-panel">
    <div class="sync-head">
      <div class="sync-head-leading">
        <md-text-button class="sync-back" @click="emit('back')">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
          {{ t("sync.backToAdvanced") }}
        </md-text-button>
        <h3 class="sync-title">{{ t("sync.title") }}</h3>
      </div>
      <span class="sync-chip">{{ scopeSummary }}</span>
    </div>

    <fieldset class="sync-group" role="group" aria-labelledby="sync-group-connection">
      <div id="sync-group-connection" class="sync-legend">{{ t("sync.groupConnection") }}</div>

      <div class="m3-field">
        <div class="field-label">
          <label for="sync-url">{{ t("sync.url") }}</label>
          <HelpTooltip :label="t('sync.url')" :text="t('sync.urlHint')" />
        </div>
        <md-outlined-text-field id="sync-url" :value="values.webdav_url ?? ''" :placeholder="t('sync.urlPlaceholder')" :error="Boolean(fieldErrors.webdav_url)" :error-text="fieldErrors.webdav_url" @input="onUrlInput" />
      </div>

      <div class="m3-field">
        <label for="sync-username">{{ t("sync.username") }}</label>
        <md-outlined-text-field id="sync-username" :value="values.webdav_username ?? ''" :placeholder="t('sync.usernamePlaceholder')" :error="Boolean(fieldErrors.webdav_username)" :error-text="fieldErrors.webdav_username" @input="onUsernameInput" />
      </div>

      <div class="m3-field">
        <div class="field-label">
          <label for="sync-password">{{ t("sync.password") }}</label>
          <HelpTooltip :label="t('sync.password')" :text="t('sync.passwordHint')" />
          <span v-if="values.webdav_password_configured" class="sync-secret-chip" :class="{ 'is-clearing': clearWebdavPassword }">{{ t(clearWebdavPassword ? "sync.passwordWillClear" : "sync.passwordSaved") }}</span>
        </div>
        <div class="sync-secret-row">
          <md-outlined-text-field
            id="sync-password"
            type="password"
            autocomplete="new-password"
            :value="webdavPassword"
            :placeholder="t(values.webdav_password_configured ? 'sync.passwordPlaceholderKeep' : 'sync.passwordPlaceholder')"
            :error="Boolean(fieldErrors.webdav_password)"
            :error-text="fieldErrors.webdav_password"
            @input="onPasswordInput"
          />
          <md-text-button v-if="values.webdav_password_configured" class="sync-secret-clear" :disabled="clearWebdavPassword" @click="clearPassword">{{ t("sync.clearPassword") }}</md-text-button>
        </div>
      </div>

      <div class="m3-field">
        <div class="field-label">
          <label for="sync-passphrase">{{ t("sync.passphrase") }}</label>
          <HelpTooltip :label="t('sync.passphrase')" :text="t('sync.passphraseHint')" />
          <span v-if="values.sync_passphrase_configured" class="sync-secret-chip" :class="{ 'is-clearing': clearSyncPassphrase }">{{ t(clearSyncPassphrase ? "sync.passphraseWillClear" : "sync.passphraseSaved") }}</span>
        </div>
        <div class="sync-secret-row">
          <md-outlined-text-field
            id="sync-passphrase"
            type="password"
            autocomplete="new-password"
            :value="syncPassphrase"
            :placeholder="t(values.sync_passphrase_configured ? 'sync.passphrasePlaceholderKeep' : 'sync.passphrasePlaceholder')"
            :error="Boolean(fieldErrors.sync_passphrase)"
            :error-text="fieldErrors.sync_passphrase"
            @input="onPassphraseInput"
          />
          <md-text-button v-if="values.sync_passphrase_configured" class="sync-secret-clear" :disabled="clearSyncPassphrase" @click="clearPassphrase">{{ t("sync.clearPassphrase") }}</md-text-button>
        </div>
      </div>
    </fieldset>

    <fieldset class="sync-group" role="group" aria-labelledby="sync-group-scope">
      <div id="sync-group-scope" class="sync-legend">{{ t("sync.groupScope") }}</div>
      <div class="sync-scope-options">
        <label class="m3-choice">
          <md-checkbox id="sync-scope-history" :checked="scopeHistory" @change="setScope('history', ($event.target as HTMLInputElement).checked)" />
          {{ t("sync.scopeHistory") }}
        </label>
        <label class="m3-choice">
          <md-checkbox id="sync-scope-auth" :checked="scopeAuth" @change="setScope('auth', ($event.target as HTMLInputElement).checked)" />
          {{ t("sync.scopeAuth") }}
        </label>
      </div>
      <p v-if="!hasScope" class="sync-note is-error" role="status">{{ t("sync.scopeRequired") }}</p>
    </fieldset>

    <div class="sync-actions" :aria-busy="Boolean(busy)">
      <md-outlined-button :disabled="!canAct" @click="runTest">{{ t(busy === "test" ? "sync.actions.testing" : "sync.actions.test") }}</md-outlined-button>
      <md-outlined-button :disabled="!canAct" @click="runUpload">{{ t(busy === "upload" ? "sync.actions.uploading" : "sync.actions.upload") }}</md-outlined-button>
      <md-outlined-button :disabled="!canAct" @click="askRestore">{{ t(busy === "download" ? "sync.actions.downloading" : "sync.actions.download") }}</md-outlined-button>
    </div>

    <p class="sync-status" :class="{ 'is-error': statusKind === 'error' }" :role="statusKind === 'error' ? 'alert' : 'status'">{{ statusText }}</p>

    <!-- 从云端恢复：二次确认（原生 dialog + 既有 .m3-dialog 配方），文案写明合并与覆盖范围 -->
    <dialog ref="restoreDialog" class="m3-dialog" aria-labelledby="sync-restore-title">
      <h2 id="sync-restore-title">{{ t("sync.restoreTitle") }}</h2>
      <p>{{ t("sync.restoreConfirm") }}</p>
      <div class="m3-row dialog-actions">
        <md-text-button @click="restoreDialog?.close()">{{ t("common.cancel") }}</md-text-button>
        <md-filled-button @click="confirmRestore">{{ t("sync.restoreAccept") }}</md-filled-button>
      </div>
    </dialog>
  </div>
</template>

<script setup lang="ts">
/**
 * 高级 → 同步（二级页）：WebDAV 连接、同步范围、测试 / 上传 / 恢复。
 *
 * 与一级共用同一份设置草稿（`values` 由 SettingsPanel 持有，这里直接改非敏感键），
 * 凭据草稿由父级持有并通过 v-model 事件回写，底部「保存」负责持久化。
 * 动作直接使用当前草稿值（config 覆盖），不要求先保存；动作期禁用全部按钮并显示进行中文案。
 * 恢复成功且导入了历史时广播 HISTORY_RESTORED_EVENT，浏览历史页据此回到第 1 页重拉。
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import HelpTooltip from "../common/HelpTooltip.vue";
import { useAuthStore } from "../../stores/auth";
import { errorMessage, errorPayload, type Settings } from "../../api/tauri";
import {
  HISTORY_RESTORED_EVENT,
  syncDownload,
  syncTestConnection,
  syncUpload,
  type SyncConfig,
  type SyncScope,
  type SyncStatus,
} from "../../api/sync";

const props = defineProps<{
  /** 设置草稿（SettingsPanel 的 form）。 */
  values: Settings;
  /** 会话级动作回执；空 = 尚未同步。 */
  status: SyncStatus | null;
  webdavPassword: string;
  clearWebdavPassword: boolean;
  syncPassphrase: string;
  clearSyncPassphrase: boolean;
}>();

const emit = defineEmits<{
  (event: "back"): void;
  (event: "update:webdavPassword", value: string): void;
  (event: "update:clearWebdavPassword", value: boolean): void;
  (event: "update:syncPassphrase", value: string): void;
  (event: "update:clearSyncPassphrase", value: boolean): void;
  (event: "update:status", value: SyncStatus): void;
}>();

const { t, te, locale } = useI18n();
const authStore = useAuthStore();
const busy = ref<"test" | "upload" | "download" | null>(null);
const fieldErrors = ref<Record<string, string>>({});
const restoreDialog = ref<HTMLDialogElement | null>(null);
/** 相对时间每分钟刷新一次；不做秒级跳动，避免状态行自己吸引注意力。 */
const now = ref(Date.now());
let ticker: ReturnType<typeof setInterval> | undefined;

/** 范围默认全选：老配置 / mock 缺键时按 true 回显。 */
const scopeHistory = computed(() => props.values.sync_scope_history !== false);
const scopeAuth = computed(() => props.values.sync_scope_auth !== false);
const hasScope = computed(() => scopeHistory.value || scopeAuth.value);
const scopes = computed<SyncScope[]>(() => [
  ...(scopeHistory.value ? ["history" as const] : []),
  ...(scopeAuth.value ? ["auth" as const] : []),
]);
const canAct = computed(() => !busy.value && hasScope.value);

const scopeSummary = computed(() => {
  if (!hasScope.value) return t("sync.scopeNone");
  if (scopeHistory.value && scopeAuth.value) return t("sync.scopeBoth");
  return scopeHistory.value ? t("sync.scopeHistoryOnly") : t("sync.scopeAuthOnly");
});

const statusKind = computed(() => props.status?.kind ?? "idle");
const statusText = computed(() => {
  if (!props.status) return t("sync.status.idle");
  return `${props.status.text} · ${relativeTime(props.status.at)}`;
});

function relativeTime(at: number): string {
  const minutes = Math.floor(Math.max(0, now.value - at) / 60_000);
  if (minutes < 1) return t("sync.status.justNow");
  if (minutes < 60) return t("sync.status.minutesAgo", { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("sync.status.hoursAgo", { count: hours });
  return new Intl.DateTimeFormat(locale.value, { dateStyle: "short", timeStyle: "short" }).format(at);
}

/** 字节数与条目数用当前界面语言格式化，与维护页同一口径。 */
function formatBytes(bytes: number): string {
  const units = ["B", "KiB", "MiB", "GiB"];
  const index = Math.min(3, Math.floor(Math.log(Math.max(1, bytes)) / Math.log(1024)));
  return `${new Intl.NumberFormat(locale.value, { maximumFractionDigits: 1 }).format(bytes / 1024 ** index)} ${units[index]}`;
}
function formatCount(value: number): string {
  return new Intl.NumberFormat(locale.value).format(value);
}

function clearFieldError(field: string): void {
  if (!fieldErrors.value[field]) return;
  const next = { ...fieldErrors.value };
  delete next[field];
  fieldErrors.value = next;
}

function onUrlInput(event: Event): void {
  props.values.webdav_url = (event.target as HTMLInputElement).value;
  clearFieldError("webdav_url");
}
function onUsernameInput(event: Event): void {
  props.values.webdav_username = (event.target as HTMLInputElement).value;
  clearFieldError("webdav_username");
}
function onPasswordInput(event: Event): void {
  emit("update:webdavPassword", (event.target as HTMLInputElement).value);
  if (props.clearWebdavPassword) emit("update:clearWebdavPassword", false);
  clearFieldError("webdav_password");
}
function onPassphraseInput(event: Event): void {
  emit("update:syncPassphrase", (event.target as HTMLInputElement).value);
  if (props.clearSyncPassphrase) emit("update:clearSyncPassphrase", false);
  clearFieldError("sync_passphrase");
}
function clearPassword(): void {
  emit("update:clearWebdavPassword", true);
  emit("update:webdavPassword", "");
  clearFieldError("webdav_password");
}
function clearPassphrase(): void {
  emit("update:clearSyncPassphrase", true);
  emit("update:syncPassphrase", "");
  clearFieldError("sync_passphrase");
}
function setScope(scope: SyncScope, checked: boolean): void {
  if (scope === "history") props.values.sync_scope_history = checked;
  else props.values.sync_scope_auth = checked;
  clearFieldError("scope");
}

/** 草稿覆盖：非敏感键始终随动作上行；凭据留空 = 沿用已保存值，显式清除 = 空串。 */
function buildConfig(): SyncConfig {
  const config: SyncConfig = {
    webdavUrl: (props.values.webdav_url ?? "").trim(),
    webdavUsername: (props.values.webdav_username ?? "").trim(),
  };
  if (props.clearWebdavPassword) config.webdavPassword = "";
  else if (props.webdavPassword.trim()) config.webdavPassword = props.webdavPassword;
  if (props.clearSyncPassphrase) config.syncPassphrase = "";
  else if (props.syncPassphrase.trim()) config.syncPassphrase = props.syncPassphrase;
  return config;
}

function setStatus(kind: SyncStatus["kind"], text: string): void {
  emit("update:status", { kind, text, at: Date.now() });
}
/** 结构化错误优先取 `sync.errors.<code>`；缺失回落后端 message，字段错误停靠到对应输入框。 */
function handleActionError(error: unknown): void {
  fieldErrors.value = {};
  const payload = errorPayload(error);
  if (!payload) {
    setStatus("error", errorMessage(error) || t("sync.errors.sync_internal"));
    return;
  }
  const key = `sync.errors.${payload.code}`;
  const text = te(key) ? t(key) : payload.message;
  if (payload.field) fieldErrors.value = { [payload.field]: text };
  setStatus("error", text);
}

async function runTest(): Promise<void> {
  if (!canAct.value) return;
  busy.value = "test";
  fieldErrors.value = {};
  try {
    const result = await syncTestConnection(buildConfig());
    setStatus("success", result.http_status != null
      ? t("sync.testOk", { status: result.http_status })
      : t("sync.testOkNoStatus"));
  } catch (error) {
    handleActionError(error);
  } finally {
    busy.value = null;
  }
}

async function runUpload(): Promise<void> {
  if (!canAct.value) return;
  busy.value = "upload";
  fieldErrors.value = {};
  try {
    const result = await syncUpload(scopes.value, buildConfig());
    setStatus("success", t("sync.uploadDone", {
      size: formatBytes(result.bytes),
      history: formatCount(result.history_count),
      accounts: formatCount(result.account_count),
    }));
  } catch (error) {
    handleActionError(error);
  } finally {
    busy.value = null;
  }
}

async function askRestore(): Promise<void> {
  if (!canAct.value) return;
  restoreDialog.value?.showModal();
}
async function confirmRestore(): Promise<void> {
  restoreDialog.value?.close();
  await runDownload();
}

async function runDownload(): Promise<void> {
  if (!canAct.value) return;
  busy.value = "download";
  fieldErrors.value = {};
  try {
    const result = await syncDownload(scopes.value, buildConfig());
    const notes = [t("sync.downloadDone", {
      history: formatCount(result.history_imported),
      accounts: formatCount(result.accounts_imported),
    })];
    if (result.active_changed) notes.push(t("sync.activeChanged"));
    if (result.auth_changed) notes.push(t("sync.authChanged"));
    setStatus("success", notes.join(" · "));
    if (result.history_imported > 0) window.dispatchEvent(new Event(HISTORY_RESTORED_EVENT));
    if (result.auth_changed) {
      // 账户 / 登录态刷新是成功后的补充动作：失败不覆盖同步结果，界面仍可手动刷新。
      try {
        await authStore.checkStatus();
        await authStore.fetchAccounts();
      } catch {
        /* 保持同步成功回执 */
      }
    }
  } catch (error) {
    handleActionError(error);
  } finally {
    busy.value = null;
  }
}

onMounted(() => { ticker = setInterval(() => { now.value = Date.now(); }, 30_000); });
onBeforeUnmount(() => { clearInterval(ticker); });
</script>

<style scoped>
/* 二级页：与一级共用设置内容区的直排字段，不加卡片容器，只用既有 token */
.sync-panel { display: grid; gap: var(--space-lg); }
.sync-head {
  display: flex; align-items: center; justify-content: space-between; gap: var(--space-md);
  padding-bottom: var(--space-md);
  border-bottom: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 25%, transparent);
}
.sync-head-leading { display: flex; align-items: center; gap: var(--space-xs); min-width: 0; }
/* 返回为 md-text-button + 18px chevron（ListRefreshButton 的内联图标配方） */
.sync-back { margin-left: -8px; flex: none; }
.sync-back svg { width: 18px; height: 18px; margin-right: var(--space-xxs); vertical-align: middle; }
.sync-title { margin: 0; font-size: 17px; font-weight: 600; color: var(--ink); line-height: 1.4; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.sync-chip {
  flex: none; display: inline-flex; align-items: center; height: 24px; padding: 0 var(--space-sm);
  border-radius: 999px; background: color-mix(in srgb, var(--md-sys-color-surface-container-highest) 70%, transparent);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 25%, transparent);
  color: var(--ink-muted); font-size: 12px; font-weight: 500; white-space: nowrap;
}
/* 子分区：组标题 12px/600 辅助层级，组间用既有 35% outline 分隔线分段（同小说翻译配方） */
.sync-group { display: block; min-width: 0; margin: 0; padding: 0; border: 0; }
.sync-group + .sync-group { padding-top: var(--space-lg); border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
.sync-legend { margin-bottom: var(--space-md); color: var(--ink-muted); font-size: 12px; font-weight: 600; }
.sync-group > .m3-field:last-child { margin-bottom: 0; }
.sync-secret-row { display: flex; align-items: center; gap: var(--space-sm); }
.sync-secret-row > md-outlined-text-field { flex: 1; min-width: 0; }
.sync-secret-clear { flex: none; }
/* 凭据状态胶囊：中性描边胶囊；待清除时文案与描边切 error 角色（同翻译 Key 配方） */
.sync-secret-chip { display: inline-flex; align-items: center; min-height: 20px; margin-left: var(--space-xxs); padding: 0 var(--space-sm); border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 60%, transparent); border-radius: 999px; color: var(--ink-muted); font-size: 12px; font-weight: 600; }
.sync-secret-chip.is-clearing { color: var(--md-sys-color-error); border-color: color-mix(in srgb, var(--md-sys-color-error) 45%, transparent); }
.sync-scope-options { display: flex; flex-wrap: wrap; gap: var(--space-sm) var(--space-xl); }
.sync-note { margin: var(--space-xs) 0 0; color: var(--ink-muted); font-size: 12px; line-height: 1.5; overflow-wrap: anywhere; }
.sync-note.is-error { color: var(--md-sys-color-error); }
/* 动作行：可换行，不撑成整行宽胶囊；进行中/禁用由 Material 按钮自身呈现 */
.sync-actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-sm) var(--space-md); padding-top: var(--space-md); border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
/* 状态行：动作回执，长错误文本可换行不截断 */
.sync-status { margin: 0; color: var(--ink-muted); font-size: 12px; line-height: 1.6; overflow-wrap: anywhere; }
.sync-status.is-error { color: var(--md-sys-color-error); }
/* 窄窗：凭据行纵排，动作按钮随内容换行 */
@media (max-width: 640px) {
  .sync-secret-row { align-items: stretch; flex-direction: column; }
  .sync-secret-row > .sync-secret-clear { align-self: flex-start; }
}
</style>
