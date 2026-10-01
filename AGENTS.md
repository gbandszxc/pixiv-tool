# AGENTS.md

本文件是给 AI agent（包括 ZCode、Claude Code 等）的入口指引。任何 agent 在动这个仓库前都应先读这里。

## 项目简介

**pixiv-tool**：本地运行的 Pixiv 客户端工具，V1 聚焦 Pixiv 小说抓取（单篇 / 系列 / 用户全集），技术栈 Tauri 2（Rust 后端）+ Vue3，IPC 通信无本地 HTTP 服务。

详见 `docs/SPEC.md`。

## Agent skills

### Issue tracker

**Local markdown**：ticket 存为 `.scratch/<feature-slug>/issues/<NN>-<slug>.md`。当前活跃 tracker 是 `.scratch/pixiv-tool-v1/`——slug 沿用旧栈命名，但新 ticket 继续在此递增编号（`01`~`18` 属 pywebview 旧栈、已归档，`19` 起是 Tauri 时代）。`.scratch/pixiv-tool-v2/` 是已关闭的旧栈 backlog，不再新增。详见 `docs/agents/issue-tracker.md`。

### Triage labels

五个标准 role（`needs-triage` / `needs-info` / `ready-for-agent` / `ready-for-human` / `wontfix`），作为 issue 文件顶部 `Status:` 行的值。详见 `docs/agents/triage-labels.md`。

### Domain docs

单上下文仓库，真相源是 `docs/SPEC.md`（不是单独的 `CONTEXT.md`）。详见 `docs/agents/domain.md`。

## 开发约定

### 必读

开工前先读：

1. `docs/SPEC.md` —— 完整规格 + 风险登记
2. `docs/adr/0001` ~ `0012` —— 关键架构决策（0008 为现行架构：Tauri 全量重构；0010 为多账号登录态；0011 为登录窗未登录态打开；0012 为浏览模式自有 UI）
3. 你要动的 ticket（`.scratch/pixiv-tool-v1/issues/<NN>-xxx.md`）
4. 涉及前端界面、组件、样式或交互时，必须先读根目录 `DESIGN.md`。
5. 涉及打包/分发时，先读 `docs/PACKAGING.md`。

### 设计系统维护

- `DESIGN.md` 是前端视觉与交互规范的真相源；其中的色彩、字号、间距、圆角、层级、动效与组件约束优先于临时页面样式。
- 做前端样式或组件改动时，优先复用 `DESIGN.md` 已定义的 token 和 Naive UI 主题配置；不要在页面中新增无来源的颜色、圆角、阴影或动效字面值。
- 如果实现需要新增或调整设计 token、组件规则或视觉方向，必须在同一变更中同步更新 `DESIGN.md` 和 `.impeccable/design.json`；二者应保持一致。
- 需要重新提炼或大幅刷新设计规范时，使用 `$impeccable:impeccable document`；已有 `DESIGN.md` 不得静默覆盖，先与用户确认合并或刷新范围。
- 前端视觉验收应至少覆盖默认、hover、focus、disabled、loading、error 状态，并检查长文本、窄窗口与“减少动态效果”偏好。

### 技术栈

| 层 | 选型 |
|---|---|
| 桌面外壳 + 后端 | Tauri 2（Rust，`#[tauri::command]` IPC） |
| 前端 | Vue 3.4+ · TypeScript · Vite 5 · Naive UI |
| HTTP 抓取 | wreq 6（Chrome147 指纹伪装；版本锁定，见 ADR 0008） |
| 数据库 | SQLite（rusqlite，schema 兼容旧版） |
| 依赖 | cargo（后端）+ pnpm（前端） |
| 打包 | Tauri bundler |

### 开发命令

```bash
cd frontend && pnpm install   # 一次性
cargo tauri dev               # 仓库根：Vite(9961) + Rust 热重载 + 窗口
cd src-tauri && cargo test    # 后端测试（单测 + IPC 冒烟）
cd frontend && pnpm build     # 前端类型检查 + 构建
cargo tauri build             # 生产打包（详见 docs/PACKAGING.md）
```

- 无后端进程/端口：IPC 直连，Vite 仅 dev 期占用 9961（strictPort）
- 系统依赖：构建需 cmake + LLVM/libclang（wreq 编译 BoringSSL 的 btls-sys 用 bindgen 生成绑定，全平台都需要；Windows 装 LLVM.LLVM，macOS 随 Xcode CLT 自带），见 docs/PACKAGING.md
- **Windows 工具链标准（本机实测，2026-10-01）**：默认 `stable-x86_64-pc-windows-gnu` 的 cdylib 链接会超 mingw ld 65535 导出上限（"export ordinal too large"），**一律改用 MSVC 工具链**。所有 cargo 命令前设置：
  ```bash
  export RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc
  export LIBCLANG_PATH="C:\Users\gbandszxc\scoop\apps\llvm\current\bin"
  export CMAKE_GENERATOR="Visual Studio 17 2022"
  ```
  MSVC 链接器缺失时包一层 `cmd /c "call <vs路径>VC\Auxiliary\Build\vcvars64.bat && cargo ..."`。`tauri` CLI 在 frontend devDependencies（`./frontend/node_modules/.bin/tauri`），仓库根跑 `tauri dev` 需 `--config '{"build":{"beforeDevCommand":""}}'` 并自起 Vite。

### Git 约定

- 提交信息格式：`<类型>([<范围>]): <中文改动说明>`。英文前缀（如 `feat`、`fix`、`refactor`、`docs`、`style`、`chore`、`perf`、`test`）表示改动大类，范围可选，中文部分写明改动内容和原因，避免过于简略。示例：`feat: 增加图片全屏预览`。
- 每完成一个 ticket 至少一次提交

### 应用图标

换应用图标三步：

1. 替换源图 `docs/icon/raw_icon.png`（正方形最佳；非正方形脚本会居中裁方）
2. `./scripts/make_icon.sh`——生成 1024×1024 源图到 `frontend/src/assets/icon.png`
   （兼作 UI 侧栏图标与 tauri icon 输入；macOS 用自带 sips，无第三方依赖）
3. `cargo tauri icon frontend/src/assets/icon.png`——生成 `src-tauri/icons/` 全平台
   图标集，再 `cargo tauri build` 生效

`src-tauri/icons/` **必须入库**（tauri.conf.json 引用，缺失会构建失败）。

### 安全边界

- 登录态存于系统凭据存储（macOS Keychain / Windows Credential Manager / Linux Secret Service），**绝不入库**；`config/` 下不得出现任何 cookie 文件
- `data/app.db` 是用户数据，**绝不入库**
- spike 代码已随 Python 栈移除，结论存档于 ADR 0004/0005（登录态方案、cookie 探测）
