# 18 — macOS 浏览器登录与手动 Session 可靠性

**Status:** done

## What to build

- 真实 Chromium 独立 profile 替代内置 WebView 作为登录主路径。
- 手动 PHPSESSID 使用 `/ajax/user/self` 权威验证，匿名 token 不得视为登录。
- macOS 使用系统 Keychain 保存登录态。
- 保留 pywebview 作为未安装 Chromium 浏览器时的回退。

## Acceptance criteria

- [x] Chrome / Edge / Chromium 登录后能通过 CDP 读取 HttpOnly PHPSESSID。
- [x] 不读取用户日常浏览器 profile。
- [x] 匿名 HTTP 200 + token 被拒绝。
- [x] 支持纯 Session 值和 `PHPSESSID=...` 粘贴格式。
- [x] macOS Cookie 保存到 Keychain。
- [x] 后端测试与前端构建通过。
