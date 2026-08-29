# ADR 0009 · 恢复 Webview 登录回退窗（Tauri 原生实现）

**状态**：已接受 · **日期**：2026-08-21 · **关联**：SPEC §4.1、风险登记 R8、ADR 0006、ADR 0008

## 背景

ADR 0006 决策第 6 条曾规定「未安装 Chromium 浏览器时回退 pywebview 登录窗」；
该条随 ADR 0008 全量重构被裁剪，裁剪理由记载于 SPEC §4.1：「Tauri 的
WKWebView / WebView2 没有统一的 Cookie 读取 API」（PHPSESSID 是 HttpOnly）。
此后，本机没有 Chrome / Edge / Chromium 的用户（典型：未装 Chrome 的
macOS / Linux）只能手动粘贴 PHPSESSID。

2026-08-21 实测推翻了上述理由：

- 项目锁定的 wry 0.55.1 + tauri 2.11 已内置统一 cookie API：三平台均基于原生
  cookie 存储实现，HttpOnly 字段无损可读——「没有统一 Cookie 读取 API」不再成立。
- macOS WKWebView 探针：pixiv 登录页完整加载，reCAPTCHA Enterprise 正常初始化
  （badge + anchor iframe 均出现），Cloudflare 无拦截。
- 真人登录验收 PASS：约 30 秒完成真实登录，无验证码循环；HttpOnly PHPSESSID
  （len=41）连同 device_token 等一并从 webview 读出。ADR 0006 当年记录的
  「macOS WKWebView 图片验证码循环」在 Tauri 栈不复现。

## 决策

1. 登录主路径不变：仍以真实 Chromium 系浏览器独立 profile + CDP 提取 Cookie
   为主（ADR 0006 第 1~3 条原样有效）。
2. 仅当找不到 Chromium 系浏览器（`BrowserNotFoundError`）时回退打开内嵌
   webview 登录窗（960×720），加载 pixiv 登录页；轮询到页面跳转 pixiv 主站后，
   经 Tauri 原生 cookie API 提取 pixiv.net 域 Cookie。
3. 环境变量 `PIXIV_TOOL_FORCE_WEBVIEW_LOGIN=1` 强制走 webview 路径，作为回归
   测试钩子（不改变默认行为）。
4. 登录窗使用独立数据目录 `<config>/login-webview-profile` 与主窗隔离，删除该
   目录即可完整复测——在 config_dir 下维护应用专属登录 profile 的做法沿用
   ADR 0006 既有先例（CDP 路径的 `<config>/login-browser-profile` 同理）。
5. 从 webview 提取的 Cookie 复用既有链路：`fetch_session_probe` 以非空
   `userData.id` 权威验证 → keyring 存储；`LoginResult` 契约不变，前端零改动。

## 后果

**正面**

- 未安装 Chrome / Edge / Chromium 的 macOS / Linux 用户重新获得自动登录路径，
  不再被迫手动粘贴 PHPSESSID。
- 回退与主路径共用会话验证与凭据存储，契约不变，前端无感知。

**负面**

- WebKit 指纹相对 Chrome 变化更快，reCAPTCHA Enterprise 对 WKWebView 风控的
  长期稳定性待观察；若未来复现验证码循环，仍有手动 Cookie 兜底（残余风险，
  见 R8）。
- 新增一个平台持久化目录（`login-webview-profile`），卸载清理与问题排查时
  需知晓其存在。
