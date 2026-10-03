# Pixiv Tool · 技术规格书（SPEC）

> **状态**：v1.2 · Tauri 2 + Rust 全量重构（ADR 0008）· 2026-10-01
> **真相源**：本文档是项目开发的唯一真相源。任何架构变更须先更新本文档（或追加 ADR），再改代码。
> v1.2：新增浏览模式（自有 UI 代理 pixiv 只读浏览，ADR 0012）。

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
- 自动更新（V1 不做；三平台分发与 CI 见 docs/PACKAGING.md）
- 错误上报（Sentry 等）

---

## 2. 技术栈

### 2.1 总览

| 层 | 选型 |
|---|---|
| 桌面外壳 + 后端 | **Tauri 2（Rust）**，单进程，IPC 通信（见 ADR 0008） |
| 前端 | **Vue 3.4+ · TypeScript · Vite 5 · Vue Router 4（hash） · Pinia · @tauri-apps/api** |
| UI 组件库 | **@material/web（Material 3）** |
| CSS | 原生 CSS + CSS Variables + Vue `<style scoped>` |
| HTTP 客户端 | **wreq 6（Chrome147 指纹伪装，BoringSSL）** |
| 数据库 | **SQLite（rusqlite）**，schema 与旧 Python 版逐字兼容 |
| 依赖管理 | 后端 **cargo** + 前端 **pnpm** |
| 打包 | **Tauri bundler**（仅本地按需打包：release / debug，无 CI） |

### 2.2 不选的替代方案与理由

- ~~**Electron / Tauri**：体积大 / Rust 学习成本~~ —— **已被 ADR 0008 取代**：旧栈 PyInstaller 分发负担与 curl_cffi 依赖促成的全量 Tauri 重构。
- **OAuth 逆向**：pixiv 风控极严，会锁号。
- **手动解密浏览器 cookie（Chrome App-Bound Encryption）**：v127+ 已基本不可行。
- ~~**pywebview 原生 JS API**：同步阻塞、无 devtools 网络面板~~ —— 旧栈（ADR 0001/0002）的历史理由，已随 ADR 0008 作废。
- **WebSocket**：进度推送用 Tauri 事件（`emit` / `listen`）已足够，双向通信是过度设计。
- **Tailwind / UnoCSS**：5 个页面用不上原子化 CSS 的扩展性。
- **Naive UI / Element Plus / Ant Design Vue**：整库自带一套视觉语言，与「自有 M3 设计系统」冲突（需整体覆盖主题）；@material/web 只提供 M3 原语，间距、布局与圆角由 `DESIGN.md` 自持。（本文档早期版本曾写 Naive UI，已作废。）
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
│  ┌─ WebView（主窗）──────────────┐  ┌─ tokio runtime ─────┐   │
│  │  Vue3 SPA（hash 路由）        │  │  #[tauri::command]   │   │
│  │  ↕ invoke / listen（IPC）     │↔ │  tokio::spawn 抓取   │   │
│  └───────────────────────────────┘  │  Semaphore(2)+0.4s   │   │
│                                     │  → emit task:// 事件 │   │
│                                     └──────────────────────┘   │
└────────────────────────────────────────────────────────────────┘
```

无本地 HTTP 服务、无端口、无 SSE——前端经 Tauri IPC 直接调 Rust 命令，
进度经 `task://progress` / `task://done` 事件推送（TasksView 同时保留 2s
轮询兜底）。

### 3.2 通信协议

- **命令类**：`invoke('<命令名>', args)`，命令与参数清单见 §7（返回体沿用旧
  HTTP 响应形状，snake_case；业务错误走返回值 `{error}`，校验错误走 reject
  string）。
- **进度推送**：Tauri 事件 `task://progress`、`task://done`。
- **安全边界**：无网络监听面；IPC 仅限本 webview。

### 3.3 数据与配置目录

dev（debug 构建）：`<repo>/data`、`<repo>/config`（与旧 Python dev 一致，
旧 app.db / settings.json 无缝沿用）。release：Windows exe 同级可写则
portable，不可写（MSI/NSIS 装进 Program Files）回退
`%LOCALAPPDATA%\pixiv-tool\`；macOS `~/Library/Application Support/pixiv-tool/`；
Linux XDG 标准目录。
解析在 `src-tauri/src/paths.rs`。

### 3.4 目录结构

```
pixiv-tool/
├─ src-tauri/                   # Tauri 2 + Rust 后端
│  ├─ Cargo.toml                # wreq 6（指纹伪装，锁版本）/ rusqlite / keyring / tokio
│  ├─ tauri.conf.json           # devUrl 9961、frontendDist ../frontend/dist
│  ├─ capabilities/default.json # IPC 权限（core + dialog + opener）
│  ├─ tests/                    # pixiv_api/（pixiv 接口测试唯一入口：live_read.rs / live_write.rs 在线实测 + offline_guard.rs 离线命令层冒烟）/ smoke_commands.rs（IPC 冒烟）
│  └─ src/
│     ├─ main.rs / lib.rs       # 入口与 Builder 装配（全部命令注册、pixiv-img 协议、关闭确认）
│     ├─ state.rs               # AppState：paths/settings/db/cookies/tasks/accounts
│     ├─ pixiv/                 # client（限速/重试/429）、api（/ajax typed）、csrf（会话与 web csrf 探测）、browse_api（浏览端点）
│     ├─ core/                  # sources / crawler / illust_crawler / task_manager / exporter
│     ├─ auth/                  # browser_login（CDP）/ cdp（WebSocket 客户端）/ webview_login（内嵌登录窗回退）
│     ├─ commands/              # 54 个 #[tauri::command]（auth 6 / browse_api 21 / tasks 9 / settings 3 / saucenao 1 / history 1 / browse_history 3 / misc 8 / app 1 / update 1）
│     ├─ db.rs                  # rusqlite：schema 与查询（含 history UNION）
│     ├─ settings.rs            # settings.json 兼容加载/校验/迁移
│     ├─ cookies.rs             # keyring CookieStore
│     ├─ accounts.rs            # 多账号索引（accounts.json）+ 每账号凭据条目
│     ├─ image_proxy.rs         # pixiv-img 协议核心（磁盘缓存 + CDN 并发闸门 + 同 URL 在途合并 + immutable 长缓存响应头）
│     ├─ saucenao.rs            # SauceNAO 以图识图客户端（file/url 两通道搜索、响应解析、错误分类；契约见 §7，接口调研见 docs/research/saucenao-api.md）
│     ├─ menu_bar.rs            # Windows 菜单栏默认隐藏 / Alt 唤出（AcceleratorKeyPressed + WM_EXITMENULOOP 收回）
│     └─ paths.rs / platform.rs / logging.rs
├─ frontend/                    # Vue3 + TS + Vite + Material Web（M3）
│  ├─ src/
│  │  ├─ views/                 # ToolsView（下载页签壳）/ TasksView / HistoryView / SaucenaoView（以图识图）
│  │  │  └─ browse/             # BrowseHome/Channel/Discover/Feed/Search/Ranking/Bookmark/History + Work/Series/Author/Novel
│  │  ├─ components/            # common/（AppPagination 公共分页）auth/（LoginDialog / AccountMenu）navigation/（核心导航 / PageBackButton）download/（共用下载表单 / 状态栏）settings/（SettingsPanel / SettingsDialog / sections.ts 分组定义）browse/（WorkCard / WorkGrid / BookmarkButton / ImageViewer / NovelContent / SectionTabs / RelatedGrid）
│  │  ├─ material.ts            # @material/web 组件按需 import
│  │  ├─ stores/                # Pinia（auth/tasks/settings/history，全走 invoke）
│  │  ├─ api/tauri.ts           # invoke 封装 + 错误归一化 + 契约类型
│  │  ├─ api/browse.ts          # 浏览契约类型 + 非 Tauri 环境的确定性 mock 层
│  │  ├─ api/saucenao.ts        # SauceNAO 契约类型 + invoke 封装 + 非 Tauri 环境的确定性 mock 层
│  │  ├─ api/devMock.ts         # auth/settings 的浏览器视觉验收 mock（仅 !isTauri() 生效）
│  │  ├─ utils/thumb.ts         # 缩略图 URL 档位改写（§6.4）
│  │  ├─ composables/useThumbTier.ts # 设置档位 → 合法 ThumbTier 的响应式读取
│  │  ├─ locales/               # zh-CN.ts / en-US.ts
│  │  ├─ styles/                # 全局 CSS Variables（--md-sys-color-* 等）
│  │  └─ router/                # hash 模式
│  └─ vite.config.ts            # port 9961 + strictPort（无 proxy）
├─ docs/                        # 本文档与 ADR、PIXIV-API 接口契约、调研、agents 约定
├─ scripts/                     # make_icon.sh（macOS 图标生成）
└─ README.md
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
   不得连接用户日常 profile。
2. CDP 只绑定随机 `127.0.0.1` 端口；拿到 Cookie 后立即经 Browser.close 关闭
   浏览器（3s 宽限后 kill）。
3. 匿名 `/ajax/user/self` 也返回 HTTP 200 与 token；必须以非空 `userData.id`
   作为 Session 有效的权威判据。探测返回 Invalid（401/403/非 200/无
   userData.id）时静默继续轮询——登录跳转尚未完成是正常情况。
4. Cookie 值不得写入日志（登录全程只记录端口 / 状态 / 计数类信息）。

**登录状态检查**（`commands/auth_cmds.rs::auth_status`）：App 启动时以同一套
wreq 指纹调 `/ajax/user/self?lang=zh` 探测 cookie 有效性（整体 2s 超时，返回
`userData.{id, pixivId, name}`）；401/403 → 清空本地 cookie，其余失败（含
超时、网络错误）→ 保留 cookie 仅记日志，UI 显示"未登录"。

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
  非空 PHPSESSID、source_id 为数字）通过后：INSERT tasks(pending) → 注册
  `TaskControls` → `tokio::spawn` 后台跑爬虫 → **立即返回 task_id**；
  校验失败不落库。
- `TaskControls`：`tokio::sync::watch` 暂停闸门（`send_replace` 写入，避免
  爬虫尚未 subscribe 时丢暂停请求）+ `AtomicBool` 取消标志；cancel 同时解除
  暂停，正在下载的当前项会下完再退出。
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

### 4.3 限速与容错（`pixiv/client.rs`）

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

**文件命名（`make_filename`）**：

| 来源 | 文件名 |
|---|---|
| 单篇 | `<title>_<novelId>.txt/.md` |
| 系列内 | `{zfill(order)}_<title>.txt/.md`，宽度 = page_count ≤ 99 ? 2 位 : 3 位 |
| 插画单页 | `<title>_<artworkId>_p{N}.<ext>`（多页 p0..pN-1；ext 从 URL 末段取，取不到 → bin） |
| ugoira 动图 | `<title>_<artworkId>_ugoira.zip`（原图 = 帧序列 zip） |

标题净化后为空：小说文件名只剩 id 后缀（`_12345.md`），插画以 artwork id
兜底（`safe_title_or_id`）。

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
  "novel_font_scale": 1.0,
  "novel_bg_color": "",
  "saucenao_api_key": ""
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
- `show_r18`：全局 R-18 展示开关（默认 `true`）。关闭后所有作品列表（首页/发现/动态/
  搜索/排行榜/收藏/作者页/频道页各板块/相关推荐/小说相关/系列目录行）在渲染期隐藏（频道手动筛选可在该频道覆盖全局档）
  `x_restrict >= 1` 的作品；详情页仍可访问；**关闭开关时**详情页对
  `x_restrict >= 1` 的作品保留模糊遮罩 + 「显示」确认（确认后本会话记忆、不持久化），
  **开启开关（默认）时不显示遮罩、直接展示**。
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
- `saucenao_api_key`：SauceNAO API Key（默认 `""`），「以图识图」（§7
  `saucenao_search`）的**必填前置**：trim 后为空一律拒绝发起搜索。在
  saucenao.com 免费注册后于 `user.php?page=search-api` 页面获取；仅保存在本机
  settings.json——不入库、不写日志、不进报错原文。接口行为与错误形态见
  `docs/research/saucenao-api.md`。
- 以上各键与既有键一样都在 `settings_save` 白名单内（见 §7）。

### 5.3 Cookie 存储（`src-tauri/src/cookies.rs` / `src-tauri/src/accounts.rs`）

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
- **多账号**（ADR 0010）：`default` 条目恒为**当前激活账号的镜像**，
  抓取客户端 / auth_status / webview 自动注入等读取方零感知；每账号另存
  独立条目 `u-<user_id>`（分片规则同上）；账号索引
  `config/accounts.json` **只存用户元信息**（user_id / pixiv_id / name /
  profile_img 原始头像 URL / avatar_file 本地缓存文件名 / saved_at 登记时间），
  **任何 cookie 都不落 config**。显式登录时双写镜像/账号条目，auth_status 只刷新
  索引元信息（旧单账号首次校验仍补建账号条目），避免启动时重复访问 Keychain；
  退出登录移除当前账号（镜像 + 条目 + 索引项），有剩余账号时自动激活列表
  中首个账号；auth_status 探测确定
  失效（401/403）时同样移除，避免死账号

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
| 作品查看器 | `/browse/work/illust|:kind=illust|manga>/:id`（多页纵向渐进加载、点击放大进入全屏翻页 + 胶卷缩略图、R-18 遮罩（仅关闭 show_r18 时）、相关推荐 / 评论面板（顶栏评论按钮切换）） | ✅ |
| 小说阅读器 | `/browse/work/novel/:id`（标记渲染、分页、系列导航、相关推荐 / 评论面板） | ✅ |
| 系列目录 | `/browse/series/:id`（游标加载） | ✅ |
| 作者页 | `/browse/user/:id`（资料 + 插画/漫画/小说/收藏 tab） | ✅ |
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
（分组顺序见 `components/settings/sections.ts`：通用 / 外观 / 图片与内容 / 高级 /
维护），含主题、配色与语言的实时预览；Esc / 点 backdrop / 标题栏 ✕ 关闭前先做脏检查，
有未保存改动则弹确认弹窗（继续编辑 / 放弃修改），放弃即回滚到已保存值并提示，
无改动直接关闭。设置项继续增多时在 `sections.ts` 与 SettingsPanel 内新增分组。
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

列表页按「返回与分区标题 + 右侧操作 → 分区导航 → 本页筛选 → 内容」组织。首页/发现、关注、我的分别以核心分区为标题，避免重复当前导航名称；频道保留具体标题。二级导航位于各列表页内，沿内容左边缘对齐，用原生路由链接胶囊标明当前页，发现的频道入口以细分隔线分组；详情不额外堆叠分区导航，侧栏仍继承来路。导航组间 16px，导航到下一区域 24px；页内筛选采用按内容定宽的 Material tabs，禁止均分整页宽度。关注类型与范围并排、窄窗自然换行。下载由父页面提供唯一标题和新建入口，下方为按内容定宽的任务/历史页签；任务类型筛选与批量管理左右分组，窄窗换行，清理完成任务并入管理组。

收藏标签从左侧固定栏改为横向换行工具带，按内容占位、最多三行后内部滚动；全部/未分类/标签计数、筛选契约、刷新与取消收藏行为保持不变，作品网格使用全宽。

现行决策见 ADR 0015。侧栏四入口发现（/browse/home）、关注（/browse/feed）、我的（/browse/bookmark）、下载（/tools/tasks）；二级导航组织现有路由，作者 / 系列 / 详情继承实际来源分组，直达归发现。56px 圆形放大镜按钮（无文字，保留可访问名称与 tooltip）+ Ctrl+K / Command+K 跳搜索并聚焦；按钮显示时分页行右侧预留 72px 空间以避免遮挡，以图识图入口在搜索页；阅读器改用顶栏搜索，全屏看图隐藏全局搜索与状态栏。

下载改为应用级非模态布局面板，宽工作区 ≥1120px 右栏 480px，否则底栏 min(45%,320px)，只放新建表单，小说/插画页签背景继承面板 surface-container。公共 fillDownloadForm(DownloadTarget) 不再导航，所有返填沿用 form/sourceType/sourceId 类型。关闭 / 跳页留草稿，账号变更清空；覆盖手动来源需确认、格式保留；失败留输入，成功更新任务并关闭面板、不跳页。旧 /、/illustration、/tools/novel、/tools/illustration 进入 /tools/tasks 并打开相应面板，保留其它 query/hash，消费 downloadForm/sourceType/sourceId；旧 /tasks、/history 继续兼容。

底部 32px 状态栏常驻且独立占位，空闲、新建入口、活跃任务数及代表任务进度回显；点击进度进入完整任务页。代表优先 running/pending/paused、同状态创建时间升序；done/total，未知总量不定进度，暂停无动画。应用级共享 task://progress、task://done 订阅与 2s 轮询兜底（有活跃任务或同步失败时）；同步失败保留快照并显示重试提示，任务页不重复订阅。

路由返回保留真实来路，无历史 replace 回所属核心入口；面板开关不入历史。原有浏览 KeepAlive 20 页 LRU、账号失效与首页快照不变；面板不重挂路由视图，布局切换恢复可见卡锚点 / 滚动。刷新按钮及 Ctrl+R / Command+R 保留，面板字段聚焦不刷新底层列表。无 Rust IPC / SQLite / settings.json / Pixiv API 变更；UI 规则见 DESIGN 与结构化伴随文件。

返回操作和历史可用性判定统一放在 `router/navigation.ts`，显式传入页面 `useRouter()` 的实际实例；禁止返回控件引用路由模块单例。避免热更新后单例与应用注入实例分离，造成地址变化而视图不返回。公共返回按钮、图片详情正常/错误态与 Escape、小说顶栏均使用同一逻辑；可用历史须指向已注册应用路由。按钮可见性及 tooltip 随完整路由变化重新判定，不缓存非响应式 HistoryState 的旧值。离线 `/tests/navigation-context.html#/browse/home` 使用与模块不同的应用路由实例复现该回归，配合 navigation 与 workspace 验收覆盖逐层来路、query、分支、旧链接及缓存位置。

### 6.2 i18n

- 框架：**vue-i18n**
- 语言：**简体中文（默认）+ 英文**
- 文件：`src/locales/zh-CN.ts` / `en-US.ts`
- 设置弹窗可切；启动期以 `localStorage["pixiv-tool-lang"]` 为准，settings.json 的
  `language` 加载后回填并向 localStorage 同步（两处同写，避免首屏语言闪变）

### 6.3 主题

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
- **IPC 契约**：浏览相关 24 个命令（浏览端点 21 + 浏览访问历史 3，§7）。浏览端点返回体统一
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
  `App.vue` 以路由 path 为键用 Vue KeepAlive 缓存浏览列表、作者与系列目录，
  最多 20 个页面、LRU 淘汰；返回时复用已加载内容、筛选、分页和主内容滚动位置，
  不执行挂载加载。插画/漫画/小说频道以及不同作者/系列互相隔离；搜索与排行的
  query 仍驱动参数变化，同一路径不因 query replace 重新挂载。
  KeepAlive 缓存仅在当前登录会话内存中，登录/退出/账号变更立即清空。
  首页另以已确认的 user_id 隔离 localStorage 卡片快照（`pixiv-tool-home-v1:`，
  24h 有效、每账号最多 120 条、仅展示字段，不含凭据）；挂载时先显示有效快照，
  同时请求最新推荐，成功替换并更新快照、失败保留旧卡片并显示错误。
  未确认账号不读写快照，已卸载页面的迟到响应不更新缓存。
  其余页面应用重启后首次进入重新加载。停用的 WorkGrid 断开无限加载观察器，
  不在后台继续自动翻页。
  作品查看器与小说阅读器不缓存，避免保留全屏/阅读键盘监听；它们返回上游列表时
  使用上述缓存。除首页首次挂载的后台刷新外，数据有变化时由用户主动刷新列表。
  渐进加载回归页：启动 `dev.ps1 frontend start` 后打开 `/tests/loading.html`。
- **图片**：`<img>` 一律经自定义协议 `pixiv-img`（§3.1、ADR 0012 §2），磁盘缓存
  `<data>/cache/img/`（1GB 上限按 mtime 淘汰），前端 `pxSrc()` 封装。封面 URL 经
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
- **作品查看器（插画/漫画）**：图片舞台为竖向滚动容器、**无自有底色**（与页面同底色，浅色
  主题即白），图片满幅（滚动区上/左/下 padding 为 0、页框直角，整体圆角由舞台 16px 圆角 +
  `overflow: hidden` 承担）；多页作品自上而下逐页排列，
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
  会话内记忆、不持久化，单页作品不进入双图。页框本身即全屏入口、可聚焦（`tabindex=0` +
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
  回显当前色（默认态 surface 底 + outline 描边），点击向上弹出居中于按钮的一排气泡
  （28px 圆形色块一行：默认 / 护眼绿 / 牛皮纸 / 暖杏 / 雾蓝 / 藕粉；弹出层与账号菜单
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
  格式化；缺失不冒充零）。小说在信息头元信息行追加三项计数与简介，正文阅读不变。
  标签解析兼容列表数组与详情的 `tags.tags[].tag`；显示原文，使用可键盘访问、可新标签
  打开的搜索链接，携带 `word`、对应 `kind` 与 `s_mode=s_tag_full` 精确标签检索。
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

---

## 7. IPC 命令设计（invoke）

命令实现于 `src-tauri/src/commands/`，返回体沿用旧 HTTP 响应形状（snake_case）。
业务错误（旧 200+`{error}` 风格）在返回值内；校验类错误（旧 4xx/5xx detail）
reject string，前端 `errorMessage()` 归一。参数从 JS 侧以 camelCase 键传入。

| 命令 | 说明 |
|---|---|
| `auth_status` | 登录态探测（2s 超时；401/403 清 cookie，其余失败保留） |
| `auth_login` | 真实 Chromium CDP 登录（长阻塞，最长 300s）；无浏览器时回退内嵌 webview 登录窗（ADR 0009），`PIXIV_TOOL_FORCE_WEBVIEW_LOGIN=1` 强制走 webview |
| `auth_login_manual(phpsessid)` | 手动 PHPSESSID（normalize → 会话探测 → 存储） |
| `auth_logout` | 退出当前账号：清 default 镜像 + 移除其账号条目与索引项；有剩余账号则自动激活首个 |
| `auth_accounts_list` | 已保存账号列表：`{active, accounts:[{user_id,pixiv_id,name,profile_img,avatar_file,avatar_url?,saved_at}]}` |
| `auth_account_switch(userId)` | 切换当前账号：目标条目写入 default → 索引 active；目标凭据缺失/失效 → Err |
| `tasks_list(category?)` | 任务列表（按小说/插画过滤） |
| `task_create(sourceType, sourceId, formats, category)` | 创建抓取任务，后台 tokio 运行 |
| `task_pause` / `task_resume` / `task_cancel(taskId)` | 任务控制 |
| `task_retry_failed(taskId)` | 失败项重试（新任务，逐 id 串行，计数累计） |
| `task_delete(taskId)` / `tasks_delete(taskIds)` / `tasks_delete_completed` | 删除任务记录（非终态先取消；有不存在 id 整批不删） |
| `settings_get` / `settings_save(settings)` | 配置读写（白名单 15 键 + 校验，含 `startup_page` / `theme_color` 与缩略图三档 / `show_r18` / `novel_font_scale` / `novel_bg_color` / `saucenao_api_key`） |
| `clear_logs` | 清空 app.log |
| `saucenao_search(sourceType, source, numres?)` | 以图识图搜索（SauceNAO；file=本地路径 POST multipart / url=公网图片 GET；pixiv 结果含 pid/作者可直接跳应用内详情；需在设置配置 API Key） |
| `history_list(category, page, pageSize, keyword?)` | 历史联合分页查询（UNION，统一行形状） |
| `browse_history_record(kind, workId, title, authorId, authorName, cover?, pageCount, xRestrict)` | 记录一次浏览访问（同 kind+workId 覆写并按访问时间置顶；作品详情页加载成功后上报） |
| `browse_history_list(page, pageSize, kind?)` | 浏览访问历史分页查询（`visited_at` 倒序，最近访问在前）；`kind` 省略=全部，否则 illust/manga/novel 过滤，非法值报错 |
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
| `browse_user_works(id, kind, page)` | 作者作品：profile/all 全集 id → 60/批 ids[] 批量 |
| `browse_novel_series(id, last_order)` | 系列元数据 + 目录（last_order 游标） |
| `browse_watchlist(kind)` | 追更列表：manga/novel 两个子 tab（/ajax/watch_list/*，按 maxPage 聚合 ≤20 页） |
| `browse_work_comments(kind, id, offset)` | 作品评论根列表（illusts/novels comments/roots，limit=10，offset 游标；作者关闭评论区 → `{"comments":[],"disabled":true}`，非报错） |
| `browse_comment_replies(kind, commentId, page)` | 评论回复列表（comments/replies，page 从 1） |
| `browse_bookmark_list(kind, rest, tag, offset, limit)` | 收藏列表（自己：illusts 48/页、novels 30/页；offset + total 翻页） |
| `browse_bookmark_tags(kind)` | 收藏标签（一次返回 public/private 两组，含「未分類」聚合标签） |
| `browse_bookmark_add(kind, id, restrict, tags)` | 添加收藏（全局 JSON 端点 + x-csrf-token；restrict 0 公开 / 1 非公开） |
| `browse_bookmark_remove(kind, id, bookmarkId)` | 取消收藏：插画走 ajax form，小说走旧式 `/novel/bookmark_setting.php` 表单 |
| `app_exit` | 退出应用（前端确认框确认后调用，与 Cmd+Q 路径一致） |
| `check_app_update` | 检查应用更新：解析 GitHub releases 页面（非 API）取最新稳定版并与当前版本比较；请求跟随系统代理、失败回退直连；发现新版本由前端弹窗询问，无更新/失败静默 |

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
1GB，CDN 并发 10、同 URL 在途合并，单个逻辑下载含全部重试与退避受 15s 总预算
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
```

任务启动（0 进度）、每篇/每页成功或失败即推送（skipped 不发）；TasksView
订阅后刷新列表，同时保留 2s 轮询兜底。任务列表以浅层响应式的完整快照替换：
事件、轮询和任务操作触发的查询共享一个在途请求，请求期间的新刷新需求合并为
一次尾随查询；所有等待者等到刷新链结束，失败后释放状态以允许下次重试。
离开任务页立即清除轮询与已登记监听；迟到的查询不能重新创建轮询，迟到的
监听登记立即解绑，避免路由往返后保留旧页面与重复刷新。

---

## 8. 开发与构建

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
  mingw ld 导出上限（"export ordinal too large"），**统一改用 MSVC 工具链**，
  cargo 命令前设置 `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`、
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
  6.0.0-rc.31 / 3.0.0-rc.14，**不可降到 Apache 化之前的版本**）
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
- **滚动**：单文件 8MB（`MAX_LOG_FILE_SIZE`），超出轮转为 `app_old.log`
- **清除**：设置弹窗"清除日志"按钮（`clear_logs` 将 app.log 清空写回）

---

## 10. 安全与隐私

| 项 | 策略 |
|---|---|
| Cookie 存储 | 系统凭据存储（Windows Credential Manager / macOS Keychain / Linux Secret Service） |
| 网络监听 | **无**——不再有本地 HTTP 服务，IPC 仅限本 webview |
| 错误上报 | **不集成**（隐私优先） |
| 鉴权 | 不需要（无网络面） |
| 日志脱敏 | 日志不记录任何 Cookie 值；URL 记录去 query |

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
| ~~R10~~ | ~~三平台发布 CI 首跑未验证~~（历史） | **✅ 已消除** | GitCode 无流水线，release CI 已整体移除（2026-08-21），打包仅在本地按需执行（§8.4） |

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
| 0016 | 作者关注与浏览写操作边界 | [adr/0016-author-follow.md](adr/0016-author-follow.md) |

ADR 按需追加，不强制一次性写完。

---

## 13. 开放问题（V2 待定）

- Linux Secret Service cookie 实现
- EPUB exporter 实现
- 自适应限速（基于响应延迟与 429 频率）
- 任务启动弹窗恢复（"上次任务进行到 80/200，是否继续"）
- 安装器（NSIS / Inno Setup）
- 自动更新
- 搜索 / ~~收藏 / 用户主页浏览~~（搜索与用户主页已于 v1.2 随浏览模式落地，ADR 0012；收藏夹浏览与浏览态写操作仍待定）
- 小说内嵌图片下载
