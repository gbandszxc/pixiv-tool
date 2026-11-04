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
      <!-- 测试连接是主动作：连通并探测云端备份后，上传 / 恢复才可用（ADR 0049） -->
      <md-filled-button :disabled="Boolean(busy)" @click="runTest">{{ t(busy === "test" ? "sync.actions.testing" : "sync.actions.test") }}</md-filled-button>
      <md-outlined-button :disabled="!canAct" @click="askUpload">{{ t(busy === "upload" ? "sync.actions.uploading" : "sync.actions.upload") }}</md-outlined-button>
      <md-outlined-button :disabled="!canAct" @click="askRestore">{{ t(busy === "download" ? "sync.actions.downloading" : "sync.actions.download") }}</md-outlined-button>
    </div>

    <!-- 未测试连接时上传 / 恢复禁用即可，不另设提示行；测试成功后显示备份回显卡 -->
    <div v-if="connection" class="sync-backup" :class="{ 'is-empty': !connection.has }" role="status">
      <svg v-if="connection.has" class="sync-backup-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m17 15-5.5 5.5L9 18" /><path d="M5.516 16.07A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 3.501 7.327" /></svg>
      <svg v-else class="sync-backup-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z" /></svg>
      <span class="sync-backup-text">
        <span class="sync-backup-label">{{ t("sync.backupLabel") }}</span>
        <span class="sync-backup-value">{{ connection.has ? t("sync.backupExists") : t("sync.backupNone") }}<template v-if="connection.has && backupTimeText"> · {{ backupTimeText }}</template><template v-else-if="!connection.has"> · {{ t("sync.backupNoneHint") }}</template></span>
      </span>
    </div>

    <p v-if="status" class="sync-status" :class="{ 'is-error': statusKind === 'error' }" :role="statusKind === 'error' ? 'alert' : 'status'">{{ statusText }}</p>

    <!-- 从云端恢复：二次确认（原生 dialog + 既有 .m3-dialog 配方），文案写明合并与覆盖范围 -->
    <dialog ref="restoreDialog" class="m3-dialog" aria-labelledby="sync-restore-title">
      <h2 id="sync-restore-title">{{ t("sync.restoreTitle") }}</h2>
      <p>{{ t("sync.restoreConfirm") }}</p>
      <div class="m3-row dialog-actions">
        <md-text-button @click="restoreDialog?.close()">{{ t("common.cancel") }}</md-text-button>
        <md-filled-button @click="confirmRestore">{{ t("sync.restoreAccept") }}</md-filled-button>
      </div>
    </dialog>

    <!-- 覆盖云端备份：已有备份时上传前二次确认，文案写明整体覆盖与不可找回 -->
    <dialog ref="uploadDialog" class="m3-dialog" aria-labelledby="sync-upload-title">
      <h2 id="sync-upload-title">{{ t("sync.uploadTitle") }}</h2>
      <p>{{ uploadConfirmText }}</p>
      <div class="m3-row dialog-actions">
        <md-text-button @click="uploadDialog?.close()">{{ t("common.cancel") }}</md-text-button>
        <md-filled-button @click="confirmUpload">{{ t("sync.uploadAccept") }}</md-filled-button>
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
 * 上传 / 恢复在测试连接成功前禁用；测试成功后由父级持有云端备份回显（会话级），
 * 地址 / 用户名 / WebDAV 密码草稿一旦变化即失效，需重新测试（ADR 0049）。
 * 云端已有备份时上传先弹原生 dialog 二次确认覆盖风险，暂无备份直接上传；
 * 「从云端恢复」同样二次确认。
 * 恢复成功且导入了历史时广播 HISTORY_RESTORED_EVENT，浏览历史页据此回到第 1 页重拉。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import HelpTooltip from "../common/HelpTooltip.vue";
import { useAuthStore } from "../../stores/auth";
import { errorMessage, errorPayload, type Settings } from "../../api/tauri";
import {
  HISTORY_RESTORED_EVENT,
  syncDownload,
  syncTestConnection,
  syncUpload,
  type SyncBackupInfo,
  type SyncConfig,
  type SyncScope,
  type SyncStatus,
} from "../../api/sync";

const props = defineProps<{
  /** 设置草稿（SettingsPanel 的 form）。 */
  values: Settings;
  /** 会话级动作回执；空 = 尚未同步。 */
  status: SyncStatus | null;
  /** 测试连接成功后的云端备份回显；null = 本次会话尚未测试成功（上传 / 恢复禁用）。 */
  connection: SyncBackupInfo | null;
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
  (event: "update:connection", value: SyncBackupInfo | null): void;
}>();

const { t, te, locale } = useI18n();
const authStore = useAuthStore();
const busy = ref<"test" | "upload" | "download" | null>(null);
const fieldErrors = ref<Record<string, string>>({});
const restoreDialog = ref<HTMLDialogElement | null>(null);
const uploadDialog = ref<HTMLDialogElement | null>(null);
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
const canAct = computed(() => !busy.value && hasScope.value && props.connection !== null);

const scopeSummary = computed(() => {
  if (!hasScope.value) return t("sync.scopeNone");
  if (scopeHistory.value && scopeAuth.value) return t("sync.scopeBoth");
  return scopeHistory.value ? t("sync.scopeHistoryOnly") : t("sync.scopeAuthOnly");
});

const statusKind = computed(() => props.status?.kind ?? "idle");
/** 无回执时不占位：未测试时门控提示行已经说明状态，空闲文案反而多一层噪音。 */
const statusText = computed(() => {
  if (!props.status) return "";
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
/** 备份时间（远端 Last-Modified / 上传完成时刻）按当前语言格式化；不可解析返回空串。 */
function formatDateTime(value: string): string {
  const at = new Date(value);
  if (Number.isNaN(at.getTime())) return "";
  return new Intl.DateTimeFormat(locale.value, { dateStyle: "medium", timeStyle: "short" }).format(at);
}
const backupTimeText = computed(() => (props.connection?.time ? formatDateTime(props.connection.time) : ""));

/** 连接草稿（地址 / 用户名 / WebDAV 密码）一旦变化，测试结果即失效：需重新测试。
 * 同步口令不影响连通性，不在此列（改动后上传 / 恢复会按新口令走，错误就地呈现）。 */
watch(
  () => [props.values.webdav_url, props.values.webdav_username, props.webdavPassword, props.clearWebdavPassword],
  () => {
    if (props.connection) emit("update:connection", null);
  },
);

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
  if (busy.value) return;
  busy.value = "test";
  fieldErrors.value = {};
  try {
    const result = await syncTestConnection(buildConfig());
    // 测试成功即解锁上传 / 恢复，并回显云端备份状态（时间取远端 Last-Modified）
    emit("update:connection", { has: result.has_backup === true, time: result.backup_time ?? null });
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
    // 上传成功 = 云端产生了新备份：回显卡同步刷新（时间取本机完成时刻）
    emit("update:connection", { has: true, time: new Date().toISOString() });
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

/** 上传入口：云端已有备份时先声明覆盖风险并二次确认，暂无备份直接上传（ADR 0049）。 */
function askUpload(): void {
  if (!canAct.value) return;
  if (!props.connection?.has) { void runUpload(); return; }
  if (!uploadDialog.value?.open) uploadDialog.value?.showModal();
}
function confirmUpload(): void {
  uploadDialog.value?.close();
  void runUpload();
}
const uploadConfirmText = computed(() => (backupTimeText.value
  ? t("sync.uploadConfirmWithTime", { time: backupTimeText.value })
  : t("sync.uploadConfirmNoTime")));

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
/* 动作行：可换行，不撑成整行宽胶囊；测试连接为 filled 主动作，上传 / 恢复 outlined 次要 */
.sync-actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-sm) var(--space-md); padding-top: var(--space-md); border-top: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 35%, transparent); }
/* 云端备份回显卡：测试成功后替代禁用提示出现；surface-container-low 底 + 25% outline 描边 */
.sync-backup {
  display: flex; align-items: center; gap: var(--space-md); min-width: 0;
  padding: var(--space-sm) var(--space-md);
  background: var(--md-sys-color-surface-container-low);
  border: 1px solid color-mix(in srgb, var(--md-sys-color-outline) 25%, transparent);
  border-radius: var(--radius-control);
}
.sync-backup-icon { flex: none; width: 18px; height: 18px; color: var(--md-sys-color-primary); }
.sync-backup-text { display: grid; gap: 2px; min-width: 0; }
.sync-backup-label { color: var(--ink-muted); font-size: 12px; font-weight: 600; }
.sync-backup-value { color: var(--ink); font-size: 13px; line-height: 1.5; overflow-wrap: anywhere; }
/* 暂无备份：图标与值都回到中性色，不渲染成「错误」 */
.sync-backup.is-empty .sync-backup-icon,
.sync-backup.is-empty .sync-backup-value { color: var(--ink-muted); }
/* 确认弹窗定宽：原生 dialog 为 fit-content，CJK 长句会整句撑开；
 * 覆盖 / 恢复两枚弹窗统一宽度，正文自然换行（配方对齐 .m3-dialog 的 min 宽度） */
.sync-panel .m3-dialog { width: min(420px, 90vw); }
/* 状态行：动作回执，长错误文本可换行不截断 */
.sync-status { margin: 0; color: var(--ink-muted); font-size: 12px; line-height: 1.6; overflow-wrap: anywhere; }
.sync-status.is-error { color: var(--md-sys-color-error); }
/* 窄窗：凭据行纵排，动作按钮随内容换行 */
@media (max-width: 640px) {
  .sync-secret-row { align-items: stretch; flex-direction: column; }
  .sync-secret-row > .sync-secret-clear { align-self: flex-start; }
}
</style>
