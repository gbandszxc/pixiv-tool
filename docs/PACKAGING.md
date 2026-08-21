# 打包与分发（Tauri）

> 2026-08-20 随 ADR 0008 全量重构更新：PyInstaller 流程已随 Python 栈移除，
> 本文为现行 Tauri bundler 打包指引。旧 PyInstaller 历史见 git log。

## 1. 构建命令

```bash
# 生产打包（自动先跑前端 build（CWD=frontend/），再编 Rust、出平台安装包）
cargo tauri build

# 只要调试二进制（不出安装包，日常验证用）
cargo tauri build --debug --no-bundle
# 产物：src-tauri/target/debug/pixiv-tool
```

产物位置：`src-tauri/target/release/bundle/`（dmg/app on macOS、nsis/msi on
Windows、deb/appimage on Linux）。

注意：`beforeDevCommand` / `beforeBuildCommand` 以 **`frontend/`** 为 CWD
执行（tauri-cli 2.x 实测行为；配置里写 `pnpm build`，不是
`pnpm --dir frontend build`——那会解析成 `frontend/frontend` 报 ENOENT，
曾是 release 打包持续失败的根因）。

## 2. 构建前置

| macOS | Xcode CLT、**cmake**（`brew install cmake`）、libclang（随 Xcode CLT） |
| Windows | MSVC Build Tools、WebView2 SDK（一般随系统）、cmake、**LLVM/libclang**（`winget install LLVM.LLVM`） |
| Linux | webkit2gtk-4.1、cmake、gcc/g++、libclang-dev |

Rust ≥ 1.85（edition 2024）。首次构建约 5–15 分钟（BoringSSL 现场编译 +
bindgen），增量秒级。

## 3. 关键依赖约束

- **wreq = 6.0.0-rc.31 / wreq-util = 3.0.0-rc.14**：指纹伪装库，锁定版本。
  wreq-util **必须 ≥ 3.0.0-rc.12**（Apache-2.0；更早版本 GPL-3.0 传染）。
  升级前需重新 spike 验证 pixiv 风控通过性（ADR 0008）。
- keyring 3 已显式开平台 features（apple-native / windows-native /
  linux-native-sync-persistent），无默认后端的平台不会静默落内存存储。

## 4. 图标

源图 `frontend/src/assets/icon.png`，重新生成：

```bash
cargo tauri icon frontend/src/assets/icon.png
```

产出 `src-tauri/icons/`（tauri.conf.json 引用的 5 个文件必须存在，缺失会
构建失败）。

## 5. 数据目录（release）

| 平台 | data / config |
|---|---|
| Windows | exe 同级 `data/`、`config/`（portable） |
| macOS | `~/Library/Application Support/pixiv-tool/{data,config}` |
| Linux | `$XDG_DATA_HOME/pixiv-tool/data`、`$XDG_CONFIG_HOME/pixiv-tool/config` |

登录态不在文件系统：macOS Keychain / Windows Credential Manager /
Linux Secret Service（service `pixiv-tool.cookies`，account `default`）。

## 6. 发布方式（仅本地打包）

GitCode 托管无流水线，`tauri-action` release CI 已移除（2026-08-21）。
**打包只在本地按需执行**：

- release 安装包：`cargo tauri build`（产物在本目录 `bundle/` 下）
- 调试二进制：`cargo tauri build --debug --no-bundle`

三平台安装包分别在对应系统本地构建；tag（`v*`）仅作版本标记，不触发自动化。
