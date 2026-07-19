# ADR 0002 · 登录策略：WebView 嵌入主导 + 手动 cookie 兜底

**状态**：已接受 · **日期**：2026-07-19 · **关联 SPEC**：§4.1

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
3. 登录成功后 `webview.get_cookies()` 取 PHPSESSID
4. 导航 pixiv 页面提取 `x-csrf-token`
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

若 spike 验证 A 失败，立即退 C：登录窗改为"打开 pixiv.net 让用户登录，引导用户复制 PHPSESSID 粘入设置"。不上 playwright（避免多依赖）。
