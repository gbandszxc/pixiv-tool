/**
 * 浏览器视觉验收 mock 层（auth / settings）—— 仅 `!isTauri()` 时由 stores 走到这里。
 *
 * 口径与 api/browse.ts 的 mock 一致：确定性样例数据，仅供普通浏览器直接打开 dev 页时
 * 做视觉验收，不依赖 Rust 后端；Tauri 生产环境完全不触碰本文件，mock 不含任何真实凭据
 * （头像为内联 SVG data URI）。覆盖命令：auth_status / auth_accounts_list /
 * auth_logout（内存标记，刷新页面复位）/ settings_get / settings_save（内存版）。
 */
import type {
  AuthAccountsResponse,
  AuthStatusResponse,
  Settings,
  StructuredError,
} from "./tauri";
import type { SyncConfig, SyncDownloadResult, SyncScope, SyncTestResult, SyncUploadResult } from "./sync";

/** 样例账号（虚构）。 */
const MOCK_USER_ID = "100001";
const MOCK_PIXIV_ID = "wllmsb";

/** 内联 SVG data URI 头像（首字母占位，无外部请求）。 */
const MOCK_AVATAR = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64"><circle cx="32" cy="32" r="32" fill="#cfe5ff"/><text x="32" y="41" text-anchor="middle" font-family="sans-serif" font-size="26" font-weight="600" fill="#001d36">W</text></svg>'
)}`;

/** auth_logout 之后的内存态：退出后视为未登录、账号列表为空，刷新页面复位。 */
let mockLoggedOut = false;

/** auth_status：默认恒为已登录的样例账号，覆盖侧栏头像 chip 与账号菜单的视觉验收。 */
export function mockAuthStatus(): AuthStatusResponse {
  if (mockLoggedOut) return { is_logged_in: false };
  return { is_logged_in: true, user_id: MOCK_USER_ID, pixiv_id: MOCK_PIXIV_ID, name: MOCK_PIXIV_ID, avatar_url: MOCK_AVATAR };
}

/** auth_accounts_list：单个样例账号且为激活态（切换按钮自然 disabled）；退出后为空。 */
export function mockAuthAccountsList(): AuthAccountsResponse {
  if (mockLoggedOut) return { active: null, accounts: [] };
  return {
    active: MOCK_USER_ID,
    accounts: [{ user_id: MOCK_USER_ID, pixiv_id: MOCK_PIXIV_ID, name: MOCK_PIXIV_ID, avatar_url: MOCK_AVATAR, saved_at: 0 }],
  };
}

/** auth_logout：置内存退出标记（确定性，刷新页面即复位）。 */
export function mockAuthLogout(): void {
  mockLoggedOut = true;
}

/** 内存版设置（惰性初始化：确保 Tauri 环境下加载本模块也不做任何读取）。 */
let mockSettings: Settings | null = null;

function ensureSettings(): Settings {
  if (!mockSettings) {
    mockSettings = {
      output_dir: "C:\\Users\\demo\\Downloads\\pixiv-tool",
      output_formats: ["txt", "markdown"],
      // 下载文件名模板（与 stores/settings.ts 初值、后端 Settings::default 保持一致）
      illust_filename_template: "{title}_{pid}_p{page}",
      novel_filename_template: "{title}_{nid}",
      novel_series_filename_template: "{order}_{title}",
      // 初始语言跟随 localStorage（与启动期语言选择一致，避免 mock 回切语言）
      language: localStorage.getItem("pixiv-tool-lang") || "zh-CN",
      theme: "auto",
      theme_color: "pixiv",
      startup_page: "/browse/home",
      backend_port: null,
      max_wait_seconds: 180,
      // 与 stores/settings.ts 初值、后端 Settings::default 保持一致
      show_r18: true,
      thumb_quality_grid: "medium",
      thumb_quality_detail: "medium",
      thumb_quality_fullscreen: "large",
      // 图片磁盘缓存上限（MiB）：区间 256~2048，默认 512
      image_cache_max_mib: 512,
      // 详情页图片宽度占比（区间 0.5~1.0），默认 1.0（撑满舞台）
      detail_image_scale: 1,
      // 小说正文字号缩放（区间 0.75~2.0），默认 1.0
      novel_font_scale: 1.0,
      // 阅读背景色语义键（空串=跟随主题），默认未设置
      novel_bg_color: "",
      saucenao_api_key: "",
      translation_api_url: "",
      translation_api_format: "chat_completions",
      translation_model: "",
      translation_target_language: "",
      // 单请求超时（秒）：默认 10 分钟，合法区间 30~3600
      translation_timeout_seconds: 600,
      translation_extra: {},
      // 系统通知开关（默认开启）
      notifications_enabled: true,
      // 高级 → 同步：默认两项范围都勾选；样例账户已保存凭据，仅用于浏览器视觉验收
      webdav_url: "https://dav.example.com/remote.php/dav/files/me/pixiv-tool/",
      webdav_username: "pixiv",
      webdav_password_configured: true,
      sync_passphrase_configured: true,
      sync_scope_history: true,
      sync_scope_auth: true,
      translation_key_configured: false,
      translation_key_error: "",
    };
  }
  return mockSettings;
}

/** settings_get：返回内存配置快照。 */
export function mockSettingsGet(): Settings {
  return { ...ensureSettings() };
}

/** settings_save：只写内存（刷新页面即回到默认），字段校验不在 mock 内复刻。 */
export function mockSettingsSave(next: Partial<Settings>): void {
  mockSettings = { ...ensureSettings(), ...next };
}

// ===== 高级 → 同步（仅浏览器验收）=====
// 与真实命令同一套错误码，便于直接检查字段停靠与错误文案；成功结果是确定性样例。

/** 抛结构化错误（形状与 Rust Err 一致，供 errorPayload 取形）。 */
function syncError(code: string, field: string | null, message: string): never {
  throw { code, field, message } satisfies StructuredError;
}

/** 按「草稿覆盖 → 已保存值」合并当前连接配置。 */
function resolveSyncConfig(config?: SyncConfig) {
  const saved = ensureSettings();
  const url = (config?.webdavUrl ?? saved.webdav_url ?? "").trim();
  const username = (config?.webdavUsername ?? saved.webdav_username ?? "").trim();
  const hasPassword = config?.webdavPassword !== undefined
    ? config.webdavPassword.trim().length > 0
    : saved.webdav_password_configured === true;
  const passphrase = config?.syncPassphrase !== undefined
    ? config.syncPassphrase.trim()
    : saved.sync_passphrase_configured ? "mock-passphrase" : "";
  return { url, username, hasPassword, passphrase };
}

/** 连接必需项校验：错误码与字段名与后端一致。 */
function assertSyncConfig(config?: SyncConfig) {
  const { url, username, hasPassword, passphrase } = resolveSyncConfig(config);
  if (!url) syncError("sync_config_incomplete", "webdav_url", "请填写 WebDAV 地址");
  if (!/^https?:\/\//i.test(url)) syncError("sync_url_invalid", "webdav_url", "WebDAV 地址必须以 http:// 或 https:// 开头");
  if (!username) syncError("sync_config_incomplete", "webdav_username", "请填写用户名");
  if (!hasPassword) syncError("sync_config_incomplete", "webdav_password", "请填写密码");
  return { url, username, passphrase };
}

/** 内存版云端备份状态：地址含 "empty" 时模拟空目录，上传后置为有备份。 */
let mockCloudBackup: { has: boolean; time: string | null } = { has: true, time: "Mon, 02 Nov 2026 08:30:00 GMT" };

export function mockSyncTestConnection(config?: SyncConfig): SyncTestResult {
  const { url } = assertSyncConfig(config);
  mockCloudBackup = url.toLowerCase().includes("empty")
    ? { has: false, time: null }
    : { has: true, time: mockCloudBackup.time ?? "Mon, 02 Nov 2026 08:30:00 GMT" };
  return {
    status: "ok",
    http_status: 207,
    has_backup: mockCloudBackup.has,
    backup_time: mockCloudBackup.time,
  };
}

function assertScopes(scopes: SyncScope[]) {
  if (scopes.length === 0) syncError("sync_no_scope", null, "至少选择一项同步范围");
}

export function mockSyncUpload(scopes: SyncScope[], config?: SyncConfig): SyncUploadResult {
  assertScopes(scopes);
  assertSyncConfig(config);
  // 同步密码可选：留空 = 内置默认密钥加密（ADR 0049）；上传即产生云端备份
  mockCloudBackup = { has: true, time: new Date().toISOString() };
  return {
    exported_at: new Date().toISOString(),
    bytes: 18432,
    history_count: scopes.includes("history") ? 128 : 0,
    account_count: scopes.includes("auth") ? 2 : 0,
  };
}

export function mockSyncDownload(scopes: SyncScope[], config?: SyncConfig): SyncDownloadResult {
  assertScopes(scopes);
  assertSyncConfig(config);
  if (!mockCloudBackup.has) syncError("sync_not_found", null, "云端没有找到同步文件");
  return {
    exported_at: mockCloudBackup.time ?? new Date().toISOString(),
    history_imported: scopes.includes("history") ? 128 : 0,
    accounts_imported: scopes.includes("auth") ? 2 : 0,
    active_changed: false,
    auth_changed: scopes.includes("auth"),
  };
}
