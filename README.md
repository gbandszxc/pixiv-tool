# pixiv-tool

本地运行的 Pixiv 客户端工具，V1 聚焦 Pixiv 小说抓取（单篇 / 系列 / 用户全集）。

## 技术栈

| 层 | 选型 |
|---|---|
| 桌面外壳 + 后端 | Tauri 2（Rust，IPC 通信，无本地 HTTP 服务） |
| 前端 | Vue 3.4+ · TypeScript · Vite 6.4.3+ · Material Web（Material 3） |
| HTTP 抓取 | wreq 6（Chrome147 TLS/HTTP2 指纹伪装） |
| 数据库 | SQLite（rusqlite，schema 兼容旧 Python 版数据） |
| 依赖 | cargo（后端）+ pnpm（前端） |

## 快速开始

### 前置条件

- Rust 1.88+（edition 2024）+ Tauri CLI —— 安装方式二选一：
  `cargo install tauri-cli`（全局，之后可用 `cargo tauri`），
  或用仓库内已声明的本地 CLI（`cd frontend && pnpm tauri ...`，无需全局安装）
- Node.js 18+ + [pnpm](https://pnpm.io/)
- **cmake**（全平台：wreq 现场编译 BoringSSL；macOS `brew install cmake`）
- **LLVM/libclang**（Windows 构建必需，btls-sys 用 bindgen 生成绑定：`winget install LLVM.LLVM`）
- Windows 10+ 或 macOS 12+；Linux 需 webkit2gtk（见下）

### 启动开发

```bash
cd frontend && pnpm install   # 一次性
cargo tauri dev               # 仓库根执行：Vite(9961) + Rust 热重载 + 应用窗口
```

测试与构建：

```bash
cd src-tauri && cargo test    # 后端测试（单测 + IPC 冒烟）
cd frontend && pnpm build     # 前端类型检查 + 构建
cargo tauri build             # 生产打包（详见 docs/PACKAGING.md）
```

dev 模式数据目录沿用仓库 `data/`、`config/`（与旧 Python 版 dev 数据无缝衔接）。

## 跨平台说明

- **Windows / macOS**：全功能支持（登录、抓取、导出）。登录优先使用独立
  Chrome / Edge / Chromium 窗口（CDP 提取 Cookie）；未安装时回退内置登录窗
  （或手动粘贴 Session 兜底）。Cookie 存系统凭据存储（Credential Manager / Keychain）。
- **Linux**：需预装 `webkit2gtk`：

  ```bash
  # Ubuntu/Debian
  sudo apt install libwebkit2gtk-4.1-dev cmake

  # Fedora
  sudo dnf install webkit2gtk4.1-devel cmake
  ```

## 目录结构

```
pixiv-tool/
├─ src-tauri/   # Tauri 2 + Rust 后端（pixiv 客户端/爬虫/命令层/存储）
├─ frontend/    # Vue3 + TS + Vite（api 层走 invoke/listen）
├─ docs/        # SPEC + ADR + 打包指引
├─ scripts/     # make_icon.sh（图标生成）
├─ config/      # 用户配置（settings.json，dev 模式生效）
└─ data/        # 用户数据（app.db，dev 模式生效）
```

## 打包发布

```bash
cargo tauri build
```

详见 `docs/PACKAGING.md` 与 `docs/SPEC.md`。
