# ADR 0006 · 真实 Chromium 登录 + macOS Keychain

**状态**：已接受 · **日期**：2026-07-23 · **关联 SPEC**：§4.1、§5.3

## 背景

Pixiv 当前登录页同时使用 reCAPTCHA Enterprise 与 Cloudflare。macOS WKWebView
容易进入图片验证码循环；旧方案只把手动粘贴 PHPSESSID 设为推荐，没有修复主路径。
同时，macOS CookieStore 仍是 stub，两种登录即使取得 Cookie 也无法保存。

## 决策

1. 登录主路径启动本机 Chrome、Edge 或 Chromium 的应用专属 profile。
2. 浏览器只在随机本地 CDP 端口开放；通过 `Storage.getCookies` 读取 Pixiv
   HttpOnly Cookie，完成后立即关闭。
3. 应用不连接、不解密用户日常浏览器 profile。
4. 手动 PHPSESSID 继续作为兜底；两条路径统一调用 `/ajax/user/self`，以
   `userData.id` 验证 Session，并从同一响应取得 token。
5. macOS 使用系统 Keychain 保存 Cookie；Windows 继续使用 DPAPI。
6. 未安装 Chromium 浏览器时回退 ADR 0005 的 pywebview 登录窗。

## 后果

- macOS 不再依赖 WKWebView 完成 reCAPTCHA。
- 独立浏览器 profile 可积累正常设备状态，也可通过删除该目录完整复测。
- 增加对本机 Chromium 浏览器的软依赖；没有浏览器时仍有回退路径。
- Linux Secret Service 仍未实现。
