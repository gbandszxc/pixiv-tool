# 打包与分发（Tauri）

> 2026-08-20 随 ADR 0008 全量重构更新：PyInstaller 流程已随 Python 栈移除，
> 本文为现行 Tauri bundler 打包指引。旧 PyInstaller 历史见 git log。

## 1. 构建命令
### 根目录开发脚本

Windows 使用 `.\dev.ps1`（兼容 Windows PowerShell 5.1 / PowerShell 7）；
macOS / Linux / Git Bash 使用 `bash ./dev.sh`。无参数、`-h` 或 `--help`
显示帮助。脚本始终以自身目录定位仓库，不依赖调用者的当前目录。

| 子命令（两份脚本一致） | 行为 |
|---|---|
| `install` | `pnpm install --frozen-lockfile`，安装前端及本地 Tauri CLI |
| `dev [start\|stop\|restart]` | 管理后台桌面开发服务（默认 `start`），复用或启动本仓库 Vite，再启动 Rust 热重载 + 桌面窗口 |
| `frontend [start\|stop\|restart]` | 管理后台 Vite 服务（默认 `start`，9961，strictPort），浏览器内不提供 Tauri IPC |
| `build` / `build release` | 前端构建 + Rust release + 平台安装包 |
| `build debug` | 前端构建 + Rust debug + 平台安装包，**不**附加 `--no-bundle` |
| `check` | 前端类型检查及生产构建，再执行 `cargo check --locked` |
| `test` | `cargo test --locked`（单元与集成测试） |
| `test-live` | `cargo test --locked --test pixiv_api -- --ignored --test-threads=1`（在线 pixiv 接口实测，需本机登录态） |
| `logs [app\|dev\|frontend\|build\|install\|check\|test\|test-live] [-f\|--follow]` | 默认读取开发态 `data/logs/app.log` 最后 100 行；指定子命令读取其控制台日志；跟随模式等待追加内容，Ctrl+C 退出 |

每次启动服务或执行前台操作覆盖 `.dev/logs/<子命令>.log`，操作内部的多个阶段追加到同一份
UTF-8 日志，合并保存子进程 stdout / stderr（含警告与空行）；该目录已忽略，
不入库。`logs app` 不查找 release 安装后的日志。
未知命令、多余参数、非法构建模式退出码为 2；缺工具或日志不存在为 1；
子进程失败保留其退出码。`dev` / `frontend` 后台运行，使用对应 `stop` 停止，
关闭调用终端不会停止服务；其他操作仍在前台运行，Ctrl+C 停止。

服务身份保存在 `.dev/pids/`（Windows 为 JSON，Unix 为 PID + 启动时间记录），
停止前核对 PID、启动时间和命令，清理过期记录，拒绝终止无关进程。`start`
重复执行不重复启动；`restart` 先停止再启动。可接管确认属于本仓库的已运行
Vite（Windows 核对命令、源映射内容及仓库 package 路径；Unix 核对命令和工作目录），
不会按端口盲杀。9961 被其他程序占用时明确失败，不自动换端口。

`dev` 先确保前端可用，运行时通过 `.dev/tauri-dev.json`（Windows）或等价
CLI 配置禁用重复的 `beforeDevCommand`。如果前端由这次 `dev start` 创建，
`dev stop` 同时停止该前端；复用的已有前端则保留。`frontend stop/restart`
仅作用于前端，已运行的桌面开发进程不会被连带停止。`dev start/restart`
等待后台工具链初始化完成后才报告成功；初始化失败在调用终端报错并写入
`dev.log`，不再把「已创建进程」当作「初始化成功」。Rust 编译仍在后台进行，
Tauri 窗口在编译成功后出现，进度通过 `logs dev -f` 查看。
Windows 的 VS 环境只在独立后台进程中初始化，不修改调用终端的 PATH / VS
环境；继承的工具目录先去重，已有匹配的 x64 VS 环境直接复用，避免重复调用
vcvars64.bat 导致 cmd.exe 报 `The input line is too long`。
进程树停止是强制终止，不等同于应用内的正常退出；操作前确保无需要保留的运行中任务。

```powershell
.\dev.ps1 install
.\dev.ps1 dev start
.\dev.ps1 dev restart
.\dev.ps1 dev stop
.\dev.ps1 frontend start
.\dev.ps1 frontend restart
.\dev.ps1 frontend stop
.\dev.ps1 build release
.\dev.ps1 build debug
.\dev.ps1 logs dev -f
# 如果当前执行策略禁止脚本：
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\dev.ps1 -h
```

对应 Bash 调用只需替换前缀，例如 `bash ./dev.sh build debug`。
Windows 的 PowerShell 入口自动使用 MSVC Rust stable 工具链、Visual Studio
2022 C++ 环境与 `Visual Studio 17 2022` CMake generator；Git Bash 入口委托
同目录 `dev.ps1`，保持完全相同的工具链初始化行为。须提前安装工具链；
脚本不会下载 Rust / Visual Studio / LLVM。LLVM 自动查找 Scoop 的
`~/scoop/apps/llvm/current/bin` 与 `%ProgramFiles%/LLVM/bin`，可通过
`LIBCLANG_PATH` 指定其他目录；`VSINSTALLDIR` 可覆盖 VS 2022 的自动发现。
macOS / Linux 直接使用当前 cargo 工具链与环境，服务管理另需 `lsof`，
系统依赖仍按下文安装。Unix 操作以 `.dev/pids/operation.lock` 目录串行化；
若控制进程被强杀，确认无启动/停止操作正在执行后可删除该空锁目录再试。

### 直接调用 CLI


```bash
# 生产打包（自动先跑前端 build（CWD=frontend/），再编 Rust、出平台安装包）
cargo tauri build

# 只要调试二进制（不出安装包，日常验证用）
cargo tauri build --debug --no-bundle
# 产物：src-tauri/target/debug/pixiv-tool（Windows 为 pixiv-tool.exe）
```

产物位置：`src-tauri/target/release/bundle/`（dmg/app on macOS、nsis/msi on
Windows、deb/rpm/appimage on Linux——`tauri.conf.json` 的 `targets` 为 `all`）。

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
export LIBCLANG_PATH="<LLVM 安装路径>\bin"        # 例（scoop）：C:\Users\<用户名>\scoop\apps\llvm\current\bin
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

## 6. 发布方式（GitHub Actions 发版 CI）

GitHub 托管，发版走 `release` 工作流（`.github/workflows/release.yml`，2026-10-01
随迁移 GitHub 重新引入；此前的 GitCode 本地打包时代结束）：

1. 仓库 **Actions → Release → Run workflow** 手动触发，输入版本号（semver，
   格式非法会在构建前置校验阶段直接失败）
2. 版本号写入 `src-tauri/tauri.conf.json` 与 `src-tauri/Cargo.toml`
   （仅本次构建环境内，不回写仓库）；**发版后需手动把仓库内四处版本号同步为
   刚发布的版本**：`tauri.conf.json` / `Cargo.toml` / `Cargo.lock` /
   `frontend/package.json`，否则本地 dev 构建与账号菜单版本回显仍是旧值
3. 六路并行构建：

   | 任务 | Runner | 产物 |
   |---|---|---|
   | `windows-x64-msi` | `windows-latest` | MSI |
   | `windows-arm64-msi` | `windows-11-arm` | MSI |
   | `macos-universal-dmg` | `macos-latest` | universal DMG（Intel + Apple Silicon） |
   | `macos-aarch64-dmg` | `macos-latest` | aarch64 DMG |
   | `linux-x64` | `ubuntu-22.04` | AppImage + deb + rpm |
   | `linux-arm64` | `ubuntu-22.04-arm` | AppImage + deb + rpm |

   Linux 基线固定 22.04（官方推荐的最老 WebKitGTK 4.1 基线，保证 glibc 下限）；
   arm64 走 GitHub **原生 arm64 runner** 编译，不做交叉编译（AppImage 的
   linuxdeploy 不支持交叉出 ARM 包，只能由原生 ARM 主机出）。Linux 构建机额外装
   `cmake` / `rpm`（rpm 打包）、`libfuse2` 与 `xdg-utils`（AppImage 打包：跑
   linuxdeploy 这个 AppImage，并把 xdg-open 打进包内；arm64 镜像不预装 xdg-utils）
   与 `go`（BoringSSL 汇编，缺失时自动补装）

   **Windows arm64 的 BoringSSL 汇编开关**：BoringSSL 的 win-aarch64 汇编
   （`gen/bcm/*-armv8-win.S`）是 GNU 汇编器语法，上游要求用 Clang 汇编；原生 arm64
   下 CMake 只会选到 MSVC 的 `armasm64`，汇编阶段必失败。btls-sys 自身只在
   **Windows 交叉编译**时关汇编（`build/main.rs` 里 `OPENSSL_NO_ASM=YES`），原生
   同架构构建不会关，因此发版 CI 经 btls-sys 的工具链文件入口
   `CMAKE_TOOLCHAIN_FILE_aarch64_pc_windows_msvc` 注入
   `src-tauri/cmake/btls-windows-arm64.cmake` 显式关闭汇编（走 C 实现，代价是 TLS
   少了 ARMv8 汇编加速；本应用以网络 I/O 为主，无实质影响）。同架构 arm64 Windows
   本地出包需自行 `export` 同一个环境变量；若将来要保留汇编，把该文件改为指定
   `clang` 作 ASM 编译器即可（镜像自带 LLVM）
4. **Release 正文拼装**：先取 `docs/releases/<版本>.md` 作为更新说明（无则跳过）
   创建 Release，再**读回平台实际存储的资产名**动态生成安装包表格并改正文（某种
   格式缺失则显示 `—`，不会产生死链）。之所以不按构建机上的文件名拼链接：GitHub
   会规整上传的资产名（`Pixiv Tool_1.1.0_x64_en-US.msi` → `Pixiv.Tool_1.1.0_x64_en-US.msi`），
   照原文件名拼出的 URL 会带空格、在 Markdown 中被截断。表格列固定为
   架构 ×（Windows / macOS / Linux）；同一列有多个同类产物时按架构子串区分（Windows
   列 `_x64_` / `_arm64_` 两个 MSI，Linux 列 `amd64` 与 `arm64\|aarch64` 两个 AppImage）
5. **幂等覆盖**：发布阶段先删除同名 `v<版本>` Release 与 Tag 再重建，
   同版本号重复执行总是覆盖；版本号不变时无需清理即可重发

产物为未签名包（macOS ad-hoc，无 Developer ID 公证；本机自签名方案见 §7，
对外分发签名另议）。本地 `cargo tauri build`（§1）仍然可用，与 CI 互相独立。

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
