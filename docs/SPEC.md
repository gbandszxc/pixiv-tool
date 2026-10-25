# Pixiv Tool · 技术规格书（SPEC）

> **状态**：v1.3 · Tauri 2 + Rust 全量重构（ADR 0008）· 2026-10-25
> **真相源**：本文档是项目开发的唯一真相源。任何架构变更须先更新本文档（或追加 ADR），再改代码。
> v1.2：新增浏览模式（自有 UI 代理 pixiv 只读浏览，ADR 0012）。
> v1.3：小说翻译改为 Rig 通用 Agent 内核上的整篇 Job（ADR 0034 / 0035），
> 构建基线固定 Rust 1.99.1；旧单页两轮实现仅作 v1 缓存只读兼容。

---

## 1. 项目目标

构建一个本地运行的 **Pixiv 客户端工具**，初版（V1）聚焦 **Pixiv 小说抓取**，支持单篇、系列、用户全集三类来源，配合图形界面与登录态管理。

### 1.1 V1 范围

| 来源 | 说明 |
|---|---|
| 单篇小说 | 含多页小说（`pageCount > 1`） |
| 系列小说 | 系列内多篇独立保存，按系列顺序编号；**不合并**为单文件 |
| 指定用户的全部小说 | 用户名下所有作品（系列内按序号编号 + 散篇），统一存 `novel/` 平铺 |
| 单幅插画 | 单张/多页作品，**一律按原图**（`img-original` 直链）下载 |
| 指定用户的全部插画 | 用户全部插画+漫画作品（`profile/all` 的 illusts+manga），ugoira 动图存原始 zip |

### 1.2 V1 明确不做（延后 V2+）

- ~~搜索小说~~、~~用户收藏夹 / 插画作品列表页浏览~~：**只读浏览**已于 v1.2 随浏览模式落地
  （§6.4、ADR 0012）；「按搜索结果 / 收藏夹批量抓取」与收藏夹浏览仍不做。列表页下载本身见 ADR 0007
- 按 tag 批量抓取
- 小说内嵌图片**落盘下载**（`[pixivimage:...]` / `[uploadedimage:...]` 标记；阅读器
  内直接显示见 §6.4，此处指抓取产物不入图）
- EPUB 输出（架构预留接口，不实现）
- 自适应限速（V1 用固定并发 + 429 暂停）
- 任务断点启动弹窗恢复
- Linux 登录功能（Windows / macOS 支持登录；Linux 走 keyring Secret Service 后端，未实机验证）
- 无人值守静默安装（不做）；应用内下载更新并引导安装见 §7.2、ADR 0016
- 错误上报（Sentry 等）

---

## 2. 技术栈

### 2.1 总览

| 层 | 选型 |
|---|---|
| 桌面外壳 + 后端 | **Tauri 2（Rust）**，单进程，IPC 通信（见 ADR 0008） |
| 前端 | **Vue 3.4+ · TypeScript · Vite 6.4.3+ · Vue Router 4（hash） · Pinia · @tauri-apps/api** |
| UI 组件库 | **@material/web（Material 3）** |
| CSS | 原生 CSS + CSS Variables + Vue `<style scoped>` |
| HTTP 客户端 | **wreq 6（Chrome147 指纹伪装，BoringSSL）** |
| Agent 运行层 | **Rig 0.45.0（`rig-core` / `rig-agent` / `rig-memory` 精确锁版本、`default-features = false`）+ 项目自建运行层**（ADR 0034） |
| 构建工具链 | **Rust 1.99.1**（`rust-toolchain.toml` 固定；Windows 使用 MSVC），edition 2024 |
| 数据库 | **SQLite（rusqlite）**，schema 与旧 Python 版逐字兼容 |
| 依赖管理 | 后端 **cargo** + 前端 **pnpm** |
| 打包 | **Tauri bundler**（仅本地按需打包：release / debug，无 CI） |

Rig 只承担三协议 wire 编解码与工具调用循环，HTTP 仍统一走 wreq；不引入 MCP、
通用聊天页面、外部插件、任意 shell、本地 HTTP 服务、固定端口或额外后端进程。

### 2.2 不选的替代方案与理由

- ~~**Electron / Tauri**：体积大 / Rust 学习成本~~ —— **已被 ADR 0008 取代**：旧栈 PyInstaller 分发负担与 curl_cffi 依赖促成的全量 Tauri 重构。
- **OAuth 逆向**：pixiv 风控极严，会锁号。
- **手动解密浏览器 cookie（Chrome App-Bound Encryption）**：v127+ 已基本不可行。
- ~~**pywebview 原生 JS API**：同步阻塞、无 devtools 网络面板~~ —— 旧栈（ADR 0001/0002）的历史理由，已随 ADR 0008 作废。
- **WebSocket**：进度推送用 Tauri 事件（`emit` / `listen`）已足够，双向通信是过度设计。
- **Tailwind / UnoCSS**：页面数量有限，用不上原子化 CSS 的扩展性。
- **Naive UI / Element Plus / Ant Design Vue**：整库自带一套视觉语言，与「自有 M3 设计系统」冲突（需整体覆盖主题）；@material/web 只提供 M3 原语，间距、布局与圆角由 `DESIGN.md` 自持。（本文档早期版本曾写 Naive UI，已作废。）
- **自研三协议 wire 与工具循环**：会与既有翻译契约产生第二套真相源，且需重复维护流式事件、工具参数增量、推理字段与结束原因；改用 Rig，但保留项目自己的 HTTP、凭据、端点与错误脱敏（ADR 0034）。
- **MCP / 通用聊天 / 第三方编排框架 / 向量库**：超出产品边界，扩大权限与数据面；Agent 只服务小说整篇翻译，不提供自由对话或外部工具接入。
- **Rig 自带 reqwest / rustls 传输**：会引入第二套 TLS 与代理语义，丢失 wreq 的浏览器指纹与既有请求头行为；只使用 Rig 的协议层。
- **Poetry / pip / Python**：全栈已于 2026-08 迁往 Rust（ADR 0008），Python 后端已删除。

---

## 3. 系统架构

### 3.1 进程拓扑

```
┌─ pixiv-tool（Tauri 单进程）───────────────────────────────────┐
│                                                                │
│  ┌─ 主线程（tao 事件循环）─────────────────────────────┐      │
│  │  窗口生命周期 + 关闭确认（事件交前端确认框，按语言） │      │
│  └──────────────────────────────────────────────────────┘      │
│                                                                │
│  ┌─ WebView（主窗）──────────────┐  ┌─ tokio runtime ─────────────┐
│  │  Vue3 SPA（hash 路由）        │  │  #[tauri::command]           │
│  │  ↕ invoke / listen（IPC）     │↔ │  tokio::spawn 抓取 / 翻译 Job│
│  └───────────────────────────────┘  │  Semaphore(2)+0.4s（抓取）   │
│                                     │  → emit task:// 事件         │
│                                     │  Agent 运行层：调度 / 预算 / │
│                                     │  压缩 / 恢复 / 事件 / 存储   │
│                                     │        ↓                     │
│                                     │  Rig 0.45 工具循环 → wreq    │
│                                     └──────────────────────────────┘
└────────────────────────────────────────────────────────────────┘
```

单进程内由 Tokio 驱动 Agent 运行层；无本地 HTTP 服务、无端口、无 SSE——前端经
Tauri IPC 直接调 Rust 命令，抓取进度经 `task://progress` / `task://done` 事件推送
（TasksView 同时保留 2s 轮询兜底），翻译任务经 `translation://job` 事件推送（§3.2）。

### 3.2 通信协议

- **命令类**：`invoke('<命令名>', args)`，命令与参数清单见 §7（返回体沿用旧
  HTTP 响应形状，snake_case；业务错误走返回值 `{error}`，校验错误走 reject
  string）。
- **进度推送**：Tauri 事件 `task://progress`、`task://done`。
- **更新下载进度**：`download_app_update` 的 IPC Channel（独立于抓取任务事件，见 §7.2）。
- **小说翻译任务**：`novel_translation_start` 立即返回 Job 快照，后台 tokio 任务继续
  执行；之后状态与进度统一走 `translation://job` 事件（payload 见 §7.1）。前端先订阅
  事件、再拉 `novel_translation_jobs` 快照，按每 Job 单调 `seq` 丢弃重复或迟到事件
  （快照序号落后于已应用事件时保留事件版本），避免「先读快照再订阅」的空窗丢事件；
  页面重新可见时重连并重拉快照。普通进度在传输层按 ≥5s 合并，生成预览按「距上次
  ≥300ms 或累计新增 ≥2048B」节流（任一满足即发），阶段切换、状态变化与终态立即
  flush。旧 `novel_translate_page` 的 IPC Channel 进度协议已随命令移除而作废。
- **安全边界**：无网络监听面；IPC 仅限本 webview。
  主窗生产 CSP 限制脚本为自身资源（Tauri 注入脚本由构建阶段补充 hash），
  禁止 object 与表单提交；开发 CSP 仅额外允许本机 Vite HMR WebSocket。
  Material Web 所需内联样式保留，图片允许既有 HTTPS、data/blob 和自定义协议。

### 3.3 数据与配置目录

dev（debug 构建）：`<repo>/data`、`<repo>/config`（与旧 Python dev 一致，
旧 app.db / settings.json 无缝沿用）。release：Windows exe 同级可写则
portable，不可写（MSI/NSIS 装进 Program Files）回退
`%LOCALAPPDATA%\pixiv-tool\`；macOS `~/Library/Application Support/pixiv-tool/`；
Linux XDG 标准目录。
解析在 `src-tauri/src/paths.rs`。

翻译缓存固定在 `<data>/translations/`：v2 为 `<novel_id>/<job_id>/`（Job 拥有全部
中间产物与恢复数据），v1 旧页缓存只读兼容；两者都由同一维护入口统计与清理（§4.3、§9）。

### 3.4 目录结构

```
pixiv-tool/
├─ rust-toolchain.toml           # 固定 Rust 1.99.1（ADR 0034）
├─ src-tauri/                   # Tauri 2 + Rust 后端
│  ├─ Cargo.toml                # wreq 6（指纹伪装，锁版本）/ rig 0.45（精确锁定）/ rusqlite / keyring / tokio
│  ├─ tauri.conf.json           # devUrl 9961、frontendDist ../frontend/dist
│  ├─ capabilities/default.json # IPC 权限（core + dialog + opener）
│  ├─ resources/agent/          # 内置 Agent 资源：prompts/、agents/（coordinator/translator/reviewer）、skills/smart-translator/
│  ├─ tests/                    # pixiv_api/（pixiv 接口测试唯一入口）/ agent_protocol_wire.rs / agent_runtime.rs / translation_job_e2e.rs / smoke_commands.rs
│  └─ src/
│     ├─ main.rs / lib.rs       # 入口与 Builder 装配（全部命令注册、pixiv-img 协议、关闭确认、翻译恢复）
│     ├─ state.rs               # AppState：paths/settings/db/cookies/tasks/accounts/translation_service
│     ├─ agent/                 # 通用 Agent 内核：runtime / scheduler / budget / compaction / recovery / events / resources / storage / model（Rig wire + wreq 传输）
│     ├─ translation/           # 整篇翻译 Job：service / source / chunking / artifacts / tools / validation / publish / legacy
│     ├─ translation_legacy.rs  # v1 单页两轮实现：只读兼容旧译文，命令入口已移除
│     ├─ translation_diagnostics.rs / translation_summary.rs
│     ├─ pixiv/                 # client（限速/重试/429）、api（/ajax typed）、csrf（会话与 web csrf 探测）、browse_api（浏览端点）
│     ├─ core/                  # sources / crawler / illust_crawler / task_manager / exporter / filename_template
│     ├─ auth/                  # browser_login（CDP）/ cdp（WebSocket 客户端）/ webview_login（内嵌登录窗回退）
│     ├─ commands/              # 与 translation/ 共 79 个 #[tauri::command]（translation 18 / auth 6 / browse_api 22 / tasks 9 / settings 3 / maintenance 3 / saucenao 1 / history 1 / browse_history 3 / misc 8 / app 1 / update 4）
│     ├─ db.rs                  # rusqlite：schema 与查询（含 history UNION）
│     ├─ settings.rs            # settings.json 兼容加载/校验/迁移（含翻译运行参数唯一区间表）
│     ├─ cookies.rs             # keyring CookieStore
│     ├─ accounts.rs            # 多账号索引（accounts.json）+ 每账号凭据条目
│     ├─ image_proxy.rs         # pixiv-img 协议核心（磁盘缓存 + CDN 并发闸门 + 同 URL 在途合并 + immutable 长缓存响应头）
│     ├─ saucenao.rs            # SauceNAO 以图识图客户端（file/url 两通道搜索、响应解析、错误分类；契约见 §7，接口调研见 docs/research/saucenao-api.md）
│     ├─ menu_bar.rs            # Windows 菜单栏默认隐藏 / Alt 唤出（AcceleratorKeyPressed + WM_EXITMENULOOP 收回）
│     └─ paths.rs / platform.rs / logging.rs
├─ frontend/                    # Vue3 + TS + Vite + Material Web（M3）
│  ├─ src/
│  │  ├─ views/                 # ToolsView（下载页签壳）/ TasksView / HistoryView / SaucenaoView（以图识图）
│  │  │  └─ browse/             # BrowseHome/Channel/Discover/Feed/Search/Ranking/Bookmark/History/Me + Work/Series/Author/Novel
│  │  ├─ components/            # common/（AppPagination 公共分页）auth/（LoginDialog / AccountMenu）navigation/（核心导航 / PageBackButton）download/（共用下载表单 / 状态栏）settings/（SettingsPanel / SettingsDialog / TranslationAdvancedPanel / TranslationAgentFields / sections.ts）browse/（WorkCard / WorkGrid / BookmarkButton / ImageViewer / NovelContent / NovelTranslationMenu / TranslationProgressBanner / SectionTabs / RelatedGrid）
│  │  ├─ material.ts            # @material/web 组件按需 import
│  │  ├─ stores/                # Pinia（auth/tasks/settings/history/translation，全走 invoke）
│  │  ├─ api/tauri.ts           # invoke 封装 + 错误归一化 + 契约类型
│  │  ├─ api/translation.ts     # 翻译 Job 契约类型 + invoke / 事件订阅
│  │  ├─ api/browse.ts          # 浏览契约类型 + 非 Tauri 环境的确定性 mock 层
│  │  ├─ api/saucenao.ts        # SauceNAO 契约类型 + invoke 封装 + 非 Tauri 环境的确定性 mock 层
│  │  ├─ api/devMock.ts         # auth/settings 的浏览器视觉验收 mock（仅 !isTauri() 生效）
│  │  ├─ utils/thumb.ts         # 缩略图 URL 档位改写（§6.4）
│  │  ├─ composables/useThumbTier.ts # 设置档位 → 合法 ThumbTier 的响应式读取
│  │  ├─ locales/               # zh-CN.ts / en-US.ts / ja-JP.ts
│  │  ├─ styles/                # 全局 CSS Variables（--md-sys-color-* 等）
│  │  └─ router/                # hash 模式
│  └─ vite.config.ts            # port 9961 + strictPort（无 proxy）
├─ docs/                        # 本文档与 ADR、PIXIV-API 接口契约、调研、agents 约定
├─ scripts/                     # make_icon.sh（macOS 图标生成）
└─ README.md
```

运行时翻译缓存（不随仓库分发）：

```text
<data>/translations/
├─ <novel_id>/<hash>.json          # v1 旧页缓存（只读兼容）
└─ v2/<novel_id>/<job_id>/
   ├─ manifest.json                # 身份、状态、阶段、输入 hash、资源版本、计数、用量、预算
   ├─ settings_snapshot.json       # 不含凭据
   ├─ resources.json               # 资源 ID / 版本 hash
   ├─ source/preparation/chunks/reviews/output
   ├─ sessions/                    # Job 内会话与压缩记录
   ├─ checkpoints/                 # 阶段检查点
   └─ temporary/                   # 原子写入临时文件
```

---

## 4. 核心模块设计

### 4.1 登录与 Cookie（V1 最高风险点）

**方案**：真实 Chromium 独立 profile CDP 登录主导 + 手动粘 PHPSESSID 兜底 +
内嵌 webview 登录窗回退。本机没有 Chrome / Edge / Chromium
（BrowserNotFoundError）时不返回 error 终态，而是回退打开内嵌 webview 登录窗
（960×720，独立数据目录 `<config>/login-webview-profile`，每次打开前整目录
重置以保证未登录态，ADR 0011）：真人登录跳转 pixiv
主站后经 Tauri 原生 cookie API（wry 0.55 内置，三平台原生存储实现，HttpOnly
无损可读）提取 Cookie，复用同一会话验证与 Keychain 存储，`LoginResult` 契约
不变（ADR 0009，恢复 ADR 0006 第 6 条、曾被 ADR 0008 裁剪的回退路径）。环境
变量 `PIXIV_TOOL_FORCE_WEBVIEW_LOGIN=1` 强制走 webview 路径（测试钩子）。

**登录流程**（`src-tauri/src/auth/browser_login.rs` + `auth/cdp.rs`）：

```
[用户点"登录"]
      ↓
find_login_browser：按平台找首个已安装的浏览器（找不到 → error 终态，
  macOS /Applications 下 Chrome→Edge→Chromium；Windows PROGRAMFILES /
  PROGRAMFILES(X86) / LOCALAPPDATA 下 chrome.exe/msedge.exe；Linux which 四连）
      ↓
spawn 隔离浏览器（stdout/stderr 丢弃）：
  --remote-debugging-port=<随机空闲端口> --remote-debugging-address=127.0.0.1
  --user-data-dir=<config>/login-browser-profile --no-first-run
  --no-default-browser-check --app=about:blank
      ↓
CDP 连接后先重置登录态（ADR 0011，登录窗语义=必然未登录打开）：
  Storage.getCookies 找 pixiv 域 PHPSESSID → Storage.setCookies 过期覆盖删除
  （保留 device_token/cf_clearance 等设备态，减少登录风控）→
  Target.createTarget 打开 accounts.pixiv.net/login → 关闭 about:blank 启动页
  （登录页请求发生在清会话之后，杜绝已登录 profile 302 回主站秒判成功）
      ↓
轮询 http://127.0.0.1:{port}/json/version 拿 webSocketDebuggerUrl
（HTTP 客户端显式 .no_proxy()，50 次 × 0.1s）
      ↓
主循环（至多 300s，每轮 0.5s）：
  - 浏览器被用户直接关闭 → cancelled
  - Target.getTargets 出现 pixiv 主站 page（host ∈ {pixiv.net, www.pixiv.net}，
    accounts.pixiv.net 不算——避免验证码阶段制造额外请求）
  - Storage.getCookies → 过滤 pixiv.net 域（剥前导 '.'）→ 拿到非空 PHPSESSID
  - fetch_session_probe 服务端权威验证（/ajax/user/self?lang=zh，复用 wreq
    Chrome147 指纹；非空 userData.id 才算有效——匿名访问同样返回 200+token）
      ↓
验证成功 → 把响应顶层 token 字段补入 cookie map 的 x-csrf-token
      ↓
CookieStore.save（keyring 存储）→ finally：CDP Browser.close，
  3s 内未退则 kill 浏览器
      ↓
主界面刷新登录态
```

**csrf token 来源**：旧 pywebview 登录窗时代（ADR 0005）需在页面 JS 上下文按
`__NEXT_DATA__...meta.apiClient.token` 等多路径提取；该登录窗已随 ADR 0008
移除，token 改由 `/ajax/user/self` 顶层 `token` 字段经服务端探测获取
（`pixiv/csrf.rs::fetch_session_probe`），不再依赖页面内部 react-query 结构。

**关键实现约束**：

1. Chrome / Edge 必须使用应用专属 `user-data-dir`（`config/login-browser-profile`），
   不得连接用户日常 profile。`login-browser-profile` / `login-webview-profile`
   两个登录专用 profile 由浏览器 / webview 引擎自管会话数据（含已退出账号
   残留的 PHPSESSID，登出/切号不触碰，下次登录开窗时清理），豁免于
   「config/ 下不得出现 cookie 文件」约束（ADR 0009/0011 既定设计）。
2. CDP 只绑定随机 `127.0.0.1` 端口；拿到 Cookie 后立即经 Browser.close 关闭
   浏览器（3s 宽限后 kill）。
3. 匿名 `/ajax/user/self` 也返回 HTTP 200 与 token；必须以非空 `userData.id`
   作为 Session 有效的权威判据。探测返回 Invalid（401/403/非 200/无
   userData.id）时静默继续轮询——登录跳转尚未完成是正常情况。
4. Cookie 值不得写入日志（登录全程只记录端口 / 状态 / 计数类信息）。

Cookie 域只匹配 `pixiv.net` 或以 `.pixiv.net` 为标签后缀的子域，不接受
`evilpixiv.net`。CDP 单次调用最多 10s，关闭 WebSocket 最多 3s；错误仅显示
方法与数字错误代码，不回显浏览器返回的 message/data。轮询借用响应数组，
避免每轮复制 Cookie/页面树。手动 Session 输入使用 password 遮罩，关闭弹窗
或切回浏览器登录时清空草稿，提交中忽略重复 Enter。

**登录状态检查**（`commands/auth_cmds.rs::auth_status`）：App 启动时以同一套
wreq 指纹调 `/ajax/user/self?lang=zh` 探测 cookie 有效性（整体 2s 超时，返回
`userData.{id, pixivId, name}`）；401/403 → 清空本地 cookie、移除失效账号
（多账号语义见 §5.3）并使会话缓存失效（见下），其余失败（含超时、网络错误）
→ 保留 cookie 仅记日志，UI 显示"未登录"。

**会话缓存失效**（触发点清单与实现见 §5.3）：登录 / 手动登录 / 切换账号 /
退出登录成功（含上述 auth_status 失效清理）后，后端进程级会话缓存——当前
用户自 uid 与 web csrf token（各 30min TTL）——统一失效，下一次请求以新
凭据重建，避免 TTL 内收藏列表仍按旧账号 uid 拼路径、写操作携带旧会话 token。

### 4.2 抓取任务模型（Source + Crawler + TaskManager）

#### Source（`core/sources.rs`，枚举替代旧抽象基类）

```rust
enum NovelSource { Single(i64), Series(i64), User(i64) }
enum IllustSource { Single(i64), User(i64) }   // 插画无系列来源：pixiv 系列插画本身是多页作品
```

`NovelSource::resolve(&PixivApi) -> Vec<(novel_id, series_order)>`：

- Single → `[(id, None)]`
- Series → `/ajax/novel/series_content/{id}` 的 seriesContents 按 contentOrder
  （缺失用 1-based 遍历序兜底）
- User → 先展开 novelSeries 内每个系列（order = 系列内序号，跨系列去重），
  再补不属于任何系列的散篇（order = None，顺序稳定）
- 非法 id（解析失败）跳过

`IllustSource::resolve`：Single → `[id]`；User → `profile/all` 的
illusts + manga（illusts 在前、manga 在后）；`resolve_total` 预取任务分母
（user = 作品总数，single = max(1, pageCount)，失败退回增量计数不中断任务）。

#### Crawler 与任务运行（`core/crawler.rs` / `core/task_manager.rs`）

- `create_task` 校验（category ∈ {novel, illustration}、source_type 合法、
  非空 PHPSESSID、source_id 为正整数；插画来源仅 single/user）通过后：INSERT tasks(pending) → 注册
  `TaskControls` → `tokio::spawn` 后台跑爬虫 → **立即返回 task_id**；
  校验失败不落库。
- `TaskControls`：`tokio::sync::watch` 暂停闸门（`send_replace` 写入，避免
  爬虫尚未 subscribe 时丢暂停请求）+ `AtomicBool` 取消标志；cancel 同时解除
  暂停，正在下载的当前项会下完再退出。
  暂停等待结束后再次检查取消，取消暂停任务不会开始下一项。
- 每项开始前检查取消与暂停（暂停时长不计入超时判定）；已下载
  （is_novel_downloaded / is_illust_downloaded）→ skipped+1 不发事件；
  单项失败 → 计入 failed_ids 不中断；每项成功即写库 + 推 `task://progress`。
- `retry_failed(taskId)`：读原任务 failed_ids，逗号拼接为新任务的 source_id，
  逐 id 以 Single 语义**串行**抓取，**共用新 task_id 累计计数**（修正旧
  Python 版逐 id 独立 run 导致计数互相覆写的 bug）。
- 超时：运行时长（扣除暂停）超过 `max_wait_seconds`（默认 180s）→ 终态
  failed，error = "任务超过最大等待时间（{N}s）"。

#### Task 状态机（V1 简化）

```
pending → running ⇄ paused
              ↓
       done / failed / canceled
```

省略 `pausing` 过渡态，pause 即时生效；取消终态写 **canceled**（修正旧版
被 mark_done 覆写为 done 的 bug）；超时 / 外层异常 → failed。

### 4.3 限速、容错与翻译运行（`pixiv/client.rs`、`agent/`、`translation/`）

#### 小说翻译 Job（Rig 通用 Agent 内核，ADR 0034 / 0035）

翻译以**一篇作品一个 Job**为单位：系列内其他作品各自独立，不合并、不共享术语。
Job 身份 = 作品 + 原文版本 + 目标语言；内置资源版本与设置快照（不含凭据）随 Job
记录。同一作品、同一原文版本、同一目标语言重复启动加入既有 Job，`force` 才新建；
原文已是目标语言且未强制时直接返回「无需翻译」，不创建 Job、不调用模型。

- **阶段**：`prepare_source`（注册整篇原文，建立页 / 行 / 片段映射，按语义边界与字符
  预算切块并生成前后邻接窗口）→ `prepare_constraints`（逐块扫描术语、人物关系、风格
  与对白，聚合并冻结约束版本）→ `translate`（按块派发翻译子 Agent）→ `review`（独立
  审查子 Agent 对照原文、译文与冻结约束）→ `repair`（只返工受影响块并复检）→
  `global_review`（主 Agent 读取摘要与抽样，不把全文塞回上下文）→ `publish`（校验块
  覆盖、审查结论与合并报告后写正式结果）。模型声明完成不构成完成条件，由 Rust 工具
  按实际产物与审查状态校验。
- **状态机**：`queued → running → completed`，运行中可进入 `awaiting_confirmation` /
  `pausing → paused` / `needs_attention` / `cancelling → cancelled` / `interrupted`；
  `deleted` 仅作删除标记。等待确认、暂停、需要处理与中断都可在检查后恢复；应用重启
  只把未结束 Job 改为 `interrupted`，不自动发起付费请求。
- **并发**：模型请求受三级闸门控制——全局并发（默认 4，1~16）、单小说子任务并发
  （默认 4，1~16）、同时活跃小说数（默认 2，1~8）；实际并发取全局与单小说的较小值。
  等待子任务、执行工具与暂停期间不持有模型请求许可，压缩摘要请求同样经过全局闸门；
  一本长篇不能占满全部槽位。同一小说仍串行，不同小说可并行。
- **预算**：整篇维护累计模型尝试预算，未显式配置时按 `32 + 16 × 初始分块数` 自动计算
  （`translation_call_budget = 0`）；父任务、子任务、压缩摘要、传输重试与返工共享。
  预算随检查点与 manifest 持久化，恢复、再次派生或细分都不重置。
- **单轮工具结果预算**：运行层按「工具名 + 参数」预估每个工具调用的结果 token
  （`read_chunk` 按块字符与邻接窗口估算，其余工具不参与）；单轮累计超过
  「可用输入 − 固定开销 − 2048」（下限 1024）时拒绝执行剩余调用，模型收到跳过原因
  并在下一轮分批读取（ADR 0036）。
- **上下文压缩**：只在完整工具批次之间执行，保留 `assistant tool_call + tool result`
  配对；摘要请求计入预算与用量。摘要失败保留原记录并暂停，不做无限「压缩—重试」；
  压缩救不回来时对历史中较早的成功工具结果正文紧急截断（头尾保留 + 中段标注，
  半深迭代到装得下），作为防 `input_too_large` 的最后防线。
  全量原文、译文、术语与审查记录始终从 Job 产物读取，不依赖有损摘要还原。
- **任务续跑**：一轮运行正常结束但完成校验未通过时，带着该轮压缩后历史与最新检查点
  提示自动续跑；连续无进展（磁盘产物无增长，扫描 / 草稿 / 审查 / 冻结 / 发布均计入）
  达 3 次才转 `needs_attention`。任务级推进不受单轮轮次上限约束，总量仍受调用预算与
  模型尝试上限约束；`job_status` 返回 `pending_scan` / `pending_translate` /
  `pending_review` 待办清单，作为续跑后推进的权威依据（ADR 0036）。
- **恢复分类**：明确未发送的连接失败有限重试；限流与暂时性服务错误按退避重试并尊重
  `Retry-After`；鉴权 / 余额 / 模型不存在 / 配置不兼容直接暂停并提示修正；已发送但
  结果未知默认不自动重放；输入超限先压缩或细分；输出截断缩块重做；工具参数 / 结构
  错误有限纠正；质检问题只返工受影响块。
- **暂停 / 取消 / 确认**：暂停停止派发并停在最近检查点；取消立即传播取消信号并保留
  已提交产物；两者都不清预算与重试记录。`translation_confirm_before_start` 开启时在
  冻结约束后进入 `awaiting_confirmation`，浮层可修改术语与风格后「确认并继续」。
- **子 Agent 与工具**：默认一层，主 Agent 可派发翻译 / 审查子 Agent，二者不再派生。
  工具按角色白名单注册（主 Agent：加载资源、准备原文、读取块摘要 / 内容、提交扫描、
  冻结约束、派发翻译 / 审查、读取审查摘要、合并草稿、记录整体复核、发布、按依赖重置
  阶段、查询任务状态；翻译：读取指定块、提交该块译文；审查：读取块与译文、提交审查
  结论）；未注册工具、路径参数与越权调用一律拒绝。工具结果设长度 / token 上限，超出
  时由调用方分页或改传产物引用，不用静默截断掩盖缺失。
- **模型契约**：沿用三协议（Chat Completions 默认 / Responses / Anthropic Messages，
  ADR 0020）、URL 与端点推导、系统代理、单请求超时（默认 600s，ADR 0023）、禁重定向、
  opencode 会话头（按小说 `pixiv-tool-novel-<novel_id>`，ADR 0021）、思考参数按协议
  归一化（ADR 0032）、输出上限分档与默认思考（ADR 0033）。生成请求固定流式
  （ADR 0025）；自动重试只用于明确未发送的连接失败与可重试的限流 / 暂时性错误，
  已发送但结果未知默认不自动重放，避免重复计费。Rig 只负责协议编解码与工具循环，
  HTTP 仍走 wreq，凭据头、代理、超时与错误脱敏与旧实现逐字对齐。
- **持久化与失败不发布**：每个 Job 独占 `data/translations/v2/<novel_id>/<job_id>/`
  目录，原文、分块、草稿、审查、整体复核、会话、检查点与临时文件全部落在其中；所有
  产物「临时文件 + rename」原子提交，草稿 / 审查带版本并拒绝版本回退。覆盖、顺序、
  重复、空值、目标语言或标记保真校验失败时不得发布；仍有未解决块则为 `partial` /
  `needs_attention`，已通过块与旧可读版本不被失败结果覆盖。
- **v1 兼容**：`translation_legacy.rs` 保留旧单页两轮实现，`novel_translation_get` 仍可
  读旧页缓存；`novel_translation_result` 只把 v2 中状态为 `complete` 的结果投影给阅读器，
  没有完整 v2 结果时回退 v1 旧缓存并标记 `partial`，绝不冒充已审查结果；v2 处于
  `partial` / `needs_attention` 时不发布阅读器投影。`novel_translate_page` 命令已移除。

| 参数（翻译运行） | 值 |
|---|---|
| 全局 / 单小说请求并发 | 4 / 4（1~16） |
| 同时活跃小说 | 2（1~8） |
| 单请求超时 | 600s（30~3600，`translation_timeout_seconds`） |
| 自动网络重试 | 2 次（0~5），退避 1s 起、上限 120s，尊重 `Retry-After` |
| 质检返工 / 超限细分 | 2 轮（0~5）/ 3 层（0~5） |
| 单子任务轮次 / 主 Agent 无进展上限 | 24（4~128）/ 8 轮（3~32） |
| 分块 | 目标 3000 字符（500~100000）、上限 6000 字符（1000~200000）、邻接前后各 600 字符 |
| 未知模型上下文 / 预算输出上限 | 32768（16384~2097152）/ 16384（1024~262144，且不超过上下文；默认分块下的实际请求上限同为 16384） |
| 压缩触发 / 近期历史 / 摘要目标 | 可用输入预算 75%（50~90%）/ 4096 / 2048 tokens |
| 累计调用预算 | 0 = 自动 `32 + 16 × 初始分块数`（1~1000000 可自定义） |

运行参数存 `settings.json`（§5.2），一级设置只显示「推荐 / 已修改 N 项」摘要，完整
参数在二级高级页；保存前做交叉校验（分块目标 ≤ 上限、输出上限 ≤ 上下文窗口、近期
历史 + 摘要 ≤ 上下文窗口）。

#### 翻译缓存与清理

- 缓存清理沿用既有 `TranslationLocks` 读写门与翻译服务的运行态检查：只要还有 Job 在
  运行或收尾中，`clear_cache("translation")` 直接拒绝；暂停且已到检查点、等待确认、
  失败与已中断的 Job 允许清理，但清理会**同时删除其恢复数据**。
- 清理成功后递增缓存世代并清空后端索引；迟到回调携带旧世代时写入层直接拒绝，不重建
  已删除的 Job 目录。删除单个 Job 走同一拥有者语义，整棵目录（含 chunks / sessions /
  checkpoints / temporary / output）一起删除，不只删最终译文。

Pixiv 抓取限速与重试参数与翻译互不影响，仍为下表：

| 参数 | 值 |
|---|---|
| 并发 | `tokio::sync::Semaphore(2)`（`CONCURRENCY = 2`） |
| 请求间隔 | 持有信号量期间 `sleep 400ms`（`REQUEST_INTERVAL_MS`），成功与失败路径都执行（≈2.5 req/s 峰值） |
| 单请求超时 | 15s（`REQUEST_TIMEOUT_SECS`） |
| 失败重试 | 最多 3 次（`MAX_RETRIES`），退避 1s → 2s（`RETRY_BACKOFF_SECS = [1,2,4]`，末位 4s 不可达——最后一次失败直接抛 last_err） |
| 429 处理 | `watch` 闸门置暂停 → 该客户端全部请求进入前等闸门，60s（`PAUSE_ON_429_SECS`）后自动恢复；当次不重试，记日志 |
| 立即终止（不重试） | 401/403 → Auth、404 → NotFound、429 → RateLimit |
| 失败队列 | 每任务 `failed_ids` 落库（tasks 表 JSON 数组），任务结束 UI 提示重试 |

请求管线（`run_gated`）：取信号量许可 → 等 429 暂停闸门 → 最多 3 次尝试 →
无论成败在 semaphore 持有期间 sleep 400ms。ajax 响应体 `error` 为真值
（Python 真值语义：null/false/0/""/[]/{} 为假）→ Client 错误，否则取 `body`
字段（缺失返回整个对象）。以上参数**写死为常量**，V1 不暴露给用户配置。

二进制下载保留 wreq 的 `bytes::Bytes` 共享缓冲：原图、ugoira zip 与头像写盘直接
借用字节，图片代理的单飞等待者只复制共享句柄，不再先转换为 `Vec<u8>`。
仅 `pixiv-img` 的 Tauri `Cow<[u8]>` 响应边界转换为独占 `Vec<u8>`；磁盘命中仍
直接返回读盘缓冲。仍是整包接收，不承诺降低单次下载的分配峰值；画质、请求
头、空响应错误、限速与重试保持不变。

下载只允许 pximg HTTPS 子域（443），拒绝 URL 内凭据和非标准端口；下载请求
关闭默认头继承，仅重新附加 UA/Referer/Accept-Language，不发送 Cookie 或
x-csrf-token。网络错误只回传类别及去 query/fragment 的地址，不包含 wreq
原始错误字符串。成功 AJAX body 以移动值返回，避免深克隆整个响应树。

### 4.4 Exporter（输出格式，`core/exporter.rs`）

导出器按配置选择（`create_exporters`）：`txt`（正文原样写入，`[newpage]`
等标记保留）与 `markdown`（行级标记转换）；未知格式忽略、顺序跟随输入、
重复保留。EPUB 为 V2+ 预留（架构预留：格式白名单之外的值直接忽略），无实现代码。

**文件名净化（`sanitize_filename`）**：`[<>:"/\\|?*\x00-\x1f]` 全部替换为
`_`，再 strip 两侧的 `'_. '`。

**Markdown 行级规则**（逐行处理，strip 后判断行首）：

| 输入 | 输出 |
|---|---|
| `[chapter:X]`（整行以 `]` 结尾） | `\n## X\n` |
| 整行 `[newpage]` | `\n---\n` |
| `[jump:` 开头行 | 删除 |
| `[pixivimage:` / `[uploadedimage:` 开头行 | `<!-- 图片占位 -->` |
| 行内 `[rb:A>B]` | `A(B)`（注音转换**已修正生效**——旧 Python 版赋值后未使用） |
| 其余行 | 原样 |

**默认文件命名（`make_filename`，即下述默认模板的产物）**：

| 来源 | 文件名 |
|---|---|
| 单篇 | `<title>_<novelId>.txt/.md` |
| 系列内 | `{zfill(order)}_<title>.txt/.md`，宽度 = page_count ≤ 99 ? 2 位 : 3 位 |
| 插画单页 | `<title>_<artworkId>_p{N}.<ext>`（多页 p0..pN-1；ext 从 URL 末段取，取不到 → bin） |
| ugoira 动图 | `<title>_<artworkId>_ugoira.zip`（原图 = 帧序列 zip） |

标题净化后为空：小说文件名只剩 id 后缀（`_12345.md`），插画以 artwork id
兜底（`safe_title_or_id`）。
插画 URL 扩展名仅接受 1–8 位 ASCII 字母数字，其余用 `bin`，防止路径分隔符或
Windows ADS 注入；正常 jpg/png/gif/zip 等后缀保持不变。

**自定义文件名模板**：settings 三键（见 §5.2），任务创建与失败项重试均在创建
时刻从设置快照取值、随任务参数固定，运行中改设置不影响进行中任务。

| settings.json 键 | 默认模板 | 作用于 |
|---|---|---|
| `illust_filename_template` | `{title}_{pid}_p{page}` | 插画普通页与 ugoira 词干 |
| `novel_filename_template` | `{title}_{nid}` | 小说单篇 |
| `novel_series_filename_template` | `{order}_{title}` | 系列各话（`series_order` 非空时） |

默认模板产物与现行命名 byte 级一致。模板 = 字面文本 + `{占位符}`：

| 占位符 | 含义 | 取值兜底 |
|---|---|---|
| `{user}` | 作者名 | 空 → `user_{uid}` |
| `{uid}` | 作者 ID | 总有值 |
| `{title}` | 作品名（渲染时统一净化） | 净化后空 → 插画用 pid、小说用 nid |
| `{pid}` | 插画 ID（仅插画模板可用） | — |
| `{page}` / `{page1}` | 页序 0 起 / 1 起（仅插画模板可用） | — |
| `{nid}` | 小说 ID（仅小说 / 系列模板可用） | — |
| `{order}` | 系列话序，零填充宽度沿用现行规则（≤ 99 → 2 位，否则 3 位） | 非系列为空值 |
| `{sid}` | 系列 ID | 非系列为空值 |

渲染规则：占位符替换后整体过 `sanitize_filename` 净化（无额外全局 strip 或连续
`_` 收敛）；空值占位符删除该 token 并从紧邻前导字面段末尾吸收 1 个分隔符
（`_` / `-`），避免悬挂分隔符；ugoira 渲染先删除全部 `{page}` / `{page1}` 及其
前导字面段末尾的分隔符（`_p` 整体删、`_` / `-` 各删 1 字符），净化后固定追加
`_ugoira` 词干（`.zip` 扩展名不变）；渲染结果净化后为空 → 回落现行默认命名。
**有意分歧**：模板路径下小说空标题以 nid 兜底（默认模板产 `12345_12345.md`），
现行 `make_filename` 无兜底产 `_12345.md`——默认模板仅在空标题场景与现行命名不同。

校验（前后端一致，前端保存前预校验、后端保存时拒绝）：非空；≤ 200 字符；禁含
`< > : " / \ | ? *` 与 ASCII 控制字符（即不支持借模板建子目录，目录布局仍按
下文）；`{}` 必须配对；占位符必须在对应用途白名单内（插画与小说两套互不可用）；
插画模板必须含 `{page}` 或 `{page1}`（否则多页 p0..pN-1 同名互相覆盖）、系列
模板必须含 `{order}`（同理防各话同名覆盖）。保存时非法模板拒绝且不落盘；手改
settings.json 写入非法模板在加载期回落对应用途的默认并记日志。设置弹窗
「下载」分组输出格式之后为三个文本框（supporting text 列占位符清单，非法时该
字段 error 态红字并阻止整表保存）；无新增 IPC 命令（模板随 `settings_save` 写入）。

**目录布局**：小说统一平铺在输出目录 `novel/` 子目录（系列归属由文件名序号
表达）；插画在 `pic/` 子目录，用户全集再套一层
`pic/users/<sanitize(作者名)>_<userId>/`（拿不到作者名退回 `user_<userId>`）。
多页原图 URL：登录态 `meta.pages[].image_urls.original` 优先，缺失时从 p0
直链按 `_p0.{ext} → _p{N}.{ext}` 推导兜底。

---

## 5. 数据模型

### 5.1 SQLite Schema

文件位置：`data/app.db`

```sql
-- 已抓小说（去重 + 历史浏览 + 更新检测）
CREATE TABLE IF NOT EXISTS novels (
  novel_id          INTEGER PRIMARY KEY,
  title             TEXT NOT NULL,
  series_id         INTEGER,
  series_order      INTEGER,
  author_id         INTEGER NOT NULL,
  author_name       TEXT,
  page_count        INTEGER,
  text_length       INTEGER,
  captured_at       TEXT NOT NULL,         -- ISO8601
  modification_date TEXT,                  -- pixiv 原始字段，检测更新
  txt_path          TEXT,
  md_path           TEXT,
  status            TEXT NOT NULL DEFAULT 'ok'   -- 'ok' | 'failed' | 'partial'
);
CREATE INDEX IF NOT EXISTS idx_novels_series ON novels(series_id);
CREATE INDEX IF NOT EXISTS idx_novels_author ON novels(author_id);

-- 任务进度（断点续传）
CREATE TABLE IF NOT EXISTS tasks (
  task_id     TEXT PRIMARY KEY,            -- uuid
  source_type TEXT NOT NULL,               -- 'single' | 'series' | 'user'
  source_id   TEXT NOT NULL,
  category    TEXT NOT NULL DEFAULT 'novel',  -- 'novel' | 'illustration'
  status      TEXT NOT NULL DEFAULT 'pending',  -- 'pending'|'running'|'paused'|'done'|'failed'|'canceled'
  total       INTEGER DEFAULT 0,
  done        INTEGER DEFAULT 0,
  skipped     INTEGER DEFAULT 0,
  failed_ids  TEXT DEFAULT '[]',           -- JSON array of novel_id / artwork_id
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL,
  error       TEXT
);

-- 已抓插画（去重 + 历史）
CREATE TABLE illustrations (
  artwork_id  INTEGER PRIMARY KEY,
  title       TEXT NOT NULL,
  author_id   INTEGER NOT NULL,
  author_name TEXT,
  illust_type INTEGER DEFAULT 0,           -- 0=插画 1=漫画 2=ugoira
  page_count  INTEGER DEFAULT 1,
  saved_paths TEXT DEFAULT '[]',           -- JSON array of 文件路径
  captured_at TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'ok'
);
CREATE INDEX idx_illustrations_author ON illustrations(author_id);

-- 浏览访问历史（自有浏览 UI 的访问记录；kind='illust'|'manga'|'novel'）
CREATE TABLE IF NOT EXISTS browse_history (
  work_id     INTEGER NOT NULL,
  kind        TEXT    NOT NULL,
  title       TEXT    NOT NULL DEFAULT '',
  author_id   INTEGER NOT NULL DEFAULT 0,
  author_name TEXT    NOT NULL DEFAULT '',
  cover       TEXT    NOT NULL DEFAULT '',
  page_count  INTEGER NOT NULL DEFAULT 0,
  x_restrict  INTEGER NOT NULL DEFAULT 0,
  visited_at  TEXT    NOT NULL,            -- ISO8601，同 kind+work_id 覆写置顶
  PRIMARY KEY (kind, work_id)
);
CREATE INDEX IF NOT EXISTS idx_browse_history_visited ON browse_history(visited_at DESC);
```

### 5.2 配置文件

`config/settings.json`（config 目录位置见 §3.3，键为 snake_case，与旧
Python 版逐字段兼容，`src-tauri/src/settings.rs`）：

```json
{
  "output_dir": "C:\\Users\\<user>\\Downloads\\pixiv-tool",
  "output_formats": ["txt", "markdown"],
  "language": "zh-CN",
  "theme": "auto",
  "theme_color": "pixiv",
  "startup_page": "/browse/home",
  "backend_port": null,
  "max_wait_seconds": 180,
  "show_r18": true,
  "thumb_quality_grid": "medium",
  "thumb_quality_detail": "medium",
  "thumb_quality_fullscreen": "large",
  "image_cache_max_mib": 512,
  "detail_image_scale": 1.0,
  "novel_font_scale": 1.0,
  "novel_bg_color": "",
  "illust_filename_template": "{title}_{pid}_p{page}",
  "novel_filename_template": "{title}_{nid}",
  "novel_series_filename_template": "{order}_{title}",
  "saucenao_api_key": "",
  "translation_api_url": "",
  "translation_api_format": "chat_completions",
  "translation_model": "",
  "translation_target_language": "",
  "translation_timeout_seconds": 600,
  "translation_extra": {},
  "translation_agent_global_concurrency": 4,
  "translation_agent_novel_concurrency": 4,
  "translation_agent_active_novels": 2,
  "translation_agent_retry_limit": 2,
  "translation_agent_backoff_base_ms": 1000,
  "translation_agent_backoff_max_seconds": 120,
  "translation_agent_rework_rounds": 2,
  "translation_agent_split_depth": 3,
  "translation_agent_tool_corrections": 2,
  "translation_agent_max_turns": 24,
  "translation_agent_stall_limit": 8,
  "translation_agent_context_window": 32768,
  "translation_agent_output_cap": 16384,
  "translation_agent_compact_percent": 75,
  "translation_agent_recent_history_tokens": 4096,
  "translation_agent_summary_tokens": 2048,
  "translation_agent_chunk_target_chars": 3000,
  "translation_agent_chunk_max_chars": 6000,
  "translation_agent_context_chars": 600,
  "translation_call_budget": 0,
  "translation_agent_replay_unknown": false,
  "translation_confirm_before_start": false
}
```

- `output_dir` 默认 = **系统下载目录/pixiv-tool**（`~/Downloads/pixiv-tool`，
  跨平台统一实现，见 `src-tauri/src/paths.rs:default_output_dir`）；旧默认值
  字面量 `"downloads"` 在加载时自动迁移为新默认。仍支持用户自填绝对路径或
  相对路径（相对路径锚定 data 目录）。JSON 损坏时备份为
  `settings.json.corrupt-{mtime_ns}` 后重建默认。

- 数据/配置目录策略写死（见 §3.3），不暴露"系统配置目录"切换开关。
- `startup_page`：应用打开默认进入的核心入口，可选发现 `/browse/home`（推荐首页，默认）、关注 `/browse/feed`、我的 `/browse/bookmark`、下载 `/tools/tasks`。设置弹窗「通用」分组选择，保存后下次启动生效；旧配置缺键与手改非法路径回落发现。保存时非字符串 / 白名单外路径拒绝（"应用启动页无效"）。启动先加载设置，再初始化路由：仅无 query 的 `/` 使用该项，明确深链原样打开，旧 `/` 下载预填链接保留 query / hash 并进入下载表单。
- `backend_port`：旧 Python 后端端口配置；Tauri 版无后端进程，仅保留字段
  兼容旧配置文件（仍在 `settings_save` 白名单内），无实际作用。
- `max_wait_seconds`：任务最大运行时长（秒），默认 180，合法区间 30~86400，
  设置弹窗可配；任务运行超过该时长自动标记为 failed（**不含暂停时间**）。
- `theme` / `theme_color`：主题模式（`light`/`dark`/`auto`）与色板（`pixiv` 默认 /
  `indigo` / `jade` / `violet` / `amber`），实现见 §6.3。两个键都在
  `settings_save` 白名单内。
- `thumb_quality_grid` / `thumb_quality_detail` / `thumb_quality_fullscreen`：
  缩略图三档设置，尺寸段与改写规则见 §6.4。三档尺寸段：`small` = `250x250_80_a2`、
  `medium` = `540x540_70`、`large` = `600x1200_90`、`original` = 不改写。取值域
  分别为 grid `small|medium|large`（默认 `medium`）、detail `medium|large|original`
  （默认 `medium`）、fullscreen `large|original`（默认 `large`）。**`thumb_quality_detail`
  的 `medium` 不映射 540 段**，而是「接口 `regular` 原样」——`/img-master/img/..._master1200.jpg`
  不插 `/c/`，与改造前逐字一致；`540x540_70` 只作详情页/全屏「先低清后高清」的占位层，
  不是任何档位的目标。`thumb_quality_fullscreen` 的消费者为全屏浮层（§6.4），
  全屏浮层右侧的胶卷缩略图消费 `thumb_quality_grid` 档（§6.4）。
  手改 settings.json 写入非法档位时，加载期回落到该键默认值（不强制回写文件）。
- `image_cache_max_mib`：图片磁盘缓存上限（MiB），默认 512，合法区间 256~2048
  （预设档 256 MiB / 512 MiB / 1 GiB / 2 GiB + 自定义输入，设置弹窗「浏览与图片」
  分组）。上限在每次图片缓存写入后实时读取；分区淘汰策略与边界行为见
  ADR 0028。手改 settings.json 写入区间外的值时，加载期回落默认 512。
- `show_r18`：全局 R-18 展示开关（默认 `true`）。关闭后所有作品列表（首页/发现/动态/
  搜索/排行榜/收藏/作者页/频道页各板块/相关推荐/小说相关/系列目录行）在渲染期隐藏（频道手动筛选可在该频道覆盖全局档）
  `x_restrict >= 1` 的作品；详情页仍可访问；**关闭开关时**详情页对
  `x_restrict >= 1` 的作品保留模糊遮罩 + 「显示」确认（确认后本会话记忆、不持久化），
  **开启开关（默认）时不显示遮罩、直接展示**。
- `detail_image_scale`：插画/漫画详情页图片宽度占比（默认 `1.0` = 撑满舞台宽度，
  即改造前观感），合法区间 `0.5`~`1.0`（10% 步进）。**设置弹窗不提供该项**，仅由
  插画/漫画详情页（§6.4）顶栏缩放控件经 `settings_save` 写入（在白名单内）。
  保存时非数字 / bool / 越界一律拒绝（"详情页图片缩放必须是 0.5~1.0 之间的数字"）；
  手改 settings.json 写入非有限值或越出区间时，加载期回落到 `1.0`（不强制回写文件）。
- `novel_font_scale`：小说正文字号缩放（默认 `1.0`，合法区间 `0.75`~`2.0`），
  小说阅读器（§6.4）正文渲染使用。**设置弹窗不提供该项**，仅由小说阅读器底栏
  缩放控件经 `settings_save` 写入（在白名单内）。保存时非数字 / bool /
  越界一律拒绝；手改 settings.json 写入非有限值或越出区间时，加载期回落到
  `1.0`（不强制回写文件）。
- `novel_bg_color`：小说阅读背景色，语义键（默认空串 = 跟随主题），合法值为
  `green`（护眼绿）/ `kraft`（牛皮纸）/ `warm`（暖杏）/ `mist`（雾蓝）/
  `blush`（藕粉），纸面 + 墨色配对与派生规则见 DESIGN.md「小说阅读背景色板」。
  **设置弹窗不提供该项**，仅由小说阅读器底栏色块按钮经 `settings_save` 写入
  （在白名单内）。保存时非字符串 / 白名单外的值一律拒绝（"阅读背景色无效"）；
  手改 settings.json 写入白名单外的值时，加载期回落到空串（不强制回写文件）。
- `illust_filename_template` / `novel_filename_template` / `novel_series_filename_template`：
  下载文件名模板（默认 `{title}_{pid}_p{page}` / `{title}_{nid}` / `{order}_{title}`），
  占位符、渲染与校验规则见 §4.4「自定义文件名模板」。保存时非法模板拒绝（不落盘）；
  手改 settings.json 写入非法值时加载期回落对应用途的默认模板（不强制回写文件）。
- `saucenao_api_key`：SauceNAO API Key（默认 `""`），「以图识图」（§7
  `saucenao_search`）的**必填前置**：trim 后为空一律拒绝发起搜索。在
  saucenao.com 免费注册后于 `user.php?page=search-api` 页面获取；仅保存在本机
  settings.json——不入库、不写日志、不进报错原文。接口行为与错误形态见
  `docs/research/saucenao-api.md`。
- `translation_api_url` / `translation_api_format` / `translation_model` / `translation_target_language` / `translation_extra`：小说翻译 API 基址或当前协议的完整端点、接口协议（`chat_completions` 默认 / `responses` / `anthropic`，白名单外加载期回落默认）、模型 ID、目标语言 code（白名单见 §7 `novel_translation_start`；空串 = 跟随界面语言 `language`）、自定义请求体 JSON 对象，默认空字符串/`chat_completions`/空字符串/空字符串/空对象。URL 按协议推导端点后缀（`/chat/completions`、`/responses`、`/messages`），`/models` 同源推导；OpenAI 系凭据走 Bearer，Anthropic 走 `x-api-key` + `anthropic-version: 2023-06-01`，请求体与响应解析随协议装配（输出上限按输入字符数分档：≤6k 字符 16,384、6k~24k 65,536、>24k 262,144（对齐现代模型输出上限，见 ADR 0033），字段随协议为 `max_completion_tokens`/`max_output_tokens`/`max_tokens`，高级 JSON 显式给出的上限优先；思考默认开启，三协议分别注入 `reasoning_effort: medium`、`reasoning: {effort: medium, summary: auto}`、`thinking` budget 4096，用户显式给出的思考字段优先、`none`/`disabled` 关闭。见 ADR 0033）。高级 JSON 支持 reasoning_effort、max_completion_tokens、供应商 thinking 等扩展；按协议归一化后再装配（ADR 0032）：Responses 把 `reasoning_effort` 移入 `reasoning.effort` 并补 `reasoning.summary: "auto"`、`max_completion_tokens`/`max_tokens` 改名 `max_output_tokens`、剔除 `stream_options`；Anthropic 把 `reasoning_effort`（或 OpenAI 形状 `reasoning.effort`）转成 `thinking: {type: enabled, budget_tokens}`、`max_completion_tokens`/`max_output_tokens` 改名 `max_tokens`、剔除 `stream_options` 与 `reasoning`；用户显式给出的目标协议字段（含 `thinking`）优先；Anthropic 开启思考时 `temperature` 归一为 1、`top_p` < 0.95 剔除（该协议硬约束）。保留字段按协议追加（responses 禁 `input`/`instructions`，anthropic 禁 `system`），不允许覆盖；`stream_options` 允许覆盖（个别不兼容网关关闭 Chat Completions 的默认用量上报注入，ADR 0030）。见 ADR 0018、ADR 0020、ADR 0034。JSON 上限 16KB，模型 ID 上限 256 字节。HTTP 仅允许本机模型，其他须 HTTPS；禁重定向、URL 凭据/query/fragment。
- `translation_timeout_seconds`：小说翻译**单次模型请求**超时（秒），默认 `600`（10 分钟，
  取代旧的写死 180s），合法区间 30~3600；位于二级「高级设置 · 模型能力与上下文」，不再
  占用一级表单（ADR 0034 / 0035 的设置分层）。
  整篇 Job 会发起多次请求，超时按每次请求计算；超时可用的错误文案跟随该值报出实际秒数。
  「检测可用」按草稿里的同名字段（缺省 600）计时，「获取模型列表」固定 30 秒。
  手改 settings.json 写入区间外的值（含 0/负数）时加载期回落默认。见 ADR 0023。
- **翻译运行参数（22 个键；仅 `translation_confirm_before_start` 留在一级，其余在二级「高级设置」）**：`TRANSLATION_AGENT_INT_RULES`
  （`settings.rs`）是区间与默认值的唯一真相源，布尔键见 `TRANSLATION_AGENT_BOOL_KEYS`。
  一级「小说翻译」只放连接与常用项；下列参数收在「高级设置」二级页，一级入口显示
  「推荐 / 已修改 N 项」摘要与错误计数。

  | 键 | 默认 | 区间 | 含义 |
  |---|---|---|---|
  | `translation_agent_global_concurrency` | 4 | 1~16 | 全局模型请求并发（所有小说共用） |
  | `translation_agent_novel_concurrency` | 4 | 1~16 | 单小说子任务并发（受全局上限约束） |
  | `translation_agent_active_novels` | 2 | 1~8 | 同时活跃小说数，其余排队 |
  | `translation_agent_retry_limit` | 2 | 0~5 | 自动网络重试次数（不含首次） |
  | `translation_agent_backoff_base_ms` | 1000 | 500~30000 | 自动退避基础延迟（毫秒） |
  | `translation_agent_backoff_max_seconds` | 120 | 10~600 | 自动退避上限（秒） |
  | `translation_agent_rework_rounds` | 2 | 0~5 | 质检返工轮数（修复 + 复检） |
  | `translation_agent_split_depth` | 3 | 0~5 | 超限细分深度 |
  | `translation_agent_tool_corrections` | 2 | 0~3 | 工具参数 / 结构纠正次数 |
  | `translation_agent_max_turns` | 24 | 4~128 | 单子任务模型轮次 |
  | `translation_agent_stall_limit` | 8 | 3~32 | 主 Agent 无进展上限（轮） |
  | `translation_agent_context_window` | 32768 | 16384~2097152 | 未知模型上下文窗口（token） |
  | `translation_agent_output_cap` | 16384 | 1024~262144 | 预算器使用的单请求输出上限（且不超过上下文窗口）；实际请求上限仍按分块输入分档，高级 JSON 显式值优先 |
  | `translation_agent_compact_percent` | 75 | 50~90 | 自动压缩触发（可用输入预算百分比） |
  | `translation_agent_recent_history_tokens` | 4096 | 512~131072 | 保留近期历史（token） |
  | `translation_agent_summary_tokens` | 2048 | 256~65536 | 摘要目标（token） |
  | `translation_agent_chunk_target_chars` | 3000 | 500~100000 | 分块目标字符数（且不超过字符上限） |
  | `translation_agent_chunk_max_chars` | 6000 | 1000~200000 | 分块字符上限 |
  | `translation_agent_context_chars` | 600 | 0~20000 | 邻接参考窗口（前后各多少字符） |
  | `translation_call_budget` | 0 | 0~1000000 | 整篇累计模型调用预算；0 = 自动 `32 + 16 × 初始分块数` |
  | `translation_agent_replay_unknown` | false | 布尔 | 请求结果未知时是否自动重放（可能重复计费） |
  | `translation_confirm_before_start` | false | 布尔 | 启动前确认术语与风格（浮层可对本次任务覆盖） |

  保存前交叉校验：分块目标 ≤ 上限、输出上限 ≤ 上下文窗口、近期历史 + 摘要目标 ≤
  上下文窗口；越界或冲突拒绝保存并定位到二级分类字段。非法值在加载期回落到默认。
  `translation_timeout_seconds`（见上条）同样归二级「模型能力与上下文」分类，但不属于
  本表 22 个 Agent 运行参数键。
- 以上各键在 `settings_save` 白名单内（`settings_cmds.rs` 的 `WRITABLE_KEYS` 共 48 个键，
  含兼容保留的 `backend_port`；`saucenao_api_key` 为用户凭据，见 §7）。`translation_api_key` 是独立写入参数，不进 Settings 或文件；省略保持，空串删除。`settings_get` 额外返回 `translation_key_configured` 布尔值与 `translation_key_error` 安全文案（凭据库不可用不影响其他设置），不返回 Key 原文。

### 5.3 Cookie 存储（`src-tauri/src/cookies.rs` / `src-tauri/src/accounts.rs`）

小说翻译凭据独立于登录态：keyring service `pixiv-tool`、account `novel-translation-api-key`，仅系统凭据存储；翻译请求不带 Pixiv Cookie，不写 config/data/日志或报错，见 ADR 0034 / 0035（凭据落点沿用 ADR 0017）。

- 统一走 **keyring crate**（keyring 3）：service `pixiv-tool.cookies`、
  account `default`、value 为 cookies map 的紧凑 JSON（含 PHPSESSID /
  x-csrf-token 等）
- Windows：系统 Credential Manager（windows-native；旧版 DPAPI 文件
  cookies.dat 不再使用）
- macOS：系统 Keychain（apple-native；service/account 与旧 Python 版一致，
  登录态互读兼容）
- Linux：Secret Service / kernel keyutils（linux-native-sync-persistent；
  无 Secret Service 时读写返回错误文案；未实机验证）
- 超长 JSON 仅在 **Windows**（Credential Manager 单条 blob 上限 2560 字节）
  自动分片为 `default.p1..pN`（头条目提交点最后写，读到头才算有效）；
  macOS / Linux 单条目存储——每个分片是独立 keychain 条目（各自 ACL），
  分片会在重编译/重打包后把授权弹窗按条目数放大（弹窗治理，见 PACKAGING）
  Windows 分片按 UTF-16 单元计数（每片最多 1200），不拆非 BMP 字符。
- **多账号**（ADR 0010）：`default` 条目恒为**当前激活账号的镜像**，
  抓取客户端 / auth_status / webview 自动注入等读取方零感知；每账号另存
  独立条目 `u-<user_id>`（分片规则同上）；账号索引
  `config/accounts.json` **只存用户元信息**（user_id / pixiv_id / name /
  profile_img 原始头像 URL / avatar_file 本地缓存文件名 / saved_at 登记时间），
  **任何 cookie 都不落 config**（豁免：`login-browser-profile` /
  `login-webview-profile` 两个登录专用目录由浏览器 / webview 引擎自管会话
  数据，属 ADR 0009/0011 既定设计，登出/切号不触碰，下次登录开窗时清理）。
  显式登录时双写镜像/账号条目，auth_status 只刷新
  索引元信息（旧单账号首次校验仍补建账号条目），避免启动时重复访问 Keychain；
  退出登录移除当前账号（镜像 + 条目 + 索引项），有剩余账号时自动激活列表
  中首个账号（回退任一步失败降级为记日志按纯登出返回，响应
  `fallback_applied:false`——此刻 default 已清、active 已空，后端真实状态
  就是未登录；回退账号凭据确定缺失时移出列表，维持「列表中的账号皆可切换」）；
  auth_status 探测确定失效（401/403）时同样移除，避免死账号

账号移除同步驱逐内存中的 CookieStore 实例；切换前检查账号已登记，避免任意
user_id 创建无界凭据缓存。实例创建与插入在同一次短锁内完成。

**会话缓存失效**：登录 / 手动登录 / 登出（含回退降级）/ 切换账号与
auth_status 失效清理在凭据变更成功后统一调用
`pixiv::browse_api::invalidate_session_caches()`，清空进程级的自 uid 缓存与
主站 csrf token 缓存（各 30min TTL）——否则切换账号后收藏列表 / 标签仍按
旧账号 uid 拼路径、写操作仍带旧会话 token，最长残留一个 TTL。主站 csrf
token 的写路径自愈覆盖全部 POST 写方法（Auth/Client 失败或 200+error 信封
一律清缓存重取）。`auth_logout` / `auth_account_switch` 在存在非终态抓取
任务时附带可选 `warning`（任务沿用创建时账号凭据快照执行，重试才读当前
账号），由前端提示。

---

## 6. 前端

### 6.1 页面（四个核心导航入口）

应用无参数启动默认进入「发现」推荐首页（`/browse/home`）；可在设置中更换启动入口（§5.2 `startup_page`）。
离线 `/tests/startup-viewer.html` 验证四个启动入口、旧下载预填与深链，以及全屏滚轮 / PgUp / PgDn 的单页与双页步进、胶卷滚动、首尾边界和退出焦点；不写真实配置、不访问 Pixiv。

| 页面 | 路由 | 必需 |
|---|---|---|
| 小说下载表单 | 全局非模态面板（旧 `/tools/novel` 兼容返填到任务页面板） | ✅ |
| 插画下载表单 | 全局非模态面板（旧 `/tools/illustration` 兼容返填到任务页面板） | ✅ |
| 工具-任务 | `/tools/tasks`（页签「任务」） | ✅ |
| 工具-历史 | `/tools/history`（页签「历史」） | ✅ |
| 浏览-首页 | `/browse/home`（推荐流，换一批去重追加） | ✅ |
| 浏览-频道 | `/browse/illustration` `/browse/manga` `/browse/novel`（关注新作/推荐/排行/热门标签板块 + 顶部 R-18 快捷筛选（全部/一般向/R18）） | ✅ |
| 浏览-发现 | `/browse/discover`（按历史推荐，前端去重无限滚动） | ✅ |
| 浏览-动态 | `/browse/feed`（关注的新作品：插画/小说 × 全部/R-18） | ✅ |
| 浏览-搜索 | `/browse/search`（类型 tab + 排序/对象/匹配 + ID/链接直达；纯数字 ID 按类型 tab 跳插画/漫画/小说详情，链接形态自带类型不受 tab 影响；链接支持作品、用户主页、小说系列与插画/漫画系列，自绘页面增加时通过 `parseInput.ts` 的识别类型/规则及搜索页 `pushTarget` 扩展，跳转分支由 TypeScript 穷尽检查约束；页码分页 20/页（接口每页 60 条切 3 页）+ 本页排序（点赞/收藏/浏览维度下拉 + 独立升/降切换，优先复用列表已有计数与详情缓存，仅所选维度缺失时补取；零值有效、未知项垫底）） | ✅ |
| 浏览-排行榜 | `/browse/ranking`（插画/漫画/动图/小说 × 周期 + 日期导航） | ✅ |
| 浏览-收藏 | `/browse/bookmark`（插画·漫画/小说 × 公开/私密 + 标签筛选） | ✅ |
| 浏览-历史 | `/browse/history`（浏览访问历史：作品级访问记录网格回显 + 类别筛选 + 分页 + 一键清空） | ✅ |
| 浏览-主页 | `/browse/me`（当前账号作者页复用：资料 + 插画/漫画/小说/收藏 tab；动作组「编辑资料 / 投稿插画作品」经作者页工具行 `lead` 插槽与刷新同行（组在左、刷新贴右）跳官方网页；未登录给登录引导；收藏 tab 为作者页的「公开收藏」语义，含私密的完整收藏仍见 /browse/bookmark） | ✅ |
| 作品查看器 | `/browse/work/illust|:kind=illust|manga>/:id`（多页纵向渐进加载、点击放大进入全屏翻页 + 胶卷缩略图、R-18 遮罩（仅关闭 show_r18 时）、相关推荐 / 评论面板（顶栏评论按钮切换）） | ✅ |
| 小说阅读器 | `/browse/work/novel/:id`（标记渲染、分页、系列导航、相关推荐 / 评论面板；整篇翻译 Job、术语确认、块预览与问题列表、原文/译文/双语切换） | ✅ |
| 系列目录 | `/browse/series/:id`（游标加载） | ✅ |
| 作者页 | `/browse/user/:id`（资料 + 插画/漫画/小说/收藏 tab；工具行首部预留可选 `lead` 插槽，供 `/browse/me` 的动作组与刷新同行；「主页」复用本页组件，见上） | ✅ |
| 以图识图 | `/saucenao`（SauceNAO 反搜：本地文件/拖拽/URL，pixiv 结果跳作品详情；需在设置中配置 API Key） | ✅ |

**下载页**：页签壳 ToolsView（/tools），默认 /tools/tasks，页签为任务 / 下载历史，右侧新建下载打开面板。旧表单路径重定向到任务页并带 downloadForm/sourceType/sourceId，面板消费后清理这三个 query 键，其余 query/hash 保留。

**侧边栏**：发现 / 关注 / 我的 / 下载四个核心项，二级入口在各页面内逐步展开；所属分组高亮，详情继承实际来路，直达归发现。展开宽 256px、折叠宽 72px，头部为 logo / 标题 / 收起按钮，窗口高度不足时导航自身滚动。侧栏底部为**头像 chip**
（头像 + 账号名 + 展开箭头；未登录显示「账号」占位），点击在 chip 上方弹出**账号
菜单**（`AccountMenu`：轻量 popover，宽 264px、surface-container 底、12px 圆角、
既有轻阴影；透明遮罩点击外部或 Esc 关闭，无深色 scrim）；内容 = 账号列表（当前
账号 ✓，点击切换）/ 添加账号（复用 LoginDialog）+ 分隔线 + **「设置」入口**（其
下同分组附「检查更新」入口，行为见 §7）；
「设置」与**退出登录**（danger 色，未登录置灰；点击后经原生 confirm 弹窗确认才
执行）之间以分隔线隔出 meta 行：左对齐**版本回显**（`getVersion()` 动态读取
app 版本，读不到显示 `--`）+ 右对齐 **GitHub 主页入口**（图标按钮，关菜单后经
`opener` 插件用系统默认浏览器打开仓库主页）。设置入口点击后关菜单并打开**设置弹窗**
（`SettingsDialog`：原生 dialog、宽 `min(840px, 94vw)`、高 `min(680px, 88vh)` 定高
（超出部分在右栏内部滚动，头部与底部保存栏不动）；主区为 176px 分组栏 + 表单区两列，
两栏各自滚动；底部为常驻「取消 / 保存」栏，保存整表一次写入）。表单为 SettingsPanel，按左栏选中的分组渲染
（分组顺序见 `components/settings/sections.ts`：通用 / 外观 / 下载 / 浏览与图片 /
小说翻译 / 维护），含主题、配色与语言的实时预览；Esc / 点 backdrop / 标题栏 ✕ 关闭前先做脏检查，
有未保存改动则弹确认弹窗（继续编辑 / 放弃修改），放弃即回滚到已保存值并提示，
无改动直接关闭。设置项继续增多时在 `sections.ts` 与 SettingsPanel 内新增分组。
「小说翻译」组内分一级常用页与二级「高级设置」页（`TranslationAdvancedPanel.vue`）：
一级只放协议、URL、Key、模型、目标语言、启动前确认、检测可用与高级入口，入口显示
「推荐 / 已修改 N 项」摘要与错误计数；二级页按「模型能力与上下文 / 并发与任务预算 /
重试与质检 / 分块与前后文 / 请求扩展 / 诊断」先给分类入口，再进入分类详情；入口页
顶部「返回小说翻译」回一级，分类详情顶部「返回分类」回入口。改动沿用同一份草稿与
底部保存，页面切换不保存、不丢草稿。保存失败时自动切到对应二级分类并滚动聚焦出错
字段；各分类与二级页提供「恢复本组推荐值 / 恢复全部推荐值」（恢复全部就地二次确认，
不叠新模态）。详细诊断开关只影响本次运行，不写 settings.json。
应用外壳恒为视口高，右侧内容区是主要滚动容器，长内容不再拉长侧栏；下载面板独立滚动，底部状态栏独立占位。内容列（`.page-view`）在窗口宽高比 ≤16:9 时铺满内容区、不设宽度上限（列表/网格列数随
宽度自适应补满），只有更宽的超宽窗口才按「视口高 × 16/9」封顶并居中
（视觉规格见 `DESIGN.md` §Layout）。

浏览模式详见 §6.4 与 ADR 0012。

**返回堆栈**：跨页面跳转（包含相关推荐、作者、系列、标签搜索、频道排行、下载页、
工具页签与侧栏）使用 Vue Router 的原生 hash history `push`，按实际来路逐层返回，
保留上游 URL 的 query/hash；仅当前页面筛选同步、表单消费预填参数和旧路径重定向
使用 `replace`。非作品页面复用 `PageBackButton` 图标按钮：普通列表和以图识图并入
标题行，作者页放在头像资料区左侧，系列页并入系列标题行，工具页并入分区标题行；插画/漫画
与小说详情沿用各自顶栏返回按钮，全部调用路由层的 `goBack()`。没有有效应用内
上游历史的深链页面以 `replace` 回所属核心入口，核心入口再回 `/browse/home`，避免兜底形成来回循环；首页无
历史时隐藏返回入口。加载、空态和错误态均保留返回入口，骨架的 `aria-hidden` 不覆盖
返回控件。缓存恢复规则见 §6.4。
浏览器 mock 回归入口：先运行 `.\dev.ps1 frontend start`，在新标签打开
`http://localhost:9961/tests/navigation.html#/browse/home`，检查底部 `PASS`；覆盖实际
首页卡片→详情→作者、相关推荐、跨页面多层返回、query、旧路径重定向、深链兜底与新分支。
新增工作区离线验收入口 `/tests/workspace.html#/browse/home`，覆盖四入口、草稿确认、返填不跳页、缓存返回、搜索聚焦、旧链接、单一任务监控与轮询、失败及重复提交防护。
同一检查验证返回控件并入标题/资料行、可访问名称及 640px 窄窗长标题布局。

### 6.1.1 核心导航与下载工作区

**按屏滚动**：普通可纵向滚动的数据区（图片舞台 / 小说正文 / 列表 / 信息与评论 / 下载面板 / 侧栏 / 设置弹窗）统一支持 PgUp/PgDn 按屏滚动：鼠标所在位置的最内层实际滚动容器优先，无鼠标目标时跟随焦点，最后兜底主内容；按 Chromium/Windows 规则每次滚动容器可见高度的 87.5%，保留阅读重叠。首尾不循环、不串动相邻或父滚动区；模态窗口内不滚动背景。输入、下拉、滑杆、可编辑内容、已消费的按键及 Ctrl/Meta/Alt/Shift 组合交给原生控件或系统。全屏图片区仍按作品页 / 双页组切换，鼠标或焦点落在可滚动胶卷时 PgUp/PgDn 只滚胶卷。无新增视觉控件、颜色或动效。
实现集中于 `composables/usePageScroll.ts`，App 统一安装与清理监听，不逐页复制按键逻辑。shadow 控件沿 composedPath 识别，滚动区域只消费无修饰键的 PgUp/PgDn；普通区域滚屏不切换数据页码。离线 `/tests/page-scroll.html` 验证多滚动区、焦点兜底、边界、shadow 输入与模态隔离；`/tests/startup-viewer.html` 同时覆盖非全屏图片与全屏胶卷。滚动步进依据：[Chromium ScrollUtils](https://chromium.googlesource.com/chromium/src/+/HEAD/cc/input/scroll_utils.h)。

作者资料行在昵称/统计与网页按钮之间提供关注按钮：未关注为 md-filled-button「关注」，已关注为 md-outlined-button「已关注」（tooltip/aria-label 为取消关注），40px 既有控件密度。公开关注，点击已关注直接取消；成功后回写 is_followed 并 Snackbar 提示，失败保持原状态。提交中禁用、防重并 aria-busy；未知状态禁用并提示刷新，本人资料不显示关注自己。按钮与网页入口为 8px 间距动作组，窄窗随资料区落行。账号切换或作者对象变化后忽略旧请求回显，刷新不与关注操作并发，不重载作品列表。 作者资料响应新增可选 is_followed，映射 Pixiv isFollowed；缺失不推断为未关注。

列表页按「返回与分区标题 + 右侧操作 → 分区导航 → 本页筛选 → 内容」组织。首页/发现、关注、我的分别以核心分区为标题，避免重复当前导航名称；频道保留具体标题。二级导航位于各列表页内，沿内容左边缘对齐，用原生路由链接胶囊标明当前页，发现的频道入口以细分隔线分组；详情不额外堆叠分区导航，侧栏仍继承来路。导航组间 16px，导航到下一区域 24px；页内筛选采用按内容定宽的 Material tabs，禁止均分整页宽度。关注类型与范围并排、窄窗自然换行；作者页作品工具行另提供排序切换（默认最新在前，语义与记忆见 §6.4「作者页作品排序」）。下载由父页面提供唯一标题和新建入口，下方为按内容定宽的任务/历史页签；任务类型筛选与批量管理左右分组，窄窗换行，清理完成任务并入管理组。

**页头吸顶**：列表型页面的前三段（返回与标题、右侧操作 / 分区导航 / 本页筛选与工具行）合为一个吸顶页头，固定在主内容滚动区顶部，滚动时只有下方内容移动；错误提示、骨架、结果计数、列表与分页属内容，随内容滚动。覆盖首页 / 发现 / 插画·漫画·小说频道 / 关注 / 追更 / 排行 / 收藏 / 浏览历史 / 搜索 / 作者作品 / 未登录主页 / 系列分集（标题行与目录工具行）与下载分区壳（标题与任务·历史页签）；下载分区的子页面（任务类型筛选行、下载历史工具行）以次级吸顶条再吸附在页头下沿。小说阅读器与插画/漫画详情页是定高 flex 壳、自带内部滚动层与常驻顶栏，以图识图页是单次表单，均不参与。滚动容器上内距在含页头的页面归零并移入页头，以免 sticky 吸附矩形被内缩后留缝；页头用不透明 surface 底与 1px outline 30% 下缘分隔线（同阅读器顶栏与吸底翻页器配方），不用阴影。层级低于吸底翻页器与全部浮层。页内 `scrollIntoView` 的目标按页头实测高度让位；页面切筛选 / 切页 / 换作品后的回到页首统一走 `utils/scroll.ts`，主内容滚动不在 window 上。视觉配方见 DESIGN.md「页头吸顶」与 `.impeccable/design.json`。

收藏标签从左侧固定栏改为横向换行工具带，按内容占位、最多三行后内部滚动；全部/未分类/标签计数、筛选契约、刷新与取消收藏行为保持不变，作品网格使用全宽。

现行决策见 ADR 0015。侧栏四入口发现（/browse/home）、关注（/browse/feed）、我的（/browse/bookmark）、下载（/tools/tasks）；二级导航组织现有路由，作者 / 系列 / 详情继承实际来源分组，直达归发现。「我的」分区二级入口为收藏 / 浏览历史 / 主页（`/browse/me`，复用作者页组件、归 library 分组，无返回历史时兜底回收藏页）。56px 圆形放大镜按钮（无文字，保留可访问名称与 tooltip）+ Ctrl+K / Command+K 跳搜索并聚焦；按钮显示时分页行右侧预留 72px 空间以避免遮挡，以图识图入口在搜索页；阅读器改用顶栏搜索，全屏看图隐藏全局搜索与状态栏。

下载改为应用级非模态布局面板，宽工作区 ≥1120px 右栏 480px，否则底栏 min(45%,320px)，只放新建表单，小说/插画页签背景继承面板 surface-container。公共 fillDownloadForm(DownloadTarget) 不再导航，所有返填沿用 form/sourceType/sourceId 类型。关闭 / 跳页留草稿，账号变更清空；覆盖手动来源需确认、格式保留；失败留输入，成功更新任务并关闭面板、不跳页。旧 /、/illustration、/tools/novel、/tools/illustration 进入 /tools/tasks 并打开相应面板，保留其它 query/hash，消费 downloadForm/sourceType/sourceId；旧 /tasks、/history 继续兼容。

底部 32px 状态栏常驻且独立占位，空闲、新建入口、活跃任务数及代表任务进度回显；点击进度进入完整任务页。代表优先 running/pending/paused、同状态创建时间升序；done/total，未知总量不定进度，暂停无动画。应用级共享 task://progress、task://done 订阅与 2s 轮询兜底（有活跃任务或同步失败时）；同步失败保留快照并显示重试提示，任务页不重复订阅。

路由返回保留真实来路，无历史 replace 回所属核心入口；面板开关不入历史。原有浏览 KeepAlive 20 页 LRU、账号失效与首页快照不变；面板不重挂路由视图，布局切换恢复可见卡锚点 / 滚动。刷新按钮及 Ctrl+R / Command+R 保留，面板字段聚焦不刷新底层列表。无 Rust IPC / SQLite / settings.json / Pixiv API 变更；UI 规则见 DESIGN 与结构化伴随文件。

返回操作和历史可用性判定统一放在 `router/navigation.ts`，显式传入页面 `useRouter()` 的实际实例；禁止返回控件引用路由模块单例。避免热更新后单例与应用注入实例分离，造成地址变化而视图不返回。公共返回按钮、图片详情正常/错误态与 Escape、小说顶栏均使用同一逻辑；可用历史须指向已注册应用路由。按钮可见性及 tooltip 随完整路由变化重新判定，不缓存非响应式 HistoryState 的旧值。离线 `/tests/navigation-context.html#/browse/home` 使用与模块不同的应用路由实例复现该回归，配合 navigation 与 workspace 验收覆盖逐层来路、query、分支、旧链接及缓存位置。

### 6.2 i18n

- 框架：**vue-i18n**
- 语言：**简体中文（默认，`zh-CN`）+ 英文（`en-US`）+ 日语（`ja-JP`）**
- 文件：`frontend/src/locales/zh-CN.ts` / `en-US.ts` / `ja-JP.ts`；三份词库的键与插值保持一致，日语以中文词库的递归类型约束结构，完整提供帮助、确认与状态文案。
- 日语遵循日本 Pixiv 用户常用表达：イラスト / マンガ / 小説 / うごイラ / ブックマーク / フォロー / 全年齢 / マイピク；共享设定集称「設定資料・用語集」。语言选项均显示「日本語」。
- 原生应用菜单的退出项与编辑菜单标题按已保存语言在启动时初始化，切换后重启生效；系统预定义编辑项沿用系统语言。小说翻译目标选择「跟随界面语言」时，`ja-JP` 归一为 `ja`，原文已是日语时沿用无需翻译与强制翻译规则。
- 设置弹窗可切；启动期以 `localStorage["pixiv-tool-lang"]` 为准，settings.json 的
  `language` 加载后回填并向 localStorage 同步（两处同写，避免首屏语言闪变）

### 6.3 主题

全局静态帮助采用公共 `components/common/HelpTooltip.vue`：设置六组的字段/分组说明、登录方式与 Session 获取步骤、搜索链接/ID 规则、以图识图介绍/使用说明默认收进相邻帮助图标。悬停、聚焦或点击展开，role=tooltip 与 aria-describedby 保留可访问关联，原生 popover top layer 不受对话框滚动区裁切，按视口空间定位/换行；焦点留在触发器，指针可移入阅读，Esc 优先关闭说明，外部点击/失焦/滚动/resize 收起。校验错误、凭据库错误、进度、空态引导、未配置状态和确认后果保留直显，维护组保留立即生效的短提示。视觉与交互细节同步 DESIGN.md / .impeccable/design.json。

维护组按缓存与日志直排分区，显示分类/总大小、确认后清理动作和最近日志；分类冗长说明收进分类旁帮助图标（HelpTooltip），路径不直接回显——由 `components/settings/CopyPathButton.vue` 复制图标承载，悬停/聚焦 tooltip 显示完整路径，点击写入剪贴板并以 Snackbar 提示、图标短暂切换为对勾。14px 标题与 12px 辅助文字沿用既有 token；日志为可聚焦的等宽纯文本滚动区，最多 240px 高，不对轮询使用 aria-live。位于末尾时跟随新日志，手动向上阅读时保留位置；空缓存/日志禁用清理，读取失败保留快照与重试；维护页关闭即停止轮询，语义及安全边界见 §9 / ADR 0027。视觉细节见 DESIGN.md / .impeccable/design.json。

小说翻译复用既有视觉角色：原文 ink，逐段译文 primary（纸色模式经 `--translation-ink` 与纸面墨色混色——浅色主题 primary 75%、深色主题 25%，保 4.5:1 对比度且与原文异色）。原分页底栏的 SVG 翻译图标点击弹出 **384px 浮层**，头部显示「整篇作品 · N 页 · M 块」、8px 状态点与 12px 状态摘要（`role="status" aria-live="polite"`，唯一播报区）；下方为四个页签：**状态**（阶段、已译/已审/待修/总块、进度条、当前页/块、已用时间、token 用量与单行流式预览——思考与摘要共用一行，思考期 ink-subtle 静态 ellipsis、正文摘要到达即取代（ink-muted，溢出 marquee），末尾思考不驻留、思考期间亦持续有内容在动，见 ADR 0030）、**术语与风格**（等待确认时可编辑术语与译注并「确认并继续」，否则只读展示冻结版本、冲突与未解决项）、**预览**（块列表 + 选中块原文/译文对照，未译块与未审查块明确标示，邻接上下文可读）、**问题**（审查问题按未解决块优先、严重度排序，只显示块/片段定位与说明）。底部常驻显示模式切换（原文 / 译文 / 双语，secondary-container 选中）与动作区（开始 / 重新翻译、继续、暂停、取消；同一时刻只显示当前状态可用的动作）。关闭浮层（Esc / 外部点击）只关面板、不取消任务；重新进入或从底栏 / 侧栏回到在途小说时自动展开一次。打开焦点进入面板、关闭回到图标，Tab 条支持 ←/→/Home/End。状态摘要是唯一 aria-live 区，进度、已用、字节、token 与逐帧预览不播报。设置页「小说翻译」一级按服务连接（接口协议 / API URL / API Key）与模型和语言（模型 ID / 目标语言）直排，另有启动前确认、检测可用与「高级设置」入口（摘要 + 错误计数）；模型 ID 在「获取模型」成功后原位变为下拉（选项含获取结果与当前值，可切回手动输入），「检测可用」以绿色/红色文字区分成败；目标语言默认「跟随界面语言」（选项回显当前界面语言名），可手动指定白名单内的语言；接口协议默认 Chat Completions，切换后 URL 帮助与占位、模型列表和探测草稿同步换协议，已存凭据在 Key 标签行以中性胶囊回显「已保存」/「保存时清除」，清除动作与 Key 字段同行、窄窗换行；原文语言与目标语言一致时只回显「无需翻译」提示（灰色指示点、不算失败），按钮变为「仍然翻译」可强制走完整流程；≤800px 收缩进度滑杆、≤600px 底栏分两行避免重叠。浮层与二级设置页的视觉细节详见 DESIGN.md 与结构化伴随视图。

所有纵向数据滚动区共用 PgUp/PgDn 输入规则（§6.1.1），优先鼠标区域、回退键盘焦点，保留控件原生按键与模态隔离；全屏胶卷与图片区分别滚屏 / 切作品页。样式及交互规则与 DESIGN.md / .impeccable/design.json 保持一致。

设置「通用」新增启动页 md-outlined-select（发现 / 关注 / 我的 / 下载，默认发现），沿用 40px 控件密度和整表保存 / 取消语义，附下次启动生效提示；样式与交互细则同步见 DESIGN.md / .impeccable/design.json。

- 选项：浅色 / 深色 / 跟随系统（设置弹窗下拉，持久化到 `settings.json` 的 `theme`）
- 实现：`frontend/src/styles/main.css` 定义 M3 颜色角色（`--md-sys-color-*`）与
  `--surface` / `--ink` / `--space-*` / `--radius-*` 别名；`dark`，或 `auto` 且
  系统 `prefers-color-scheme: dark` 时，`App.vue` 给 `<html>` 加 `.dark` 类并调
  `setWindowTheme`（主题确已接线，旧描述"深色未生效"已作废）
- 色板：`theme_color` 五档（pixiv 默认 / indigo / jade / violet / amber）经
  `html[data-palette]` 覆盖 primary / secondary 及其 on/container 角色，
  `html.dark[data-palette]` 提供配对深色值
- UI 层为 `@material/web`（M3 原语），视觉规范的真相源是根目录 `DESIGN.md`，
  `.impeccable/design.json` 与其保持同步
- 浏览资源列表头部统一提供低强调刷新文字按钮（18px 线性图标、加载中禁用），
  Ctrl+R / Command+R 与按钮执行同一数据刷新，不重载 WebView；配方见 `DESIGN.md`。
- 返回统一为 40×40 `md-icon-button`、20px lucide `chevron-left` 官方路径（stroke 2）与
  `on-surface-variant` 颜色，沿用 8px 标题行间距，无独立文字返回行、无新增 token；
  悬停提示及可访问名称为「返回上一页」，无历史时为「返回所属分区」或「返回首页」，replace 返回所属核心入口，核心入口再回推荐首页。

### 6.4 浏览模式（browse，ADR 0012）

自有 UI 只读浏览 pixiv：8 个列表页 + 作品查看器 / 小说阅读器 / 系列目录 / 作者页，
路由见 §6.1。数据层要点：

- **接口**：全部走 `www.pixiv.net/ajax/*` 同源 GET + 既有 `PixivClient` 限速；
  端点与响应结构的契约事实源为 `docs/PIXIV-API.md`（`docs/research/pixiv-browse-api.md`
  为 2026-10-01 调研证据档案，保留当日字段细节与失效端点勘误）。
  唯一例外是首页 street 流（POST，需 csrf token，见 ADR 0012 §3）。
- **IPC 契约**：浏览相关 25 个命令（浏览端点 22 + 浏览访问历史 3，§7）。浏览端点返回体统一
  `BrowseWorkItem` 卡片结构（id/kind/title/author/cover/page_count/x_restrict/tags/series…），
  浏览访问历史返回 `browse_history_list` 的分页行（§7）；前端契约类型与
  mock 层在 `frontend/src/api/browse.ts`（非 Tauri 环境返回确定性样例数据，供浏览器
  视觉验收；生产不受影响）。
- **列表刷新与返回缓存**：首页、插画/漫画/小说频道、发现、动态、追更、搜索、
  排行榜、收藏、作者作品与系列目录均提供刷新。保留当前筛选；无限列表重新加载
  第 1 批，排行榜与系列目录刷新当前页；作者页刷新资料与当前 tab，收藏同时刷新
  标签计数；首页「换一批」继续作为追加动作，与重置列表的「刷新」分开。
  Ctrl+R / Command+R 只作用于当前激活列表（无关键词搜索不请求，模态弹窗打开时
  不刷新底层列表，加载中忽略重复触发），刷新回到主内容顶部。
  `App.vue` 以路由 path 为键用 Vue KeepAlive 缓存浏览列表、作者与系列目录、插画/漫画
  作品详情，最多 20 个页面、LRU 淘汰（列表页与详情页共用同一池）；
  返回时复用已加载内容、筛选、分页和整体滚动位置，不执行挂载加载。插画/漫画/小说
  频道以及不同作者/系列/作品互相隔离；搜索与排行的
  query 仍驱动参数变化，同一路径不因 query replace 重新挂载。
  KeepAlive 缓存仅在当前登录会话内存中，登录/退出/账号变更立即清空。
  离开可缓存页时即保存实际滚动位置，首屏尚未登记 afterEach 的页面也能正确
  返回；下载历史读取失败保留现有快照、恢复 loading 并用统一通知提示错误。
  首页另以已确认的 user_id 隔离 localStorage 卡片快照（`pixiv-tool-home-v1:`，
  24h 有效、每账号最多 120 条、仅展示字段，不含凭据）；挂载时先显示有效快照，
  同时请求最新推荐，成功替换并更新快照、失败保留旧卡片并显示错误。
  未确认账号不读写快照，已卸载页面的迟到响应不更新缓存。
  其余页面应用重启后首次进入重新加载。停用的 WorkGrid 断开无限加载观察器，
  不在后台继续自动翻页。
  无限列表每次 reset/reload 递增请求世代，卸载后忽略迟到响应和错误；收藏
  游标与 total 同样受保护。发现页停用后停止自动补拉、重新激活按需恢复，
  作者页停用时移除 resize 监听。应用退出与拖拽的异步订阅若卸载后才完成，
  立即反订阅；卸载也清除通知计时器和布局 DOM 引用。
  小说阅读器不缓存（阅读位置与翻译状态自持）。插画/漫画详情页停用时摘除全屏/翻页
  键盘监听、自存舞台与信息列滚动，激活时还原，因此从作者页或相关推荐返回复用同一
  实例、不重新加载，返回上游列表同样使用上述缓存。除首页首次挂载的后台刷新外，
  数据有变化时由用户主动刷新列表。
  渐进加载回归页：启动 `dev.ps1 frontend start` 后打开 `/tests/loading.html`。
- **作者页作品排序**：作品工具行（插画/漫画/小说三类 tab）提供排序切换，默认时间倒序
  （= id 降序 = 现行行为），可切正序（id 升序）；收藏 tab 不参与排序。偏好经 localStorage
  `pixiv-tool-author-order` 记忆、**全局不按账号隔离**（排序属阅读习惯、非账号数据），
  与 settings.json 无关。切换排序只重置三类作品列表并回到作品区顶部，不触发其它 tab
  请求。`browse_user_works` 因此新增 `order` 参数（§7）：**先排序后切片**，且**批内条目由
  后端按请求 id 顺序重排**——对象形响应经 serde_json BTreeMap 按字符串键升序迭代，
  不重排会呈现「正序」观感（这正是排序需求的来源）；仅本地排序，pixiv 请求串不变。
- **图片**：`<img>` 一律经自定义协议 `pixiv-img`（§3.1、ADR 0012 §2），磁盘缓存
  `<data>/cache/img/`（上限由设置 `image_cache_max_mib` 决定，默认 512 MiB，
  分区淘汰见 §5.2 与 ADR 0028），前端 `pxSrc()` 封装。封面 URL 经
  `frontend/src/utils/thumb.ts` 按设置档位（`thumb_quality_grid` /
  `thumb_quality_detail` / `thumb_quality_fullscreen`，见 §5.2）改写：**只替换路径里已有的
  `/c/<尺寸段>/` 段**（列表卡片多为 `c/540x540_70`），**或给 `/img-master/img/`
  前缀插入 `/c/<尺寸段>/`**；`/img-original/`、`/img-zip-ugoira/`、`/user-profile/`
  等其它路径与 `original` 档一律原样返回（头像保留接口给的 `_50`/`_170` 后缀）。
  **绝不构造 `{datePath}` 与文件名**（拼错即 404）。
  WorkCard 先请求 small（250px）封面，小图完成后再请求所选网格档位，高清就绪后
  覆盖小图、失败保留小图；small 档或同 URL 不重复请求，改写失败回退接口原始 URL。
  WorkGrid 前 12 张以 eager/high 请求小图，之后 lazy；高清升级请求为 low 优先级。
  代理冷缓存下载完成即返回共享字节，后台串行写临时文件并原子重命名，目录扫描与
  淘汰放到阻塞线程；后台持有在途条目至落盘维护完成，期间新请求复用同一下载。
  缓存写入失败不影响已经返回的图片；仍按单张完整字节响应，不做字节流式传输。
  后台缓存队列最多保留 10 份完整响应，慢磁盘拥塞时跳过新缓存写入但仍返回
  图片，结束时释放许可与在途条目；缓存 key 复用既有 sha2 实现。
- **作品查看器（插画/漫画）**：图片舞台为竖向滚动容器、**无自有底色**（与页面同底色，浅色
  主题即白），图片满幅（滚动区上/左/下 padding 为 0、页框直角，整体圆角由舞台 16px 圆角 +
  `overflow: hidden` 承担）；页框宽度可缩放——详情页顶栏最右提供 `[−] 值 [+] 重置` 缩放组
  （写设置键 `detail_image_scale`，默认 1.0 = 撑满舞台 = 既有观感，区间 0.5~1.0、10% 步进，
  见 §5.2），只收窄页框宽度并保持居中，乐观步进防连点丢步、失败回滚并提示；多页作品自上而下逐页排列，
  滚动到视口附近才发起加载（渐进式；未加载页为按该页 `width` / `height` 预留纵横比的
  纯色占位块，缺省 2:3，避免加载完成后布局跳动），每页先铺 540 低清占位层再换
  `thumb_quality_detail`；单页作品与 R-18 遮罩态整幅在舞台内垂直居中。点击任意页进入
  全屏浮层（`thumb_quality_fullscreen`）并定位到该页，浮层内 ‹ › 按钮与键盘 ←/→ 翻页、
  Esc 关闭；全屏鼠标滚轮向下 / PgDn 前进，向上 / PgUp 后退，单页步进 1、双页步进 2，不受左右阅读方向影响，首尾页不循环。滚轮翻页间隔至少 250ms；右侧胶卷保持原生滚动，鼠标或焦点在可滚动胶卷时 PgUp/PgDn 只滚动胶卷（§6.1.1）；水平滚动与 Ctrl/Meta 滚轮不翻页，输入框焦点及带 Ctrl/Meta/Alt/Shift 的 PgUp/PgDn 不触发翻页。浮层页框按「可用高度 × 该页纵横比」定尺寸（纵向页撑满高度、横向页撑满
  宽度，小图同样放大到该尺寸，不受原始像素限制）。底部控制条与右上角 ✕ 默认隐藏，
  指针进入对应热区或键盘聚焦才显现（触摸设备常驻）；右侧胶卷缩略图列常驻
  （`thumb_quality_grid` 档、64px 方形，列内出现滚动条时按剩余宽度收窄，滚动条为 6px
  常显样式；屏幕上出现的页白色描边、点击直达该页，翻页时自动把当前缩略图滚入视野）。
  控制条除「‹ › + 页码区间」外另有**双图（跨页）开关**：开启后一屏并排两张、跨页按
  1-2 / 3-4 对齐（点胶卷任意一页落到所属跨页）、翻页步进 2 页、页码显示为区间，并可切换
  阅读方向「从右往左」（默认，当前页在右）/「从左往右」；从右往左时翻页组整组镜像——
  前进按钮落到左侧、箭头改为朝左、键盘 ← 为前进，双图与方向开关不参与镜像；模式与方向
  会话内记忆、不持久化，单页作品不进入双图。控制条还提供**竖屏模式开关**（连续纵向滚动，
  条漫友好，仅多页作品）：舞台整列自上而下滚动、**固定单列**——与双图（跨页）模式互斥，
  进入竖屏即按单列渲染，双页开关随之隐藏，退出竖屏后开关恢复可见并保持进入前的选择；
  竖屏固定从上到下、不提供方向切换（方向开关随之隐藏、翻页组不镜像）；滚轮 / ↑↓ /
  PgUp / PgDn 走原生滚动与全局按屏滚动（不整页翻页），←/→ 与翻页组对齐上/下一页行首，
  Esc 关闭，胶卷点击滚到该页行首、白色描边标记视口内各页（宽度比例变化等重排后重新标定），
  页码恒显示视口内最靠上的单页；页框宽度由**宽度比例组** `[−] 值 [+] 重置` 控制
  （0.5~1.0、10% 步进，比例即整幅页框宽度），与详情页缩放设置键相互独立；进出竖屏保持
  当前页（行首对齐，退出竖屏时按跨页对对齐）；图片以 aspect-ratio 占位 + 懒加载窗口
  （DOM 全量占位、`<img>` 进入视口附近才挂载）。模式与宽度同样会话内记忆、不持久化。
  paged（非竖屏）分支的结构与行为不变。页框本身即全屏入口、可聚焦（`tabindex=0` +
  `role="button"`，Enter / Space 进入浮层并定位到该页，`focus-visible` 为 primary 2px 内环；
  R-18 遮罩态不可聚焦、`tabindex=-1`），右下角「第 N / M 页」深色胶囊徽标跟随滚动，舞台可聚焦
  （Esc 退出浮层后焦点回到舞台）、↑/↓ 原生滚动；舞台滚动条与右侧信息列同配方：6px 常显、
  thumb 为 outline 派生色（40% / hover 60%）；滚动区右侧留 6px 内边距（与信息列同配方），
  图片与滚动条不贴靠，左侧满幅不缩进。ugoira 仍只显示封面帧 + 说明行。
- **小说阅读器**：整页固定 100vh 三行 flex——顶栏（非 sticky，天然贴窗口上边）/ 中间唯一
  滚动层（`flex:1; min-height:0; overflow-y:auto`，6px 细滚动条）/ 底栏 AppPagination
  （`variant="reader"`）；顶底栏贴窗口上下边、只有中间层滚动。正文列默认不限宽，仅
  `@media (min-width: 1921px), (min-height: 1081px)`（大于 16:9 1080p 的屏幕）限 1280px
  （列内边距 `--space-lg` 16px，100% 字号下约 78 个全角字/行；1920×1080 及以下仍满宽）；
  正文 16px 基准 × `--novel-scale`，章节标题 1.15em。**正文内嵌图**：
  `[uploadedimage:id]` 的 id 由详情响应 `embedded_images`（`id → pximg URL`，取自同一
  响应的 `textEmbeddedImages`，无额外请求）解析为图片，整行成块居中、限宽于正文列、
  `--radius-control` 圆角、`loading="lazy"`，经 `pixiv-img` 协议代理与磁盘缓存显示；
  混排段落内的内嵌图按行内小图渲染。取不到 URL 的标记（未收录的 id、
  `[pixivimage:illustId]` 插图引用）渲染为占位块，不静默丢图。`--novel-scale` 取自设置键
  `novel_font_scale`（默认 1.0、区间 0.75~2.0，见 §5.2）；底栏左侧经 AppPagination reader
  变体的 `#leading` 插槽挂字号缩放控件 `[−] [百分比] [+] [重置]`（步进 0.1、到界禁用；
  重置回到 100%（100% 时禁用）；`aria-live="polite"`）。重置按钮右侧为**阅读背景色块
  按钮**（`NovelBgPicker`，i18n `browse.novel.bgLabel` / `bgDefault` / `bgGreen` /
  `bgKraft` / `bgWarm` / `bgMist` / `bgBlush`）：`md-icon-button` 内嵌 18px 圆形色块
  回显当前色、恒带 outline 1px 描边（默认态无底色、只有描边；选中纸色后色块与底栏
  同色，描边是唯一分界），点击向上弹出居中于按钮的一排气泡
  （28px 圆形色块一行、同样恒带 outline 1px 描边：默认 / 护眼绿 / 牛皮纸 / 暖杏 / 雾蓝 / 藕粉；弹出层与账号菜单
  同 recipe——surface-container 底、`--radius-control` 圆角、既定轻阴影、透明遮罩点击
  外部关闭 + Esc 关闭、0.15s ease 上浮），点击色块立即应用并经 `settings_save` 持久化
  `novel_bg_color`（语义键，见 §5.2）；选中纸色后整页根节点挂 `read-bg-*` 类，
  顶栏 + 正文 + 底栏一起落纸色 + 墨色配对（暗色主题同样以纸面呈现，配对见
  DESIGN.md「小说阅读背景色板」）。底栏右侧经 `#trailing` 插槽挂阅读
  进度条（`md-slider` 拉条 + 右侧百分比回显）：展示当前页内滚动进度（`scrollTop /
  (scrollHeight - clientHeight)`，无滚动余量恒 100%），滚动与内容高度变化实时回显，
  拖动按百分比快速定位正文位置（input 即乐观同步回显，scroll 回声仅作确认、不依赖）；
  键盘 ←/→ 在滑杆聚焦时归滑杆调值、不翻页。
  键盘 ←/→ 翻页与顶栏动作不变（见下条）。
- **详情信息（插画 / 漫画 / 小说）**：三类详情均展示点赞数、收藏（喜欢）数、浏览数，
  分别来自详情响应 `likeCount` / `bookmarkCount` / `viewCount`（零值保留，按语言
  格式化；缺失不冒充零）。小说在信息头元信息行追加三项计数与简介，正文阅读不变；
  元信息行同时回显发布时间（详情响应 `createDate`，ISO 带时区，固定按 JST
  （`Asia/Tokyo`）格式化，与 pixiv 网页端展示一致；年月日与时分排版随界面语言）。阅读时长取 `readingTime`（接口单位为**秒**），
  展示前除以 60 向下取整换算为分钟，不足 1 分钟按 1 分钟。
  标签解析兼容列表数组与详情的 `tags.tags[].tag`；显示原文，使用可键盘访问、可新标签
  打开的搜索链接，携带 `word`、对应 `kind` 与 `s_mode=s_tag_full` 精确标签检索。
  所属系列（合集）入口为标签行正下方的胶囊 chip（`primary-container` 底、lucide layers
  图标，与标签 chip 的 `surface-container` 检索形态区分「应用内跳转 vs 检索」），插画/漫画
  与小说同 recipe；`seriesId <= 0` 不渲染（解析层守卫 + 前端双保险，见
  `docs/PIXIV-API.md`）。
  描述（插画/漫画 `illustComment`、小说 `description`）统一转为纯文本，解码实体、
  保留换行与段落、去除脚本/样式等非正文，不执行/挂载外部 HTML；空简介/标签不占位。
- **详情页顶栏与面板（查看器 / 阅读器共用）**：顶栏带文案的动作保持 40px 胶囊（收藏），
  图标动作统一 `md-icon-button`（40×40、无描边、20px lucide 官方路径线性图标，stroke 2），
  顺序为「收藏 / 评论 / 返填表单 / 在浏览器中打开」；**不再用只有图标的
  `md-outlined-button`**（左右各 24px 内距会把单个图标撑成约 66px 宽的胶囊），
  也不再用 `‹ › ✕` 文本字形充当图标——页码翻页器统一为公共 AppPagination
  （`components/common/`，自绘 chevron 同为 20px lucide 官方路径 stroke 2），排行榜日期切换用
  `md-icon-button` + 20px chevron，同规。顶栏评论按钮（toggle + selected，标签在
  「查看评论」/「返回相关推荐」间切换）切换查看器右列下段与阅读器正文列下段的
  面板：默认「相关推荐」，切到「评论」时 `CommentsSection` 才挂载并拉第一页
  （roots 沿用「加载更多评论」offset 分页、回复展开后按页续拉），切换时把面板
  滚入视野（阅读器 `block: start` + 72px 吸顶余量、查看器 `block: nearest`）；
  作者关闭评论区的作品（roots 恒 400 → 契约 `disabled` 信封）显示「作者已关闭
  评论区」终态提示，非错误、无重试；作品切换（kind / id 变化）时面板回到
  「相关推荐」。
- **R-18 显示**：全局开关 `show_r18`（§5.2）关闭后，各列表在**渲染期**过滤
  `x_restrict >= 1` 的作品——只隐藏已取得的条目，**不重新请求**；`x_restrict`
  缺失（无法判定）的条目按 fail-closed 处理：仅在「全部」档可见，一般向与
  R-18 档都不收录。整页被滤空也不改变
  分页判定，仍按服务端返回的 `total` / `lastPage` / `next` 继续翻页。详情页仍可访问，
  且仅在开关关闭时保留模糊遮罩 + 「显示」确认（确认后本会话记忆、不持久化）；开关开启
  （默认）时详情页直接展示、不再出现确认遮罩。频道页顶部另有 R-18 快捷筛选
  （全部 / 一般向 / R18）：默认跟随全局设置，手动选择只覆盖当前频道页
  （插画 / 漫画 / 小说三档各自独立、不串档）、
  不改写设置。R-18 档请求 `/ajax/top/*?mode=r18`；全部与一般向复用普通频道
  快照，一般向继续本地过滤。切入/切出 R-18 或切换频道时清除旧快照并重载，只
  接收最新请求结果；频道网格显式使用频道档位，避免被全局开关再次过滤。
  服务端 R-18 候选可能混入一般向/未知条目，仍按 `x_restrict` 过滤。
  插画频道展示响应中的 `recommendByTag` 标签推荐分区（每区最多 12 条，空区
  隐藏），完整榜单入口随 R-18 档预选 `daily_r18`，标签搜索继承内容档位。
  排行榜的插画/漫画/动图条目在接口里
  **没有顶层 `x_restrict`**，由 `illust_content_type.sexual`（0 一般 / 1 R-18 /
  2 R-18G）补入 `x_restrict`（实测见 `docs/research/pixiv-browse-api.md` §9，
  部分 ugoira 条目的 `illust_content_type` 为空数组、无从判定，同样按 fail-closed
  在关闭 R-18 时隐藏），
  因此与其它列表同样参与过滤；小说排行条目自带顶层 `x_restrict`，无需补字段。
- **分页**：统一收敛为 `next_page` / `is_last_page` / `next_last_order`（游标）语义；
  发现页与首页推荐无服务端翻页，前端重复调用按 id 去重。页码翻页 UI（历史 / 任务 /
  浏览历史 / 排行榜 / 系列分集 / 小说阅读器）统一收敛到 `components/common/AppPagination`
  （历史、任务、浏览历史为 default 变体，排行榜走未知总页数模式，系列与阅读器为
  `variant="reader"`）；游标 / 无限滚动调用点不使用该组件。
- **V1 限制**：浏览支持作品收藏与作者关注（ADR 0016）；无点赞/发评论；ugoira 显示封面帧；小说内嵌图中
  `[pixivimage:illustId]` 插图引用（现行 pixiv 编辑器已不产出，实测见
  `docs/research/pixiv-browse-api.md` §7.2）与未收录 id 显示占位块，`[uploadedimage:]`
  正常出图；评论只在详情页面板内按需加载（只读，无评论/回复发布）。
- **打开原页 / 返填**：浏览页的「在浏览器中打开」走系统默认浏览器
  （官方 `tauri-plugin-opener`，capability `opener:default`）；
  频道页卡片与作品级页面另有「返填表单」→ 就地打开下载面板并预填来源，不改变当前 URL 或路由历史。
  公共外链入口只接受无 userinfo 的 HTTP/HTTPS URL，拒绝 file/javascript 等协议。

---

## 7. IPC 命令设计（invoke）

翻译的模型流由 Rig 的 Chat Completions / Responses / Anthropic Messages wire 解析，项目
保留自己的 HTTP、凭据、端点、超时与错误语义。流必须收到对应协议的完成信号（文本
`stop` / `[DONE]` / `response.completed` / `end_turn` / `message_stop`，工具调用同样有
协议对应的结束原因）才认为本轮成功；仅 HTTP 200 或正常 EOF 不代表生成完成。增量按
正文 / 思考二分类外供预览（ADR 0030）：思考增量（Chat `reasoning_content`/`reasoning`、
Responses `reasoning_summary_text.delta`/`reasoning_text.delta`、Anthropic `thinking_delta`）
不进译文，只进浮窗思考行；空字符串不算增量，同一帧同时带正文与思考时两路都收。工具
参数增量、tool call ID 与工具结果配对、推理签名 / 不透明续传字段按协议保留，不当作
普通文本摘要丢弃。思考参数在装配前按协议归一化（ADR 0032），输出上限按输入分档
（ADR 0033），高级 JSON 显式值优先。token 用量按协议落点解析（Chat 末帧 `usage`、
Responses `response.usage`、Anthropic `message_start`/`message_delta` 分帧合并），
汇总进 Job 事件与快照。兼容服务在 SSE 末尾直接追加的裸 JSON 错误体同样识别：智谱
`1301` 明确提示内容审核拒绝，不回显服务原始 message、原文或凭据，不保存残缺译文；
输出上限截断单独提示实际生效的上限与可执行动作，缩块重做而不是原样重发；审核拒绝
不自动重试。

离线验收：
- `src-tauri/tests/agent_protocol_wire.rs` 覆盖三协议流式工具调用、端点整体重写（含无
  `/v1` 的 Anthropic 网关）、opencode 会话头只对相应主机生效，以及 drop 流后关闭连接并
  释放请求闸门。
- `src-tauri/tests/agent_runtime.rs` 覆盖工具调用后文本收尾与用量、取消、预算耗尽、
  压缩保留工具配对、重复非法工具调用触发无进展上限，以及调度器与小说许可可被宿主复用。
- `src-tauri/tests/translation_job_e2e.rs` 用本地模拟 HTTP 覆盖整篇 Job 完成与页 / 行映射、
  重复启动合并、暂停后恢复不重做块、确认模式等待后继续、取消保留检查点、缓存清理递增
  世代并拒绝迟到写、损坏 manifest 不发起请求、删除整棵 Job 目录、断线失败不冒充完成、
  输出截断缩块后仍完成、非法工具参数在纠正上限后停止、重启从磁盘重建与恢复、会话 /
  压缩记录不含正文、问题汇总排除已通过块，以及两本小说并发、活跃小说上限排队与取消
  一本不影响另一本。
- `src-tauri/src/translation/**` 单测覆盖分块与页行片段映射、约束聚合与冲突保留、覆盖 /
  顺序 / 重复 / 标记保真校验、发布条件、设置快照不含凭据、版本拒绝回退；v1 旧实现的
  单测（含忽略的在线两轮测试）保留在 `translation_legacy.rs`，只用于兼容性回归。
- 前端 `/tests/translation.html` 与 `/tests/translation-tasks.html` 使用 mock IPC 覆盖浮层
  四页签、术语确认、块预览、问题列表、开始 / 暂停 / 继续 / 取消、关闭浮层不取消、完成
  事件触发阅读器投影刷新、失败与待处理状态点、设置一级 / 二级切换与错误定位，以及多
  小说隔离、seq 追赶、底栏单 / 多任务形态与选择菜单；不读写真实 Key、不访问 Pixiv 或
  付费模型。

真实整篇付费模型尚未完成参数校准（见 §13）；本机复测只允许在用户授权后读取当前设置与
系统凭据，全程不打印正文或 Key，不修改应用设置，临时记录结束后清理。

小说翻译设置的高级 JSON 示例与二级参数说明在中英文文案中使用 vue-i18n 字面量插值转义
花括号，保证生产构建不会因非法占位符抛出异常而使整组设置空白。运行
`node frontend/tests/production-settings.mjs` 可用生产编译与 `tauri.conf.json` 的生产 CSP
执行真实组件离线验收，断言中英文下翻译字段正常渲染、JSON 示例完整显示，并继续验证
保存、凭据草稿及模型探测。需本机 Chrome；默认使用 Windows 标准安装路径，可用
`CHROME_PATH` 指定。测试产物与独立浏览器配置均存临时目录，结束后清理。

全局帮助离线验收：`/tests/help-tooltips.html` 使用真实 SettingsDialog、SettingsPanel、
LoginDialog、搜索/以图识图页面，覆盖六组设置、两种登录、默认收起、悬停/焦点/点击、
可悬停阅读、Esc 不误关设置与外部关闭，提供宽窄/深浅主题/中英文预览；浏览器 mock，
不读取真实凭据。

命令实现于 `src-tauri/src/commands/`，返回体沿用旧 HTTP 响应形状（snake_case）。
业务错误（旧 200+`{error}` 风格）在返回值内；校验类错误（旧 4xx/5xx detail）
reject string，前端 `errorMessage()` 归一。参数从 JS 侧以 camelCase 键传入。
翻译 Job 命令实现于 `commands/translation_cmds.rs`，业务真相源在
`translation/service.rs`（状态、索引、事件与缓存拥有者）；v1 只读命令
`novel_translation_get` / `translation_models` / `translation_test` 由 `translation/mod.rs`
再导出。`novel_translate_page` 已从注册表移除，旧 Channel 协议不再可用。

| 命令 | 说明 |
|---|---|
| `auth_status` | 登录态探测（2s 超时；401/403 清 cookie 并移除死账号 + 会话缓存失效，其余失败保留） |
| `auth_login` | 真实 Chromium CDP 登录（长阻塞，最长 300s）；无浏览器时回退内嵌 webview 登录窗（ADR 0009），`PIXIV_TOOL_FORCE_WEBVIEW_LOGIN=1` 强制走 webview |
| `auth_login_manual(phpsessid)` | 手动 PHPSESSID（normalize → 会话探测 → 存储） |
| `auth_logout` | 退出当前账号：清 default 镜像 + 移除其账号条目与索引项；有剩余账号则自动激活首个，回退失败降级为纯登出；返回 `{status, fallback_applied, warning?}`（warning 为非终态抓取任务仍按创建时账号执行的提示） |
| `auth_accounts_list` | 已保存账号列表：`{active, accounts:[{user_id,pixiv_id,name,profile_img,avatar_file,avatar_url?,saved_at}]}` |
| `auth_account_switch(userId)` | 切换当前账号：目标条目写入 default → 索引 active → 会话缓存失效；目标凭据缺失/失效 → Err；存在非终态抓取任务时返回可选 `warning` |
| `tasks_list(category?)` | 任务列表（按小说/插画过滤） |
| `task_create(sourceType, sourceId, formats, category)` | 创建抓取任务，后台 tokio 运行 |
| `task_pause` / `task_resume` / `task_cancel(taskId)` | 任务控制 |
| `task_retry_failed(taskId)` | 失败项重试（新任务，逐 id 串行，计数累计） |
| `task_delete(taskId)` / `tasks_delete(taskIds)` / `tasks_delete_completed` | 删除任务记录（非终态先取消；有不存在 id 整批不删） |
| `settings_get` / `settings_save(settings)` | 配置读写（白名单 48 键 + 校验，含翻译 URL / 协议 / 模型 / 目标语言 / 单请求超时 / 高级 JSON / 22 个翻译运行参数 / 详情页图片缩放 / 三个下载文件名模板；API Key 独立写入，仅返回 configured 状态，见 §5.2） |
| `novel_translation_start(novel, force)` | 启动或加入整篇翻译 Job；novel=`{novel_id,title,tags,description,content}`（content ≤ 8MB，非法 id / 空内容拒绝）。启动前校验设置、URL、模型与 Key，任何失败不创建 Job；原文已是目标语言且 `force=false` 时返回 `already_target_language:true` 与未落库的空快照。同一作品、原文版本、目标语言重复启动加入既有 Job，`force=true` 新建。返回 `{job: TranslationJobSnapshot, already_target_language}`，命令立即返回、后台 tokio 继续执行。目标语言白名单 `zh-CN`/`zh-TW`/`en`/`ja`/`ko`/`es`/`fr`/`de`/`ru`，空设置跟随界面语言（ADR 0018） |
| `novel_translation_jobs()` | 任务列表 `{seq, jobs:[TranslationJobSnapshot]}`（内存索引 + 磁盘扫描，含重启后的 `interrupted`）；`seq` 为所有 Job 的最大事件序号 |
| `novel_translation_status(jobId)` | 单个 Job 快照：`{job_id,novel_id,state,phase,counts,current,usage,elapsed_ms,preview,message,error_code,seq,constraint_version,budget,already_target_language,target_language}`；counts=`{pages,chunks,translated,reviewed,needs_repair,terms}`，current=`{page?,chunk_id?,role}`，usage=`{input,output,total}`，budget=`{used,limit,manual_extra,remaining}`；任务不存在或清单损坏返回可读错误 |
| `novel_translation_result(novelId)` | 阅读器投影：优先已完成且状态为 `complete` 的 v2 Job，否则回退 v1 旧缓存并标记 `partial`；无结果返回 null。返回 `{novel_id,job_id,target_language,pages:{页码:[{line,text}]},status:"complete"|"partial"|"needs_attention",unresolved}` |
| `novel_translation_confirm(jobId, terms, style)` | 确认术语与风格（等待确认时由浮层提交）。terms 为 `[{source_term,locked_translation,term_type?,aliases?,notes?}]`；确认后约束版本 +1、清除冲突与未解决项并继续。任务不存在 / 已删除返回错误 |
| `novel_translation_pause(jobId)` | 停止派发并停在最近检查点（`queued`/`running` → `pausing` → `paused`）；等待确认时要求先确认或取消；其它状态拒绝 |
| `novel_translation_resume(jobId)` | 从检查点恢复（`paused`/`needs_attention`/`interrupted`/`queued`）；先校验缓存世代、资源版本与清单，不一致时拒绝静默续跑；内存中无句柄时从磁盘重建 |
| `novel_translation_cancel(jobId)` | 立即传播取消信号并保留已提交产物与检查点；已取消时幂等；运行中先进入 `cancelling` 再收敛到 `cancelled` |
| `novel_translation_delete(jobId)` | 删除整个 Job 目录（含 chunks / sessions / checkpoints / temporary / output 全部中间产物）；运行中拒绝，需先暂停或取消；清单损坏时提示通过清理翻译缓存处理 |
| `novel_translation_preview(jobId, chunkId)` | 浮层预览：`{chunk_id,order,segments,translated,context_prev,context_next,draft_version,reviewed,needs_repair,review?}`；segments 含 `{id,page,line,text,kind}`，translated 为 `[{id,text}]`；未译块返回空译文列表，不伪造 |
| `novel_translation_constraints(jobId)` | 约束只读视图：`{version,terms,style,conflicts,unresolved,frozen,awaiting_confirmation}`；约束尚未生成时返回明确错误，可与运行中 Job 并发调用 |
| `novel_translation_issues(jobId)` | 审查问题只读视图：`{issues,affected_chunks,unresolved_chunks,reviewed,total}`；每条问题含块 / 片段定位、严重度与说明，不含原文或译文 |
| `novel_translation_diagnostics()` | 诊断摘要 `{generation,active,jobs,by_state}`（缓存世代、活跃 Job 数、Job 总数与按状态计数），不含正文 |
| `translation_set_diagnostic(enabled)` | 会话级详细诊断开关：开启后放行 `agent` target 的 DEBUG（工具 / 请求 / 子任务起止与长度），仅本次运行有效、重启复位；返回生效状态。不输出原文、译文、提示词、Key 或 Cookie |
| `translation_diagnostic_status()` | 当前诊断开关状态，供设置页回显 |
| `novel_translation_get(novel)` | v1 只读兼容：读取旧单页缓存中的设定集和已译页，novel=`{novel_id,title,tags,description,content}`；返回 `{bible:{style,terms},pages:{页码:[{line,text}]}}`，原文和元信息 SHA256 区分版本 |
| `translation_models(probe)` | 用未保存草稿探测模型列表端点（按 `format` 协议从同一基址推导 `/models`；OpenAI 系 Bearer，Anthropic `x-api-key` + 版本头，opencode 主机附会话标识头），返回排序去重后的模型 ID 列表（≤2000 项）；Key 省略时沿用已保存凭据，仅本次请求使用，不写配置或凭据库 |
| `translation_test(probe)` | 用未保存草稿按所选协议发起一次简短生成请求验证端点+Key+模型+高级 JSON 可用性（模型必填）；须返回 `{"ok":true}` 语义 JSON 才算通过，同 probe 凭据规则；超时用草稿里的 `timeout_seconds`（缺省 600 秒）——「获取模型列表」固定 30 秒 |
| `clear_logs` | 截断 logs 下当前及遗留轮转的普通 .log 文件；保留活动 logger 句柄，后续继续记录 |
| `maintenance_info` | 返回 translation/images/logs 的 `{path,bytes,files}`，缓存分类/总大小及日志总大小；路径只由 AppPaths 派生 |
| `read_logs` | 返回 app.log 最近最多 200 行（末尾 64KiB），不存在返回空串 |
| `clear_cache(kind)` | kind 仅 translation/images，清理 translations 或 cache/img 内文件；拒绝链接目录，翻译运行时拒绝清理译文，不影响数据库与下载作品 |
| `saucenao_search(sourceType, source, numres?)` | 以图识图搜索（SauceNAO；file=本地路径 POST multipart / url=公网图片 GET；pixiv 结果含 pid/作者可直接跳应用内详情；需在设置配置 API Key） |
| `history_list(category, page, pageSize, keyword?)` | 历史联合分页查询（UNION，统一行形状）；page ≥ 1，pageSize 为 1–200，拒绝偏移溢出 |
| `browse_history_record(kind, workId, title, authorId, authorName, cover?, pageCount, xRestrict)` | 记录一次浏览访问（同 kind+workId 覆写并按访问时间置顶；作品详情页加载成功后上报） |
| `browse_history_list(page, pageSize, kind?)` | 浏览访问历史分页查询（`visited_at` 倒序，最近访问在前）；page ≥ 1，pageSize 为 1–200，拒绝偏移溢出；`kind` 省略=全部，否则 illust/manga/novel 过滤，非法值报错 |
| `browse_history_clear()` | 清空浏览访问历史（返回 `{status, deleted}`） |
| `novel_delete` / `novels_batch_delete` / `novels_delete_all` | 小说记录删除（可选删文件） |
| `illustration_delete` / `illustrations_batch_delete` / `illustrations_delete_all` | 插画记录删除（可选删文件） |
| `open_novel_file(novelId)` / `open_illustration_folder(artworkId)` | 在系统文件管理器中定位 |
| `browse_home_feed` | 首页推荐流（street POST + csrf token，进程内 30min 缓存） |
| `browse_channel(kind, mode?)` | 频道仪表盘：关注新作/推荐/排行/标签推荐/最新投稿/热门标签（/ajax/top/*）；mode 省略或 all 为普通快照，r18 为受限频道，其余拒绝 |
| `browse_discover` | 发现推荐 60 条（无服务端翻页，前端去重复调） |
| `browse_follow_latest(kind, mode, page)` | 关注的新作品（p + isLastPage） |
| `browse_search(kind, word, order, mode, s_mode, type, page)` | 插画/漫画/小说搜索（total + lastPage；透传已有可选 like_count/bookmark_count/view_count，小说自带收藏数） |
| `browse_work_counts(kind, ids)` | 作品三项计数批量（搜索页本页排序用；逐项请求详情端点，ids ≤ 60；单项失败跳过，全局限速排队） |
| `browse_ranking(kind, mode, page, date)` | 排行榜：illust/manga/ugoira 走 ranking.php?format=json，novel 走 /ajax/ranking/novel（每页 50，含 prev/next_date） |
| `browse_work_detail(kind, id)` | 作品详情：illust+pages+ugoira_meta / novel 全文（含 series 导航 + 正文内嵌图索引 `embedded_images`） |
| `browse_related(kind, id, limit)` | 相关推荐一次性池（recommend/init，page 参数无效） |
| `browse_user_follow(id, followed)` | 公开关注/取消关注，返回 `{ is_followed: boolean }`；正整数 ID 校验后走登录守卫、CSRF 和既有限速 |
| `browse_user_profile(id)` | 作者资料（/ajax/user/{id}?full=1） |
| `browse_user_works(id, kind, page, order?)` | 作者作品：profile/all 全集 id → 60/批 ids[] 批量；order = `asc`/`desc`（Optional，缺省/非法回落 `desc` = id 降序 = 时间倒序）；**先排序后切片**、批内条目按请求 id 顺序重排（响应为对象形时键迭代序不可依赖）；分页语义不变；仅本地排序，pixiv 请求串不变 |
| `browse_novel_series(id, last_order)` | 系列元数据 + 目录（last_order 游标） |
| `browse_illust_series(id, page)` | 插画/漫画系列元数据 + 目录（官方页码制，12 条/页） |
| `browse_watchlist(kind)` | 追更列表：manga/novel 两个子 tab（/ajax/watch_list/*，按 maxPage 聚合 ≤20 页） |
| `browse_work_comments(kind, id, offset)` | 作品评论根列表（illusts/novels comments/roots，limit=10，offset 游标；作者关闭评论区 → `{"comments":[],"disabled":true}`，非报错） |
| `browse_comment_replies(kind, commentId, page)` | 评论回复列表（comments/replies，page 从 1） |
| `browse_comment_add(kind, id, authorId, comment?, stampId?, parentId?)` | 发表评论 / 回复评论 / 官方表情贴图（插画·漫画 `/rpc/post_comment.php`、小说 `/novel/rpc/post_comment.php`，form + `x-csrf-token`；`authorId` = 作品作者 userId；`comment` 与 `stampId` **二选一**，正文 1–140 字，`stampId` 为官方贴图 id、不带正文；给出 `parentId` 即回复该评论；返回 `{comment_id,user_id,user_name,parent_id?,stamp_id?}`） |
| `browse_bookmark_list(kind, rest, tag, offset, limit, userId?)` | 收藏列表（userId 缺省 = 自己：illusts 48/页、novels 30/页，offset + total 翻页；显式传入 = 该作者的他人公开收藏，仅 rest=show，作者页收藏 tab 用） |
| `browse_bookmark_tags(kind)` | 收藏标签（一次返回 public/private 两组，含「未分類」聚合标签） |
| `browse_bookmark_add(kind, id, restrict, tags)` | 添加收藏（全局 JSON 端点 + x-csrf-token；restrict 0 公开 / 1 非公开） |
| `browse_bookmark_remove(kind, id, bookmarkId)` | 取消收藏：插画走 ajax form，小说走旧式 `/novel/bookmark_setting.php` 表单 |
| `app_exit` | 退出应用（前端确认框确认后调用，与 Cmd+Q 路径一致） |
| `check_app_update` | 检查更新：解析 GitHub releases 页面取最新稳定版，返回 has_update/current_version/latest_version/release_url/platform/package_type；系统代理失败回退直连；启动检查无更新/失败静默，手动检查给提示 |
| `download_app_update(version, progress)` | 下载并校验对应版本、当前平台/架构/包类型的 Release 包；progress 为 IPC Channel，返回 path/installer_opened/directory_opened/package_type；仅一个更新下载可在途 |
| `cancel_app_update` | 取消当前更新下载（含元数据请求），清理临时目录；无在途下载时幂等 |
| `open_update_directory` | 打开最近一次已完成更新包目录，仅后端保留的路径，不接受前端任意文件路径 |

**应用菜单栏**：Windows 上默认隐藏（`SetMenu(hwnd, NULL)`），按 Alt 唤起并
进入菜单循环、退出循环（选中 / Esc / 窗口失活）后自动收回。实现见
`menu_bar.rs`：用 `ICoreWebView2Controller::AcceleratorKeyPressed` 捕获 Alt
（Chromium 不把单独按下的 Alt 派发为 DOM 事件，线程级键盘钩子也收不到——
键盘消息落在 WebView2 自己的线程），配顶层窗口子类化处理 `WM_EXITMENULOOP`。
菜单在 macOS 渲染于系统顶栏、恒显，不受影响。菜单栏只承载「退出」（等价
Alt+F4 / 标题栏关闭）与 Edit 项（撤销/剪切/复制/粘贴/全选，WebView2 原生
支持），默认隐藏不影响文本编辑快捷键。

以上数据命令实现于 `commands/browse_api_cmds.rs`，公共登录守卫 `build_browse_api`：
无 PHPSESSID 一律 `Err("未登录或登录态已失效，请先登录")`（前端据此弹登录窗）。
端点 / 参数 / 响应解析契约见 `docs/PIXIV-API.md`（含端点 → 实现 → 测试的维护矩阵）。
图片经 `pixiv-img` 自定义协议（`image_proxy.rs`，白名单 `*.pximg.net`，磁盘缓存
上限跟随设置 `image_cache_max_mib`（默认 512 MiB，三档分区淘汰见 §5.2 / ADR 0028），
CDN 并发 10、同 URL 在途合并，单个逻辑下载含全部重试与退避受 15s 总预算
约束（`DOWNLOAD_TIMEOUT_SECS`），失败 200/500ms 短退避后重试至多 3 次，命中与
回源统一 `Cache-Control: public, max-age=31536000, immutable`），不走 invoke。
CDN 下载不占 ajax 限速与 400ms 请求间隔；CDN 侧的 429 不重试、立即回落 502，
无跨请求退避（与改造前一致的既有边界）。

目录选择不走命令：前端直接用 `@tauri-apps/plugin-dialog` 的
`open({directory: true})`。

历史联合查询规则：`novels` + `illustrations` 两表 UNION ALL（统一行形状
id / category / title / author_name / pages / series_id / illust_type /
`captured_at`），按 `captured_at` 倒序分页，keyword 同时过滤两表；前端抓取
时间列按东八区固定偏移显示。浏览访问历史（`browse_history` 表 / 三个
`browse_history_*` 命令，ADR 0014）是独立的作品级访问记录，与该联合查询无关、不参与
UNION。"打开所在文件夹"由 Rust 侧
`platform.rs::reveal_in_file_manager` 实现：Windows `explorer /select,`、
macOS `open -R`、Linux `xdg-open`；文件不存在回退父目录。

### 7.1 事件设计（listen）

```
task://progress   {task_id, status:"running", done, total, skipped, current_title?, failed_item?{id,title,error}}
task://done       {task_id, status, done, total, failed, skipped}

translation://job {
  seq, job_id, novel_id,
  state, phase, counts{pages,chunks,translated,reviewed,needs_repair,terms},
  current{page?,chunk_id?,role} | null,
  usage{input,output,total}, elapsed_ms,
  preview{summary,reasoning},
  message, error_code?
}
```

`translation://job` 与 `novel_translation_status` 的快照同构（事件缺
`constraint_version` / `budget` / `target_language` / `already_target_language`，前端由
既有快照补齐）。每个 Job 的 `seq` 从 1 单调递增；
前端先订阅、再拉 `novel_translation_jobs` 快照，事件 `seq ≤` 本地已应用序号时丢弃，快照
序号落后时保留事件版本。普通进度按 ≥5s 合并，生成预览按「≥300ms 或累计新增 ≥2048B」
节流；阶段切换、状态变化、错误与终态立即发送，并在阶段结束 / 终态 flush。`message`
与 `error_code` 只含安全摘要，不含原文、译文、提示词或凭据。任务启动（0 进度）、
每篇/每页成功或失败即推送抓取事件（skipped 不发）；TasksView 订阅后刷新列表，同时保留
2s 轮询兜底。任务列表以浅层响应式的完整快照替换：事件、轮询和任务操作触发的查询共享
一个在途请求，请求期间的新刷新需求合并为一次尾随查询；所有等待者等到刷新链结束，失败
后释放状态以允许下次重试。离开任务页立即清除轮询与已登记监听；迟到的查询不能重新创建
轮询，迟到的监听登记立即解绑，避免路由往返后保留旧页面与重复刷新。翻译事件监听属于
App 级单例，离开小说页或关闭浮层不断开；缓存清理成功后前端清空 Job 快照与投影，不保留
可点击的幽灵条目。

---

### 7.2 应用内更新

启动 3s 后静默检查一次，仅新版弹确认；手动检查沿用账号菜单。用户确认后进入
`UpdateDialog`：available → downloading → opening → guide；失败进入 error（重新下载 / 打开发布页），取消进入 cancelled（可重新下载）。
更新不复用携带 Pixiv Cookie 的客户端，不引入本地 HTTP 服务。资产清单双源获取：首选 GitHub REST
`/repos/gbandszxc/pixiv-tool/releases/tags/{tag}`（结构化、带精确大小与摘要），优先 v 前缀再裸 tag，
仅接受精确版本稳定 Release；API 被匿名限额或网络失败时回退发布页 `expanded_assets/<tag>` 片段
（同 github.com 域、不受 API 限额影响），从 `releases/download/<tag>/` 链接提取资产名（安全字符集、去重）、
从 digest 复制按钮（aria-label 与 value 同开标签）提取 SHA256；片段没有精确大小（按 0 处理），
两通道均失败才报"无法读取该版本的发布包"。
按 Tauri bundler 内嵌包类型与编译架构匹配 CI 文件名（见 PACKAGING §8），不模糊匹配、不跨类型降级；
debug/便携未知包类型提示发布页兜底。macOS 原生包优先同架构 DMG，通用 Mach-O 保持 universal DMG。

每次下载写系统临时目录 `pixiv-tool-update/<version>/<uuid>/<asset>.part`，成功才 rename；
失败/取消清理该次目录，完成包保留供安装。校验发布资产 size（精确大小存在时对账 Content-Length 与
实际接收长度；片段回退的 size=0 资产以响应 Content-Length 为对账基准，缺失时仅剩摘要与来源校验），
以及 GitHub asset digest（存在时必须为合法 SHA256）；缺摘要时仍检查大小，
摘要不是签名验证。URL 只接受本仓库固定 releases/download 前缀和该版本文件名。
元数据请求每通道 15s；安装包响应头等待 30s、每通道请求总超时 1h、分块 30s 无数据超时；先系统代理、请求失败再直连，
传输中断由用户重试，不断点续传。单次更新独立于 Pixiv 任务队列、不占抓取并发。
更新检查优先从重定向后的 URL 提取版本，成功即不读取 HTML；HTML 回退与
Release JSON 均按实际响应字节累计限制在 4MB，不能仅依赖 Content-Length。

`progress: Channel<UpdateProgress>` 载荷为 `{phase:downloading|opening,file_name,downloaded,total,bytes_per_second}`，
下载中最多约每 200ms 发一次、速度取近 2s 窗口，开始和完成各发一次；前端展示百分比、已下载/总大小、
KB/MB/GB 与速度。Esc 在下载中取消，在 opening 状态等待；下载中关闭背景交互。
Windows 由系统文件关联启动 MSI/NSIS；macOS `open`、Linux `xdg-open` 检查退出状态（15s 上限），
自动打开失败尝试文件管理器定位，失败再打开目录。两种系统打开均失败仍是下载成功，界面提供目录操作与完整路径。
安装引导按 Windows/macOS/Linux 显示覆盖安装、关闭当前应用和重启步骤，不自动退出、不宣称安装已完成。
离线回归：Rust 包匹配/来源验证/流式大小摘要测试 + `/tests/update.html` 真实组件 IPC mock（不会启动安装包）。

---

## 8. 开发与构建

构建基线固定 **Rust 1.99.1**（edition 2024），由仓库根 `rust-toolchain.toml` 的
`channel = "1.99.1"` 固定，`src-tauri/Cargo.toml` 的 `rust-version` 同步；本机与发版 CI
都显式选择该版本，不跟随 `stable` 漂移（ADR 0034）。该基线取代 ADR 0019 的「最低 1.88」
承诺，ADR 0019 关于 Vite / Rust 传递依赖安全补丁的理由仍然有效。Vite 使用 **6.4.3+**，
修复 Windows 开发服务器路径绕过与 UNC 路径触发的凭据泄露风险。

Agent 依赖 `rig-core` / `rig-agent` / `rig-memory` 均以 `=0.45.0` 精确锁定且
`default-features = false`，只启用协议与工具循环所需能力；不启用 server / MCP / vector /
embeddings / image / audio / pdf，也不使用其 reqwest / rustls 传输。内置 Agent 资源
（`src-tauri/resources/agent/**`：系统提示词、coordinator / translator / reviewer 角色、
smart-translator skill 与参考资料）通过 `include_str!` 编译期嵌入，运行时不依赖源码
仓库绝对路径或本机 skill 目录。wreq 仍锁 `6.0.0-rc.31`，不因 Agent 接入改变指纹与
代理语义。

### 8.1 开发模式

```bash
# 推荐入口（Windows 替换为 .\dev.ps1；无参数或 -h 显示帮助）
bash ./dev.sh install
bash ./dev.sh dev start  # 默认动作也是 start；后台 Vite + Rust 热重载 + 窗口
```

- 应用仍无额外后端进程/端口；开发脚本单独管理 Vite（9961，strictPort），
  `tauri dev` 复用该前端，不再重复执行 `beforeDevCommand`。
  debug 构建的数据目录为 `<repo>/data`、`<repo>/config`，与旧 dev 数据无缝衔接。
- Rust 改动自动重编译重启；前端走 Vite HMR
- 测试：`cd src-tauri && cargo test`（单测 + IPC 冒烟集成测试，全离线）；
  pixiv 在线接口实测走仓库根 `./dev.ps1 test-live`（Git Bash：`bash ./dev.sh test-live`）
  = `cargo test --locked --test pixiv_api -- --ignored --test-threads=1`：
  前置条件是本机已有登录态（系统凭据存储），**串行执行、会真实访问 pixiv**；
  前端类型检查：`cd frontend && pnpm build`
- **Windows 工具链**：本机（Windows 11）默认 gnu 工具链的 cdylib 链接超
  mingw ld 导出上限（"export ordinal too large"），**统一改用 1.99.1 的 MSVC 工具链**，
  cargo 命令前设置 `RUSTUP_TOOLCHAIN=1.99.1-x86_64-pc-windows-msvc`、
  `LIBCLANG_PATH`（LLVM 安装路径）、`CMAKE_GENERATOR="Visual Studio 17 2022"`；
  详见 AGENTS.md 开发命令一节
- 根目录 `dev.ps1` / `dev.sh` 封装 `install`、
  `dev [start|stop|restart]`、`frontend [start|stop|restart]`、
  `build [release|debug]`、`check`、`test`、`test-live`、`logs [app|子命令] [-f|--follow]`。
  无参数或 `-h` / `--help` 显示帮助；脚本以自身位置定位仓库。
  `dev` / `frontend` 默认 `start`，后台运行，重复启动幂等；通过 PID、启动时间
  和命令确认进程身份，过期记录自动清理。可验证并接管本仓库已有 Vite；
  不终止无关进程，不自动换端口。`dev stop` 只连带停止由它启动的前端，
  复用的已有前端保留；`frontend stop/restart` 不连带停止桌面开发进程。
  停止操作强制终止对应进程树，不能代替应用内正常退出。
  `dev start/restart` 等待后台工具链初始化完成后才报告成功；初始化失败返回错误，
  原因写入 `dev.log`。窗口需等待 Rust 编译完成，进度通过 `logs dev -f` 查看。
  服务状态存 `.dev/pids/`，操作输出写入 `.dev/logs/<子命令>.log`
  （UTF-8，启动服务或执行前台操作时覆盖），均不入库；应用日志仍在 §9 的位置。
  Windows 在独立后台进程初始化 VS 2022 / MSVC / LLVM，不修改启动服务的终端环境；
  继承的工具搜索路径去重，已有匹配的 x64 VS 环境直接复用，避免重复初始化撑爆
  cmd.exe 的命令行长度。Git Bash 委托 PowerShell 入口；
  macOS / Linux 使用当前工具链，服务管理另需 `lsof`。参数错误退出 2，
  前置条件错误退出 1；前台子进程失败保留其退出码。完整用法见
  `docs/PACKAGING.md` §1。

### 8.2 依赖管理

- 后端：**cargo**（`src-tauri/Cargo.lock` 入库；wreq/wreq-util 锁定
  6.0.0-rc.31 / 3.0.0-rc.14，**不可降到 Apache 化之前的版本**；rig-core / rig-agent /
  rig-memory 均锁 `=0.45.0` 且关闭默认 feature，升级需重新验证三协议、工具调用、流式
  事件与取消，见 ADR 0034）
- 工具链：**Rust 1.99.1**（`rust-toolchain.toml`、`Cargo.toml` 的 `rust-version`、`dev.ps1`
  / `dev.sh` 与发版 CI 四处一致；不使用 nightly / beta，也不静默漂移）
- 前端：**pnpm**（`pnpm-lock.yaml` 入库）
- 系统依赖：macOS/Linux 构建需 **cmake**（wreq 现场编译 BoringSSL）；
  Linux 运行需 webkit2gtk

### 8.3 构建

1. `cargo tauri build`（自动 `pnpm build` 前端 → 嵌入 → bundler 产出安装包）
2. 调试产物：`cargo tauri build --debug --no-bundle` →
   `src-tauri/target/debug/pixiv-tool`（Windows 为 `pixiv-tool.exe`）
3. 图标：`cargo tauri icon frontend/src/assets/icon.png`（已生成于
   `src-tauri/icons/`）
4. 脚本 `build` / `build release` 生成 release 安装包，`build debug` 生成
   debug 安装包（均调用本地 Tauri CLI，不使用 `--no-bundle`）。

### 8.4 发布方式（GitHub Actions 发版 CI）

发版走 GitHub Actions `release` 工作流（`.github/workflows/release.yml`）：
Actions 页手动触发（workflow_dispatch），输入 semver 版本号——构建前写入
`tauri.conf.json` / `Cargo.toml`（不回写仓库），六路并行出 Windows x64/arm64 MSI、
macOS universal/aarch64 DMG、Linux x64/arm64 的 AppImage + deb + rpm
（三处 arm64 均走 GitHub 原生 arm64 runner，不交叉编译——AppImage 的 linuxdeploy
不支持交叉出 ARM 包，原生 ARM 主机可以；Windows arm64 经 `src-tauri/cmake/`
下的工具链文件关掉 BoringSSL 汇编，见 `docs/PACKAGING.md` §6），最后**幂等覆盖**式
发布 `v<版本>` Release（先删旧 Release/Tag 再重建，可对同版本号重复执行）。
产物未签名。本地 `cargo tauri build`（§8.3）保留，与 CI 独立；详见
`docs/PACKAGING.md` §6。

Release 正文由发布任务拼装：`docs/releases/<版本>.md`（存在则作为更新说明）＋
按本次实际产物动态生成的安装包表格（避免文件名写死导致死链），因此每次发版前
需在 `docs/releases/` 下准备该版本的更新说明。

CI 只在构建期注入版本号、不回写仓库，因此**每次发版后需手动同步仓库内的版本号**
（`tauri.conf.json` / `Cargo.toml` / `Cargo.lock` / `frontend/package.json` 四处），
否则本地 dev 构建与账号菜单的版本回显仍停在旧版本。

---

## 9. 日志（`src-tauri/src/logging.rs`，tauri-plugin-log）

- **位置**：`data/logs/app.log`
- **级别**：INFO；dev（debug 构建）同时输出 stdout + 文件，release 仅文件
- **格式**：`%Y-%m-%d %H:%M:%S%.3f [级别] [target] 消息`（chrono 本地时间戳）
- **编码**：UTF-8
- **滚动**：单文件 8MiB（`MAX_LOG_FILE_SIZE`），插件默认 KeepOne，超出重建当前日志
- **维护**：设置维护组显示日志总大小、目录复制和确认后清理；清理截断当前及遗留轮转普通 .log 文件，后续日志继续追加。最近 200 行每 2 秒轮询，大小每 10 秒统计，仅维护页可见时执行，读取上限 64KiB。缓存显示译文/图片分类与总大小、目录复制、确认后清理；翻译运行时禁止清理译文，清理后阅读器同步去除旧回显。不删除下载作品、数据库或凭据（ADR 0027）。
- **翻译诊断**：HTTP 错误正文读取上限 64KiB，仅记录 code/type/message/param/request_id；流内错误保留同样的原始错误诊断，截断/提前结束/超时/连接失败记录成因。Key、Cookie 等凭据与 URL 脱敏，字符串限 4096 字并压平控制字符。非 JSON 错误正文只记格式与大小，模型 JSON 解析错误只记类别/行列/字节数；不记录请求、小说原文、生成正文或完整响应，界面仍显示概括错误（ADR 0027）。
- **Agent 日志固定键**：`novel=<id> job=<job_id> agent=<main|translator|reviewer> phase=<prepare_source|prepare_constraints|translate|review|repair|global_review|publish> chunk=<c0001|-> event=<事件> k=v…`（`agent/events.rs::log_line`）。`novel` / `job` / `agent` / `phase` 恒有，`chunk` 无分块时为 `-`；字段值经安全清洗、限长，只放计数、耗时、原因码与状态。标题不作主键，默认不记录小说标题或内容。
- **级别与密度**：INFO 记录排队 / 启动 / 终态、阶段起止与统计、子任务批次、确认 / 暂停 / 恢复 / 取消、缓存清理与每次压缩起止；执行中按小说至多每 5 秒聚合一次进展摘要，只在计数变化时输出；WARN 记录重试、截断、缩块、返工、预算临近 / 耗尽、结果未知与压缩失败（带原因码与下一动作）；ERROR 记录不可恢复的运行 / 存储失败（失败步骤、错误码、安全诊断）；DEBUG 仅在详细诊断开启时输出单个工具 / 请求 / 子任务的起止、耗时与长度，不输出正文或工具原始参数。
- **诊断开关**：设置二级「诊断」页的详细诊断只对本次应用运行生效，不写 `settings.json`，重启复位为关闭；开启时只把 `agent` target 的日志门限放到 DEBUG，不放大依赖库噪声。`RUST_LOG` 显式注入时尊重开发者设置、不覆盖门限。
- **禁止记录**：原文、译文、提示词、压缩摘要正文、API Key、Cookie、完整请求头或工具原始参数；压缩记录只含估算计数、耗时、保留范围与结果。

---

## 10. 安全与隐私

| 项 | 策略 |
|---|---|
| Cookie 存储 | 系统凭据存储（Windows Credential Manager / macOS Keychain / Linux Secret Service） |
| 网络监听 | **无**——不再有本地 HTTP 服务，IPC 仅限本 webview |
| 错误上报 | **不集成**（隐私优先） |
| 鉴权 | 不需要（无网络面） |
| 日志脱敏 | 日志不记录任何 Cookie 值；URL 记录去 query；Agent 日志不记录原文、译文、提示词或工具原始参数 |
| 翻译缓存 | 清理会同时删除 v1 旧译文与 v2 的原文、分块、会话、压缩记录、检查点与临时文件；暂停 / 待确认 / 已中断任务清理后不可恢复 |

Session 与 SauceNAO Key 输入采用 password 遮罩；翻译 Key 继续使用系统凭据库，
SauceNAO Key 的本机 settings.json 落点沿用现有契约。Git 忽略整个 `/data/` 和
`/config/`，覆盖翻译缓存、登录 profile 及未来新增用户文件。翻译缓存的清理不只删除
最终译文，也会删除恢复所需的检查点、会话与压缩记录；界面在确认前明确提示这一后果。
审查范围、回归结果和平台依赖残余风险见 [安全与资源审查记录](research/security-review-2026-10-04.md)。

---

## 11. 风险登记

| # | 风险 | 等级 | 缓解 |
|---|---|---|---|
| ~~R1~~ | ~~旧栈 pywebview `get_cookies()` 拿不到 HttpOnly PHPSESSID~~（历史，已随旧栈移除） | **✅ 已解决** | 旧栈 Spike 验证通过见 ADR 0005；现行方案为真实浏览器 CDP（ADR 0006/0008） |
| R2 | pixiv 接口变动或加强风控 | 中 | 限速保守（2 并发 + 0.4s）；429 暂停 60s；图片 CDN 走独立闸门（10 并发 + 同 URL 在途合并，不占 ajax 限速；CDN 侧 429 不重试、立即 502、无跨请求退避），遇风控可把 `CDN_MAX_CONCURRENT_DOWNLOADS` 调回 6 |
| R3 | WebView2 runtime 未预装（少数 Win10） | 低 | Tauri Windows 安装包默认 downloadBootstrapper 模式联网安装 |
| R4 | Linux WebKitGTK（webkit2gtk-4.1）缺失或版本过旧 | 中 | README 注明系统依赖，无法绕过 |
| ~~R5~~ | ~~PyInstaller hidden import 漏配~~（历史） | **✅ 已消除** | 随 Python 旧栈整体移除（ADR 0008），无打包 spec 需维护 |
| R6 | 长任务断点续传数据一致性 | 中 | 每篇抓完即写库（单条 INSERT 原子）；任务进度逐项落库 |
| R7 | 登录探测依赖 `/ajax/user/self` 扁平结构（顶层 userData/token） | 低 | `fetch_session_probe` 双分类错误 + 以非空 `userData.id` 为权威判据；pixiv 改版时重新探测（旧栈页面内 `meta.apiClient.token` 多路径 JS 提取已随旧栈移除） |
| R8 | Chromium CDP 登录依赖本机浏览器 | 低 | 支持 Chrome/Edge/Chromium；缺失时回退内嵌 webview 登录窗（ADR 0009，2026-08-21 真人登录实测 PASS）；残余风险：WebKit 指纹变化可能影响 reCAPTCHA 通过率，届时仍有手动 Cookie 兜底 |
| R9 | wreq 为 RC 版本且锁版本，风控指纹需随 pixiv 更新 | 中 | 升级 emulation 档位需重新 spike 验证；版本线不可低于 Apache 化（3.0.0-rc.12） |
| R10 | Linux GTK 传递依赖 glib 0.18.5 含 VariantStrIter 安全性问题（RUSTSEC-2024-0429） | 中 | Windows 不使用此依赖树；修复需 glib≥0.20 与 GTK/WebKit 上游兼容升级，保留风险并跟踪，详见 2026-10-04 安全审查记录 |
| ~~R10~~ | ~~三平台发布 CI 首跑未验证~~（历史） | **✅ 已消除** | GitCode 无流水线，release CI 已整体移除（2026-08-21），打包仅在本地按需执行（§8.4） |
| R11 | Rig 0.45 很新，后续升级预期包含 breaking change | 中 | 三个 crate 精确锁 `=0.45.0`；升级时重新验证三协议 wire、工具调用、流式事件、取消、端点重写与请求头（ADR 0034） |
| R12 | Rig 无条件开启 `serde_json/preserve_order`，JSON Map 迭代序变为插入序 | 中 | 依赖 Map 顺序的位置必须显式排序；作者作品对象形响应已修复，新代码不得依赖默认迭代序 |
| R13 | 翻译运行参数（并发、分块、预算、压缩）尚未用真实长篇校准 | 中 | 计划值只作起点；真实付费模型下校准分块、预算与默认并发，费用与请求不进入离线测试；校准结果同批回写设置说明与本文 |
| R14 | 整篇 Job 的模型调用量显著高于单页两轮 | 中 | 累计调用预算（默认 `32 + 16 × 初始分块数`）、三级并发闸门与暂停 / 取消共同控制成本；界面不承诺固定费用 |

---

## 12. ADR 索引

| # | 标题 | 文件 |
|---|---|---|
| 0001 | 选 pywebview + FastAPI + Vue3 而非 Electron/Tauri | [adr/0001-pywebview-fastapi-vue.md](adr/0001-pywebview-fastapi-vue.md) |
| 0002 | 嵌入式 WebView 登录主导 + 手动 cookie 兜底 | [adr/0002-login-strategy.md](adr/0002-login-strategy.md) |
| 0003 | Source + Crawler + Task 任务模型 | [adr/0003-task-model.md](adr/0003-task-model.md) |
| 0004 | Windows DPAPI 加密 cookie，跨平台接口预留 | [adr/0004-cookie-storage.md](adr/0004-cookie-storage.md) |
| 0005 | Spike 结果：pywebview cookie 探测可行性（R1 已解决） | [adr/0005-cookie-probe-result.md](adr/0005-cookie-probe-result.md) |
| 0006 | 真实 Chromium 登录 + macOS Keychain | [adr/0006-browser-login-keychain.md](adr/0006-browser-login-keychain.md) |
| 0007 | 插画抓取（原图/ugoira） | [adr/0007-illustration-crawling.md](adr/0007-illustration-crawling.md) |
| 0008 | 全量重构为 Tauri 2 + Rust，移除 Python 后端 | [adr/0008-tauri-rewrite.md](adr/0008-tauri-rewrite.md) |
| 0009 | 恢复 Webview 登录回退窗（Tauri 原生实现） | [adr/0009-webview-login-fallback.md](adr/0009-webview-login-fallback.md) |
| 0010 | 多账号登录态存储与切换 | [adr/0010-multi-account-login.md](adr/0010-multi-account-login.md) |
| 0011 | 登录窗必然以未登录态打开 | [adr/0011-fresh-login-window.md](adr/0011-fresh-login-window.md) |
| 0012 | 浏览模式：自有 UI 代理 pixiv 只读接口（内嵌浏览器保留） | [adr/0012-browse-mode-own-ui.md](adr/0012-browse-mode-own-ui.md) |
| 0013 | 移除内嵌 Pixiv 浏览器（/pixiv），自有浏览 UI 为唯一入口 | [adr/0013-remove-embedded-browser.md](adr/0013-remove-embedded-browser.md) |
| 0014 | 浏览访问历史持久化（SQLite 表 + 3 个 IPC 命令） | [adr/0014-browse-history-persistence.md](adr/0014-browse-history-persistence.md) |
| 0015 | 核心导航与非模态下载工作区 | [adr/0015-navigation-download-workspace.md](adr/0015-navigation-download-workspace.md) |
| 0016 | 应用内更新下载与安装引导 | [adr/0016-in-app-update-installation.md](adr/0016-in-app-update-installation.md) |
| 0016 | 作者关注与浏览写操作边界 | [adr/0016-author-follow.md](adr/0016-author-follow.md) |
| 0017 | 小说单页两轮翻译与共享设定集（已被 ADR 0035 部分取代，v1 缓存只读兼容） | [adr/0017-novel-page-translation.md](adr/0017-novel-page-translation.md) |
| 0018 | 小说翻译目标语言与语言一致的快速返回 | [adr/0018-novel-translation-target-language.md](adr/0018-novel-translation-target-language.md) |
| 0019 | 安全补丁驱动的构建工具链升级 | [adr/0019-security-patched-toolchain.md](adr/0019-security-patched-toolchain.md) |
| 0020 | 小说翻译支持三种模型接口协议 | [adr/0020-translation-api-formats.md](adr/0020-translation-api-formats.md) |
| 0021 | 翻译请求对 opencode 网关携带会话标识头 | [adr/0021-translation-session-header.md](adr/0021-translation-session-header.md) |
| 0022 | 浏览模式发表评论与回复（扩展 ADR 0016 写操作边界） | [adr/0022-browse-comment-posting.md](adr/0022-browse-comment-posting.md) |
| 0023 | 小说翻译单请求超时可配置（默认 10 分钟） | [adr/0023-translation-timeout-setting.md](adr/0023-translation-timeout-setting.md) |
| 0024 | 翻译设定集冲突保留既有值，不中断整页 | [adr/0024-translation-bible-conflict.md](adr/0024-translation-bible-conflict.md) |
| 0025 | 翻译生成请求改用流式，避免长请求被链路按空闲切断 | [adr/0025-translation-streaming.md](adr/0025-translation-streaming.md) |
| 0026 | 浏览模式发表官方表情（文本表情与贴图） | [adr/0026-browse-comment-emojis.md](adr/0026-browse-comment-emojis.md) |
| 0027 | 缓存与日志维护及模型错误诊断 | [adr/0027-storage-maintenance-diagnostics.md](adr/0027-storage-maintenance-diagnostics.md) |
| 0028 | 图片缓存上限可配置 + 三档分区淘汰 | [adr/0028-image-cache-limit-and-partitioned-eviction.md](adr/0028-image-cache-limit-and-partitioned-eviction.md) |
| 0029 | 小说翻译实时进度与后台续跑（Channel 载体已由 ADR 0035 的 `translation://job` 取代，目标保留） | [adr/0029-translation-progress-events.md](adr/0029-translation-progress-events.md) |
| 0030 | 翻译浮窗流式展示思考过程与 token 用量 | [adr/0030-translation-stream-preview-and-usage.md](adr/0030-translation-stream-preview-and-usage.md) |
| 0031 | 多小说并行后台翻译与底栏任务选择 | [adr/0031-parallel-novel-translation-and-status-bar-picker.md](adr/0031-parallel-novel-translation-and-status-bar-picker.md) |
| 0032 | 思考参数按协议归一化（Responses 嵌套 / Anthropic thinking） | [adr/0032-translation-thinking-params-per-format.md](adr/0032-translation-thinking-params-per-format.md) |
| 0033 | 翻译输出上限按输入分档、默认开启思考 | [adr/0033-translation-dynamic-output-cap-and-default-thinking.md](adr/0033-translation-dynamic-output-cap-and-default-thinking.md) |
| 0034 | Rig 通用 Agent 内核与 Rust 1.99.1 工具链 | [adr/0034-rig-agent-kernel-and-rust-toolchain.md](adr/0034-rig-agent-kernel-and-rust-toolchain.md) |
| 0035 | 整篇小说 Agent 翻译（Job 化，取代 ADR 0017 的单页两轮与文件布局） | [adr/0035-whole-novel-agent-translation.md](adr/0035-whole-novel-agent-translation.md) |

ADR 按需追加，不强制一次性写完。

---

## 13. 开放问题（V2 待定）

- Linux Secret Service cookie 实现
- EPUB exporter 实现
- 自适应限速（基于响应延迟与 429 频率）
- 任务启动弹窗恢复（"上次任务进行到 80/200，是否继续"）
- 安装器（NSIS / Inno Setup）
- 带签名的无人值守静默安装（应用内下载与安装引导已落地，见 §7.2）
- 搜索 / ~~收藏 / 用户主页浏览~~（搜索与用户主页已于 v1.2 随浏览模式落地，ADR 0012；收藏夹浏览与浏览态写操作仍待定）
- 小说内嵌图片下载
- 整篇翻译的跨平台验证：macOS / Linux 的 Agent 运行、文件原子提交与安装包尚未实机验收
  （Windows MSVC 已通过离线与集成测试）
- 翻译运行参数的真实长篇校准：并发、分块、压缩与预算默认值只作起点，尚未用真实付费
  模型与长篇样本校准
- v1 旧翻译缓存只读兼容的保留期限：当前不设自动迁移或删除时间点，只由维护入口统一清理
