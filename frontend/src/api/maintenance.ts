import { invoke, isTauri } from "./tauri";
export type CacheKind = "translation" | "images";
export interface StorageUsage { path: string; bytes: number; files: number }
export interface MaintenanceInfo { translation: StorageUsage; images: StorageUsage; logs: StorageUsage }
// 浏览器验收只用内存数据，不接触用户文件。
const mock: MaintenanceInfo = {
  translation: { path: "D:\\Pixiv Tool\\data\\translations", bytes: 2097152, files: 12 },
  images: { path: "D:\\Pixiv Tool\\data\\cache\\img", bytes: 132120576, files: 240 },
  logs: { path: "D:\\Pixiv Tool\\data\\logs", bytes: 8388608, files: 2 },
};
let mockLogs = '[WARN] 翻译服务错误：{"code":"1301","message":"内容审核拒绝"}';
export async function getMaintenanceInfo(): Promise<MaintenanceInfo> { return isTauri() ? invoke("maintenance_info") : structuredClone(mock); }
export async function readLogs(): Promise<string> { return isTauri() ? invoke("read_logs") : mockLogs; }
export async function clearCache(kind: CacheKind): Promise<void> {
  if (isTauri()) await invoke("clear_cache", { kind });
  else { mock[kind].bytes = 0; mock[kind].files = 0; }
  if (kind === "translation") window.dispatchEvent(new Event("pixiv-tool:translation-cache-cleared"));
}
export async function clearLogs(): Promise<void> {
  if (isTauri()) await invoke("clear_logs");
  else { mock.logs.bytes = 0; mock.logs.files = 0; mockLogs = ""; }
}
