# Pixiv Tool · 技术规格书（SPEC）

> **状态**：v1.0 · 已通过 grilling 评审 · 2026-07-19
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

### 1.2 V1 明确不做（延后 V2+）

- 搜索小说
- 用户收藏夹 / 用户主页作品列表浏览
- 按 tag 批量抓取
- 小说内嵌图片下载（`[pixivimage:...]` / `[uploadedimage:...]` 标记）
- EPUB 输出（架构预留接口，不实现）
- 自适应限速（V1 用固定并发 + 429 暂停）
- 任务断点启动弹窗恢复
- macOS / Linux 登录功能（仅 Windows 全功能；Mac/Linux 仅可启动与浏览 UI）
- 自动更新、安装器（V1 解压即用）
- 错误上报（Sentry 等）

---

## 2. 技术栈

### 2.1 总览

| 层 | 选型 |
|---|---|
| 桌面外壳 | **pywebview 4+**（Windows: WebView2 / macOS: WKWebView / Linux: WebKitGTK） |
| 后端 | **Python 3.11+ · FastAPI · uvicorn** |
| 前端 | **Vue 3.4+ · TypeScript · Vite 5 · Vue Router 4 · Pinia · axios** |
| UI 组件库 | **Naive UI** |
| CSS | 原生 CSS + CSS Variables + Vue `<style scoped>` |
| HTTP 客户端 | **httpx**（async） |
| 数据库 | **SQLite**（标准库 `sqlite3`） |
| 依赖管理 | 后端 **uv** + 前端 **pnpm** |
| 打包 | **PyInstaller --onedir** |
| CI | **GitHub Actions** 三平台 matrix |

### 2.2 不选的替代方案与理由

- **Electron / Tauri**：体积大 / Rust 学习成本，pywebview 在 Windows 上调用 Edge WebView2 已足够。
- **OAuth 逆向**：pixiv 风控极严，会锁号。
- **手动解密浏览器 cookie（Chrome App-Bound Encryption）**：v127+ 已基本不可行。
- **pywebview 原生 JS API**：同步阻塞、无 devtools 网络面板、SSE 推进度困难。
- **WebSocket**：V1 进度推送 SSE 足够，双向通信是过度设计。
- **Tailwind / UnoCSS**：4 个页面用不上原子化 CSS 的扩展性。
- **Element Plus / Ant Design Vue**：TS 类型与按需引入不如 Naive UI。
- **Poetry / pip**：uv 在 2024-2025 已成事实标准，速度快 10-100 倍。

---

## 3. 系统架构

### 3.1 进程拓扑

```
┌─ pixiv-tool.exe (主进程) ─────────────────────────────────────┐
│                                                                │
│  ┌─ MainThread ─────────────────────────────────────────┐     │
│  │  1. find_available_port() 探测 [9962, 9999]          │     │
│  │  2. threading.Thread(uvicorn.Server).start()         │     │
│  │  3. pywebview.create_window(url=http://127.0.0.1:p/) │     │
│  │  4. webview.start()  ← 阻塞，关闭即退出              │     │
│  └──────────────────────────────────────────────────────┘     │
│                                                                │
│  ┌─ FastAPI Thread (uvicorn) ─────┐  ┌─ WebView2 (主窗) ───┐  │
│  │  /api/auth/*                    │  │  Vue3 SPA           │  │
│  │  /api/novel/{id}                │  │  ↕ HTTP/SSE          │  │
│  │  /api/series/{id}               │  │  (localhost)        │  │
│  │  /api/user/{id}/novels          │  └─────────────────────┘  │
│  │  /api/tasks/{id}/events (SSE)   │                            │
│  │  / (StaticFiles 挂载 SPA)       │                            │
│  └─────────────────────────────────┘                            │
│                                                                │
│  ┌─ 抓取 asyncio (FastAPI 线程内事件循环) ───────────────┐    │
│  │  asyncio.Semaphore(2) + 0.4s sleep                    │    │
│  │  → 通过 SSE 推进度给前端                               │    │
│  └────────────────────────────────────────────────────────┘    │
└────────────────────────────────────────────────────────────────┘
```

### 3.2 通信协议

- **命令类**：HTTP REST（`POST /api/...`、`GET /api/...`）
- **进度推送**：SSE（`GET /api/tasks/{id}/events` 返回 `text/event-stream`）
- **安全边界**：FastAPI 只绑 `127.0.0.1`，V1 不做 token 鉴权（本机信任），留 TODO 待未来加随机 token

### 3.3 端口策略

| 模式 | 端口 |
|---|---|
| **dev 前端**（Vite） | **固定 9961** |
| **dev 后端**（uvicorn） | **固定 9962** |
| **prod 后端** | **范围探测 `[9962, 9999]`**，全占用回退 `[10000, 19999]`，仍失败抛 `PortAllocationError` |

dev 端口固定便于 Vite proxy、浏览器收藏；prod 动态端口写入 `os.environ['PIXIV_TOOL_PORT']` 供 pywebview 读取。

### 3.4 目录结构

```
pixiv-tool/
├─ src/
│  └─ pixiv_tool/               # Python 后端包（snake_case,PEP 8）
│     ├─ main.py                # 入口：探测端口 + 起 uvicorn + 起 pywebview
│     ├─ api/                   # FastAPI 路由
│     │  ├─ auth.py             # 登录、cookie 管理
│     │  ├─ novels.py           # 单篇、系列、用户
│     │  ├─ tasks.py            # 任务 + SSE
│     │  └─ system.py           # /api/health + /api/ping + /api/test/events
│     ├─ core/                  # 业务核心
│     │  ├─ pixiv_client.py     # httpx + 限速 + 重试
│     │  ├─ crawler.py          # Crawler 编排（id 流 → 抓取）
│     │  ├─ source.py           # NovelSource 抽象 + 3 实现
│     │  ├─ task.py             # Task 状态机
│     │  └─ exporter.py         # Exporter 接口 + txt/md 实现
│     ├─ auth/                  # 登录窗
│     │  └─ login_window.py     # pywebview 登录窗 + cookie 提取
│     ├─ storage/               # 持久化
│     │  ├─ db.py               # SQLite + schema 初始化
│     │  ├─ models.py           # dataclass 模型
│     │  ├─ cookies.py          # CookieStore 接口 + 工厂
│     │  ├─ cookie_dpapi.py     # Windows DPAPI（V1 实现）
│     │  └─ settings.py         # JSON 配置
│     ├─ logging_config.py      # logging 配置
│     └─ static/                # 前端构建产物（pnpm build 复制,.gitignore）
├─ tests/                       # pytest 测试（src layout 下保留 root）
├─ frontend/                    # Vue3 + TS + Vite
│  ├─ src/
│  │  ├─ views/                 # CrawlView / TasksView / HistoryView / SettingsView
│  │  ├─ components/
│  │  ├─ stores/                # Pinia（auth/tasks/settings/history）
│  │  ├─ api/                   # axios 封装
│  │  ├─ locales/               # zh-CN.ts / en-US.ts
│  │  ├─ styles/                # 全局 CSS Variables
│  │  ├─ router/
│  │  └─ App.vue
│  ├─ vite.config.ts            # proxy /api/* → 127.0.0.1:9962
│  └─ package.json
├─ scripts/
│  ├─ dev.ps1                   # Windows dev 服务管理（start/stop/restart/logs/status）
│  ├─ dev.sh                    # macOS/Linux 版（V2 由 dev.ps1 转换）
│  ├─ build.py                  # pnpm build + pyinstaller
│  └─ find_port.py              # 独立端口探测工具（供 spike 与 prod 复用）
├─ pyproject.toml               # 项目配置（hatchling + uv + pytest）
├─ pixiv-tool.spec              # PyInstaller 打包配置
├─ .github/workflows/release.yml# 三平台 CI
├─ docs/                        # 本文档与 ADR
└─ README.md
```

---

## 4. 核心模块设计

### 4.1 登录与 Cookie（V1 最高风险点）

**方案**：D —— WebView2 嵌入登录主导（A）+ 手动粘 PHPSESSID 兜底（C）。

**登录流程**：

```
[用户点"登录"]
      ↓
弹 pywebview 窗口加载 https://accounts.pixiv.net/login
      ↓
用户输账号密码 / 过验证码 / 过 2FA
      ↓
登录成功（重定向到 www.pixiv.net）
      ↓
调用 webview.get_cookies() 取 cookie  ← ✅ Spike 已验证（见 ADR 0005）
      ↓
导航到 www.pixiv.net，提取 x-csrf-token
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

**关键实现约束**（spike 调查得出）：

1. **`evaluate_js` 必须用 callback 模式**：同步模式不 await Promise，async 函数返回 None。所有需要 await Promise 的 JS 调用都要用 `threading.Event` 把 callback 同步包装。
2. **JS 写成 `new Promise(...)` 而非 `async () => {...}`**：pywebview 的 Promise 识别更可靠。
3. **`window.get_cookies()` 返回 `list[SimpleCookie]`**：每个 SimpleCookie 是 dict-like 容器，要遍历 `.items()` 取 `(name, Morsel)` 对。`SimpleCookie.Morsel` 继承自 dict，判断 dict 路径时要显式排除 Morsel。
4. **pywebview 必须配置 `private_mode=False` + `http_server=True` + 固定 `http_port`**：cookie 才会持久化到 WebView2 数据目录，跨会话复用。

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
CREATE TABLE tasks (
  task_id     TEXT PRIMARY KEY,            -- uuid
  source_type TEXT NOT NULL,               -- 'single' | 'series' | 'user'
  source_id   TEXT NOT NULL,
  status      TEXT NOT NULL,               -- 'pending'|'running'|'paused'|'done'|'failed'|'canceled'
  total       INTEGER DEFAULT 0,
  done        INTEGER DEFAULT 0,
  skipped     INTEGER DEFAULT 0,
  failed_ids  TEXT DEFAULT '[]',           -- JSON array of novel_id
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL,
  error       TEXT
);
```

### 5.2 配置文件

`config/settings.json`（portable 模式，与 exe 同级）：

```json
{
  "output_dir": "downloads",
  "output_formats": ["txt", "markdown"],
  "language": "zh-CN",
  "theme": "auto",
  "backend_port": null
}
```

- V1 写死 portable 模式，不暴露"系统配置目录"切换开关。
- `backend_port: null` 时使用范围探测；用户可手动指定。

### 5.3 Cookie 存储

- 位置：`config/cookies.dat`
- 加密：**Windows DPAPI**（`win32crypt.CryptProtectData`，纯 ctypes 调 `crypt32.dll`）
- 接口：`CookieStore` 抽象，按 `sys.platform` 工厂选择实现
- V1：仅 Windows 完整实现；macOS（keychain）/ Linux（secretstorage）为 stub，抛 `NotImplementedError`

---

## 6. 前端

### 6.1 页面（侧边栏导航布局）

| 页面 | 路由 | 必需 |
|---|---|---|
| 抓取 | `/` | ✅ |
| 任务 | `/tasks` | ✅ |
| 历史 | `/history` | ✅ |
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

## 7. API 设计

### 7.1 认证

| Method | Path | 说明 |
|---|---|---|
| GET | `/api/auth/status` | 查询当前登录态（含用户名） |
| POST | `/api/auth/login` | 打开 pywebview 登录窗 |
| POST | `/api/auth/login/manual` | 手动提交 PHPSESSID（C 兜底） |
| POST | `/api/auth/logout` | 清空本地 cookie |

### 7.2 抓取

| Method | Path | 说明 |
|---|---|---|
| GET | `/api/novel/{id}` | 查询单篇小说元数据 |
| POST | `/api/tasks` | 创建抓取任务（body: `{source_type, source_id, formats}`） |
| GET | `/api/tasks` | 任务列表 |
| GET | `/api/tasks/{id}` | 任务详情 |
| GET | `/api/tasks/{id}/events` | **SSE** 进度流 |
| POST | `/api/tasks/{id}/pause` | 暂停 |
| POST | `/api/tasks/{id}/resume` | 继续 |
| POST | `/api/tasks/{id}/cancel` | 取消 |
| POST | `/api/tasks/{id}/retry-failed` | 重试失败项 |

### 7.3 历史

| Method | Path | 说明 |
|---|---|---|
| GET | `/api/novels` | 分页查询已抓小说（支持 series_id/author_id/关键词过滤） |
| DELETE | `/api/novels/{id}` | 删除记录（可选删文件） |

### 7.4 设置

| Method | Path | 说明 |
|---|---|---|
| GET | `/api/settings` | 读取配置 |
| PUT | `/api/settings` | 更新配置 |
| POST | `/api/settings/clear-logs` | 清除本地日志 |

### 7.5 健康检查

| Method | Path | 说明 |
|---|---|---|
| GET | `/api/health` | 返回 `{status: "ok"}`（dev.ps1 status 命令探活用） |

### 7.6 SSE 事件设计

```
event: progress
data: {"task_id":"...","done":12,"total":50,"skipped":2,"current_title":"第3话 风起"}

event: item
data: {"task_id":"...","novel_id":12345,"status":"ok","title":"..."}

event: failed
data: {"task_id":"...","novel_id":12345,"error":"HTTP 404"}

event: done
data: {"task_id":"...","done":50,"failed":1,"skipped":2}
```

---

## 8. 开发与构建

### 8.1 开发模式

- **dev 端口固定**：前端 9961、后端 9962
- **统一脚本**：`scripts/dev.ps1`，子命令 `start|stop|restart|logs|status`，支持 `[all|frontend|backend]` 参数
- **PID/日志**：`.dev/pids/{frontend,backend}.pid` + `.dev/logs/{frontend,backend}.log`
- **健康检查**：frontend → `GET 127.0.0.1:9961/`；backend → `GET 127.0.0.1:9962/api/health`
- **注释规范**：所有平台特定命令在注释里标注 sh 等价物，方便 V2 转 `dev.sh`
- **后端热重载**：uvicorn `reload=True`
- **前端热重载**：Vite HMR

### 8.2 依赖管理

- 后端：`uv`（`uv sync` 安装，`uv.lock` 入库）
- 前端：`pnpm`（`pnpm install` 安装，`pnpm-lock.yaml` 入库）

### 8.3 构建

1. 前端 `pnpm build` → 产物输出到 `src/pixiv_tool/static/`
2. FastAPI 用 `StaticFiles` 挂载 `src/pixiv_tool/static/`，SPA fallback 到 `index.html`
3. `pyinstaller pixiv-tool.spec --onedir` 打包
4. 产物：`dist/pixiv-tool/`（解压即用）

### 8.4 跨平台 CI

`.github/workflows/release.yml`：push tag `v*` 触发，matrix `[windows-latest, macos-latest, ubuntu-latest]`，分别构建并上传 Release。

产物：
- `pixiv-tool-windows-x64.zip`
- `pixiv-tool-macos-x64.zip`（PyInstaller 出 `.app`，压 zip）
- `pixiv-tool-linux-x64.tar.gz`（README 注明需预装 `webkit2gtk`）

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
| Cookie 存储 | Windows DPAPI 加密，仅当前 Windows 用户可解 |
| 网络监听 | FastAPI 仅绑 `127.0.0.1`，拒绝外部连接 |
| 错误上报 | **不集成**（隐私优先） |
| 鉴权 | V1 无（本机信任），TODO 留随机 token |
| 日志脱敏 | PHPSESSID 等敏感字段在日志中掩码（仅前 8 位） |

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

---

## 12. ADR 索引

| # | 标题 | 文件 |
|---|---|---|
| 0001 | 选 pywebview + FastAPI + Vue3 而非 Electron/Tauri | [adr/0001-pywebview-fastapi-vue.md](adr/0001-pywebview-fastapi-vue.md) |
| 0002 | 嵌入式 WebView 登录主导 + 手动 cookie 兜底 | [adr/0002-login-strategy.md](adr/0002-login-strategy.md) |
| 0003 | Source + Crawler + Task 任务模型 | [adr/0003-task-model.md](adr/0003-task-model.md) |
| 0004 | Windows DPAPI 加密 cookie，跨平台接口预留 | [adr/0004-cookie-storage.md](adr/0004-cookie-storage.md) |
| 0005 | Spike 结果：pywebview cookie 探测可行性（R1 已解决） | [adr/0005-cookie-probe-result.md](adr/0005-cookie-probe-result.md) |

ADR 按需追加，不强制一次性写完。

---

## 13. 开放问题（V2 待定）

- macOS keychain / Linux secretstorage cookie 实现
- EPUB exporter 实现
- 自适应限速（基于响应延迟与 429 频率）
- 任务启动弹窗恢复（"上次任务进行到 80/200，是否继续"）
- 安装器（NSIS / Inno Setup）
- 自动更新
- 搜索 / 收藏 / 用户主页浏览
- 小说内嵌图片下载
