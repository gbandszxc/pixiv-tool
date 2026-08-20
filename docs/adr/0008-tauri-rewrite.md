# ADR 0008 · 全量重构为 Tauri 2 + Rust，移除 Python 后端

> 状态：已采纳 · 2026-08-20
> 取代：ADR 0001 中"选 pywebview + FastAPI 而非 Electron/Tauri"的桌面壳与后端选型

## 背景

V1 以 pywebview + Python FastAPI + Vue3 交付。运行中暴露的痛点：

1. **双运行时分发负担**：PyInstaller --onedir 产物大、hidden import 脆弱（R5），
   curl_cffi 动态库需专用 hook 收集；每次升级 Python 依赖都可能破坏打包。
2. **进程拓扑复杂**：uvicorn 线程 + 端口探测 + pywebview 窗口，dev/prod 端口
   策略与启动时序是长期复杂度来源。
3. **风控对抗绑定 curl_cffi**：Python 生态的指纹伪装能力更新滞后且 API 不稳。

Rust 生态的指纹伪装库（wreq，rquest 后继）已成熟。spike 验证（2026-08-19）：
wreq 6.0.0-rc.31 + wreq-util 3.0.0-rc.14，Chrome147 emulation 匿名访问
`/ajax/user/self` 与 `/ajax/novel/{id}` 均返回 200 + JSON，pixiv 风控未拦截——
重写的最大技术风险解除。

## 决策

**全量重构为 Tauri 2 单进程架构，Python 后端整体移除：**

| 层 | 旧 | 新 |
|---|---|---|
| 桌面外壳 | pywebview | Tauri 2（WKWebView / WebView2 / WebKitGTK） |
| 后端 | Python FastAPI + uvicorn | Rust，`#[tauri::command]` IPC |
| HTTP 抓取 | curl_cffi (chrome124) | wreq Chrome147 emulation（BoringSSL） |
| 进度推送 | SSE（实际前端轮询，SSE 是死代码） | Tauri 事件 `task://progress`、`task://done` |
| Cookie 存储 | DPAPI 文件 / keyring.py | keyring crate（Windows Credential Manager / macOS Keychain / Linux Secret Service） |
| 数据库 | sqlite3 | rusqlite（schema 逐字兼容，旧 app.db 可直接打开） |
| 打包 | PyInstaller | Tauri bundler |

前端 Vue3 + Naive UI 保留，api 层从 axios/EventSource 改为 invoke/listen
（26 处调用逐条对账迁移）。

## 语义保持与有意变更

**保持**：全部业务规则（限速 2 并发 + 0.4s、重试 3 次、429 全队列暂停 60s、
max_wait 排除暂停时长）、DB schema、settings.json 键与迁移逻辑、文件命名与
目录布局、CDP 登录流程、IPC 层沿用旧 HTTP 的 200+`{error}` 业务错误风格。

**有意变更（重构中修正或裁剪）**：

1. 修正 Python 版三个潜伏 bug：取消终态被 `mark_done` 覆写为 done；markdown
   `[rb:A>B]` 注音转换赋值后未使用；retry-failed 逐 id 独立 run 导致计数互相
   覆写（现为共用 task_id 累计）。
2. **回退登录窗裁剪**：pywebview 缺浏览器时的登录回退窗不复存在。Tauri 下
   WKWebView/WebView2 无统一 Cookie 读取 API（PHPSESSID 是 HttpOnly）。缺
   Chrome/Edge/Chromium 时提示改用手动 Cookie 登录。
3. Windows Cookie 从 DPAPI 文件（cookies.dat）改为系统 Credential Manager；
   macOS Keychain service/account 不变，与旧版登录态互读兼容。
4. 路由改 hash 模式（Tauri 自定义协议下无服务端 fallback）。
5. 三平台发布 CI（release.yml）随 Python 栈移除，Tauri 版 CI 待建（wreq 需
   cmake + C++ 工具链，见 docs/PACKAGING.md）。

## 后果

- 单二进制 + 前端资源，分发体积与依赖面显著下降；无端口、无本地 HTTP 面。
- 浏览器指纹伪装锁定 wreq 6.0.0-rc.31 / wreq-util 3.0.0-rc.14（**必须 ≥
  3.0.0-rc.12**，更早版本 GPL-3.0 传染）。升级需重新验证风控通过性。
- 构建 macOS/Linux 需 cmake（BoringSSL 现场编译）。
- Rust 学习成本与重写成本已一次性支付（本次约 4.5k 行 Rust + 116 测试）。

## 验证记录（2026-08-20）

- `cargo test` 116 通过；`cargo clippy` 0 警告；`pnpm build`（vue-tsc + vite）通过。
- `cargo tauri build --debug --no-bundle` 产物实机启动：进程稳定、旧 app.db
  兼容打开、settings 自动初始化、webview 加载 Vue 后 355ms 内 `auth_status`
  invoke 到达 Rust（IPC 链路铁证）。
- 像素级截图因验证环境无屏幕录制权限未做；登录/抓取网络链路需真实会话，
  留待人工验收。
