/**
 * 高级 → 同步（WebDAV）IPC 封装：命令、返回类型与结构化错误。
 *
 * 动作直接使用当前草稿值：`config` 允许覆盖未保存的地址 / 用户名 / 密码 / 同步密码，
 * 不要求先点底部「保存」。凭据只随 config 或 settings_save 上行，永不出现在返回体里。
 */
import { invoke, isTauri } from "./tauri";
import { mockSyncDownload, mockSyncTestConnection, mockSyncUpload } from "./devMock";

/** 同步范围：浏览历史 / 登录状态（含多账户）。 */
export type SyncScope = "history" | "auth";

/** 动作期的草稿覆盖；省略字段表示沿用已保存值，空串表示该动作不使用该凭据。 */
export interface SyncConfig {
  webdavUrl?: string;
  webdavUsername?: string;
  webdavPassword?: string;
  syncPassphrase?: string;
}

/** sync_test_connection：HTTP 状态可能缺失（如非 HTTP 层失败）。 */
export interface SyncTestResult {
  status: string;
  http_status?: number | null;
}

/** sync_upload：bytes 为上传负载字节数。 */
export interface SyncUploadResult {
  exported_at: string;
  bytes: number;
  history_count: number;
  account_count: number;
}

/** sync_download：导入计数 + 当前账户 / 登录态是否被云端覆盖。 */
export interface SyncDownloadResult {
  exported_at: string;
  history_imported: number;
  accounts_imported: number;
  active_changed: boolean;
  auth_changed: boolean;
}

/**
 * 会话级动作回执（不持久化）：错误文案、成功摘要与发生时间。
 * 由 SettingsPanel 持有，切到二级页 / 返回菜单都保留，关闭设置即复位。
 */
export interface SyncStatus {
  kind: "success" | "error";
  text: string;
  at: number;
}

/** 下载导入历史后广播：浏览历史页监听后回到第 1 页重拉（同维护清理事件模式）。 */
export const HISTORY_RESTORED_EVENT = "pixiv-tool:history-restored";

export async function syncTestConnection(config?: SyncConfig): Promise<SyncTestResult> {
  if (!isTauri()) return mockSyncTestConnection(config);
  return invoke<SyncTestResult>("sync_test_connection", config ? { config } : {});
}

export async function syncUpload(scopes: SyncScope[], config?: SyncConfig): Promise<SyncUploadResult> {
  if (!isTauri()) return mockSyncUpload(scopes, config);
  return invoke<SyncUploadResult>("sync_upload", config ? { scopes, config } : { scopes });
}

export async function syncDownload(scopes: SyncScope[], config?: SyncConfig): Promise<SyncDownloadResult> {
  if (!isTauri()) return mockSyncDownload(scopes, config);
  return invoke<SyncDownloadResult>("sync_download", config ? { scopes, config } : { scopes });
}
