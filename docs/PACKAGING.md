# 打包与分发

本文档说明 pixiv-tool 如何从源码打包成可分发的桌面应用，以及打包后如何使用。

对应 SPEC §8.3 构建 + §3.1 进程拓扑 + `pixiv-tool.spec` + `scripts/build.py`。

---

## 1. 前置要求

| 工具 | 版本 | 用途 |
|---|---|---|
| Python | 3.11+（venv 实测 3.13 也可） | 后端运行时 |
| uv | 最新 | 后端依赖管理 |
| pnpm | 10+ | 前端依赖管理 |
| Node.js | 20+（fnm/nvm 管理） | 前端构建 |
| Windows | 10/11 x64 | V1 仅 Windows 全功能；WebView2 运行时 Win11 自带 |

首次拉代码后先同步依赖：

```powershell
uv sync --extra win --extra dev     # 后端(含 pywebview + pyinstaller)
pnpm install --dir frontend         # 前端
```

> `pyinstaller` 在 `pyproject.toml` 的 `dev` extra 里，**不装就打不了包**（默认 sync 不带 `--extra dev` 会缺）。

---

## 2. 一键打包

```powershell
# 完整构建（前端 + 复制静态 + PyInstaller）
.venv/Scripts/python.exe scripts/build.py

# 跳过前端（只改了后端代码时，复用已有的 frontend/dist）
.venv/Scripts/python.exe scripts/build.py --skip-fe
```

`build.py` 三个步骤：

| 步骤 | 动作 | 产物 |
|---|---|---|
| 1 | `pnpm install` + `pnpm build` | `frontend/dist/`（HTML+JS+CSS） |
| 2 | 清空并复制 `frontend/dist/` → `src/pixiv_tool/static/` | `src/pixiv_tool/static/index.html` 等 |
| 3 | `python -m PyInstaller pixiv-tool.spec --noconfirm` | `dist/pixiv-tool/`（可分发） |

构建耗时参考：首次 ~60s（PyInstaller 分析依赖 + 解压 DLL），增量 ~30s。

---

## 3. 产物结构

**可分发目录**：`dist/pixiv-tool/`

```
dist/pixiv-tool/
├── pixiv-tool.exe              ← 主入口(双击即用)
└── _internal/                  ← 运行时依赖(exe 同级,不可拆)
    ├── python313.dll           ← 嵌入的 Python 解释器
    ├── base_library.zip
    ├── pixiv_tool/
    │   └── static/             ← 前端 SPA(被打包进 _internal)
    │       ├── index.html
    │       └── assets/
    ├── webview/                ← pywebview + 平台后端
    ├── pythonnet/              ← .NET 桥接(WebView2 需要)
    └── *.dll                   ← 扩展模块 + VC 运行时
```

### ⚠ 不要跑 `build/pixiv-tool/pixiv-tool.exe`

`build/` 是 PyInstaller 的**中间工作目录**（Analysis、PYZ、PKG 等），里面的 `pixiv-tool.exe` 只是 bootloader 的拷贝，**没有配套的 `_internal/python313.dll`**，双击会报：

```
[PYI-xxxx:ERROR] Failed to load Python DLL '...\build\pixiv-tool\_internal\python313.dll'.
LoadLibrary: ?????????
```

只有 `dist/pixiv-tool/` 下的 exe 是完整的。`build/` 已在 `.gitignore`，本地可随时删。

### 压缩分发

```powershell
Compress-Archive -Path dist\pixiv-tool -DestinationPath pixiv-tool-windows-x64.zip
```

解压即用，无需安装。接收方机器需有 WebView2 Runtime（Windows 11 自带，Windows 10 可能需要装 [Evergreen Bootstrapper](https://developer.microsoft.com/microsoft-edge/webview2/)）。

---

## 4. 运行模式

`pixiv-tool.exe` 支持三种启动方式（`src/pixiv_tool/main.py:main()` 分发）：

| 命令 | 用途 |
|---|---|
| `pixiv-tool.exe`（双击） | **prod 桌面模式**：起 uvicorn（daemon 线程）+ pywebview 主窗 |
| `pixiv-tool.exe --no-window` | 纯 API 模式：仅起 uvicorn，不起窗（无头调试 / CI 用） |
| `pixiv-tool.exe --login-window --result-file X` | 子入口：登录窗启动器，由主进程 spawn（不直接调） |

### 4.1 prod 桌面模式（默认）

双击 `dist/pixiv-tool/pixiv-tool.exe`。

进程拓扑（SPEC §3.1）：

```
pixiv-tool.exe（主进程）
├── MainThread          ← pywebview 事件循环（阻塞，关窗即退出）
└── daemon-thread       ← uvicorn.Server.run()（FastAPI）
```

- 主线程跑 pywebview，加载 `http://127.0.0.1:<port>/`
- daemon 线程跑 uvicorn，绑 `127.0.0.1`
- 端口在 `[9962, 9999]` 范围探测空闲（全占用回退 `[10000, 19999]`），写入 `os.environ['PIXIV_TOOL_PORT']`
- 关闭主窗 → uvicorn `should_exit=True` → daemon 线程退出 → 进程结束

### 4.2 纯 API 模式（调试用）

```powershell
.\dist\pixiv-tool\pixiv-tool.exe --no-window
```

不弹窗，直接在终端打印 uvicorn 日志。可以用 curl/浏览器直连 `http://127.0.0.1:9962/` 验证。

> `console=False`（见 `pixiv-tool.spec`）时这个模式没有终端输出。调试启动崩溃请见下面第 6 节。

### 4.3 登录子入口（内部）

由 `POST /api/auth/login` 触发，不直接用。见 `src/pixiv_tool/auth/login_window.py` + `src/pixiv_tool/api/auth.py:_spawn_login_subprocess`。

dev 模式：`python -m pixiv_tool.auth.login_window --result-file X`
prod 模式：`pixiv-tool.exe --login-window --result-file X`（同一个 exe 自调）

---

## 5. 数据存放

数据目录锚定由 `src/pixiv_tool/storage/paths.py` 统一管理：

- **dev 模式**（venv python 跑源码）：锚定 `<repo>/data`、`<repo>/config`。
- **frozen 模式**（PyInstaller 打包的 exe）：锚定 `<exe_dir>/data`、`<exe_dir>/config`（exe 同级，portable 模式）。
  这与 `main._redirect_stdio_if_needed` 的 `stdout.log` 锚定一致——升级（重打包）时只要不删 `data/` 目录，用户数据就保留。

| 数据 | dev 模式位置 | frozen 模式位置 |
|---|---|---|
| 任务/已抓小说 (app.db) | `<repo>/data/app.db` | `<exe_dir>/data/app.db` |
| 登录 cookie (cookies.dat) | `<repo>/config/cookies.dat` | `<exe_dir>/config/cookies.dat` |
| 用户设置 (settings.json) | `<repo>/config/settings.json` | `<exe_dir>/config/settings.json` |
| 运行日志 | `<repo>/data/logs/` | `<exe_dir>/data/logs/` |

> **DPAPI 绑定用户账户**：`cookies.dat` 是 Windows DPAPI 加密，只能在加密时的同一 Windows 用户账户下解密。换机器/换用户需重新登录。

---

## 6. 调试启动崩溃

prod 模式 `console=False`，启动失败时窗口闪退看不到 traceback。三种排查方式：

### 方式 A：临时改 console=True

编辑 `pixiv-tool.spec`，把 `console=False` 改成 `True`，重新打包：

```powershell
.venv/Scripts/python.exe scripts/build.py --skip-fe
.\dist\pixiv-tool\pixiv-tool.exe
```

会弹一个 cmd 黑窗显示完整 traceback。排查完改回 `False`。

### 方式 B：`--no-window` 模式

纯 API 模式不需要窗口，可以快速验证后端能否启动：

```powershell
.\dist\pixiv-tool\pixiv-tool.exe --no-window
```

如果这个能跑而双击不行，问题在 pywebview/WebView2 加载。

### 方式 C：用 venv python 跑源码

跳过打包，直接跑源码定位是不是代码本身的问题：

```powershell
.venv/Scripts/python.exe -m pixiv_tool.main           # prod 等价（起主窗）
.venv/Scripts/python.exe -m pixiv_tool.main --no-window   # 纯 API
```

---

## 7. 常见问题

### Q1: 双击 exe 闪退，没报错

默认 `console=False` 看不到 traceback。按第 6 节"调试启动崩溃"排查。

### Q2: `Failed to load Python DLL`

跑错目录了。只有 `dist/pixiv-tool/pixiv-tool.exe` 可用，`build/pixiv-tool/pixiv-tool.exe` 是中间产物不能跑。

### Q3: 窗口起来但白屏

WebView2 运行时缺失。Win11 自带；Win10 装 [Edge WebView2 Evergreen Bootstrapper](https://developer.microsoft.com/microsoft-edge/webview2/)。

### Q4: 登录窗弹不出

prod 模式登录走子进程（`pixiv-tool.exe --login-window`），依赖 `pythonnet` 加载 .NET 运行时。检查 `dist/pixiv-tool/_internal/pythonnet/` 是否存在。

### Q5: 打包后体积大

当前 ~44 MB（含 Python 解释器 + pywebview + pythonnet + WebView2 桥接）。SPEC §5 ADR 0001 已记录此取舍（pywebview 比 Tauri 大但 Python 全栈）。

### Q6: 改了后端代码要重新打包吗

是的。后端代码在打包时编进了 `_internal/base_library.zip` + PYZ 归档。改完代码必须重跑 `scripts/build.py --skip-fe`（~30s）。

---

## 8. PyInstaller spec 关键配置

`pixiv-tool.spec` 要点（修改后必须重新打包）：

| 配置 | 值 | 理由 |
|---|---|---|
| 入口 | `src/pixiv_tool/main.py` | 主进程入口 |
| 模式 | `onedir`（COLLECT 段） | 解压即用，启动比 onefile 快 |
| `console` | `False` | prod 不弹 cmd 黑窗 |
| `datas` | `src/pixiv_tool/static` → `src/pixiv_tool/static` | 前端 SPA 打进 `_internal/src/pixiv_tool/static/` |
| `hiddenimports` | `webview.platforms.edgechromium/winforms`、`pixiv_tool.auth.login_window`、`pixiv_tool.storage.cookie_dpapi` | pywebview 平台后端动态 import，PyInstaller 静态分析抓不到；子入口模块需要显式声明 |
| `excludes` | `numpy/scipy/pandas/matplotlib/tkinter/unittest/test` | 减体积，这些本项目不用 |

> **改动陷阱**：`excludes` 千万不要加 `email`/`xml`/`pydoc`——fastapi/starlette/pydantic 间接依赖，排掉后 frozen exe 一启动就 `ModuleNotFoundError`（曾经踩过）。

---

## 9. 相关文件

| 文件 | 作用 |
|---|---|
| `scripts/build.py` | 一键构建脚本 |
| `pixiv-tool.spec` | PyInstaller 配置 |
| `src/pixiv_tool/main.py` | 入口（端口探测 + uvicorn + pywebview + 子入口分发） |
| `src/pixiv_tool/api/auth.py` | `/api/auth/login` spawn 登录子进程 |
| `src/pixiv_tool/auth/login_window.py` | 登录窗子进程主循环 |
| `docs/SPEC.md §3.1, §8.3` | 架构与构建规格 |
