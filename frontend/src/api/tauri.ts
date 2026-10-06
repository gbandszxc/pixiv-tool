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
import { getCurrentWindow } from "@tauri-apps/api/window";

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

/** 设置窗口原生主题（NSAppearance），深浅色模式下 webview 的
 * prefers-color-scheme 随之切换。仅 Tauri 环境可用；非 Tauri（浏览器调试）静默跳过。 */
export async function setWindowTheme(theme: "light" | "dark"): Promise<void> {
  if (!isTauri()) return;
  await getCurrentWindow().setTheme(theme);
}

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
  theme_color: string;
  /** 应用启动入口：发现 / 关注 / 我的 / 下载。 */
  startup_page: string;
  backend_port: number | null;
  max_wait_seconds: number;
  /** 全局 R-18 展示开关；关闭后列表隐藏 x_restrict >= 1 的作品（详情页仍可访问） */
  show_r18: boolean;
  /** 列表 / 网格封面档位：small | medium | large（经 useThumbTier 读取，脏值兜底 medium） */
  thumb_quality_grid: string;
  /** 详情页主图档位：medium（= 接口 regular 原样，不插 /c/）| large | original */
  thumb_quality_detail: string;
  /** 大图 / 全屏浮层档位：large | original */
  thumb_quality_fullscreen: string;
  /** 图片磁盘缓存上限（MiB），合法区间 256~2048，默认 512（分区淘汰见 ADR 0028） */
  image_cache_max_mib: number;
  /**
   * 插画/漫画详情页图片宽度占比（详情页顶栏缩放控件写入），默认 1.0（撑满舞台），
   * 合法区间 0.5~1.0。老配置 / 部分 mock 不含该键，消费方读取时按 `|| 1` 兜底。
   */
  detail_image_scale: number;
  /** 小说正文字号缩放（小说阅读器底栏缩放控件写入），默认 1.0，合法区间 0.75~2.0 */
  novel_font_scale: number;
  /** 阅读背景色（语义键，空串=跟随主题） */
  novel_bg_color: string;
  /** SauceNAO API Key（以图识图必需；仅保存在本机配置文件，不写日志） */
  saucenao_api_key: string;
  translation_api_url: string;
  /** 接口协议（取值见 api/translation.ts 的 API_FORMATS），默认 chat_completions */
  translation_api_format: string;
  translation_model: string;
  /** 翻译目标语言（取值见 api/translation.ts 的 TARGET_LANGUAGE_CODES），空串 = 跟随界面语言 */
  translation_target_language: string;
  /** 小说翻译单请求超时（秒），默认 600（10 分钟），合法区间 30~3600 */
  translation_timeout_seconds: number;
  translation_extra: Record<string, unknown>;
  /** 只回传凭据是否存在，永不回传 API Key。 */
  translation_key_configured: boolean;
  translation_key_error: string;
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
  /** 本地头像缓存的可访问 URL（pixiv-avatar:// 协议），auth_status 提供 */
  avatar_url?: string;
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

/** 账号索引条目（auth_accounts_list.accounts 元素，snake_case）。 */
export interface AccountEntry {
  user_id: string;
  pixiv_id?: string;
  name?: string;
  profile_img?: string;
  avatar_file?: string;
  /** 本地头像缓存协议 URL（pixiv-avatar://）；缺省回退首字母 */
  avatar_url?: string;
  saved_at?: number;
}

/** auth_accounts_list：已保存账号列表 + 当前激活账号 user_id。 */
export interface AuthAccountsResponse {
  active: string | null;
  accounts: AccountEntry[];
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
