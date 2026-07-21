<#
.SYNOPSIS
    pixiv-tool 开发服务管理脚本(Windows PowerShell)。
.DESCRIPTION
    支持 start/stop/restart/status/logs 子命令,管理前后端 dev 进程。
    设计原则:
      - stop = 强杀。扫所有命令行含本仓库路径的 python.exe/node.exe 进程,
        taskkill /F /T 连子进程树一起杀。不依赖 PID 文件、不依赖端口反查。
      - start = 启动后轮询 /api/health 直到通(最多 10s),不靠 sleep 赌时序。
      - restart = stop + start,无中间状态。
    所有平台特定命令在注释里标注 sh 等价物,方便后续转 dev.sh。
.PARAMETER Command
    子命令:start, stop, restart, status, logs
.PARAMETER Target
    目标:all(默认), frontend, backend
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
# sh: SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"; REPO_ROOT="$(dirname "$SCRIPT_DIR")"
# $PSScriptRoot 在 dot-source 上下文下为空,回退到 $PSCommandPath
$ScriptDir = if ($PSScriptRoot) { $PSScriptRoot } elseif ($PSCommandPath) { Split-Path -Parent $PSCommandPath } else { $PWD.Path }
$RepoRoot = Split-Path -Parent $ScriptDir

$DevDir = Join-Path $RepoRoot ".dev"
$PidDir = Join-Path $DevDir "pids"
$LogDir = Join-Path $DevDir "logs"

$FrontendPort = 9961
$BackendPort = 9962
$FrontendPidFile = Join-Path $PidDir "frontend.pid"
$BackendPidFile = Join-Path $PidDir "backend.pid"
$FrontendLogFile = Join-Path $LogDir "frontend.log"
$BackendLogFile = Join-Path $LogDir "backend.log"

# start 后等 /api/health 200 的最长秒数(本机冷启动实测 10s 够)
$StartTimeoutSec = 10

# -------------------------------------------------------------------
# 工具函数
# -------------------------------------------------------------------

function Ensure-Dirs {
    # sh: mkdir -p "$DEV_DIR" "$PID_DIR" "$LOG_DIR"
    @($DevDir, $PidDir, $LogDir) | ForEach-Object {
        if (-not (Test-Path $_)) { New-Item -ItemType Directory -Path $_ -Force | Out-Null }
    }
}

function Resolve-Command([string]$Name) {
    # Windows 上 pnpm/uv 通常是 .cmd/.exe shim,Start-Process 不能直接吃 .ps1 shim。
    # 从所有候选里优先取 .exe,其次 .cmd,最后回退 Get-Command 默认结果。
    $candidates = Get-Command $Name -All -ErrorAction SilentlyContinue
    if ($null -eq $candidates) { return $Name }
    $exe = $candidates | Where-Object { $_.Source -like '*.exe' } | Select-Object -First 1
    if ($null -ne $exe) { return $exe.Source }
    $cmd = $candidates | Where-Object { $_.Source -like '*.cmd' } | Select-Object -First 1
    if ($null -ne $cmd) { return $cmd.Source }
    return $candidates[0].Source
}

function Get-PidFromFile([string]$PidFile) {
    if (Test-Path $PidFile) {
        $procId = (Get-Content $PidFile -Raw).Trim()
        if ($procId -match '^\d+$') { return [int]$procId }
    }
    return $null
}

function Set-ManagedPidFile([string]$PidFile, [string]$Target) {
    # pnpm/uv 的 shim 进程可能会立刻退出；就绪后记录真正的服务根进程，
    # 使 PID 文件可用于人工排查，停止逻辑仍以命令行匹配为准。
    $managed = @(Find-PixivProcesses -Target $Target)
    if ($managed.Count -gt 0) {
        $managed[0].ProcessId | Out-File -FilePath $PidFile -Encoding ascii -NoNewline
    }
}

function Find-PixivProcesses([string]$Target) {
    # 扫所有 cmdline 含本仓库路径的 Vite / uvicorn 进程。
    # $Target="frontend" 只返回 vite(node); "backend" 只返回 uvicorn(python);
    # "all" 全返回。
    # 用 Win32_Process 而非 Get-Process,因为前者暴露 CommandLine。
    # 调用方必须用 @(Find-PixivProcesses ...) 包装 —— PowerShell 单元素返回值
    # 会被自动 unwrap 成标量,显式 @() 才能保证始终是数组。
    $repoPath = $RepoRoot.TrimEnd('\').ToLower()
    $found = @()
    try {
        $procs = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue
        foreach ($p in $procs) {
            $cmd = $p.CommandLine
            if (-not $cmd) { continue }
            if ($cmd.ToLower() -notlike "*$repoPath*") { continue }
            $isFrontend = ($p.Name -eq 'node.exe') -and ($cmd -like '*vite*')
            # uv run 会先启动 uvicorn.exe，再由它派生 python 的 reload
            # supervisor 与 worker。三个进程都要识别，才能按进程树清理干净。
            $isBackend = ($p.Name -in @('python.exe', 'uvicorn.exe')) -and ($cmd -like '*uvicorn*')
            if ($Target -eq 'frontend' -and $isFrontend) { $found += $p }
            elseif ($Target -eq 'backend' -and $isBackend) { $found += $p }
            elseif ($Target -eq 'all' -and ($isFrontend -or $isBackend)) { $found += $p }
        }
    } catch { }
    return $found
}

function Get-PortListenerPids([int]$Port) {
    # sh: lsof -ti tcp:$Port
    # 仅用于拒绝占用 dev 固定端口的外部程序；stop 绝不按端口杀，避免误杀用户进程。
    try {
        return @(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty OwningProcess -Unique)
    } catch {
        return @()
    }
}

function Test-PortOpen([int]$Port) {
    # sh: curl -sf http://127.0.0.1:$Port/ > /dev/null 2>&1 || curl -sf http://[::1]:$Port/
    # Vite 默认绑 [::1](IPv6),uvicorn 绑 127.0.0.1(IPv4),两个都试。
    # 4xx/5xx 也算端口在响应(后端根路径返回 404 是正常的)。
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

function Wait-PortReady([int]$Port, [string]$HealthPath, [int]$TimeoutSec) {
    # 轮询端口直到健康检查通过或超时。
    # 用于 start 后等服务真的就绪,不靠 sleep 赌时序。
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if ($HealthPath) {
            try {
                $r = Invoke-WebRequest -Uri "http://127.0.0.1:$Port/$HealthPath" -TimeoutSec 1 -UseBasicParsing -ErrorAction Stop
                if ($r.StatusCode -eq 200) { return $true }
            } catch { }
        } else {
            if (Test-PortOpen -Port $Port) { return $true }
        }
        Start-Sleep -Milliseconds 300
    }
    return $false
}

# -------------------------------------------------------------------
# stop(强杀)
# -------------------------------------------------------------------

function Stop-Target([string]$TargetName, [string]$Target) {
    # 用 taskkill /F /T 连子进程树一起强杀。/T = tree,/F = force。
    # sh: pkill -f pixiv-tool | kill -9 <pid>
    $procs = @(Find-PixivProcesses -Target $Target)
    if ($procs.Count -eq 0) {
        Write-Host "$TargetName : 无进程"
        return
    }
    # taskkill 对已死 PID 会写 stderr "not found",正常情况,要忽略。
    # 临时把 ErrorActionPreference 降级,避免被脚本顶部的 "Stop" 抛出。
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    try {
        foreach ($p in $procs) {
            Write-Host "$TargetName : 杀 PID $($p.ProcessId) ($($p.Name)) + 子进程树"
            & taskkill.exe /F /T /PID $p.ProcessId 2>&1 | Out-Null
        }
        # 杀完一轮后再扫一次,确认没残留(uvicorn --reload 偶尔留 multiprocessing worker)
        $deadline = (Get-Date).AddSeconds(5)
        do {
            Start-Sleep -Milliseconds 300
            $leftover = @(Find-PixivProcesses -Target $Target)
            foreach ($p in $leftover) {
                Write-Host "$TargetName : 补杀 PID $($p.ProcessId) ($($p.Name))"
                & taskkill.exe /F /T /PID $p.ProcessId 2>&1 | Out-Null
            }
        } while ($leftover.Count -gt 0 -and (Get-Date) -lt $deadline)

        if ($leftover.Count -gt 0) {
            $pids = $leftover.ProcessId -join ','
            throw "$TargetName 停止失败，残留 PID: $pids"
        }
    } finally {
        $ErrorActionPreference = $prevEap
    }
}

function Invoke-Stop {
    Ensure-Dirs
    switch ($Target) {
        "all"      { Stop-Target "前端" "frontend"; Stop-Target "后端" "backend" }
        "frontend" { Stop-Target "前端" "frontend" }
        "backend"  { Stop-Target "后端" "backend" }
    }
    # 清理对应 PID 文件；不能因 stop frontend 破坏仍在运行的 backend 记录。
    switch ($Target) {
        'all'      { Remove-Item $FrontendPidFile, $BackendPidFile -Force -ErrorAction SilentlyContinue }
        'frontend' { Remove-Item $FrontendPidFile -Force -ErrorAction SilentlyContinue }
        'backend'  { Remove-Item $BackendPidFile -Force -ErrorAction SilentlyContinue }
    }
    Write-Host "已停止。"
}

# -------------------------------------------------------------------
# start
# -------------------------------------------------------------------

function Start-Frontend {
    $existing = @(Find-PixivProcesses -Target "frontend")
    if ($existing.Count -gt 0) {
        if (Wait-PortReady -Port $FrontendPort -HealthPath "" -TimeoutSec $StartTimeoutSec) {
            Write-Host "前端已在运行 (PID: $($existing[0].ProcessId))"
            return $true
        }
        Write-Host "前端进程未就绪，先清理后重启。"
        Stop-Target "前端" "frontend"
    }
    $listeners = @(Get-PortListenerPids -Port $FrontendPort)
    if ($listeners.Count -gt 0) {
        throw "前端端口 $FrontendPort 已被非 pixiv-tool 进程占用 (PID: $($listeners -join ','))"
    }
    Ensure-Dirs
    Write-Host "启动前端 (Vite, port $FrontendPort)..."
    # sh: cd "$REPO_ROOT/frontend" && pnpm dev > "$LOG_FILE" 2>&1 &
    $pnpmExe = Resolve-Command "pnpm"
    $proc = Start-Process -FilePath $pnpmExe -ArgumentList "dev" `
        -WorkingDirectory (Join-Path $RepoRoot "frontend") `
        -RedirectStandardOutput $FrontendLogFile `
        -RedirectStandardError (Join-Path $LogDir "frontend.err.log") `
        -NoNewWindow -PassThru
    $proc.Id | Out-File -FilePath $FrontendPidFile -Encoding ascii -Nonewline

    # 轮询端口(Vite 没健康检查端点,只用端口探测)
    if (Wait-PortReady -Port $FrontendPort -HealthPath "" -TimeoutSec $StartTimeoutSec) {
        Set-ManagedPidFile -PidFile $FrontendPidFile -Target 'frontend'
        Write-Host "  ✓ 前端就绪 (PID: $($proc.Id), port $FrontendPort)"
        return $true
    }
    Write-Host "  ✗ 前端在 ${StartTimeoutSec}s 内未就绪,见日志: $FrontendLogFile"
    Stop-Target "前端" "frontend"
    return $false
}

function Start-Backend {
    $existing = @(Find-PixivProcesses -Target "backend")
    if ($existing.Count -gt 0) {
        if (Wait-PortReady -Port $BackendPort -HealthPath "api/health" -TimeoutSec $StartTimeoutSec) {
            Write-Host "后端已在运行 (PID: $($existing[0].ProcessId))"
            return $true
        }
        Write-Host "后端进程未就绪，先清理后重启。"
        Stop-Target "后端" "backend"
    }
    $listeners = @(Get-PortListenerPids -Port $BackendPort)
    if ($listeners.Count -gt 0) {
        throw "后端端口 $BackendPort 已被非 pixiv-tool 进程占用 (PID: $($listeners -join ','))"
    }
    Ensure-Dirs
    Write-Host "启动后端 (uvicorn, port $BackendPort)..."
    # sh: cd "$REPO_ROOT" && uv run uvicorn pixiv_tool.main:app --host 127.0.0.1 --port 9962 --reload > "$LOG_FILE" 2>&1 &
    $uvExe = Resolve-Command "uv"
    $proc = Start-Process -FilePath $uvExe -ArgumentList "run", "uvicorn", "pixiv_tool.main:app", "--host", "127.0.0.1", "--port", "$BackendPort", "--reload" `
        -WorkingDirectory $RepoRoot `
        -RedirectStandardOutput $BackendLogFile `
        -RedirectStandardError (Join-Path $LogDir "backend.err.log") `
        -NoNewWindow -PassThru
    $proc.Id | Out-File -FilePath $BackendPidFile -Encoding ascii -NoNewline

    # 轮询 /api/health(确认不仅是端口开,而且 FastAPI 应用已 ready)
    if (Wait-PortReady -Port $BackendPort -HealthPath "api/health" -TimeoutSec $StartTimeoutSec) {
        Set-ManagedPidFile -PidFile $BackendPidFile -Target 'backend'
        Write-Host "  ✓ 后端就绪 (PID: $($proc.Id), port $BackendPort)"
        return $true
    }
    Write-Host "  ✗ 后端在 ${StartTimeoutSec}s 内未就绪,见日志: $BackendLogFile"
    Stop-Target "后端" "backend"
    return $false
}

function Invoke-Start {
    Ensure-Dirs
    $ok = $true
    switch ($Target) {
        "all"      {
            $fe = Start-Frontend
            if ($fe) { $be = Start-Backend; $ok = $be }
            else { $ok = $false }
            if (-not $ok) { Stop-Target "前端" "frontend"; Stop-Target "后端" "backend" }
        }
        "frontend" { $ok = Start-Frontend }
        "backend"  { $ok = Start-Backend }
    }
    Write-Host ""
    if ($ok) {
        Write-Host "开发服务已启动。前端: http://localhost:$FrontendPort  后端: http://localhost:$BackendPort"
    } else {
        throw "开发服务启动失败，已清理本次启动的进程；详见上面 ✗ 行和日志。"
    }
}

# -------------------------------------------------------------------
# restart = stop + start
# -------------------------------------------------------------------

function Invoke-Restart {
    # 严格 stop 然后 start,不在中间 sleep(stop 已等子进程结束)
    Invoke-Stop
    Invoke-Start
}

# -------------------------------------------------------------------
# status
# -------------------------------------------------------------------

function Invoke-Status {
    Ensure-Dirs
    Write-Host "=== pixiv-tool 开发服务状态 ==="

    # 前端
    $feProcs = @(Find-PixivProcesses -Target "frontend")
    $fePort = Test-PortOpen -Port $FrontendPort
    $feStatus = if ($feProcs.Count -gt 0 -and $fePort) { "运行中" }
                elseif ($feProcs.Count -gt 0) { "进程存在但端口未响应" }
                else { "未运行" }
    $fePids = if ($feProcs.Count -gt 0) { ($feProcs.ProcessId -join ',') } else { 'N/A' }
    Write-Host "前端: $feStatus (PID: $fePids, port: $FrontendPort)"

    # 后端
    $beProcs = @(Find-PixivProcesses -Target "backend")
    $bePort = Test-PortOpen -Port $BackendPort
    $beStatus = if ($beProcs.Count -gt 0 -and $bePort) { "运行中" }
                elseif ($beProcs.Count -gt 0) { "进程存在但端口未响应" }
                else { "未运行" }
    $bePids = if ($beProcs.Count -gt 0) { ($beProcs.ProcessId -join ',') } else { 'N/A' }
    Write-Host "后端: $beStatus (PID: $bePids, port: $BackendPort)"

    # 后端健康检查
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
    "start"   { Invoke-Start }
    "stop"    { Invoke-Stop }
    "restart" { Invoke-Restart }
    "status"  { Invoke-Status }
    "logs"    { Invoke-Logs }
}
