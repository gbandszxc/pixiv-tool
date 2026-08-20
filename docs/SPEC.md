# Pixiv Tool · 技术规格书（SPEC）

> **状态**：v1.1 · Tauri 2 + Rust 全量重构（ADR 0008）· 2026-08-20
> **真相源**：本文档是项目开发的唯一真相源。任何架构变更须先更新本文档（或追加 ADR），再改代码。

---

## 1. 项目目标

构建一个本地运行的 **Pixiv 客户端工具**，初版（V1）聚焦 **Pixiv 小说抓取**，支持单篇、系列、用户全集三类来源，配合图形界面与登录态管理。

### 1.1 V1 范围

| 来源 | 说明 |
|---|---|
| 单篇小说 | 含多页小说（`pageCount > 1`） |
| 系列小说 | 系列内多篇独立保存，按系列顺序编号；**不合并**为单文件 |
| 指定用户的全部小说 | 用户名下所有作品，按系列分目录 + 散篇 |
| 单幅插画 | 单张/多页作品，**一律按原图**（`img-original` 直链）下载 |
| 指定用户的全部插画 | 用户全部插画+漫画作品（`profile/all` 的 illusts+manga），ugoira 动图存原始 zip |

### 1.2 V1 明确不做（延后 V2+）

- 搜索小说
- 用户收藏夹 / 插画作品列表页浏览（下载已支持，见 ADR 0007）
- 按 tag 批量抓取
- 小说内嵌图片下载（`[pixivimage:...]` / `[uploadedimage:...]` 标记）
- EPUB 输出（架构预留接口，不实现）
- 自适应限速（V1 用固定并发 + 429 暂停）
- 任务断点启动弹窗恢复
- Linux 登录功能（Windows / macOS 支持登录；Linux 走 keyring Secret Service 后端，未实机验证）
- 自动更新、安装器（V1 解压即用）
- 错误上报（Sentry 等）

---

## 2. 技术栈

### 2.1 总览

| 层 | 选型 |
|---|---|
| 桌面外壳 + 后端 | **Tauri 2（Rust）**，单进程，IPC 通信（见 ADR 0008） |
| 前端 | **Vue 3.4+ · TypeScript · Vite 5 · Vue Router 4（hash） · Pinia · @tauri-apps/api** |
| UI 组件库 | **Naive UI** |
| CSS | 原生 CSS + CSS Variables + Vue `<style scoped>` |
| HTTP 客户端 | **wreq 6（Chrome147 指纹伪装，BoringSSL）** |
| 数据库 | **SQLite（rusqlite）**，schema 与旧 Python 版逐字兼容 |
| 依赖管理 | 后端 **cargo** + 前端 **pnpm** |
| 打包 | **Tauri bundler**（三平台 CI 待建，见 docs/PACKAGING.md） |

### 2.2 不选的替代方案与理由

- ~~**Electron / Tauri**：体积大 / Rust 学习成本~~ —— **已被 ADR 0008 取代**：PyInstaller 分发负担与 curl_cffi 依赖促成的全量 Tauri 重构。
- **OAuth 逆向**：pixiv 风控极严，会锁号。
- **手动解密浏览器 cookie（Chrome App-Bound Encryption）**：v127+ 已基本不可行。
- **pywebview 原生 JS API**：同步阻塞、无 devtools 网络面板、SSE 推进度困难。
- **WebSocket**：V1 进度推送 SSE 足够，双向通信是过度设计。
- **Tailwind / UnoCSS**：4 个页面用不上原子化 CSS 的扩展性。
- **Element Plus / Ant Design Vue**：TS 类型与按需引入不如 Naive UI。
- **Poetry / pip / Python**：全栈已于 2026-08 迁往 Rust（ADR 0008），Python 后端已删除。

---

## 3. 系统架构

### 3.1 进程拓扑

```
┌─ pixiv-tool（Tauri 单进程）───────────────────────────────────┐
│                                                                │
│  ┌─ 主线程（tao 事件循环）─────────────────────────────┐      │
│  │  窗口生命周期 + 关闭确认（plugin-dialog，按语言中英） │      │
│  └──────────────────────────────────────────────────────┘      │
│                                                                │
│  ┌─ WebView（主窗）──────────────┐  ┌─ tokio runtime ─────┐   │
│  │  Vue3 SPA（hash 路由）        │  │  #[tauri::command]   │   │
│  │  ↕ invoke / listen（IPC）     │↔ │  抓取 asyncio→tokio  │   │
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
旧 app.db / settings.json 无缝沿用）。release：Windows exe 同级 portable；
macOS `~/Library/Application Support/pixiv-tool/`；Linux XDG 标准目录。
解析在 `src-tauri/src/paths.rs`。

### 3.4 目录结构

```
pixiv-tool/
├─ src-tauri/                   # Tauri 2 + Rust 后端
│  ├─ Cargo.toml                # wreq 6（指纹伪装，锁版本）/ rusqlite / keyring / tokio
│  ├─ tauri.conf.json           # devUrl 9961、frontendDist ../frontend/dist
│  ├─ capabilities/default.json # IPC 权限（core + dialog）
│  └─ src/
│     ├─ main.rs / lib.rs       # 入口与 Builder 装配（全部命令注册、关闭确认）
│     ├─ state.rs               # AppState：paths/settings/db/cookies/tasks
│     ├─ pixiv/                 # client（限速/重试/429）、api（/ajax typed）、csrf（会话探测）
│     ├─ core/                  # sources / crawler / illust_crawler / task_manager / exporter
│     ├─ auth/                  # browser_login（CDP）/ cdp（WebSocket 客户端）
│     ├─ commands/              # 25 个 #[tauri::command]（auth/tasks/settings/history/misc）
│     ├─ db.rs                  # rusqlite：schema 与查询（含 history UNION）
│     ├─ settings.rs            # settings.json 兼容加载/校验/迁移
│     ├─ cookies.rs             # keyring CookieStore
│     ├─ paths.rs / platform.rs / logging.rs
│     └─ tests/smoke_commands.rs# IPC 层集成冒烟
├─ frontend/                    # Vue3 + TS + Vite
│  ├─ src/
│  │  ├─ views/                 # CrawlView / IllustrationView / TasksView / HistoryView / SettingsView
│  │  ├─ components/
│  │  ├─ stores/                # Pinia（auth/tasks/settings/history，全走 invoke）
│  │  ├─ api/tauri.ts           # invoke 封装 + 错误归一化 + 契约类型
│  │  ├─ locales/               # zh-CN.ts / en-US.ts
│  │  ├─ styles/                # 全局 CSS Variables
│  │  ├─ router/                # hash 模式
│  │  └─ App.vue
│  └─ vite.config.ts            # port 9961 + strictPort（无 proxy）
├─ docs/                        # 本文档与 ADR
└─ README.md
```

---

## 4. 核心模块设计

### 4.1 登录与 Cookie（V1 最高风险点）

**方案**：真实 Chromium 独立 profile 主导 + 手动粘 PHPSESSID 兜底；未安装
Chromium 浏览器时回退 pywebview 登录窗。

**登录流程**：

```
[用户点"登录"]
      ↓
启动 Chrome / Edge / Chromium 独立 profile 加载 https://accounts.pixiv.net/login
      ↓
用户输账号密码 / 过验证码 / 过 2FA
      ↓
登录成功（重定向到 www.pixiv.net）
      ↓
通过 Chrome DevTools Protocol Storage.getCookies 取 HttpOnly cookie
      ↓
调用 /ajax/user/self 同时验证 userData 并取得 x-csrf-token
      ↓
CookieStore.save({ PHPSESSID, x-csrf-token, ... })  ← DPAPI 加密
      ↓
关闭登录窗，主界面刷新登录态
```

**Spike 已完成（2026-07-19）**：详见 [ADR 0005](adr/0005-cookie-probe-result.md)。A 方案完全成立，6 个验证点全部通过。提取 `x-csrf-token` 的正确路径是：

```
__NEXT_DATA__.props.pageProps.dehydratedState.queries[*].meta.apiClient.token
```

（不是早期假设的 `pageProps.token`，也不是 react-query 刷新后的 `state.data.token`——藏在 `meta.apiClient.token` 里，pixiv apiClient 自定义注入。）

**关键实现约束**：

1. Chrome / Edge 必须使用应用专属 `user-data-dir`，不得连接用户日常 profile。
2. CDP 只绑定随机 `127.0.0.1` 端口，拿到 Cookie 后立即关闭浏览器。
3. 匿名 `/ajax/user/self` 也会返回 HTTP 200 与 token；必须以非空 `userData.id`
   作为 Session 有效的权威判据。
4. pywebview 回退路径继续遵循 ADR 0005 的 callback / Morsel 提取约束。

**登录状态检查**：App 启动时调 `/ajax/user/self?lang=zh` 接口探测 cookie 有效性（返回 `userData.{id, pixivId, name}`）；失效则清空本地 cookie，UI 显示"未登录"。

### 4.2 抓取任务模型（Source + Crawler + Task）

#### Source 抽象（变）

```python
class NovelSource(ABC):
    """解析"来源"得到带序号的 novel id 流。"""
    @abstractmethod
    async def resolve(self, client: PixivClient) -> AsyncIterator[tuple[int, int | None]]:
        """yield (novel_id, series_order)，单篇 series_order=None"""

class SingleNovelSource(NovelSource): ...      # 直接 yield (id, None)
class SeriesSource(NovelSource): ...           # /ajax/novel/series/{id} → 按顺序 yield
class UserNovelsSource(NovelSource): ...       # /ajax/user/{id}/profile/all → 全部 novel id
```

#### Crawler（不变）

```python
class Crawler:
    async def run(self, source: NovelSource, task: Task):
        sem = asyncio.Semaphore(self.settings.concurrency)  # = 2
        async for novel_id, order in source.resolve(self.client):
            if self.db.is_downloaded(novel_id):
                task.inc_skipped(); continue
            async with sem:
                await self._crawl_one(novel_id, order, task)
                await asyncio.sleep(self.settings.request_interval)  # 0.4s
```

#### Task 状态机（V1 简化）

```
pending → running ⇄ paused
              ↓
       done / failed / canceled
```

省略 `pausing` 过渡态，pause 即时生效。

### 4.3 限速与容错

| 参数 | 值 |
|---|---|
| 并发 | `asyncio.Semaphore(2)` |
| 请求间隔 | `sleep(0.4)`（≈2.5 req/s 峰值） |
| 单请求超时 | 15s |
| 失败重试 | 3 次，指数退避 1s → 2s → 4s |
| 429 处理 | 全队列暂停 60s，记日志，恢复后继续 |
| 失败队列 | 内存 `failed: set[int]`，任务结束 UI 提示重试 |

以上参数**写死为常量**，V1 不暴露给用户配置。

### 4.4 Exporter（输出格式）

```python
class Exporter(ABC):
    @abstractmethod
    def export(self, novel: NovelData, target_dir: Path, series_order: int | None) -> list[Path]:
        """返回生成的文件路径列表"""

class TxtExporter(Exporter): ...        # V1
class MarkdownExporter(Exporter): ...   # V1（章节标记 → ##、[newpage] → ---）
class EpubExporter(Exporter): ...       # V2 stub，仅注册名不实现
```

**文件命名规则**：

| 来源 | 文件名 |
|---|---|
| 单篇 | `<title>_<novelId>.txt/.md` |
| 系列内 | `[NN]_<episode>_<title>.txt/.md`（padded 2-3 位） |
| 系列目录 | `<seriesTitle>_<seriesId>/` + 轻量 `series.json` |
| 用户集 | `<author>_<userId>/` 顶层，下按系列分目录 + 散篇 |
| 插画单作品 | `<title>_<artworkId>_p{N}.<ext>`（多页 p0..pN-1） |
| 插画用户全集 | `pic/users/<author>_<userId>/<title>_<artworkId>_p{N}.<ext>` |
| ugoira 动图 | `<title>_<artworkId>_ugoira.zip`（原图 = 帧序列 zip） |

**目录布局**：小说统一在输出目录 `novel/` 子目录下；插画在 `pic/` 子目录
（用户全集再套一层 `pic/users/<作者>_<userId>/`）。

---

## 5. 数据模型

### 5.1 SQLite Schema

文件位置：`data/app.db`

```sql
-- 已抓小说（去重 + 历史浏览 + 更新检测）
CREATE TABLE novels (
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
  status            TEXT NOT NULL          -- 'ok' | 'failed' | 'partial'
);
CREATE INDEX idx_novels_series ON novels(series_id);
CREATE INDEX idx_novels_author ON novels(author_id);

-- 任务进度（断点续传）
CREATE TABLE IF NOT EXISTS tasks (
  task_id     TEXT PRIMARY KEY,            -- uuid
  source_type TEXT NOT NULL,               -- 'single' | 'series' | 'user'
  source_id   TEXT NOT NULL,
  category    TEXT NOT NULL DEFAULT 'novel',  -- 'novel' | 'illustration'
  status      TEXT NOT NULL,               -- 'pending'|'running'|'paused'|'done'|'failed'|'canceled'
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

`config/settings.json`（portable 模式，与 exe 同级）：

```json
{
  "output_dir": "C:\\Users\\<user>\\Downloads\\pixiv-tool",
  "output_formats": ["txt", "markdown"],
  "language": "zh-CN",
  "theme": "auto",
  "backend_port": null,
  "max_wait_seconds": 180
}
```

- `output_dir` 默认 = **系统下载目录/pixiv-tool**（`~/Downloads/pixiv-tool`，区分
  平台统一实现，见 `storage/paths.py:default_output_dir`）；旧默认值字面量
  `"downloads"` 在加载时自动迁移为新默认。仍支持用户自填绝对路径或相对路径
  （相对路径锚定 data 目录）。

- V1 写死 portable 模式，不暴露"系统配置目录"切换开关。
- `backend_port: null` 时使用范围探测；用户可手动指定。
- `max_wait_seconds`：任务最大运行时长（秒），默认 180，设置页可配；
  任务运行超过该时长自动标记为 failed（**不含暂停时间**）。

### 5.3 Cookie 存储

- 统一走 **keyring crate**（`src-tauri/src/cookies.rs`）：service
  `pixiv-tool.cookies`、account `default`、value 为 cookies map 的紧凑 JSON
- Windows：系统 Credential Manager（旧版 DPAPI 文件 cookies.dat 不再使用）
- macOS：系统 Keychain（service/account 与旧 Python 版一致，登录态互读兼容）
- Linux：Secret Service（keyring linux-native 后端，未实机验证）

---

## 6. 前端

### 6.1 页面（侧边栏导航布局）

| 页面 | 路由 | 必需 |
|---|---|---|
| 抓取-小说 | `/` | ✅ |
| 抓取-插画 | `/illustration` | ✅ |
| 任务 | `/tasks`（支持小说/插画分类筛选） | ✅ |
| 历史 | `/history`（支持小说/插画分类切换） | ✅ |
| 设置 | `/settings` | ✅ |

### 6.2 i18n

- 框架：**vue-i18n**
- 语言：**简体中文（默认）+ 英文**
- 文件：`src/locales/zh-CN.ts` / `en-US.ts`
- 设置页可切，记忆到 `settings.json`

### 6.3 主题

- 选项：浅色 / 深色 / 跟随系统
- 实现：CSS Variables（根 `--bg` `--fg` `--accent`）+ Naive UI `n-config-provider` 注入 `darkTheme`

---

## 7. IPC 命令设计（invoke）

命令实现于 `src-tauri/src/commands/`，返回体沿用旧 HTTP 响应形状（snake_case）。
业务错误（旧 200+`{error}` 风格）在返回值内；校验类错误（旧 4xx/5xx detail）
reject string，前端 `errorMessage()` 归一。参数从 JS 侧以 camelCase 键传入。

| 命令 | 说明 |
|---|---|
| `auth_status` | 登录态探测（2s 超时；401/403 清 cookie，其余失败保留） |
| `auth_login` | 真实 Chromium CDP 登录（长阻塞，最长 300s）；无浏览器时返回 error 提示改用手动登录 |
| `auth_login_manual(phpsessid)` | 手动 PHPSESSID（normalize → 会话探测 → 存储） |
| `auth_logout` | 清空系统凭据存储 |
| `tasks_list(category?)` | 任务列表（按小说/插画过滤） |
| `task_create(sourceType, sourceId, formats, category)` | 创建抓取任务，后台 tokio 运行 |
| `task_pause` / `task_resume` / `task_cancel(taskId)` | 任务控制 |
| `task_retry_failed(taskId)` | 失败项重试（新任务，逐 id 串行，计数累计） |
| `task_delete(taskId)` / `tasks_delete(taskIds)` / `tasks_delete_completed` | 删除任务记录（非终态先取消；有不存在 id 整批不删） |
| `settings_get` / `settings_save(settings)` | 配置读写（白名单 6 键 + 校验） |
| `clear_logs` | 清空 app.log |
| `history_list(category, page, pageSize, keyword?)` | 历史联合分页查询（UNION，统一行形状） |
| `novel_delete` / `novels_batch_delete` / `novels_delete_all` | 小说记录删除（可选删文件） |
| `illustration_delete` / `illustrations_batch_delete` / `illustrations_delete_all` | 插画记录删除（可选删文件） |
| `open_novel_file(novelId)` / `open_illustration_folder(artworkId)` | 在系统文件管理器中定位 |

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
task://progress   {task_id, status, done, total, skipped, current_title?}
task://done       {task_id, status, done, total, failed, skipped}
```

每篇/每页完成即推送；TasksView 订阅后刷新列表，同时保留 2s 轮询兜底。

---

## 8. 开发与构建

### 8.1 开发模式

```bash
# 前置（一次性）
cd frontend && pnpm install

# 一键启动（Vite 9961 + Rust 热重载 + 窗口）
cargo tauri dev        # 仓库根执行；等价 cd frontend && pnpm tauri dev
```

- 无后端进程/端口管理——`tauri dev` 拉起 Vite（9961，strictPort）并加载
  debug 构建（数据目录用 `<repo>/data`、`<repo>/config`，与旧 dev 数据无缝衔接）
- Rust 改动自动重编译重启；前端走 Vite HMR
- 测试：`cd src-tauri && cargo test`（单测 + IPC 冒烟集成测试）；
  前端类型检查：`cd frontend && pnpm build`

### 8.2 依赖管理

- 后端：**cargo**（`src-tauri/Cargo.lock` 入库；wreq/wreq-util 锁定
  6.0.0-rc.31 / 3.0.0-rc.14，**不可降到 Apache 化之前的版本**）
- 前端：**pnpm**（`pnpm-lock.yaml` 入库）
- 系统依赖：macOS/Linux 构建需 **cmake**（wreq 现场编译 BoringSSL）；
  Linux 运行需 webkit2gtk

### 8.3 构建

1. `cargo tauri build`（自动 `pnpm build` 前端 → 嵌入 → bundler 产出安装包）
2. 调试产物：`cargo tauri build --debug --no-bundle` →
   `src-tauri/target/debug/pixiv-tool`
3. 图标：`cargo tauri icon frontend/src/assets/icon.png`（已生成于
   `src-tauri/icons/`）

### 8.4 跨平台 CI

原 Python 三平台 release workflow 已随旧栈移除；Tauri 版 CI 待建（需预装
cmake + Rust + pnpm，matrix 三平台跑 `cargo tauri build`）。见
`docs/PACKAGING.md`。

---

## 9. 日志

- **位置**：`data/logs/app.log`
- **级别**：INFO（dev 同时输出 console + file）
- **编码**：UTF-8
- **滚动**：V1 不做（V2 加 RotatingFileHandler）
- **清除**：设置页"清除日志"按钮

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
| ~~R1~~ | ~~pywebview `get_cookies()` 拿不到 HttpOnly PHPSESSID~~ | **✅ 已解决** | Spike 验证通过，见 ADR 0005 |
| R2 | pixiv 接口变动或加强风控 | 中 | 限速保守（2 并发 + 0.4s）；429 暂停 60s |
| R3 | WebView2 runtime 未预装（少数 Win10） | 低 | zip 内带 WebView2 Evergreen Bootstrapper |
| R4 | Linux pywebview 需 webkit2gtk | 中 | README 注明，无法绕过 |
| R5 | PyInstaller hidden import 漏配 | 中 | spec 文件显式声明；CI 构建测试 |
| R6 | 长任务断点续传数据一致性 | 中 | 每篇抓完即写库；事务包裹 |
| R7 | csrf token 路径依赖 pixiv 内部 react-query meta 结构 | 低 | `EXTRACT_AND_VERIFY_JS` 写多路径兜底（A/B/C/D）；pixiv 改版时重新探测 |
| R8 | Chromium CDP 登录依赖本机浏览器 | 中 | 支持 Chrome/Edge/Chromium；**缺失时无回退登录窗**（ADR 0008 裁剪），提示改用手动 Cookie 登录 |
| R9 | wreq 为 RC 版本且锁版本，风控指纹需随 pixiv 更新 | 中 | 升级 emulation 档位需重新 spike 验证；版本线不可低于 Apache 化（3.0.0-rc.12） |
| R10 | 三平台发布 CI 待重建（wreq 需 cmake） | 中 | 见 docs/PACKAGING.md；短期本地手动构建 |

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

ADR 按需追加，不强制一次性写完。

---

## 13. 开放问题（V2 待定）

- Linux Secret Service cookie 实现
- EPUB exporter 实现
- 自适应限速（基于响应延迟与 429 频率）
- 任务启动弹窗恢复（"上次任务进行到 80/200，是否继续"）
- 安装器（NSIS / Inno Setup）
- 自动更新
- 搜索 / 收藏 / 用户主页浏览
- 小说内嵌图片下载
