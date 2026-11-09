# Domain Docs

工程 skills 探索代码库时应如何消费本仓库的领域文档。

## 探索前必读

- **`GLOSSARY.md`** —— 领域术语表（权威）：命名领域概念、写 issue 标题 / 提案 / 测试名时，用词以此为准，不漂移到同义词
- **`docs/SPEC.md`** —— 本项目的单一真相源（13 章 + 风险登记 + ADR 索引）
- **`docs/adr/`** —— 读涉及你即将修改区域的 ADR。完整索引与一句话速查见根目录 `AGENTS.md` 的「ADR 速查」，此处不重复维护清单

## 文件结构

单上下文仓库：

```
/
├── AGENTS.md / GLOSSARY.md / DESIGN.md / PRODUCT.md / README.md
├── config/                        ← 用户配置（settings.json；登录态在系统凭据存储，不在文件系统）
├── data/                          ← 用户数据（app.db、logs/；绝不入库）
├── docs/
│   ├── SPEC.md                    ← 真相源（项目规格 + 风险登记）
│   ├── PACKAGING.md               ← Tauri 打包与分发指引
│   ├── PIXIV-API.md               ← pixiv 接口契约事实源（端点/参数/分页/实现与测试映射/维护矩阵）
│   ├── adr/                       ← 架构决策记录（0001 ~ 0014）
│   ├── research/                  ← 外部接口调研证据档案（pixiv 只读浏览 API 等）
│   ├── agents/                    ← skills 配置（本目录）
│   └── icon/                      ← 应用图标源图（raw_icon.png）
├── frontend/                      ← Vue3 + TS + Vite（views/、components/、stores/、api/、styles/、locales/；UI 库 @material/web，api 层走 invoke/listen）
├── scripts/
│   └── make_icon.sh               ← 图标生成（raw_icon.png → 1024×1024 源图）
└── src-tauri/                     ← Tauri 2 + Rust 后端（单进程，IPC 通信）
    ├── Cargo.toml / tauri.conf.json / build.rs
    ├── capabilities/              ← Tauri 权限声明
    ├── icons/                     ← 全平台图标（tauri.conf.json 引用，必须入库）
    ├── tests/                     ← pixiv 接口测试（pixiv_api/：live_read / live_write 在线实测 + offline_guard 离线命令层）+ IPC 冒烟（smoke_commands.rs）
    └── src/
        ├── main.rs / lib.rs       ← 入口薄壳 / 业务库（lib 名 pixiv_tool_lib）
        ├── state.rs / db.rs / settings.rs / cookies.rs / accounts.rs / paths.rs / platform.rs / logging.rs / image_proxy.rs
        ├── pixiv/                 ← API 客户端（client / api / csrf / browse_api）
        ├── core/                  ← 任务模型（sources / crawler / illust_crawler / exporter / task_manager）
        ├── auth/                  ← 登录（browser_login / cdp / webview_login）
        └── commands/              ← #[tauri::command] IPC 命令层
```

> **本地目录（不入库，不作为项目文档索引）**：`.scratch/` 是 local markdown
> issue tracker，`.archive/pywebview-era/` 是 pywebview 旧栈文档归档，
> `.zcode/` / `.subdriver/` 是 AI 工具会话产物。它们被 `.gitignore` 忽略，
> 换台机器 clone 不到；现行真相源只有 `docs/`、根目录 `AGENTS.md` /
> `GLOSSARY.md` / `DESIGN.md` / `PRODUCT.md` / `README.md`。

## 用 GLOSSARY.md 的术语

当你的输出（issue 标题、重构提案、假设、测试名）命名一个领域概念时，使用根目录 `GLOSSARY.md` 中定义的术语。不要漂移到同义词。

如果你需要的概念不在 `GLOSSARY.md` 里——这是个信号：要么你在发明项目不用的语言（重新考虑），要么有真实空缺（标记给后续 grilling，术语定版后回写 `GLOSSARY.md`）。

## 标记 ADR 冲突

如果你的输出与某个 ADR 矛盾，**显式指出**而非悄悄覆盖：

> _与 ADR-0002（嵌入式 WebView 登录主导）矛盾 —— 但值得重开因为..._

## 不要做的事

- 不要悄悄改 `docs/SPEC.md` —— 任何变更要么先在对话里对齐，要么新增 ADR
- 不要假设旧 spec 内容仍然有效 —— 先读最新版（git 历史）
- 不要引用旧栈（pywebview / FastAPI / PyInstaller）的代码或工作文档 —— spike 代码已随
  Python 栈移除（见 ADR 0008），旧栈 ticket 与调度协议已归档至 `.archive/pywebview-era/`，
  不属于项目文档索引
