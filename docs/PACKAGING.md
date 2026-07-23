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
| Windows | 10/11 x64 | **全功能**（登录/抓取/导出）；WebView2 运行时 Win11 自带 |
| macOS | 12+（arm64/x86_64） | 能构建、能启动、能浏览；**登录不可用**（cookie 走 Windows DPAPI，mac keychain 是 V2） |
| Linux | webkit2gtk（见 README） | 能构建、能启动、能浏览；登录同样不可用 |

### 平台依赖组

`pyproject.toml` 的 `[project.optional-dependencies]` 按平台拆了三个 extra（各装对应的 pywebview 后端）：

| extra | 平台 | 内容 |
|---|---|---|
| `win` | Windows | pywebview（WebView2 后端） |
| `macos` | macOS | pywebview（WebKit 后端） |
| `linux` | Linux | pywebview（WebKitGTK 后端，需系统预装 `webkit2gtk`） |
| `dev` | 全平台 | pyinstaller（打包工具，不进运行时） |

首次拉代码后先同步依赖：

```powershell
# Windows
uv sync --extra win --extra dev     # 后端(含 pywebview + pyinstaller)
pnpm install --dir frontend         # 前端
```

```bash
# macOS / Linux
uv sync --extra macos --extra dev   # linux 机器把 macos 换成 linux
pnpm install --dir frontend
```

> `pyinstaller` 在 `dev` extra 里，**不装就打不了包**（默认 `uv sync` 不带 `--extra dev` 会缺）。**必须同时带平台 extra**，否则 mac/linux 上连 `pywebview` 都没有，打包出来的产物一启动就 `ModuleNotFoundError`。

---

## 2. 一键打包

```powershell
# Windows（完整构建：前端 + 复制静态 + PyInstaller）
.venv/Scripts/python.exe scripts/build.py

# Windows（跳过前端，只改了后端代码时，复用已有的 frontend/dist）
.venv/Scripts/python.exe scripts/build.py --skip-fe
```

```bash
# macOS / Linux（venv 解释器路径不同，其余等价）
.venv/bin/python scripts/build.py
.venv/bin/python scripts/build.py --skip-fe
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

## 4. macOS / Linux 打包

Windows 之外两个平台的打包链路与 Windows 基本一致（同一份 `pixiv-tool.spec` + `scripts/build.py`），区别只在：依赖 extra、产物形态、压缩工具、数据目录。

### 4.1 依赖

```bash
# macOS
uv sync --extra macos --extra dev
# Linux（需先 sudo apt install libwebkit2gtk-4.1-dev，见 README）
uv sync --extra linux --extra dev

pnpm install --dir frontend
```

> mac 装完 pywebview 后第一次 `build.py` 会自动拉 WebKit 后端；Linux 没预装 `webkit2gtk` 会在 **运行**（不是打包）时崩，报 `Gtk cannot be initialized` 之类。

### 4.2 产物：`.app`（mac） vs onedir 目录（linux）

- **mac**：`pixiv-tool.spec` 若声明了 `BUNDLE`，PyInstaller 会产出 `dist/Pixiv Tool.app`（注意名字里有空格），双击进 Dock 运行；没声明 BUNDLE 时回退到 `dist/pixiv-tool/`（和 Windows 一样的 onedir 目录，靠 `dist/pixiv-tool/pixiv-tool` 命令行启动）。
- **Linux**：无 `.app` 概念，产物恒为 `dist/pixiv-tool/`，运行 `./dist/pixiv-tool/pixiv-tool`。

### 4.3 压缩（必须用 `ditto` 打 `.app`）

mac 上打包 `.app` **不能用 `zip`/`tar`**——它们会丢掉 bundle 内的符号链接和可执行权限位，解压后双击没反应。用系统自带的 `ditto`：

```bash
# mac：有 .app 时
cd dist && ditto -c -k --keepParent "Pixiv Tool.app" ../pixiv-tool-macos-x64.zip
# mac：无 .app（回退到 onedir 目录）/ Linux
tar -czf pixiv-tool-macos-x64.zip -C dist/pixiv-tool .     # mac 回退
tar -czf pixiv-tool-linux-x64.tar.gz -C dist/pixiv-tool .  # linux
```

`--keepParent` 保留 `.app` 这一层目录结构。CI（`release.yml`）已按「优先 `.app`、找不到回退 onedir」写好。

### 4.4 数据目录（frozen 模式）

`src/pixiv_tool/storage/paths.py` 按平台锚定用户数据（与 Windows 的 portable exe 同级目录不同，mac/linux frozen 用各平台标准位置）：

| 数据 | macOS frozen | Linux frozen |
|---|---|---|
| app.db / 日志 | `~/Library/Application Support/pixiv-tool/data/` | `~/.local/share/pixiv-tool/data/` |
| cookies / settings | `~/Library/Application Support/pixiv-tool/config/` | `~/.config/pixiv-tool/config/` |

> dev 模式（venv python 跑源码）三平台都锚定 `<repo>/data`、`<repo>/config`，和 Windows 一致。

### 4.5 代码签名与公证（可选）

mac 产物默认 **不签名**。不签名的 `.app` 首次双击会被 Gatekeeper 拦，用户需右键→打开，或 `xattr -dr com.apple.quarantine /path/to.app` 去隔离属性。要正式分发需 **Apple Developer 证书**：

```bash
# 1) 导入证书到临时 keychain（证书 base64 存 CI secret）
echo "$MACOS_CERTIFICATE" | base64 --decode > cert.p12
security create-keychain -p build build.keychain
security import cert.p12 -k build.keychain -P "$MACOS_CERTIFICATE_PWD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k build build.keychain

# 2) ad-hoc 或 Developer ID 签名（--options runtime 才能过公证）
codesign --force --deep --options runtime \
  --sign "Developer ID Application: <名字>" "dist/Pixiv Tool.app"

# 3) 公证 + 装订票据（需 Apple ID app-specific password，存 secret）
xcrun notarytool submit pixiv-tool-macos-x64.zip \
  --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_ID_PWD" --wait
xcrun stapler staple "dist/Pixiv Tool.app"
```

`release.yml` 里已预留一个 `Codesign (macOS, optional)` step，默认不跑：手动触发 workflow（`workflow_dispatch`）勾 `enable_macos_sign` 且配好 `MACOS_CERTIFICATE` / `MACOS_CERTIFICATE_PWD` / `MACOS_CERTIFICATE_NAME` 三个 secret 才执行。**公证（notarytool）未接入**——那还需 Apple ID secrets，按需再加。

---

## 5. 运行模式

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

> `console=False`（见 `pixiv-tool.spec`）时这个模式没有终端输出。调试启动崩溃请见下面第 7 节。

### 4.3 登录子入口（内部）

由 `POST /api/auth/login` 触发，不直接用。见 `src/pixiv_tool/auth/login_window.py` + `src/pixiv_tool/api/auth.py:_spawn_login_subprocess`。

dev 模式：`python -m pixiv_tool.auth.login_window --result-file X`
prod 模式：`pixiv-tool.exe --login-window --result-file X`（同一个 exe 自调）

---

## 6. 数据存放

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

## 7. 调试启动崩溃

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

## 8. 常见问题

### Q1: 双击 exe 闪退，没报错

默认 `console=False` 看不到 traceback。按第 7 节"调试启动崩溃"排查。

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

### Q7: 打包后登录/抓取又触发 Pixiv 风控（dev 不会）

**根因**：登录后业务请求之前走裸 httpx，TLS 指纹（CPython OpenSSL）和残缺 UA（断在 `AppleWebKit/537.36`）全方位不像浏览器，容易被 Pixiv 风控识别。现已改为 `curl_cffi`（`impersonate="chrome124"`）做 TLS 指纹伪装（JA3/JA4 + HTTP/2 + 浏览器 header 顺序一次性解决）。

**验证伪装是否生效**：打包后启动 frozen exe，访问 `http://127.0.0.1:9962/api/auth/diag-version`，或看启动日志里 `PixivClient 使用 HTTP 后端:` 这行——应是 `curl_cffi`。如果是 `httpx`，说明 curl_cffi 动态库没打进去，已静默降级（风控会重新触发）。

**排查动态库缺失**：curl_cffi 的 `libcurl-impersonate`（Win: `.dll`、mac: `.dylib`、linux: `.so`）必须随包分发，由 `pyinstaller-hooks/hook-curl_cffi.py` 用 `collect_dynamic_libs` 自动收集。检查 `dist/pixiv-tool/_internal/` 下有没有 `libcurl-impersonate*` 文件。没有的话：
- 确认打包机上 `curl_cffi` 真的装了（`python -c "import curl_cffi; print(curl_cffi.__file__)"`）
- 确认 `pixiv-tool.spec` 的 `hookspath=[HOOKS_DIR]` 指向了 `pyinstaller-hooks`
- 打包时看日志有没有 `Analyzing ... hook-curl_cffi` 行

curl_cffi issue #5、#455 记录过类似 PyInstaller 打包问题，根因都是动态库没收集。

---

## 9. PyInstaller spec 关键配置

`pixiv-tool.spec` 要点（修改后必须重新打包）：

| 配置 | 值 | 理由 |
|---|---|---|
| 入口 | `src/pixiv_tool/main.py` | 主进程入口 |
| 模式 | `onedir`（COLLECT 段） | 解压即用，启动比 onefile 快 |
| `console` | `False` | prod 不弹 cmd 黑窗 |
| `datas` | `src/pixiv_tool/static` → `src/pixiv_tool/static` | 前端 SPA 打进 `_internal/src/pixiv_tool/static/` |
| `hiddenimports` | `webview.platforms.edgechromium/winforms`、`pixiv_tool.auth.login_window`、`pixiv_tool.storage.cookie_dpapi`、`curl_cffi.*`、`pixiv_tool.core.http_factory` | pywebview 平台后端动态 import，PyInstaller 静态分析抓不到；子入口 + curl_cffi 指纹伪装后端需要显式声明 |
| `hookspath` | `pyinstaller-hooks/` | 指向自定义 hook 目录，`hook-curl_cffi.py` 负责收集 `libcurl-impersonate` 动态库（TLS 指纹伪装依赖，漏打会静默降级到 httpx 重新触发风控） |
| `excludes` | `numpy/scipy/pandas/matplotlib/tkinter/unittest/test` | 减体积，这些本项目不用 |

> **改动陷阱**：`excludes` 千万不要加 `email`/`xml`/`pydoc`——fastapi/starlette/pydantic 间接依赖，排掉后 frozen exe 一启动就 `ModuleNotFoundError`（曾经踩过）。

---

## 10. 相关文件

| 文件 | 作用 |
|---|---|
| `scripts/build.py` | 一键构建脚本 |
| `pixiv-tool.spec` | PyInstaller 配置 |
| `src/pixiv_tool/main.py` | 入口（端口探测 + uvicorn + pywebview + 子入口分发） |
| `src/pixiv_tool/api/auth.py` | `/api/auth/login` spawn 登录子进程 |
| `src/pixiv_tool/auth/login_window.py` | 登录窗子进程主循环 |
| `docs/SPEC.md §3.1, §8.3` | 架构与构建规格 |
