# 20 — 无 Chromium 浏览器时回退内嵌 webview 登录

**Status:** ready-for-human

## What to build

从用户视角：本机没装 Chrome / Edge / Chromium 时（典型：未装 Chrome 的
macOS / Linux），在登录弹窗点「打开浏览器登录」不再报错终态，而是应用内弹出
一个 960×720 的 webview 窗口加载 Pixiv 登录页；用户正常输账密、过验证码、过
2FA，跳转 pixiv 主站后窗口自动关闭，主界面提示登录成功。PHPSESSID（HttpOnly）
经 Tauri 原生 cookie API 提取，复用既有会话验证与 Keychain 存储，前端契约不变。

恢复 ADR 0006 第 6 条被 ADR 0008 裁剪的回退路径，决策依据与实测证据见 ADR 0009。

## Acceptance criteria

- [x] `src-tauri/src/auth/webview_login.rs`：建窗（label `login-webview`、960×720、独立数据目录 `<config>/login-webview-profile`）→ 轮询 URL → 主站后提取 cookie → `fetch_session_probe` 验证 → success；cancelled / timeout / error 终态语义与 CDP 版一致并统一关窗
- [x] `auth_login` 接线：仅 `BrowserNotFoundError` 触发回退；浏览器路径中途其他错误不回退；成功保存逻辑逐字未动
- [x] 测试钩子：`PIXIV_TOOL_FORCE_WEBVIEW_LOGIN=1` 强制走 webview 路径
- [x] Cookie 过滤纯函数与 host 门禁配离线单测（伪装域/空值/缺字段容错对齐 CDP 版语义）
- [x] 任何日志不出现 Cookie 值（只出现名字/长度/计数）
- [x] 前端错误文案透传：store 抛 `LoginError(status, message)`，弹窗 cancelled→温和提示、timeout/error→展示后端原因
- [x] `cargo test` 全绿（115+5）、`cargo clippy --all-targets` 零告警、`cargo fmt --check` 干净、`pnpm build` 通过
- [ ] **人工验收**：`PIXIV_TOOL_FORCE_WEBVIEW_LOGIN=1 cargo tauri dev` 实机走一次 webview 登录全流程（含验证码），确认主界面头像出现、重启后登录态保持

## Comments

- 2026-08-21：可行性探针先行验证（离线 HttpOnly 读取 + 真实登录 PASS，len=41），
  证据记录于 ADR 0009。代码由 agent 实现，集成验收通过，仅剩人工 GUI 验收一项。
