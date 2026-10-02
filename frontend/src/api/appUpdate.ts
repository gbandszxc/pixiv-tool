/**
 * 应用更新检查 IPC 封装：对应后端 check_app_update 命令（无参数）。
 */
import { invoke } from "./tauri";

/** check_app_update 返回体（snake_case，与 Rust 命令契约一致）。 */
export interface UpdateCheckInfo {
  has_update: boolean;
  current_version: string;
  latest_version: string;
  release_url: string;
}

/** 检查应用更新；失败时 invoke reject，由调用方用 errorMessage() 归一化展示。 */
export async function checkAppUpdate(): Promise<UpdateCheckInfo> {
  return invoke<UpdateCheckInfo>("check_app_update");
}
