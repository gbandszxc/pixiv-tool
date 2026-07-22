# pixiv-tool

本地运行的 Pixiv 客户端工具，V1 聚焦 Pixiv 小说抓取（单篇 / 系列 / 用户全集）。

## 技术栈

| 层 | 选型 |
|---|---|
| 桌面外壳 | pywebview 5+（Windows: WebView2） |
| 后端 | Python 3.11+ · FastAPI · uvicorn |
| 前端 | Vue 3.4+ · TypeScript · Vite 5 · Naive UI |
| 数据库 | SQLite（标准库 sqlite3） |
| 依赖 | uv（后端）+ pnpm（前端） |

## 快速开始

### 前置条件

- Python 3.11+（推荐用 pyenv 管理）
- [uv](https://docs.astral.sh/uv/)（Python 依赖管理）
- Node.js 18+ + [pnpm](https://pnpm.io/)
- Windows 10+（macOS/Linux 部分功能受限，见下方说明）

### 启动开发服务

**Windows (PowerShell)**

```powershell
# 一键启动前后端
./scripts/dev.ps1 start

# 仅启动前端
./scripts/dev.ps1 start frontend

# 仅启动后端
./scripts/dev.ps1 start backend

# 查看状态
./scripts/dev.ps1 status

# 查看日志
./scripts/dev.ps1 logs

# 停止所有
./scripts/dev.ps1 stop
```

**macOS / Linux (bash)**

```bash
# 一键启动前后端
./scripts/dev.sh start

# 仅启动前端 / 后端
./scripts/dev.sh start frontend
./scripts/dev.sh start backend

# 查看状态 / 日志 / 停止
./scripts/dev.sh status
./scripts/dev.sh logs
./scripts/dev.sh stop
```

`dev.sh` 与 `dev.ps1` 行为一致：stop 按进程组强杀（连 uvicorn `--reload` 子进程一起清理），start 轮询 `/api/health` 与端口确认就绪。

启动后访问 `http://localhost:9961`。

### 安装依赖

```bash
# 后端（在仓库根目录）
uv sync

# 前端
cd frontend && pnpm install
```

## 跨平台说明

- **Windows**：全功能支持（登录、抓取、导出）
- **macOS / Linux**：可启动 UI 和浏览，但**登录功能不可用**（cookie 存储依赖 Windows DPAPI，macOS keychain / Linux secretstorage 留 V2 实现）。Linux 用户需预装 `webkit2gtk`：

  ```bash
  # Ubuntu/Debian
  sudo apt install libwebkit2gtk-4.1-dev

  # Fedora
  sudo dnf install webkit2gtk4.1-devel
  ```

  mac/linux 本地构建打包用对应平台 extra：`uv sync --extra macos --extra dev`（linux 把 `macos` 换成 `linux`），再 `.venv/bin/python scripts/build.py`，产出 `dist/pixiv-tool/`（mac 若 spec 声明了 BUNDLE 则是 `dist/Pixiv Tool.app`）。详见 `docs/PACKAGING.md` §4。

## 目录结构

```
pixiv-tool/
├─ src/pixiv_tool/   # Python 后端（FastAPI + uvicorn）
├─ frontend/         # Vue3 + TS + Vite
├─ scripts/          # 开发与构建脚本（dev.ps1 / dev.sh / build.py）
├─ docs/             # SPEC + ADR
└─ spike/            # 探索性验证代码
```

## 打包发布

```powershell
python scripts/build.py
```

详见 `docs/SPEC.md`。
