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

### Windows 工具链标准（本机实测，2026-10-01）

默认 `stable-x86_64-pc-windows-gnu` 的 cdylib 链接超 mingw ld 65535 导出上限
（"export ordinal too large"，debug/release 均复现），**统一改用 MSVC 工具链**。
所有 cargo 命令（dev/test/build）前设置：

```bash
export RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc
export LIBCLANG_PATH="<LLVM 安装路径>\bin"        # 本机：C:\Users\gbandszxc\scoop\apps\llvm\current\bin
export CMAKE_GENERATOR="Visual Studio 17 2022"    # 避免 MSYS Makefiles 误选
```

MSVC 链接器（link.exe）不在 PATH 时包一层
`cmd /c "call <vs路径>\VC\Auxiliary\Build\vcvars64.bat && cargo ..."`。
Git Bash 里 tauri CLI 用 frontend 的本地依赖：`./frontend/node_modules/.bin/tauri`。

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
| Windows | exe 同级可写：`<exe_dir>/data`、`<exe_dir>/config`（portable）；
  不可写（MSI/NSIS 装进 Program Files）自动回退
  `%LOCALAPPDATA%\pixiv-tool\{data,config}` |
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

## 7. macOS 签名与 Keychain 授权弹窗（重要）

默认 `cargo tauri build` 产出 **ad-hoc 签名**（无签名身份）的包：二进制
每次重打包哈希都变，macOS Keychain 的「始终允许」授权按签名绑定，**跨
构建一律失效**——每次装新包首次访问登录态都会重新弹窗。这是「弹窗过于
频繁」的根因之一（另一因素：keyring 分账号条目越多、首次访问弹窗越多；
已通过非 Windows 单条目存储、状态校验零凭据访问与切换零重复归档收敛，
见 SPEC §5.3）。

**彻底解法：本机建一个自签名代码签名证书**（无需 Apple 开发者账号）：

1. 打开「钥匙串访问」→ 菜单「证书助理」→「创建证书」：
   - 名称自定（如 `pixiv-tool-local`），身份类型「自签名根证书」，
     证书类型「代码签名」，创建。
2. 打包时指定身份（tauri bundler 读取该环境变量）：
   ```bash
   APPLE_SIGNING_IDENTITY="pixiv-tool-local" cargo tauri build
   ```
3. 用同一证书签出的包「始终允许」跨构建持久；首次访问每条目仍会弹一次，
   允许后不再弹。

注意：自签名包只是让 Keychain ACL 可持久，不具备对外分发所需的
Developer ID 公证能力（对外分发另议）。
