# Domain Docs

工程 skills 探索代码库时应如何消费本仓库的领域文档。

## 探索前必读

- **`docs/SPEC.md`** —— 本项目的单一真相源（13 章 + 风险登记 + ADR 索引）
- **`docs/adr/`** —— 读涉及你即将修改区域的 ADR。当前 0001 ~ 0008：
  - `0001`：初版技术栈选型（桌面壳 + Python 后端；已被 0008 取代，Vue3 前端沿用）
  - `0002`：登录策略（浏览器登录主导 + 手动 Cookie 兜底）
  - `0003`：任务模型（Source + Crawler + Task 状态机）
  - `0004`：Cookie 存储策略（已被 0008 的 keyring 方案取代）
  - `0005`：R1 spike 结论（Cookie 探测可行性 + spike→主代码复用清单）
  - `0006`：真实 Chromium 登录 + macOS Keychain
  - `0007`：插画抓取（IllustSource / IllustCrawler + novel/pic 目录分域）
  - `0008`：全量重构为 Tauri 2 + Rust（现行架构）

> 注：标准 mattpocock 流程用 `CONTEXT.md` 作为术语表。本项目目前用 `docs/SPEC.md` 统一承载规格 + 术语，未单独建 `CONTEXT.md`。如果术语开始膨胀（>20 个专有名词），再用 `/domain-modeling` 拆出 `CONTEXT.md`。

## 文件结构

单上下文仓库：

```
/
├── AGENTS.md / DESIGN.md / PRODUCT.md / README.md
├── config/                        ← 用户配置（settings.json；登录态在系统凭据存储，不在文件系统）
├── data/                          ← 用户数据（app.db、logs/；绝不入库）
├── docs/
│   ├── SPEC.md                    ← 真相源（项目规格 + 风险登记）
│   ├── PACKAGING.md               ← Tauri 打包与分发指引
│   ├── adr/                       ← 架构决策记录（0001 ~ 0008）
│   ├── agents/                    ← skills 配置（本目录）
│   └── icon/                      ← 应用图标源图（raw_icon.png）
├── frontend/                      ← Vue3 + TS + Vite（api 层走 invoke/listen）
├── scripts/
│   └── make_icon.sh               ← 图标生成（raw_icon.png → 1024×1024 源图）
├── spike/                         ← 风险验证脚本
│   └── cookie_probe/
└── src-tauri/                     ← Tauri 2 + Rust 后端（单进程，IPC 通信）
    ├── Cargo.toml / tauri.conf.json / build.rs
    ├── capabilities/              ← Tauri 权限声明
    ├── icons/                     ← 全平台图标（tauri.conf.json 引用，必须入库）
    ├── tests/                     ← IPC 冒烟测试（smoke_commands.rs）
    └── src/
        ├── main.rs / lib.rs       ← 入口薄壳 / 业务库（lib 名 pixiv_tool_lib）
        ├── state.rs / db.rs / settings.rs / cookies.rs / paths.rs / platform.rs / logging.rs
        ├── pixiv/                 ← API 客户端（client / api / csrf）
        ├── core/                  ← 任务模型（sources / crawler / illust_crawler / exporter / task_manager）
        ├── auth/                  ← 登录（browser_login / cdp）
        └── commands/              ← #[tauri::command] IPC 命令层
```

## 用 SPEC 的术语

当你的输出（issue 标题、重构提案、假设、测试名）命名一个领域概念时，使用 `docs/SPEC.md` 中定义的术语。不要漂移到同义词。

如果你需要的概念不在 SPEC 里——这是个信号：要么你在发明项目不用的语言（重新考虑），要么有真实空缺（标记给后续 grilling）。

## 标记 ADR 冲突

如果你的输出与某个 ADR 矛盾，**显式指出**而非悄悄覆盖：

> _与 ADR-0002（嵌入式 WebView 登录主导）矛盾 —— 但值得重开因为..._

## 不要做的事

- 不要悄悄改 `docs/SPEC.md` —— 任何变更要么先在对话里对齐，要么新增 ADR
- 不要假设旧 spec 内容仍然有效 —— 先读最新版（git 历史）
- 不要复用 spike 的代码到主代码 —— spike 是参考实现，主代码要重新组织（见 ADR 0005 复用清单）
