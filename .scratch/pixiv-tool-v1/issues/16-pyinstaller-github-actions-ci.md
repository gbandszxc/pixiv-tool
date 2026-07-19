# 16 — PyInstaller 打包 + GitHub Actions 三平台 CI

**What to build:**

从用户视角：开发者执行 `python scripts/build.py` 在本地打包出可执行的 `dist/pixiv-tool/` 目录，双击 `pixiv-tool.exe` 即可启动完整 App（前端已编译嵌入，无需 Node 环境）。在 GitHub 仓库打 tag `v0.1.0` 并 push，GitHub Actions 自动在 Windows/macOS/Linux 三平台构建并上传到 Release，用户下载对应平台的压缩包解压即用。

**Blocked by:** 所有功能 ticket（01-15，需要功能完整才能打包发布）

**Status:** done

**Acceptance criteria:**

- [x] `scripts/build.py` 实现：
  - [x] 第 1 步：`cd frontend && pnpm install && pnpm build` 构建前端到 `frontend/dist/`
  - [x] 第 2 步：复制 `frontend/dist/` 到 `backend/static/`
  - [x] 第 3 步：FastAPI 用 StaticFiles 挂载 `backend/static/`，SPA fallback 到 `index.html`
  - [x] 第 4 步：`pyinstaller pixiv-tool.spec --onedir` 打包
- [x] `pixiv-tool.spec`（PyInstaller spec 文件）：
  - [x] 显式声明所有 hidden imports（pywebview、fastapi、uvicorn 等）
  - [x] 包含 frontend 静态资源
  - [x] 排除不必要的依赖（numpy 等减小体积）
  - [x] 入口是 `backend/main.py`
- [x] 产物结构：`dist/pixiv-tool/pixiv-tool.exe`（Windows）/ `dist/pixiv-tool/pixiv-tool`（Mac/Linux）+ 依赖目录
- [x] 启动验证：双击 exe 后 pywebview 窗口弹出，加载 `http://127.0.0.1:<port>/` 显示 Vue 应用
- [x] prod 模式端口探测（[9962, 9999]）正常工作
- [x] `.github/workflows/release.yml`：
  - [x] 触发：push tag `v*`
  - [x] matrix：windows-latest、macos-latest、ubuntu-latest
  - [x] steps：checkout、setup-python、setup-node、uv sync、pnpm install、pnpm build、pyinstaller
  - [x] 上传 artifact：`pixiv-tool-<os>-x64.zip`（Win/Mac）或 `.tar.gz`（Linux）
  - [x] release 阶段把三个 artifact 上传到 GitHub Release
- [x] Windows 构建包含 WebView2 Evergreen Bootstrapper（约 2MB），首次启动检测未装时静默安装
- [x] README 含 Linux 用户预装 webkit2gtk 的说明（SPEC §11 R4）
- [x] .gitignore 已包含 `dist/` 和 `build/`（前序工作已完成）
- [x] 验证：本地打包跑通；CI 在三平台都成功构建（至少 Windows + macOS，Linux 可有 webkit2gtk 警告）
- [x] V1 已知限制（写入 README）：macOS/Linux 登录抛 NotImplementedError（cookie 存储未实现，见 ADR 0004），仅 Windows 全功能
