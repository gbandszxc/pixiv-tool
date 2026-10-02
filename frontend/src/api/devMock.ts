/**
 * 浏览器视觉验收 mock 层（auth / settings）—— 仅 `!isTauri()` 时由 stores 走到这里。
 *
 * 口径与 api/browse.ts 的 mock 一致：确定性样例数据，仅供普通浏览器直接打开 dev 页时
 * 做视觉验收，不依赖 Rust 后端；Tauri 生产环境完全不触碰本文件，mock 不含任何真实凭据
 * （头像为内联 SVG data URI）。覆盖命令：auth_status / auth_accounts_list /
 * auth_logout（内存标记，刷新页面复位）/ settings_get / settings_save（内存版）。
 */
import type { AuthAccountsResponse, AuthStatusResponse, Settings } from "./tauri";

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
      // 初始语言跟随 localStorage（与启动期语言选择一致，避免 mock 回切语言）
      language: localStorage.getItem("pixiv-tool-lang") || "zh-CN",
      theme: "auto",
      theme_color: "pixiv",
      backend_port: null,
      max_wait_seconds: 180,
      // 与 stores/settings.ts 初值、后端 Settings::default 保持一致
      show_r18: true,
      thumb_quality_grid: "medium",
      thumb_quality_detail: "medium",
      thumb_quality_fullscreen: "large",
      // 小说正文字号缩放（区间 0.75~2.0），默认 1.0
      novel_font_scale: 1.0,
      saucenao_api_key: "",
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
