/**
 * Tauri IPC 封装 —— 前端所有后端调用统一走这里。
 *
 * 约定（与 Rust 命令契约一致）：
 * - 调用参数用 camelCase 键；返回体保持与旧 HTTP 响应相同的 snake_case。
 * - 业务错误两种形态：
 *   a) 返回值内含 { error: string }（旧 200+error 风格，调用方自行判断）；
 *   b) invoke reject 一个 string（旧 4xx/5xx detail），统一用 errorMessage() 归一化。
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

/** 是否运行在 Tauri 窗口内（浏览器里直接打开 dev 页时为 false）。 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && window.__TAURI_INTERNALS__ !== undefined;
}

function ensureTauri(): void {
  if (!isTauri()) {
    throw new Error(
      "未检测到 Tauri 运行环境：请在 Tauri 应用窗口中运行（开发期使用 pnpm tauri dev），不要在普通浏览器中直接打开页面。"
    );
  }
}

/** invoke 薄封装：非 Tauri 环境下立即抛出可读错误，便于误用定位。 */
export async function invoke<T = unknown>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  ensureTauri();
  return tauriInvoke<T>(command, args);
}

/**
 * 错误归一化：把任意错误形状转成可直接展示的 string。
 * - Tauri invoke reject 的 string → 原样（后端 4xx/5xx detail 风格）
 * - Error → message
 * - 旧 axios 形状（err.response.data.error / .detail）→ 取 detail（过渡兼容）
 * - 其他 → String(err)
 */
export function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err && typeof err === "object") {
    // 过渡兼容：旧 axios 错误形状。注意 axios 错误同时是 Error 实例，
    // 因此先取 response.data 里的业务错误，取不到再落到 message。
    const data = (err as { response?: { data?: { error?: unknown; detail?: unknown } } })
      .response?.data;
    const detail = data?.error ?? data?.detail;
    if (typeof detail === "string" && detail) return detail;
    if (err instanceof Error && err.message) return err.message;
  }
  return String(err);
}

// 事件订阅原样转出（listen 内部同样依赖 Tauri 环境，调用方先用 isTauri() 判断）。
export { listen } from "@tauri-apps/api/event";
export type { UnlistenFn } from "@tauri-apps/api/event";

// ===== 后端契约类型（snake_case，与 Rust 命令返回体一致）=====

/** tasks_list 返回的任务行。failed_ids 为 JSON 字符串。 */
export interface TaskRow {
  task_id: string;
  source_type: string;
  source_id: string;
  category: string;
  status: string;
  total: number;
  done: number;
  skipped: number;
  failed_ids: string;
  created_at: string;
  updated_at: string;
  error: string | null;
}

/** task_create / task_retry_failed 的返回：成功含 task_id，失败含 error。 */
export interface TaskMutationResult {
  task_id?: string;
  status?: string;
  error?: string;
}

export interface Settings {
  output_dir: string;
  output_formats: string[];
  language: string;
  theme: string;
  backend_port: number | null;
  max_wait_seconds: number;
}

export interface HistoryItem {
  id: number;
  category: "novel" | "illustration";
  title: string;
  author_name: string | null;
  pages: number | null;
  series_id: number | null;
  illust_type: number | null;
  captured_at: string;
}

export interface HistoryListResponse {
  items: HistoryItem[];
  total: number;
  page: number;
  page_size: number;
}

/** auth_status / auth_login.user 共有的用户字段（snake_case）。 */
export interface AuthUserFields {
  user_id?: string;
  pixiv_id?: string;
  name?: string;
  profile_img?: string;
}

export interface AuthStatusResponse extends AuthUserFields {
  is_logged_in: boolean;
}

/** auth_login：真实浏览器登录，长阻塞，status 区分取消/超时/失败。 */
export interface AuthLoginResponse {
  status: "success" | "cancelled" | "timeout" | "error";
  message?: string;
  user?: AuthUserFields;
}

/** auth_login_manual：失败时 invoke reject string，成功返回 user。 */
export interface AuthLoginManualResponse {
  status: "success";
  message: string;
  user: AuthUserFields;
}

/** browse_sync_login：从内嵌浏览页提取 cookies 并同步到系统凭据存储。 */
export interface BrowseSyncLoginResponse {
  status: "success" | "injected" | "no_session" | "invalid" | "error";
  message?: string;
}

/** task://progress 事件 payload。 */
export interface TaskProgressEvent {
  task_id: string;
  status: string;
  done: number;
  total: number;
  skipped: number;
  current_title?: string;
}

/** task://done 事件 payload。 */
export interface TaskDoneEvent {
  task_id: string;
  status: string;
  done: number;
  total: number;
  failed: number;
  skipped: number;
}
