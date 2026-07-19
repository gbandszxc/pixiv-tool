# AGENTS.md

本文件是给 AI agent（包括 ZCode、Claude Code 等）的入口指引。任何 agent 在动这个仓库前都应先读这里。

## 项目简介

**pixiv-tool**：本地运行的 Pixiv 客户端工具，V1 聚焦 Pixiv 小说抓取（单篇 / 系列 / 用户全集），技术栈 Python + pywebview + Vue3 + FastAPI。

详见 `docs/SPEC.md`。

## Agent skills

### Issue tracker

**Local markdown**：ticket 存为 `.scratch/<feature-slug>/issues/<NN>-<slug>.md`。当前 feature 是 `pixiv-tool-v1`。详见 `docs/agents/issue-tracker.md`。

### Triage labels

五个标准 role（`needs-triage` / `needs-info` / `ready-for-agent` / `ready-for-human` / `wontfix`），作为 issue 文件顶部 `Status:` 行的值。详见 `docs/agents/triage-labels.md`。

### Domain docs

单上下文仓库，真相源是 `docs/SPEC.md`（不是单独的 `CONTEXT.md`）。详见 `docs/agents/domain.md`。

## 开发约定

### 必读

开工前先读：

1. `docs/SPEC.md` —— 完整规格 + 风险登记
2. `docs/adr/0001` ~ `0005` —— 关键架构决策
3. 你要动的 ticket（`.scratch/pixiv-tool-v1/issues/<NN>-xxx.md`）

### 技术栈

| 层 | 选型 |
|---|---|
| 桌面外壳 | pywebview 5+ |
| 后端 | Python 3.11+ · FastAPI · uvicorn |
| 前端 | Vue 3.4+ · TypeScript · Vite 5 · Naive UI |
| 数据库 | SQLite（标准库 `sqlite3`） |
| 依赖 | uv（后端）+ pnpm（前端） |
| 打包 | PyInstaller --onedir |

### 开发命令

```powershell
# 启动 dev 服务（前后端统一管理）
./scripts/dev.ps1 start           # 启动所有
./scripts/dev.ps1 start frontend  # 仅前端
./scripts/dev.ps1 start backend   # 仅后端
./scripts/dev.ps1 status          # 查看状态
./scripts/dev.ps1 logs            # 查看日志
./scripts/dev.ps1 stop            # 停止所有
```

- dev 端口：前端 9961、后端 9962
- prod 端口：范围 `[9962, 9999]` 探测

### Git 约定

- 提交信息用 conventional commits（`feat:` / `fix:` / `docs:` / `refactor:` / `chore:` / `spike:`）
- 每完成一个 ticket 至少一次提交
- 不要直接改 `docs/SPEC.md` —— 先对齐或新增 ADR

### 安全边界

- `config/cookies.dat` 是 DPAPI 加密的登录态，**绝不入库**（已在 `.gitignore`）
- `data/app.db` 是用户数据，**绝不入库**
- spike 代码可参考但**不直接复用**到主代码（见 ADR 0005 复用清单）
