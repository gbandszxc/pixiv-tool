# ADR 0002 · 登录策略：WebView 嵌入主导 + 手动 cookie 兜底

**状态**：被 ADR 0006 取代 · **日期**：2026-07-19 · **关联 SPEC**：§4.1

> 状态补充（2026-08-20）：手动 PHPSESSID 兜底与主登录方案仍有效并由 Rust 实现承接；「pywebview 回退登录窗」路径已在 [ADR 0008](0008-tauri-rewrite.md) 裁剪（缺浏览器时改用手动 Cookie 登录）。
> 状态补充（2026-10-01）：上一条的「回退登录窗已裁剪」已被 [ADR 0009](0009-webview-login-fallback.md) 推翻——缺浏览器时改为回退 Tauri 原生 webview 登录窗（现行流程见 SPEC §4.1），「无回退窗」的表述作废。

> 2026-07-23：Pixiv 的 reCAPTCHA Enterprise 在 macOS WKWebView 中出现图片
> 验证循环，登录主路径改为真实 Chromium；本 ADR 的 pywebview 实现仅保留为回退。

## 背景

Pixiv Web API 需 `PHPSESSID`（HttpOnly）+ `x-csrf-token`。候选登录方案：

- A. 嵌入 WebView2 登录窗
- B. 系统浏览器登录 + 导入 cookie（Chrome v127+ App-Bound Encryption 已死）
- C. 手动粘 PHPSESSID
- D. OAuth 逆向（风控锁号，不可行）

## 决策

**D 策略 = A 主导 + C 兜底**。

流程：
1. 主窗点"登录" → 弹 pywebview 加载 `https://accounts.pixiv.net/login`
2. 用户正常输账密 / 过验证码 / 过 2FA
3. 登录成功后 `webview.get_cookies()` 取 PHPSESSID（✅ Spike 验证可读 HttpOnly，见 ADR 0005）
4. 导航 pixiv 页面提取 `x-csrf-token`（正确路径：`dehydratedState.queries[*].meta.apiClient.token`）
5. `CookieStore.save()` 加密持久化

## 后果

**正面**
- A 方案风控最低（真浏览器指纹），同类工具（Pixez 等）通用做法。
- 2FA 天然支持。
- C 兜底保证 spike 失败仍可用。

**负面**
- 依赖 R1（spike 验证 get_cookies 读 HttpOnly）。
- 若 A 失败，UX 降级为"手动粘 cookie"，对非技术用户不友好。

## 备选触发条件

**Spike 已通过**（ADR 0005），A 方案不触发退路。

C 兜底仍保留作为应急方案：若未来 pixiv 改版导致 pywebview 拿不到 cookie，可临时启用"用户手动粘 PHPSESSID"模式。
