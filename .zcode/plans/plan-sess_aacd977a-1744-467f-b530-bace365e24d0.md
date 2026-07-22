## 目标

参考 `scripts/dev.ps1` 实现 `scripts/dev.sh`，在 macOS/Linux 下提供等价的开发服务管理（start/stop/restart/status/logs）。

注意：`dev.ps1` 顶部及各函数注释里已经大量预留了 `# sh: ...` 等价物提示——原作者本就计划好转写，本次实现就是兑现这些注释，**忠实保留 dev.ps1 的三条核心设计原则**：
1. **stop = 强杀**：按进程 cmdline 特征精确匹配本服务进程并 `kill -9`（含子进程树补杀），不依赖 PID 文件、不按端口盲目杀（避免误杀用户其他进程）。
2. **start = 轮询就绪**：启动后轮询健康检查（backend 打 `/api/health` 200，frontend 端口探测）直到通或超时（10s），不靠 sleep 赌时序。
3. **restart = stop + start**，无中间状态。

## 子命令与参数（与 dev.ps1 完全一致）

```
./scripts/dev.sh start   [all|frontend|backend]   # 默认 all
./scripts/dev.sh stop    [all|frontend|backend]
./scripts/dev.sh restart [all|frontend|backend]
./scripts/dev.sh status
./scripts/dev.sh logs    [all|frontend|backend]
```

## Unix 适配要点（与 PowerShell 的关键差异）

1. **进程匹配**（`Find-PixivProcesses` 的对应）：
   - Unix 进程 cmdline **不含 cwd**（不像 Windows 的 CommandLine），所以 dev.ps1 靠 "cmdline 含仓库路径" 过滤的策略在 Unix 上对 backend 可能失效（uv 可能用托管的 python，路径不含仓库）。
   - 改用**强特征词 + 仓库路径**双重策略，跨 macOS/Linux 都可靠（统一用 `ps -eo pid=,command=`，非 tty 下不截断）：
     - **frontend**：cmdline 含 `$REPO_ROOT` **且** 含 `vite`（vite dev server 的 `node .../vite/bin/vite.js` 是绝对路径，必然含仓库路径）。
     - **backend**：cmdline 含 `pixiv_tool.main`（本项目独有模块名，强特征），同时含 `uvicorn`。
   - 用 `grep` + `awk` 提取 PID，`sort -u` 去重。

2. **强杀 + 子进程树**（`Stop-Target` / `taskkill /F /T` 的对应）：
   - 对匹配到的每个 PID 执行 `kill -9`，同时用 `pkill -9 -P <pid>` 杀直接子进程。
   - 杀完一轮后 sleep 0.3s 再扫一次，补杀残留（`uvicorn --reload` 偶尔留 multiprocessing worker），与 dev.ps1 的两轮补杀逻辑一致，5s 上限。

3. **后台启动**（`Start-Process -NoNewWindow -PassThru` 的对应）：
   - `nohup <cmd> > "$LOG_FILE" 2> "$ERR_LOG_FILE" &` 然后 `echo $! > "$PID_FILE"` + `disown`，让进程脱离脚本、忽略 SIGHUP，且 stdout/stderr 分文件落盘（与 dev.ps1 的 `.log` / `.err.log` 分离一致）。

4. **端口探测**（`Test-PortOpen` / `Get-PortListenerPids`）：
   - `lsof -ti tcp:$PORT` 取监听者 PID（用于 start 时拒绝被外部程序占用的端口）。
   - `curl -sf http://127.0.0.1:$PORT/` 失败再试 `http://[::1]:$PORT/`（Vite 默认绑 IPv6，uvicorn 绑 IPv4，两个都试；4xx/5xx 也算端口在响应）。

5. **就绪轮询**（`Wait-PortReady`）：300ms 间隔轮询，backend 要求 `/api/health` 返回 200，frontend 仅端口探测，10s 超时。

6. **工具解析**（`Resolve-Command`）：sh 能直接执行 `pnpm`/`uv` 的 shim，**不需要**像 PowerShell 那样预解析 `.exe`/`.cmd`——这个函数在 dev.sh 里直接省略，命令行直接写 `pnpm` / `uv`。

7. **Shebang**：用 `#!/usr/bin/env bash`（macOS 自带 bash 3.2 也支持 `[[ ]]`/`case`/数组等用到的基础语法，Linux 自带 bash 4+）。

## 与 dev.ps1 行为对齐的细节

- 目录常量：`.dev/pids/`、`.dev/logs/`，PID/日志文件名同 ps1（`frontend.pid`/`backend.pid`/`frontend.log`/`backend.err.log` 等）。
- 端口常量：前端 9961、后端 9962。
- start 时若发现已有进程：先探端口是否就绪，就绪则报"已在运行"直接返回；未就绪则先 stop 再重启（与 ps1 一致）。
- start 失败时清理本次启动的进程（与 ps1 的 `Invoke-Start` 收尾一致）。
- 日志输出文案与符号（`✓`/`✗`）与 ps1 对齐。
- `set -euo pipefail` 作为脚本顶部安全网；但 stop/status 等只读探测段用 `|| true` 包裹，避免因 grep 无匹配（返回 1）触发 `set -e` 退出。

## 文件清单

1. **新增** `scripts/dev.sh`（约 280–340 行，含完整中文注释，注释风格对齐 dev.ps1）。
2. **`chmod +x scripts/dev.sh`**。
3. **更新** `README.md`：
   - 在"启动开发服务"段落补一段 Linux/macOS 用 `./scripts/dev.sh` 的示例（与现有 PowerShell 示例并列）。
   - 修正 `cd backend && uv sync` → 仓库根 `uv sync`（顺带修这个错误，否则照 README 跑会失败）。

## 验证步骤（实现后执行）

1. `bash -n scripts/dev.sh`（语法检查）。
2. `shellcheck scripts/dev.sh`（若本机有）。
3. `./scripts/dev.sh status`（在无进程时应干净显示"未运行"，不报错）。
4. 先手动 `uv sync` + `cd frontend && pnpm install` 后，`./scripts/dev.sh start` → 验证前后端都 `✓ 就绪`，`curl http://127.0.0.1:9962/api/health` 返回 `{"status":"ok"}`。
5. `./scripts/dev.sh stop` → 验证进程被杀干净，`status` 显示未运行。
6. `./scripts/dev.sh restart` → 验证无残留。

> 说明：上述验证若后端因依赖未装等原因起不来，脚本应按设计在日志里指明 `✗ ... 见日志`，这本身也是验证错误路径正确的一部分。