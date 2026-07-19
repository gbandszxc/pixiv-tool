# 01 — 项目骨架 + dev.ps1 + 端口管理

**What to build:**

从用户视角：开发者 clone 仓库后，执行 `./scripts/dev.ps1 start` 一行命令，前后端同时启动，浏览器自动打开 `http://localhost:9961/` 看到 Vue3 欢迎页，控制台显示前后端 PID 和健康状态。`./scripts/dev.ps1 status` 能查看运行状态，`stop` 能干净停止。整个仓库具备 backend (FastAPI) + frontend (Vue3+Vite) 的最小可运行骨架，prod 模式端口能在 [9962, 9999] 范围内自动探测空闲端口。

**Blocked by:** None — 可立即开始

**Status:** ready-for-agent

**Acceptance criteria:**

- [ ] 仓库根目录有 `backend/` 和 `frontend/` 两个子目录，结构符合 SPEC §3.4
- [ ] `backend/pyproject.toml` 声明 Python 3.11+，依赖含 fastapi、uvicorn、pywebview、httpx，用 uv 管理
- [ ] `frontend/package.json` 用 pnpm，含 vue@3.4+、vue-router@4、pinia、naive-ui、axios、typescript、vite@5
- [ ] `backend/main.py` 能启动一个最小 FastAPI 应用，监听 `127.0.0.1:9962`，提供 `GET /api/health` 返回 `{"status": "ok"}`
- [ ] `frontend/` 默认页面（`/`）显示 "pixiv-tool" 标题，Vite dev server 监听 9961
- [ ] `frontend/vite.config.ts` 配置 proxy，把 `/api/*` 转发到 `http://127.0.0.1:9962`
- [ ] `scripts/dev.ps1` 支持 `start [all|frontend|backend]`、`stop`、`restart`、`status`、`logs [frontend|backend]` 子命令
- [ ] PID 写到 `.dev/pids/{frontend,backend}.pid`，日志写到 `.dev/logs/{frontend,backend}.log`
- [ ] `status` 子命令对 frontend 调 `GET 127.0.0.1:9961/`、对 backend 调 `GET 127.0.0.1:9962/api/health` 做健康检查
- [ ] dev.ps1 注释完整，所有 PowerShell 特定命令标注 sh 等价物（为后续转 dev.sh 准备）
- [ ] `backend/main.py` 含 `find_available_port(start=9962, end=9999)` 函数，回退到 [10000, 19999]，仍失败抛 `PortAllocationError`
- [ ] prod 模式端口写入 `os.environ['PIXIV_TOOL_PORT']` 供 pywebview 读取
- [ ] README.md 含 dev 启动说明
- [ ] `.gitignore` 已配置（前序工作已完成，本 ticket 验证覆盖完整）
