#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# pixiv-tool 开发服务管理脚本(macOS / Linux bash 版)。
#
# 与 scripts/dev.ps1 行为对等,管理前后端 dev 进程。
# 设计原则(同 dev.ps1):
#   - stop = 强杀。按进程命令行特征精确匹配本服务的 vite/uvicorn 进程,
#     kill -9 连子进程树一起杀。不依赖 PID 文件、不按端口反查(避免误杀
#     用户其他进程)。
#   - start = 启动后轮询就绪(后端打 /api/health 200,前端做端口探测)直到
#     通或超时(10s),不靠 sleep 赌时序。
#   - restart = stop + start,无中间状态。
#
# 用法:
#   ./scripts/dev.sh start   [all|frontend|backend]   # 默认 all
#   ./scripts/dev.sh stop    [all|frontend|backend]
#   ./scripts/dev.sh restart [all|frontend|backend]
#   ./scripts/dev.sh status
#   ./scripts/dev.sh logs    [all|frontend|backend]
#
# 兼容性:macOS 自带 bash 3.2 与 Linux bash 4+ 均可。
# ---------------------------------------------------------------------------

set -euo pipefail
# 注意:不在这里全局 `set -m`。job control 只在 start 的子 shell 内【局部】开启
# (见 start_frontend/start_backend),让每个服务进程成为独立进程组的 leader。
# 全局开 set -m 会干扰 stop/status 里的进程扫描(ps/grep 管道、kill)与作业
# 通知,偶发挂起。局部开则既拿到进程组语义,又不影响主流程。

# -------------------------------------------------------------------
# 路径与端口常量(与 dev.ps1 完全一致)
# -------------------------------------------------------------------

# 定位脚本与仓库根。$0 在不同调用方式下可能是相对路径,用 cd + pwd 归一。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

DEV_DIR="$REPO_ROOT/.dev"
PID_DIR="$DEV_DIR/pids"
LOG_DIR="$DEV_DIR/logs"

FRONTEND_PORT=9961
BACKEND_PORT=9962

FRONTEND_PID_FILE="$PID_DIR/frontend.pid"
BACKEND_PID_FILE="$PID_DIR/backend.pid"
FRONTEND_LOG_FILE="$LOG_DIR/frontend.log"
FRONTEND_ERR_FILE="$LOG_DIR/frontend.err.log"
BACKEND_LOG_FILE="$LOG_DIR/backend.log"
BACKEND_ERR_FILE="$LOG_DIR/backend.err.log"

# start 后等服务真的就绪的最长秒数(本机冷启动实测 10s 够,同 dev.ps1)。
START_TIMEOUT_SEC=10

# -------------------------------------------------------------------
# 参数解析
# -------------------------------------------------------------------

# 子命令: start, stop, restart, status, logs
COMMAND="${1:-status}"
# 目标: all(默认), frontend, backend
TARGET="${2:-all}"

case "$COMMAND" in
    start|stop|restart|status|logs) ;;
    *)
        echo "错误: 未知子命令 '$COMMAND'" >&2
        echo "用法: $0 {start|stop|restart|status|logs} [all|frontend|backend]" >&2
        exit 2
        ;;
esac

case "$TARGET" in
    all|frontend|backend) ;;
    *)
        echo "错误: 未知目标 '$TARGET'(应为 all|frontend|backend)" >&2
        exit 2
        ;;
esac

# -------------------------------------------------------------------
# 工具函数
# -------------------------------------------------------------------

ensure_dirs() {
    # 等价 dev.ps1 Ensure-Dirs:mkdir -p "$DEV_DIR" "$PID_DIR" "$LOG_DIR"
    mkdir -p "$DEV_DIR" "$PID_DIR" "$LOG_DIR"
}

# 输出匹配目标的服务进程 PID 列表(每行一个,去重),供 stop/status 使用。
# 等价 dev.ps1 Find-PixivProcesses。
#
# Unix 进程命令行不含 cwd(不像 Windows 的 CommandLine),所以 dev.ps1
# 靠 "命令行含仓库路径" 过滤的策略对后端可能失效(uv 可能用托管 python,
# 命令行不含仓库路径)。这里改用强特征词过滤,跨 macOS/Linux 都可靠:
#   - frontend: 命令行含 $REPO_ROOT 且含 vite
#               (vite dev server 是 node .../vite/bin/vite.js,绝对路径,
#                必然含仓库路径)
#   - backend:  命令行含 pixiv_tool.main(本项目独有模块名)且含 uvicorn
#
# 用 ps -eo pid=,command=,非 tty 下不截断命令行。
find_pixiv_pids() {
    local target="$1"
    local pids=""

    # ps 在 macOS(BSD 风格)与 Linux 均支持 -eo。`command=` 输出完整命令行。
    local ps_out
    ps_out="$(ps -eo pid=,command= 2>/dev/null || true)"

    while IFS= read -r line; do
        # 字段以空白分隔,第一段是 PID,其余是命令行。
        local pid="${line%% *}"
        local cmd="${line#* }"
        [ -z "$pid" ] && continue

        local is_frontend=false is_backend=false
        # [[ ]] 内右侧不加引号才能做模式匹配;这里用 case 做子串包含更稳。
        case "$cmd" in
            *"$REPO_ROOT"*vite*|*vite*"$REPO_ROOT"*) is_frontend=true ;;
        esac
        case "$cmd" in
            *pixiv_tool.main*uvicorn*|*uvicorn*pixiv_tool.main*) is_backend=true ;;
        esac

        local hit=false
        case "$target" in
            frontend) [ "$is_frontend" = true ] && hit=true ;;
            backend)  [ "$is_backend"  = true ] && hit=true ;;
            all)      { [ "$is_frontend" = true ] || [ "$is_backend" = true ]; } && hit=true ;;
        esac

        if [ "$hit" = true ]; then
            pids="$pids$pid"$'\n'
        fi
    done <<< "$ps_out"

    # 去重 + 去空行。
    printf '%s' "$pids" | sort -un | grep -v '^$' || true
}

# 取占用某端口的监听进程 PID 列表(去重)。
# 等价 dev.ps1 Get-PortListenerPids: lsof -ti tcp:$Port
# 仅用于 start 时拒绝被外部程序占用的 dev 固定端口;stop 绝不按端口杀。
get_port_listener_pids() {
    local port="$1"
    lsof -ti "tcp:$port" 2>/dev/null | sort -un | grep -v '^$' || true
}

# 探测端口是否在响应(4xx/5xx 也算在响应,后端根路径返回 404 是正常的)。
# 等价 dev.ps1 Test-PortOpen:Vite 默认绑 [::1](IPv6),uvicorn 绑 127.0.0.1(IPv4),
# 两个都试。
test_port_open() {
    local port="$1"
    for h in 127.0.0.1 '[::1]'; do
        # 取 HTTP 状态码;连接失败/拒绝时为 000。任何非 000 的码都说明端口在
        # 响应(后端根路径返回 404 也算)。
        # 注意:只能用 `|| true` 吞返回码,绝不能用 `|| echo 000` —— curl 失败时
        # -w 已经输出了 000,再 echo 000 会拼成 000000,绕过下方的 != "000" 判断。
        local code
        code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 2 "http://$h:$port/" 2>/dev/null || true)"
        if [ "$code" != "000" ] && [ -n "$code" ]; then
            return 0
        fi
    done
    return 1
}

# 轮询端口直到就绪或超时。
# 等价 dev.ps1 Wait-PortReady:有 health_path 时要求返回 200;否则只做端口探测。
# 返回 0 = 就绪,1 = 超时。
wait_port_ready() {
    local port="$1" health_path="$2" timeout_sec="$3"
    local deadline=$(( $(date +%s) + timeout_sec ))

    while [ "$(date +%s)" -lt "$deadline" ]; do
        if [ -n "$health_path" ]; then
            # 要求健康检查端点返回 200。
            if curl -sf --max-time 1 "http://127.0.0.1:$port/$health_path" >/dev/null 2>&1; then
                return 0
            fi
        else
            if test_port_open "$port"; then
                return 0
            fi
        fi
        sleep 0.3
    done
    return 1
}

# -------------------------------------------------------------------
# stop(强杀)
# -------------------------------------------------------------------

# 强杀指定 target 的服务进程及其全部派生子进程(含 uvicorn --reload worker)。
# 等价 dev.ps1 Stop-Target:taskkill /F /T /PID <pid>。
#
# 两段式,保证既能处理"本脚本 start 的进程"(有进程组),也能处理"外部启动的
# 残留进程"(无进程组记录,靠命令行特征词兜底):
#   1) 进程组 kill:从 PID 文件读 pgid(本脚本用 job control 启动,leader 的
#      pid == pgid),`kill -9 -- -<pgid>` 杀整个进程组 —— 这是杀干净 reload
#      worker 的唯一可靠手段(worker 命令行无特征词,find_pixiv_pids 抓不到)。
#   2) 特征词补杀:find_pixiv_pids 按命令行特征匹配,扫残留(对未由本脚本
#      启动的进程,或进程组已失效的情况兜底)。
kill_target() {
    local target_name="$1" target="$2"
    local pid_file=""
    case "$target" in
        frontend) pid_file="$FRONTEND_PID_FILE" ;;
        backend)  pid_file="$BACKEND_PID_FILE" ;;
    esac

    # 先看本脚本启动时记录的进程组 leader pid。
    local leader=""
    if [ -n "$pid_file" ] && [ -f "$pid_file" ]; then
        leader="$(cat "$pid_file" 2>/dev/null | tr -d '[:space:]')"
    fi

    local feature_pids
    feature_pids="$(find_pixiv_pids "$target")"

    # 没有进程组记录、也没有特征词命中 => 确实无进程。
    if [ -z "$leader" ] && [ -z "$feature_pids" ]; then
        echo "$target_name : 无进程"
        return 0
    fi

    # --- 1) 进程组 kill(杀整个组,含 reload worker) ---
    # 用 leader pid 当 pgid(job control 下二者相等)。进程组可能已不存在,|| true。
    if [ -n "$leader" ]; then
        # 前置 '-' 表示按进程组发信号。
        if kill -0 -- "-$leader" 2>/dev/null; then
            echo "$target_name : 杀进程组 $leader (含 reload worker)"
            kill -9 -- "-$leader" 2>/dev/null || true
        else
            # 进程组不在了(可能已死或非本脚本启动),落到特征词补杀。
            echo "$target_name : 无进程组记录,按特征词杀"
        fi
    fi

    # --- 2) 特征词补杀(扫任何残留的本服务进程) ---
    local p
    for p in $feature_pids; do
        echo "$target_name : 杀 PID $p"
        kill -9 "$p" 2>/dev/null || true
    done

    # 杀完一轮后再扫,确认没残留(uvicorn --reload 偶尔留 multiprocessing worker)。
    # 与 dev.ps1 一致:5s 上限,每轮 0.3s。
    local deadline=$(( $(date +%s) + 5 ))
    local leftover
    while true; do
        sleep 0.3
        leftover="$(find_pixiv_pids "$target")"
        if [ -z "$leftover" ]; then
            break
        fi
        for p in $leftover; do
            echo "$target_name : 补杀 PID $p"
            kill -9 "$p" 2>/dev/null || true
        done
        if [ "$(date +%s)" -ge "$deadline" ]; then
            echo "错误:$target_name 停止失败,残留 PID:$(echo "$leftover" | tr '\n' ' ')" >&2
            return 1
        fi
    done
    return 0
}

invoke_stop() {
    ensure_dirs
    case "$TARGET" in
        all)      kill_target "前端" "frontend"; kill_target "后端" "backend" ;;
        frontend) kill_target "前端" "frontend" ;;
        backend)  kill_target "后端" "backend" ;;
    esac
    # 清理对应 PID 文件;不能因 stop frontend 破坏仍在运行的 backend 记录。
    case "$TARGET" in
        all)      rm -f "$FRONTEND_PID_FILE" "$BACKEND_PID_FILE" ;;
        frontend) rm -f "$FRONTEND_PID_FILE" ;;
        backend)  rm -f "$BACKEND_PID_FILE" ;;
    esac
    echo "已停止。"
}

# -------------------------------------------------------------------
# start
# -------------------------------------------------------------------

start_frontend() {
    local existing
    existing="$(find_pixiv_pids "frontend")"
    if [ -n "$existing" ]; then
        if wait_port_ready "$FRONTEND_PORT" "" "$START_TIMEOUT_SEC"; then
            echo "前端已在运行 (PID: $(echo "$existing" | head -1))"
            return 0
        fi
        echo "前端进程未就绪,先清理后重启。"
        kill_target "前端" "frontend"
    fi

    local listeners
    listeners="$(get_port_listener_pids "$FRONTEND_PORT")"
    if [ -n "$listeners" ]; then
        echo "错误:前端端口 $FRONTEND_PORT 已被非 pixiv-tool 进程占用 (PID: $(echo "$listeners" | tr '\n' ' '))" >&2
        return 1
    fi

    ensure_dirs
    echo "启动前端 (Vite, port $FRONTEND_PORT)..."
    # 等价 dev.ps1:cd "$REPO_ROOT/frontend" && pnpm dev > "$LOG_FILE" 2> "$ERR_FILE" &
    #
    # 关键:子 shell 内 `set -m` 开 job control,再 `&` 后台启动 —— 此时
    # pnpm/node 会成为【独立进程组的 leader】(pgid == pid),它派生的 vite
    # 子进程都进同一进程组。stop 用 `kill -- -<pgid>` 即可杀整组,无需
    # 按特征词逐个找。$! 在 job control 下就是 leader 的 pid,记入 PID 文件。
    # 子 shell 立即退出(不 wait),后台作业脱离脚本继续运行。
    ( set -m
      cd "$REPO_ROOT/frontend"
      nohup pnpm dev >"$FRONTEND_LOG_FILE" 2>"$FRONTEND_ERR_FILE" &
      echo $! >"$FRONTEND_PID_FILE"
    ) </dev/null >/dev/null 2>&1
    local pid
    pid="$(cat "$FRONTEND_PID_FILE" 2>/dev/null | tr -d '[:space:]' || echo '?')"

    # 轮询端口(Vite 没健康检查端点,只用端口探测)。
    if wait_port_ready "$FRONTEND_PORT" "" "$START_TIMEOUT_SEC"; then
        echo "  ✓ 前端就绪 (PID: $pid, port $FRONTEND_PORT)"
        return 0
    fi
    echo "  ✗ 前端在 ${START_TIMEOUT_SEC}s 内未就绪,见日志: $FRONTEND_LOG_FILE"
    kill_target "前端" "frontend"
    return 1
}

start_backend() {
    local existing
    existing="$(find_pixiv_pids "backend")"
    if [ -n "$existing" ]; then
        if wait_port_ready "$BACKEND_PORT" "api/health" "$START_TIMEOUT_SEC"; then
            echo "后端已在运行 (PID: $(echo "$existing" | head -1))"
            return 0
        fi
        echo "后端进程未就绪,先清理后重启。"
        kill_target "后端" "backend"
    fi

    local listeners
    listeners="$(get_port_listener_pids "$BACKEND_PORT")"
    if [ -n "$listeners" ]; then
        echo "错误:后端端口 $BACKEND_PORT 已被非 pixiv-tool 进程占用 (PID: $(echo "$listeners" | tr '\n' ' '))" >&2
        return 1
    fi

    ensure_dirs
    echo "启动后端 (uvicorn, port $BACKEND_PORT)..."
    # 等价 dev.ps1:cd "$REPO_ROOT" && uv run uvicorn pixiv_tool.main:app --host 127.0.0.1 --port 9962 --reload > "$LOG_FILE" 2> "$ERR_FILE" &
    #
    # 关键:子 shell 内 `set -m` 开 job control,再 `&` 后台启动 —— uv run /
    # uvicorn 成为独立进程组 leader(pgid == pid),它派生的 --reload worker
    # 都进同一进程组。stop 用 `kill -- -<pgid>` 杀整组(见 kill_target),这是
    # 清理 reload worker 的唯一可靠手段(worker 命令行无特征词,无法靠
    # find_pixiv_pids 匹配)。$! 记入 PID 文件,stop 时当 pgid 用。
    ( set -m
      cd "$REPO_ROOT"
      nohup uv run uvicorn pixiv_tool.main:app --host 127.0.0.1 --port "$BACKEND_PORT" --reload >"$BACKEND_LOG_FILE" 2>"$BACKEND_ERR_FILE" &
      echo $! >"$BACKEND_PID_FILE"
    ) </dev/null >/dev/null 2>&1
    local pid
    pid="$(cat "$BACKEND_PID_FILE" 2>/dev/null | tr -d '[:space:]' || echo '?')"

    # 轮询 /api/health(确认不仅是端口开,而且 FastAPI 应用已 ready)。
    if wait_port_ready "$BACKEND_PORT" "api/health" "$START_TIMEOUT_SEC"; then
        echo "  ✓ 后端就绪 (PID: $pid, port $BACKEND_PORT)"
        return 0
    fi
    echo "  ✗ 后端在 ${START_TIMEOUT_SEC}s 内未就绪,见日志: $BACKEND_LOG_FILE"
    kill_target "后端" "backend"
    return 1
}

invoke_start() {
    ensure_dirs
    local ok=0
    case "$TARGET" in
        all)
            if start_frontend; then
                if start_backend; then ok=0; else ok=1; fi
            else
                ok=1
            fi
            # start 失败时清理本次启动的进程(等价 dev.ps1 Invoke-Start 收尾)。
            if [ "$ok" -ne 0 ]; then
                kill_target "前端" "frontend"
                kill_target "后端" "backend"
            fi
            ;;
        frontend) start_frontend || ok=1 ;;
        backend)  start_backend  || ok=1 ;;
    esac

    echo ""
    if [ "$ok" -eq 0 ]; then
        echo "开发服务已启动。前端: http://localhost:$FRONTEND_PORT  后端: http://localhost:$BACKEND_PORT"
    else
        echo "错误:开发服务启动失败,已清理本次启动的进程;详见上面 ✗ 行和日志。" >&2
        exit 1
    fi
}

# -------------------------------------------------------------------
# restart = stop + start
# -------------------------------------------------------------------

invoke_restart() {
    # 严格 stop 然后 start,不在中间 sleep(stop 已等子进程结束)。
    invoke_stop
    invoke_start
}

# -------------------------------------------------------------------
# status
# -------------------------------------------------------------------

invoke_status() {
    ensure_dirs
    echo "=== pixiv-tool 开发服务状态 ==="

    # 前端
    local fe_pids fe_port_ok fe_status fe_pids_str
    fe_pids="$(find_pixiv_pids "frontend")"
    if test_port_open "$FRONTEND_PORT"; then fe_port_ok=1; else fe_port_ok=0; fi
    if [ -n "$fe_pids" ] && [ "$fe_port_ok" -eq 1 ]; then
        fe_status="运行中"
    elif [ -n "$fe_pids" ]; then
        fe_status="进程存在但端口未响应"
    else
        fe_status="未运行"
    fi
    if [ -n "$fe_pids" ]; then fe_pids_str="$(echo "$fe_pids" | tr '\n' ',' | sed 's/,$//')"; else fe_pids_str="N/A"; fi
    echo "前端: $fe_status (PID: $fe_pids_str, port: $FRONTEND_PORT)"

    # 后端
    local be_pids be_port_ok be_status be_pids_str
    be_pids="$(find_pixiv_pids "backend")"
    if test_port_open "$BACKEND_PORT"; then be_port_ok=1; else be_port_ok=0; fi
    if [ -n "$be_pids" ] && [ "$be_port_ok" -eq 1 ]; then
        be_status="运行中"
    elif [ -n "$be_pids" ]; then
        be_status="进程存在但端口未响应"
    else
        be_status="未运行"
    fi
    if [ -n "$be_pids" ]; then be_pids_str="$(echo "$be_pids" | tr '\n' ',' | sed 's/,$//')"; else be_pids_str="N/A"; fi
    echo "后端: $be_status (PID: $be_pids_str, port: $BACKEND_PORT)"

    # 后端健康检查
    if [ "$be_port_ok" -eq 1 ]; then
        local health
        health="$(curl -sf --max-time 2 "http://127.0.0.1:$BACKEND_PORT/api/health" 2>/dev/null || true)"
        if [ -n "$health" ]; then
            echo "后端健康检查: $health"
        else
            echo "后端健康检查: 失败"
        fi
    fi
}

# -------------------------------------------------------------------
# logs
# -------------------------------------------------------------------

invoke_logs() {
    ensure_dirs
    case "$TARGET" in
        all)
            echo "=== 前端日志 ==="
            if [ -f "$FRONTEND_LOG_FILE" ]; then tail -n 50 "$FRONTEND_LOG_FILE"; else echo "(无日志文件)"; fi
            echo ""
            echo "=== 后端日志 ==="
            if [ -f "$BACKEND_LOG_FILE" ]; then tail -n 50 "$BACKEND_LOG_FILE"; else echo "(无日志文件)"; fi
            ;;
        frontend)
            if [ -f "$FRONTEND_LOG_FILE" ]; then tail -n 100 "$FRONTEND_LOG_FILE"; else echo "(无前端日志文件)"; fi
            ;;
        backend)
            if [ -f "$BACKEND_LOG_FILE" ]; then tail -n 100 "$BACKEND_LOG_FILE"; else echo "(无后端日志文件)"; fi
            ;;
    esac
}

# -------------------------------------------------------------------
# 主分发
# -------------------------------------------------------------------

case "$COMMAND" in
    start)   invoke_start ;;
    stop)    invoke_stop ;;
    restart) invoke_restart ;;
    status)  invoke_status ;;
    logs)    invoke_logs ;;
esac
