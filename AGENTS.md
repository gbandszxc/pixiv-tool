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
4. 涉及前端界面、组件、样式或交互时，必须先读根目录 `DESIGN.md`。
5. 涉及打包/PyInstaller/分发时，先读 `docs/PACKAGING.md`。

### 设计系统维护

- `DESIGN.md` 是前端视觉与交互规范的真相源；其中的色彩、字号、间距、圆角、层级、动效与组件约束优先于临时页面样式。
- 做前端样式或组件改动时，优先复用 `DESIGN.md` 已定义的 token 和 Naive UI 主题配置；不要在页面中新增无来源的颜色、圆角、阴影或动效字面值。
- 如果实现需要新增或调整设计 token、组件规则或视觉方向，必须在同一变更中同步更新 `DESIGN.md` 和 `.impeccable/design.json`；二者应保持一致。
- 需要重新提炼或大幅刷新设计规范时，使用 `$impeccable:impeccable document`；已有 `DESIGN.md` 不得静默覆盖，先与用户确认合并或刷新范围。
- 前端视觉验收应至少覆盖默认、hover、focus、disabled、loading、error 状态，并检查长文本、窄窗口与“减少动态效果”偏好。

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

- 提交信息格式：`<类型>([<范围>]): <中文改动说明>`。英文前缀（如 `feat`、`fix`、`refactor`、`docs`、`style`、`chore`、`perf`、`test`）表示改动大类，范围可选，中文部分写明改动内容和原因，避免过于简略。示例：`feat: 增加图片全屏预览`。
- 每完成一个 ticket 至少一次提交

### 安全边界

- `config/cookies.dat` 是 DPAPI 加密的登录态，**绝不入库**（已在 `.gitignore`）
- `data/app.db` 是用户数据，**绝不入库**
- spike 代码可参考但**不直接复用**到主代码（见 ADR 0005 复用清单）
