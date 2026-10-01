# AGENTS.md

AI agent 动本仓库前的入口。先读本文件，再按「文档地图」取用对应真相源。会话语言用中文。

## 项目

**pixiv-tool**：本地运行的 Pixiv 客户端。V1 = 小说抓取（单篇 / 系列 / 用户全集）+ 插画抓取（单幅 / 用户全集），v1.2 起增加只读浏览模式。

技术栈：Tauri 2（Rust 后端，`#[tauri::command]` IPC）+ Vue 3 / TypeScript / Vite 5 / **@material/web（Material 3）**。**无本地 HTTP 服务、无后端进程**，IPC 直连。

真相源：`docs/SPEC.md`（单上下文仓库，不另设 `CONTEXT.md`）。

---

## 硬约束

### 1. 改代码必须同批改文档

**同一次变更里代码与文档一起改，不留「下次再补」。** 高频对照：

- 增删/改名 `#[tauri::command]` → `docs/SPEC.md` §7 命令表 + §3.4 命令计数
- 改 SQLite schema → §5.1；改 `settings.json` 键 → §5.2
- 增删页面 / 路由 → §6.1 + §3.4
- 改登录、凭据存储、多账号 → §4.1 §5.3 + 受影响 ADR
- 改任务事件 / payload → §3.2 + §7.1；改限速 / 超时 / 并发 → §4.3
- 改主题、色板、token、组件规则 → §6.3 + `DESIGN.md` **与** `.impeccable/design.json`（同批，三者保持一致）
- 改构建 / 打包 / 工具链 / 图标流程 → §8 + `docs/PACKAGING.md`
- 改 `src-tauri/src/pixiv/**` 或 `commands/browse_api_cmds.rs` 的端点调用 / 参数 / 解析 → `docs/PIXIV-API.md`（§维护矩阵）与 `src-tauri/tests/pixiv_api/` 对应用例同批更新；pixiv 侧行为变化先用 `./dev.ps1 test-live` 定位

**推翻或改写任一 ADR 的决策时，新开一条 ADR，只改 SPEC 不算数。** 触发条件：换或加技术栈层、推翻既有 ADR、凭据落点变化、schema 结构变更、登录流程结构性变化、新增外部契约（URI scheme / IPC 契约 / 第三方 API 代理）、锁死版本敏感依赖、用户可见产物布局变化。反之，参数调优、端点勘误、加个页面、修 bug 只需改 SPEC。

各真相源对应的维护时机见下方「文档地图」。

### 2. 安全边界（不可协商）

- 登录态只进系统凭据存储（macOS Keychain / Windows Credential Manager / Linux Secret Service）。**绝不入库**；`config/` 下不得出现任何 cookie 文件
- `data/` 是用户数据（app.db、logs）。**绝不入库**
- 界面、日志、报错中不得出现 Cookie / token / 登录凭据原文
- `src-tauri/icons/` **必须入库**（tauri.conf.json 引用，缺失即构建失败）

### 3. 不做

- 不引入本地 HTTP 服务、后端进程或固定端口
- 不在页面里新增无来源的颜色 / 圆角 / 阴影 / 动效字面值——只用 `DESIGN.md` 的 token 与 M3 颜色角色
- 不引用、复用、索引 `.scratch/` 与 `.archive/` 中的 pywebview 旧栈内容
- 不同步改文档就改代码，反之亦然

---

## 文档地图（真相源 + 维护时机）

| 文档 | 定位 | 何时更新 |
|---|---|---|
| `docs/SPEC.md` | 项目规格：范围 / 技术栈 / 架构 / 数据模型 / IPC 命令 / 风险登记 | 任何行为、契约、参数、目录职责变化（对照「硬约束 1」） |
| `DESIGN.md` | 前端视觉与交互规范真相源：色彩 / 字号 / 间距 / 圆角 / 层级 / 动效 / 组件 | 改 token、组件规则或视觉方向时，与 `.impeccable/design.json` 同批更新 |
| `.impeccable/design.json` | `DESIGN.md` 的结构化伴随视图（impeccable 工具消费） | 与 `DESIGN.md` 一一对应，同上 |
| `docs/adr/` | 架构决策记录，当前 `0001` ~ `0013` | 满足「新开 ADR 触发条件」时追加；编号连续，旧档不删 |
| `docs/PACKAGING.md` | 打包 / 分发 / 构建环境 / 三平台图标 / macOS 签名 | 改构建命令、工具链、Tauri 权限声明、图标流程、发布或签名策略 |
| `docs/PIXIV-API.md` | pixiv 接口契约事实源（端点 / 参数 / 分页 / 实现与测试映射 / 维护矩阵） | 任何 pixiv 端点、参数、响应解析、分页语义变化时，与代码、`src-tauri/tests/pixiv_api/` 同批更新 |
| `docs/research/` | 外部接口调研证据档案（pixiv 只读浏览 API 等） | 补充新的抓包 / 实测证据；契约或分页语义变化改 `docs/PIXIV-API.md` 并回填勘误 |
| `docs/agents/` | skills 配置：issue tracker / triage 标签 / domain 导航 | 改 issue 路径或编号规则、triage 标签、领域文档布局 |
| `PRODUCT.md` | 产品定位、目标用户、范围边界 | 产品定位或用户可见范围变化 |
| `.scratch/` · `.archive/pywebview-era/` | **非项目文档**：本机 issue tracker / 旧栈归档，被 `.gitignore` 忽略 | 不维护、不索引，换台机器 clone 不到 |

**ADR 速查**：`0008` 是现行架构基座（Tauri 2 全量重构），`0009` / `0010` / `0011` / `0012` / `0013` 是最新决策（webview 登录回退 / 多账号 / 登录窗未登录态 / 浏览模式 / 移除内嵌浏览器）。`0001` / `0004` / `0005` 描述的是已废弃的 pywebview 旧栈，读其结论、不读其实现。

**开工顺序**：任何改动先读 `docs/SPEC.md`；改前端加读 `DESIGN.md`；改打包加读 `docs/PACKAGING.md`；改浏览 / 接口层加读 `docs/PIXIV-API.md`；改浏览模式加读 `docs/research/pixiv-browse-api.md` + ADR 0012；本次 ticket 在 `.scratch/pixiv-tool-v1/issues/<NN>-<slug>.md`。

---

## 技术栈

| 层 | 选型 |
|---|---|
| 桌面外壳 + 后端 | Tauri 2（Rust，`#[tauri::command]` IPC） |
| 前端 | Vue 3.4+ · TypeScript · Vite 5 · Vue Router 4 · Pinia · @material/web（Material 3） |
| HTTP 抓取 | wreq 6（Chrome147 指纹伪装；版本锁定，见 ADR 0008） |
| 数据库 | SQLite（rusqlite，schema 兼容旧版） |
| 依赖 | cargo（后端）+ pnpm（前端） |
| 打包 | Tauri bundler |

> 本仓库**不用 Naive UI**。「Material 3」指 `--md-sys-color-*` 颜色角色体系，详见 `DESIGN.md`。

## 开发命令

开发操作优先使用仓库根目录的 `dev.ps1`（Windows）或 `dev.sh`（Bash）。AI 开始开发操作前，先在仓库根运行 `.\dev.ps1 -h` 或 `bash ./dev.sh -h`，获取当前命令用法；完整用法以脚本帮助为准，不在本文重复维护命令表。

- Vite 仅 dev 期占用 9961（strictPort）
- 系统依赖：cmake + LLVM/libclang（wreq 编译 BoringSSL，btls-sys 用 bindgen 生成绑定，全平台都需要），见 `docs/PACKAGING.md`
- **Windows 一律用 MSVC 工具链**：默认 `windows-gnu` 的 cdylib 链接会超 mingw ld 导出上限（"export ordinal too large"）。所有 cargo 命令前设置：
  ```bash
  export RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc
  export LIBCLANG_PATH="<LLVM 安装路径>\bin"
  export CMAKE_GENERATOR="Visual Studio 17 2022"
  ```
  MSVC 链接器缺失时包一层 `cmd /c "call <vs路径>VC\Auxiliary\Build\vcvars64.bat && cargo ..."`
- **tauri CLI 用仓库内那份**：`cargo tauri` 需全局安装，本机通常没有；在**仓库根**执行 `./frontend/node_modules/.bin/tauri dev`。注意 CLI 从 CWD 向下探测 `src-tauri/`，**不能** `cd frontend` 后调用（frontend 与 src-tauri 平级，会报 "Couldn't recognize the current folder as a Tauri project"）。从仓库根直接调本地 CLI 若 Vite 未自动拉起，加 `--config '{"build":{"beforeDevCommand":""}}'` 并自起 Vite
- 需要真实登录态的 pixiv 在线接口实测用 `.\dev.ps1 test-live`（Git Bash：`bash ./dev.sh test-live`）：真实访问 pixiv、串行执行，前置条件是本机已有登录态；默认 `test` 保持全离线

## 工程约定

### Git

- 提交信息：`<类型>([<范围>]): <中文说明>`。前缀取 `feat` / `fix` / `refactor` / `docs` / `style` / `chore` / `perf` / `test`，范围可选。示例：`feat: 增加图片全屏预览`
- 每完成一个 ticket 至少一次提交

### 设计系统

- 优先复用 `DESIGN.md` 已定义的 token 与 Material Web 组件；调整 token 必须回写 `DESIGN.md` + `.impeccable/design.json`
- 大幅刷新规范用 `$impeccable:impeccable document`；已有 `DESIGN.md` 不得静默覆盖，先确认刷新范围
- 视觉验收至少覆盖 default / hover / focus / disabled / loading / error，并检查长文本、窄窗口与 `prefers-reduced-motion`

### Issue tracker 与 triage

- ticket 存 `.scratch/pixiv-tool-v1/issues/<NN>-<slug>.md`，新 ticket 递增编号（当前至 `33`）；`.scratch/` 不入库
- triage 五个 role 写在 issue 顶部 `Status:` 行：`needs-triage` / `needs-info` / `ready-for-agent` / `ready-for-human` / `wontfix`
- 详见 `docs/agents/issue-tracker.md`、`docs/agents/triage-labels.md`

### 应用图标

① 换源图 `docs/icon/raw_icon.png`（非正方形会居中裁方）→ ② `./scripts/make_icon.sh` 生成 1024×1024 的 `frontend/src/assets/icon.png`（**仅 macOS 可直接跑**，依赖自带 sips；Windows 需自行裁方缩放后覆盖）→ ③ `cargo tauri icon frontend/src/assets/icon.png` 生成 `src-tauri/icons/`。
