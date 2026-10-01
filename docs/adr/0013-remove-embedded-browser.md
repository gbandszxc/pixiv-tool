# ADR 0013 — 移除内嵌 Pixiv 浏览器（/pixiv），自有浏览 UI 为唯一入口

**状态**：已接受 · **日期**：2026-10-01 · **关联 SPEC**：§3.4、§6.1、§6.4、§7
**关联**：ADR 0012（本 ADR 推翻其「内嵌浏览器保留」决策）；ADR 0009 / 0011（登录回退窗，不受影响）

## 背景

V1.1 起应用提供 `/pixiv` 内嵌浏览器（子 WebView 直连 pixiv 主站，ADR 0008 体系）。
ADR 0012 决定「先保留内嵌浏览器，左侧栏追加自有 UI，逐步转向第三方客户端」，
并注明内嵌浏览器为「计划后续版本移除」的过渡形态。

自有浏览模式（`/browse/*`，ADR 0012）已覆盖首页 / 频道 / 发现 / 动态 / 搜索 /
排行榜 / 作品查看器 / 小说阅读器 / 系列目录 / 作者页；「打开原页」已改走系统默认
浏览器（`tauri-plugin-opener`），「返填表单」直达下载流程。内嵌浏览器的浏览职责
不再成立，其资源成本（完整站点前端 + URL 轮询 + 子 WebView 状态机）成为纯负担。

## 决策

1. **整体移除 `/pixiv` 内嵌浏览器**：
   - 前端：`views/PixivView.vue`、`/pixiv` 路由、侧栏入口、`browse_*` 控制调用、
     `nav.pixiv` / `pixiv.*` 文案、`pixiv` 侧栏图标；
   - 后端：`src-tauri/src/browse/`（子 WebView 生命周期 / 域白名单 / URL 轮询 /
     登录态注入 / 深色补丁）、`commands/browse_cmds.rs` 的 10 个控制命令及其注册、
     多账号登录 / 切换 / 退出时的 `sync_browse_webview` 同步逻辑、
     `webview2-com` / `windows-core` 依赖（仅该模块使用，随之移除）。
2. **浏览模式自有 UI（`/browse/*`）为唯一 Pixiv 浏览入口**；「打开原页」固定走
   系统默认浏览器，不再有任何页面跳转到内嵌浏览器。
3. **登录回退窗不随本决策移除**：无 Chromium 系浏览器时的内嵌 webview 登录窗
   （`auth/webview_login.rs`，ADR 0009 / 0011）是登录能力而非浏览功能，继续有效——
   独立窗口、独立 profile（`<config>/login-webview-profile`）、
   `PIXIV_TOOL_FORCE_WEBVIEW_LOGIN=1` 测试钩子均保留。原供 browse 模块复用的
   `extract_pixiv_cookies_from_store` 收为模块私有（登录轮询自身仍在使用）。

## 后果

- 子 WebView 状态机（活动标记 / 最后 URL 记忆 / 首次加载自动注入 / 深色补丁）与
  10 个 IPC 命令全部删除；IPC 命令总数相应减少 10（auth 6 / browse_api 11
  等分组不变）。
- 账号切换 / 退出只更新凭据存储与账号索引，不再需要「app 已是新账号、内嵌页
  还是旧账号」的同步（该问题随内嵌页消失而消失）。
- Windows 依赖收窄：应用代码不再直接调用 WebView2 原生 CookieManager。
- 若未来要恢复内嵌浏览能力，需重走 ADR 0012 的评估与实现。

## 验证

- `cargo test`（MSVC 配方）全绿；`pnpm build` 类型检查与构建通过。
- dev 实机冒烟：应用启动、侧栏无 Pixiv 入口、浏览页各路由正常、
  `#/pixiv` 不再存在。
