/**
 * 应用更新检查 IPC 封装：对应后端 check_app_update 命令（无参数）。
 */
import { invoke } from "./tauri";
import { Channel } from "@tauri-apps/api/core";

/** check_app_update 返回体（snake_case，与 Rust 命令契约一致）。 */
export interface UpdateCheckInfo {
  has_update: boolean;
  current_version: string;
  latest_version: string;
  release_url: string;
  platform: string;
  package_type: string | null;
}

/** 检查应用更新；失败时 invoke reject，由调用方用 errorMessage() 归一化展示。 */
export async function checkAppUpdate(): Promise<UpdateCheckInfo> {
  return invoke<UpdateCheckInfo>("check_app_update");
}

export interface UpdateProgress {
  phase: "downloading" | "opening";
  file_name: string;
  downloaded: number;
  total: number;
  bytes_per_second: number;
}
export interface UpdateDownloadResult {
  path: string;
  installer_opened: boolean;
  directory_opened: boolean;
  package_type: string;
}
export function downloadAppUpdate(version: string, onProgress: (value: UpdateProgress) => void) {
  const progress = new Channel<UpdateProgress>();
  progress.onmessage = onProgress;
  return invoke<UpdateDownloadResult>("download_app_update", { version, progress });
}
export function cancelAppUpdate() { return invoke<void>("cancel_app_update"); }
export function openUpdateDirectory() { return invoke<void>("open_update_directory"); }
