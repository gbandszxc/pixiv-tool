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
- 小说内嵌图片下载（`[pixivimage:...]` / `[uploadedimage:...]` 标记）
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
│  ├─ tests/                    # smoke_commands.rs（IPC 冒烟）/ browse_smoke.rs（浏览命令离线冒烟）
│  └─ src/
│     ├─ main.rs / lib.rs       # 入口与 Builder 装配（全部命令注册、pixiv-img 协议、关闭确认）
│     ├─ state.rs               # AppState：paths/settings/db/cookies/tasks/accounts
│     ├─ pixiv/                 # client（限速/重试/429）、api（/ajax typed）、csrf（会话与 web csrf 探测）、browse_api（浏览端点）
│     ├─ core/                  # sources / crawler / illust_crawler / task_manager / exporter
│     ├─ auth/                  # browser_login（CDP）/ cdp（WebSocket 客户端）/ webview_login（内嵌登录窗回退）
│     ├─ commands/              # 46 个 #[tauri::command]（auth 6 / browse_api 18 / tasks 9 / settings 3 / history 1 / misc 8 / app 1）
│     ├─ db.rs                  # rusqlite：schema 与查询（含 history UNION）
│     ├─ settings.rs            # settings.json 兼容加载/校验/迁移
│     ├─ cookies.rs             # keyring CookieStore
│     ├─ accounts.rs            # 多账号索引（accounts.json）+ 每账号凭据条目
│     ├─ image_proxy.rs         # pixiv-img 协议核心（磁盘缓存 + CDN 并发闸门 + 同 URL 在途合并 + immutable 长缓存响应头）
│     ├─ menu_bar.rs            # Windows 菜单栏默认隐藏 / Alt 唤出（AcceleratorKeyPressed + WM_EXITMENULOOP 收回）
│     └─ paths.rs / platform.rs / logging.rs
├─ frontend/                    # Vue3 + TS + Vite + Material Web（M3）
│  ├─ src/
│  │  ├─ views/                 # ToolsView（工具页签壳）/ CrawlView / IllustrationView / TasksView / HistoryView
│  │  │  └─ browse/             # BrowseHome/Channel/Discover/Feed/Search/Ranking/Bookmark + Work/Series/Author/Novel
│  │  ├─ components/            # auth/（LoginDialog / AccountMenu）navigation/ settings/（SettingsPanel / SettingsDialog）browse/（WorkCard / WorkGrid / BookmarkButton / ImageViewer / NovelContent / SectionTabs / RelatedGrid）
│  │  ├─ material.ts            # @material/web 组件按需 import
│  │  ├─ stores/                # Pinia（auth/tasks/settings/history，全走 invoke）
│  │  ├─ api/tauri.ts           # invoke 封装 + 错误归一化 + 契约类型
│  │  ├─ api/browse.ts          # 浏览契约类型 + 非 Tauri 环境的确定性 mock 层
│  │  ├─ api/devMock.ts         # auth/settings 的浏览器视觉验收 mock（仅 !isTauri() 生效）
│  │  ├─ utils/thumb.ts         # 缩略图 URL 档位改写（§6.4）
│  │  ├─ composables/useThumbTier.ts # 设置档位 → 合法 ThumbTier 的响应式读取
│  │  ├─ locales/               # zh-CN.ts / en-US.ts
│  │  ├─ styles/                # 全局 CSS Variables（--md-sys-color-* 等）
│  │  └─ router/                # hash 模式
│  └─ vite.config.ts            # port 9961 + strictPort（无 proxy）
├─ docs/                        # 本文档与 ADR、调研、agents 约定
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
  "backend_port": null,
  "max_wait_seconds": 180,
  "show_r18": true,
  "thumb_quality_grid": "medium",
  "thumb_quality_detail": "medium",
  "thumb_quality_fullscreen": "large"
}
```

- `output_dir` 默认 = **系统下载目录/pixiv-tool**（`~/Downloads/pixiv-tool`，
  跨平台统一实现，见 `src-tauri/src/paths.rs:default_output_dir`）；旧默认值
  字面量 `"downloads"` 在加载时自动迁移为新默认。仍支持用户自填绝对路径或
  相对路径（相对路径锚定 data 目录）。JSON 损坏时备份为
  `settings.json.corrupt-{mtime_ns}` 后重建默认。

- 数据/配置目录策略写死（见 §3.3），不暴露"系统配置目录"切换开关。
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
  不是任何档位的目标。`thumb_quality_fullscreen` 的消费者为全屏浮层（§6.4）。
  手改 settings.json 写入非法档位时，加载期回落到该键默认值（不强制回写文件）。
- `show_r18`：全局 R-18 展示开关（默认 `true`）。关闭后所有作品列表（首页/发现/动态/
  搜索/排行榜/收藏/作者页/频道页各板块/相关推荐/小说相关/系列目录行）在渲染期隐藏
  `x_restrict >= 1` 的作品；详情页仍可访问并保留模糊遮罩。
- 以上四键与既有键一样都在 `settings_save` 白名单内（见 §7）。

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

### 6.1 页面（侧边栏导航布局）

| 页面 | 路由 | 必需 |
|---|---|---|
| 工具-小说抓取 | `/tools/novel`（页签「小说」） | ✅ |
| 工具-插画抓取 | `/tools/illustration`（页签「插画」） | ✅ |
| 工具-任务 | `/tools/tasks`（页签「任务」） | ✅ |
| 工具-历史 | `/tools/history`（页签「历史」） | ✅ |
| 浏览-首页 | `/browse/home`（推荐流，换一批去重追加） | ✅ |
| 浏览-频道 | `/browse/illustration` `/browse/manga` `/browse/novel`（关注新作/推荐/排行/热门标签板块 + 顶部 R-18 快捷筛选（全部/一般向/R18）） | ✅ |
| 浏览-发现 | `/browse/discover`（按历史推荐，前端去重无限滚动） | ✅ |
| 浏览-动态 | `/browse/feed`（关注的新作品：插画/小说 × 全部/R-18） | ✅ |
| 浏览-搜索 | `/browse/search`（类型 tab + 排序/对象/匹配 + ID/链接直达） | ✅ |
| 浏览-排行榜 | `/browse/ranking`（插画/漫画/动图/小说 × 周期 + 日期导航） | ✅ |
| 浏览-收藏 | `/browse/bookmark`（插画·漫画/小说 × 公开/私密 + 标签筛选） | ✅ |
| 作品查看器 | `/browse/work/illust|:kind=illust|manga>/:id`（多页翻页、R-18 遮罩、相关推荐） | ✅ |
| 小说阅读器 | `/browse/work/novel/:id`（标记渲染、分页、系列导航） | ✅ |
| 系列目录 | `/browse/series/:id`（游标加载） | ✅ |
| 作者页 | `/browse/user/:id`（资料 + 插画/漫画/小说/收藏 tab） | ✅ |

**工具页**：页签壳 `ToolsView`（`/tools`），顶部 md-secondary-tab（复用 SectionTabs 封装）
即子路由导航、与路由双向同步；`/tools` 重定向到 `/tools/novel`。旧路径 `/`、
`/illustration`、`/tasks`、`/history` 保留为函数式 redirect，透传 query 与 hash
（浏览页「返填表单」跳旧路径并带 `sourceType`/`sourceId` 预填，依赖此处）。
`/settings` 路由已移除：账号菜单只留「设置」入口，设置表单承载于模态设置弹窗（见下）。

**侧边栏**：扁平菜单、无分组标题——浏览区（首页/插画/漫画/小说/发现/动态/搜索/
排行榜/收藏）在上，其后一条分隔线，最后是「工具」单项（`/tools*` 前缀高亮，
浏览项按路径精确匹配）。头部行 = logo + 标题 + 收起侧栏按钮（折叠态仅留展开按钮）；
折叠态 72px 只显示图标，窗口高度不足时导航区自身滚动。侧栏底部为**头像 chip**
（头像 + 账号名 + 展开箭头；未登录显示「账号」占位），点击在 chip 上方弹出**账号
菜单**（`AccountMenu`：轻量 popover，宽 264px、surface-container 底、12px 圆角、
既有轻阴影；透明遮罩点击外部或 Esc 关闭，无深色 scrim）；内容 = 账号列表（当前
账号 ✓，点击切换）/ 添加账号（复用 LoginDialog）+ 分隔线 + **「设置」入口**与
**退出登录**（danger 色，未登录置灰；点击后经原生 confirm 弹窗确认才执行）。设置入口点击后关菜单并打开**设置弹窗**
（`SettingsDialog`：原生 dialog、宽 `min(600px, 92vw)`、表单区自身滚动、
Esc / 点 backdrop / 标题栏 ✕ 关闭），表单为 SettingsPanel，含主题实时预览；
关闭弹窗时若预览未保存则恢复已保存主题并提示，设置项增多时在弹窗内分组扩展。
应用外壳恒为视口高，右侧内容区是唯一滚动容器，长内容不再拉长侧栏。

浏览模式详见 §6.4 与 ADR 0012。

### 6.2 i18n

- 框架：**vue-i18n**
- 语言：**简体中文（默认）+ 英文**
- 文件：`src/locales/zh-CN.ts` / `en-US.ts`
- 设置弹窗可切；启动期以 `localStorage["pixiv-tool-lang"]` 为准，settings.json 的
  `language` 加载后回填并向 localStorage 同步（两处同写，避免首屏语言闪变）

### 6.3 主题

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

### 6.4 浏览模式（browse，ADR 0012）

自有 UI 只读浏览 pixiv：8 个列表页 + 作品查看器 / 小说阅读器 / 系列目录 / 作者页，
路由见 §6.1。数据层要点：

- **接口**：全部走 `www.pixiv.net/ajax/*` 同源 GET + 既有 `PixivClient` 限速；
  端点与响应结构真相源为 `docs/research/pixiv-browse-api.md`（2026-10-01 实测，
  含失效端点勘误）。唯一例外是首页 street 流（POST，需 csrf token，见 ADR 0012 §3）。
- **IPC 契约**：11 个命令（§7），返回体统一 `BrowseWorkItem` 卡片结构
  （id/kind/title/author/cover/page_count/x_restrict/tags/series…），前端契约类型与
  mock 层在 `frontend/src/api/browse.ts`（非 Tauri 环境返回确定性样例数据，供浏览器
  视觉验收；生产不受影响）。
- **图片**：`<img>` 一律经自定义协议 `pixiv-img`（§3.1、ADR 0012 §2），磁盘缓存
  `<data>/cache/img/`（1GB 上限按 mtime 淘汰），前端 `pxSrc()` 封装。封面 URL 经
  `frontend/src/utils/thumb.ts` 按设置档位（`thumb_quality_grid` /
  `thumb_quality_detail` / `thumb_quality_fullscreen`，见 §5.2）改写：**只替换路径里已有的
  `/c/<尺寸段>/` 段**（列表卡片多为 `c/540x540_70`），**或给 `/img-master/img/`
  前缀插入 `/c/<尺寸段>/`**；`/img-original/`、`/img-zip-ugoira/`、`/user-profile/`
  等其它路径与 `original` 档一律原样返回（头像保留接口给的 `_50`/`_170` 后缀）。
  **绝不构造 `{datePath}` 与文件名**（拼错即 404）。
- **R-18 显示**：全局开关 `show_r18`（§5.2）关闭后，各列表在**渲染期**过滤
  `x_restrict >= 1` 的作品——只隐藏已取得的条目，**不重新请求**；`x_restrict`
  缺失（无法判定）的条目按 fail-closed 处理：仅在「全部」档可见，一般向与
  R-18 档都不收录。整页被滤空也不改变
  分页判定，仍按服务端返回的 `total` / `lastPage` / `next` 继续翻页。详情页仍可访问
  并保留模糊遮罩。频道页顶部另有 R-18 快捷筛选（全部 / 一般向 / R18）：默认跟随全局
  设置，手动选择只覆盖当前频道页（插画 / 漫画 / 小说三档各自独立、不串档）、
  不改写设置。排行榜的插画/漫画/动图条目在接口里
  **没有顶层 `x_restrict`**，由 `illust_content_type.sexual`（0 一般 / 1 R-18 /
  2 R-18G）补入 `x_restrict`（实测见 `docs/research/pixiv-browse-api.md` §9，
  部分 ugoira 条目的 `illust_content_type` 为空数组、无从判定，同样按 fail-closed
  在关闭 R-18 时隐藏），
  因此与其它列表同样参与过滤；小说排行条目自带顶层 `x_restrict`，无需补字段。
- **分页**：统一收敛为 `next_page` / `is_last_page` / `next_last_order`（游标）语义；
  发现页与首页推荐无服务端翻页，前端重复调用按 id 去重。
- **V1 限制**：只读（无点赞/收藏/关注）；ugoira 显示封面帧；小说内嵌图
  （`[pixivimage:]`）显示占位块；评论不展示。
- **打开原页 / 返填**：浏览页的「在浏览器中打开」走系统默认浏览器
  （官方 `tauri-plugin-opener`，capability `opener:default`）；
  频道页卡片与作品级页面另有「返填表单」→ 跳对应抓取页并预填 `sourceType` / `sourceId`。

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
| `settings_get` / `settings_save(settings)` | 配置读写（白名单 11 键 + 校验，含 `theme_color` 与 §5.2 四个新键） |
| `clear_logs` | 清空 app.log |
| `history_list(category, page, pageSize, keyword?)` | 历史联合分页查询（UNION，统一行形状） |
| `novel_delete` / `novels_batch_delete` / `novels_delete_all` | 小说记录删除（可选删文件） |
| `illustration_delete` / `illustrations_batch_delete` / `illustrations_delete_all` | 插画记录删除（可选删文件） |
| `open_novel_file(novelId)` / `open_illustration_folder(artworkId)` | 在系统文件管理器中定位 |
| `browse_home_feed` | 首页推荐流（street POST + csrf token，进程内 30min 缓存） |
| `browse_channel(kind)` | 频道仪表盘：关注新作/推荐/排行/最新投稿/热门标签（/ajax/top/*） |
| `browse_discover` | 发现推荐 60 条（无服务端翻页，前端去重复调） |
| `browse_follow_latest(kind, mode, page)` | 关注的新作品（p + isLastPage） |
| `browse_search(kind, word, order, mode, s_mode, type, page)` | 插画/漫画/小说搜索（total + lastPage） |
| `browse_ranking(kind, mode, page, date)` | 排行榜：illust/manga/ugoira 走 ranking.php?format=json，novel 走 /ajax/ranking/novel（每页 50，含 prev/next_date） |
| `browse_work_detail(kind, id)` | 作品详情：illust+pages+ugoira_meta / novel 全文（含 series 导航） |
| `browse_related(kind, id, limit)` | 相关推荐一次性池（recommend/init，page 参数无效） |
| `browse_user_profile(id)` | 作者资料（/ajax/user/{id}?full=1） |
| `browse_user_works(id, kind, page)` | 作者作品：profile/all 全集 id → 60/批 ids[] 批量 |
| `browse_novel_series(id, last_order)` | 系列元数据 + 目录（last_order 游标） |
| `app_exit` | 退出应用（前端确认框确认后调用，与 Cmd+Q 路径一致） |

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
captured_at），按 `captured_at` 倒序分页，keyword 同时过滤两表；前端抓取
时间列按东八区固定偏移显示。"打开所在文件夹"由 Rust 侧
`platform.rs::reveal_in_file_manager` 实现：Windows `explorer /select,`、
macOS `open -R`、Linux `xdg-open`；文件不存在回退父目录。

### 7.1 事件设计（listen）

```
task://progress   {task_id, status:"running", done, total, skipped, current_title?, failed_item?{id,title,error}}
task://done       {task_id, status, done, total, failed, skipped}
```

任务启动（0 进度）、每篇/每页成功或失败即推送（skipped 不发）；TasksView
订阅后刷新列表，同时保留 2s 轮询兜底。

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
- 测试：`cd src-tauri && cargo test`（单测 + IPC 冒烟集成测试）；
  前端类型检查：`cd frontend && pnpm build`
- **Windows 工具链**：本机（Windows 11）默认 gnu 工具链的 cdylib 链接超
  mingw ld 导出上限（"export ordinal too large"），**统一改用 MSVC 工具链**，
  cargo 命令前设置 `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`、
  `LIBCLANG_PATH`（LLVM 安装路径）、`CMAKE_GENERATOR="Visual Studio 17 2022"`；
  详见 AGENTS.md 开发命令一节
- 根目录 `dev.ps1` / `dev.sh` 封装 `install`、
  `dev [start|stop|restart]`、`frontend [start|stop|restart]`、
  `build [release|debug]`、`check`、`test`、`logs [app|子命令] [-f|--follow]`。
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
`tauri.conf.json` / `Cargo.toml`（不回写仓库），四路并行出 Windows x64 MSI、
macOS universal/aarch64 DMG、Linux x64 AppImage，最后**幂等覆盖**式发布
`v<版本>` Release（先删旧 Release/Tag 再重建，可对同版本号重复执行）。
产物未签名。本地 `cargo tauri build`（§8.3）保留，与 CI 独立；详见
`docs/PACKAGING.md` §6。

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
