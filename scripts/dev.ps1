<#
.SYNOPSIS
    pixiv-tool 开发服务管理脚本（Windows PowerShell）。
.DESCRIPTION
    支持 start/stop/restart/status/logs 子命令，管理前后端 dev 进程。
    所有平台特定命令在注释里标注 sh 等价物，方便后续转 dev.sh。
.PARAMETER Command
    子命令：start, stop, restart, status, logs
.PARAMETER Target
    目标：all（默认）, frontend, backend
.EXAMPLE
    ./scripts/dev.ps1 start
    ./scripts/dev.ps1 start frontend
    ./scripts/dev.ps1 status
    ./scripts/dev.ps1 logs backend
    ./scripts/dev.ps1 stop
#>

param(
    [Parameter(Position = 0)]
    [ValidateSet("start", "stop", "restart", "status", "logs")]
    [string]$Command = "status",

    [Parameter(Position = 1)]
    [ValidateSet("all", "frontend", "backend")]
    [string]$Target = "all"
)

$ErrorActionPreference = "Stop"

# --- 路径常量 ---
$RepoRoot = Split-Path -Parent $PSScriptRoot
$DevDir = Join-Path $RepoRoot ".dev"
$PidDir = Join-Path $DevDir "pids"
$LogDir = Join-Path $DevDir "logs"

# sh 等价: SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"; REPO_ROOT="$(dirname "$SCRIPT_DIR")"

$FrontendPort = 9961
$BackendPort = 9962
$FrontendPidFile = Join-Path $PidDir "frontend.pid"
$BackendPidFile = Join-Path $PidDir "backend.pid"
$FrontendLogFile = Join-Path $LogDir "frontend.log"
$BackendLogFile = Join-Path $LogDir "backend.log"

# -------------------------------------------------------------------
# 工具函数
# -------------------------------------------------------------------

function Resolve-Command([string]$Name) {
    # Windows 上 pnpm/uv 通常是 .cmd/.exe shim，Start-Process 不能直接吃 .ps1 shim。
    # 从所有候选里优先取 .exe，其次 .cmd，最后回退 Get-Command 默认结果。
    $candidates = Get-Command $Name -All -ErrorAction SilentlyContinue
    if ($null -eq $candidates) { return $Name }
    $exe = $candidates | Where-Object { $_.Source -like '*.exe' } | Select-Object -First 1
    if ($null -ne $exe) { return $exe.Source }
    $cmd = $candidates | Where-Object { $_.Source -like '*.cmd' } | Select-Object -First 1
    if ($null -ne $cmd) { return $cmd.Source }
    return $candidates[0].Source
}

function Ensure-Dirs {
    # sh: mkdir -p "$DEV_DIR" "$PID_DIR" "$LOG_DIR"
    @($DevDir, $PidDir, $LogDir) | ForEach-Object {
        if (-not (Test-Path $_)) { New-Item -ItemType Directory -Path $_ -Force | Out-Null }
    }
}

function Test-PortOpen([int]$Port) {
    # sh: curl -sf http://127.0.0.1:$Port/ > /dev/null 2>&1 || curl -sf http://[::1]:$Port/
    # Vite 默认绑 [::1]（IPv6），uvicorn 绑 127.0.0.1（IPv4），两个都试。
    # 4xx/5xx 也算端口在响应（后端根路径返回 404 是正常的）。
    foreach ($host_ in @('127.0.0.1', '[::1]')) {
        try {
            Invoke-WebRequest -Uri "http://${host_}:$Port/" -TimeoutSec 2 -UseBasicParsing -ErrorAction Stop | Out-Null
            return $true
        } catch {
            if ($null -ne $_.Exception.Response) { return $true }
        }
    }
    return $false
}

function Get-PidFromFile([string]$PidFile) {
    if (Test-Path $PidFile) {
        $procId = (Get-Content $PidFile -Raw).Trim()
        if ($procId -match '^\d+$') { return [int]$procId }
    }
    return $null
}

function Test-ProcessAlive([int]$ProcessId) {
    # sh: kill -0 $PID 2>/dev/null
    try {
        Get-Process -Id $ProcessId -ErrorAction Stop | Out-Null
        return $true
    } catch {
        return $false
    }
}

function Stop-ProcessByPidFile([string]$PidFile, [string]$Name, [int]$Port) {
    # PID 文件记的是 cmd.exe wrapper（Start-Process uv.CMD/pnpm.CMD 拿到的），
    # uvicorn --reload 还会 fork worker，只杀 wrapper 会留孤儿占用端口。
    # 保险起见：先按 PID 文件杀，再按端口反查 owning process 补杀。
    $procId = Get-PidFromFile $PidFile
    if ($null -ne $procId -and (Test-ProcessAlive $procId)) {
        Write-Host "停止 $Name (PID: $procId)..."
        # sh: kill $PID
        Stop-Process -Id $procId -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 500
    }

    # 按端口反查（双栈），杀掉仍在占用端口的孤儿进程
    foreach ($host_ in @('127.0.0.1', '[::1]')) {
        try {
            $conns = Get-NetTCPConnection -LocalPort $Port -LocalAddress $host_ -State Listen -ErrorAction SilentlyContinue
            foreach ($conn in $conns) {
                $orphanId = $conn.OwningProcess
                if ($null -ne $orphanId -and (Test-ProcessAlive $orphanId)) {
                    Write-Host "  清理孤儿进程 $Name (port $Port, PID: $orphanId)..."
                    Stop-Process -Id $orphanId -Force -ErrorAction SilentlyContinue
                }
            }
        } catch { }
        Start-Sleep -Milliseconds 300
    }
    if (Test-Path $PidFile) { Remove-Item $PidFile -Force }
}

# -------------------------------------------------------------------
# start
# -------------------------------------------------------------------

function Start-Frontend {
    $procId = Get-PidFromFile $FrontendPidFile
    if ($null -ne $procId -and (Test-ProcessAlive $procId)) {
        Write-Host "前端已在运行 (PID: $procId)"
        return
    }
    Ensure-Dirs
    Write-Host "启动前端 (Vite dev server, port $FrontendPort)..."
    # sh: cd "$REPO_ROOT/frontend" && pnpm dev > "$LOG_FILE" 2>&1 &
    $pnpmExe = Resolve-Command "pnpm"
    $proc = Start-Process -FilePath $pnpmExe -ArgumentList "dev" `
        -WorkingDirectory (Join-Path $RepoRoot "frontend") `
        -RedirectStandardOutput $FrontendLogFile `
        -RedirectStandardError (Join-Path $LogDir "frontend.err.log") `
        -NoNewWindow -PassThru
    $proc.Id | Out-File -FilePath $FrontendPidFile -Encoding ascii -NoNewline
    Write-Host "前端已启动 (PID: $($proc.Id))"
}

function Start-Backend {
    $procId = Get-PidFromFile $BackendPidFile
    if ($null -ne $procId -and (Test-ProcessAlive $procId)) {
        Write-Host "后端已在运行 (PID: $procId)"
        return
    }
    Ensure-Dirs
    Write-Host "启动后端 (uvicorn, port $BackendPort)..."
    # sh: cd "$REPO_ROOT" && uv run uvicorn backend.main:app --host 127.0.0.1 --port 9962 --reload > "$LOG_FILE" 2>&1 &
    $uvExe = Resolve-Command "uv"
    $proc = Start-Process -FilePath $uvExe -ArgumentList "run", "uvicorn", "backend.main:app", "--host", "127.0.0.1", "--port", "$BackendPort", "--reload" `
        -WorkingDirectory $RepoRoot `
        -RedirectStandardOutput $BackendLogFile `
        -RedirectStandardError (Join-Path $LogDir "backend.err.log") `
        -NoNewWindow -PassThru
    $proc.Id | Out-File -FilePath $BackendPidFile -Encoding ascii -NoNewline
    Write-Host "后端已启动 (PID: $($proc.Id))"
}

function Invoke-Start {
    Ensure-Dirs
    switch ($Target) {
        "all"      { Start-Frontend; Start-Backend }
        "frontend" { Start-Frontend }
        "backend"  { Start-Backend }
    }
    Write-Host ""
    Write-Host "开发服务已启动。前端: http://localhost:$FrontendPort  后端: http://localhost:$BackendPort"
}

# -------------------------------------------------------------------
# stop
# -------------------------------------------------------------------

function Invoke-Stop {
    Ensure-Dirs
    switch ($Target) {
        "all"      { Stop-ProcessByPidFile $FrontendPidFile "前端" $FrontendPort; Stop-ProcessByPidFile $BackendPidFile "后端" $BackendPort }
        "frontend" { Stop-ProcessByPidFile $FrontendPidFile "前端" $FrontendPort }
        "backend"  { Stop-ProcessByPidFile $BackendPidFile "后端" $BackendPort }
    }
    Write-Host "已停止。"
}

# -------------------------------------------------------------------
# restart
# -------------------------------------------------------------------

function Invoke-Restart {
    Invoke-Stop
    Start-Sleep -Milliseconds 500
    Invoke-Start
}

# -------------------------------------------------------------------
# status
# -------------------------------------------------------------------

function Invoke-Status {
    Ensure-Dirs
    Write-Host "=== pixiv-tool 开发服务状态 ==="

    # 前端
    $fePid = Get-PidFromFile $FrontendPidFile
    $feAlive = ($null -ne $fePid) -and (Test-ProcessAlive $fePid)
    $fePort = Test-PortOpen $FrontendPort
    $feStatus = if ($feAlive -and $fePort) { "运行中" } elseif ($feAlive) { "进程存在但端口未响应" } else { "未运行" }
    $fePidDisplay = if ($null -ne $fePid) { $fePid } else { 'N/A' }
    Write-Host "前端: $feStatus (PID: $fePidDisplay, port: $FrontendPort)"

    # 后端
    $bePid = Get-PidFromFile $BackendPidFile
    $beAlive = ($null -ne $bePid) -and (Test-ProcessAlive $bePid)
    $bePort = Test-PortOpen $BackendPort
    $beStatus = if ($beAlive -and $bePort) { "运行中" } elseif ($beAlive) { "进程存在但端口未响应" } else { "未运行" }
    $bePidDisplay = if ($null -ne $bePid) { $bePid } else { 'N/A' }
    Write-Host "后端: $beStatus (PID: $bePidDisplay, port: $BackendPort)"

    # 健康检查
    if ($bePort) {
        try {
            $r = Invoke-WebRequest -Uri "http://127.0.0.1:$BackendPort/api/health" -TimeoutSec 2 -UseBasicParsing
            Write-Host "后端健康检查: $($r.Content)"
        } catch {
            Write-Host "后端健康检查: 失败"
        }
    }
}

# -------------------------------------------------------------------
# logs
# -------------------------------------------------------------------

function Invoke-Logs {
    Ensure-Dirs
    switch ($Target) {
        "all" {
            Write-Host "=== 前端日志 ==="
            if (Test-Path $FrontendLogFile) { Get-Content $FrontendLogFile -Tail 50 }
            else { Write-Host "(无日志文件)" }
            Write-Host "`n=== 后端日志 ==="
            if (Test-Path $BackendLogFile) { Get-Content $BackendLogFile -Tail 50 }
            else { Write-Host "(无日志文件)" }
        }
        "frontend" {
            if (Test-Path $FrontendLogFile) { Get-Content $FrontendLogFile -Tail 100 }
            else { Write-Host "(无前端日志文件)" }
        }
        "backend" {
            if (Test-Path $BackendLogFile) { Get-Content $BackendLogFile -Tail 100 }
            else { Write-Host "(无后端日志文件)" }
        }
    }
}

# -------------------------------------------------------------------
# 主分发
# -------------------------------------------------------------------

switch ($Command) {
    "start"  { Invoke-Start }
    "stop"   { Invoke-Stop }
    "restart" { Invoke-Restart }
    "status" { Invoke-Status }
    "logs"   { Invoke-Logs }
}
