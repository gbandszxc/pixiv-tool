<#
.SYNOPSIS
    pixiv-tool 开发服务管理脚本(Windows PowerShell)。
.DESCRIPTION
    支持 start/stop/restart/status/logs 子命令,管理前后端 dev 进程。
    设计原则:
      - stop = 强杀。扫所有命令行含本仓库路径的 python.exe/node.exe 进程,
        taskkill /F /T 连子进程树一起杀。不依赖 PID 文件、不依赖端口反查。
      - start = 启动进程并输出访问地址后立即返回；就绪状态由 status 查询。
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

# 依赖就绪门控信号:前端看 node_modules,后端看 .venv(首次拉代码两者都不存在)。
# 注意:Windows PowerShell 5.1 的 Join-Path 只吃两个参数(无 -AdditionalChildPath),
# 多段路径必须嵌套调用。
$VenvDir = Join-Path $RepoRoot ".venv"
$FrontendNodeModules = Join-Path (Join-Path $RepoRoot "frontend") "node_modules"

$FrontendPort = 9961
$BackendPort = 9962
$FrontendPidFile = Join-Path $PidDir "frontend.pid"
$BackendPidFile = Join-Path $PidDir "backend.pid"
$FrontendLogFile = Join-Path $LogDir "frontend.log"
$BackendLogFile = Join-Path $LogDir "backend.log"

# 已发出强杀后，等待 Windows 回收进程树的最长秒数。
# 仅 stop 使用；start 不等待服务就绪。
$StopVerificationTimeoutSec = 2

# -------------------------------------------------------------------
# 工具函数
# -------------------------------------------------------------------

function Ensure-Dirs {
    # sh: mkdir -p "$DEV_DIR" "$PID_DIR" "$LOG_DIR"
    @($DevDir, $PidDir, $LogDir) | ForEach-Object {
        if (-not (Test-Path $_)) { New-Item -ItemType Directory -Path $_ -Force | Out-Null }
    }
}

# -------------------------------------------------------------------
# 依赖自动安装(仅 start 触发,已就绪则跳过)
# -------------------------------------------------------------------

# sh: command -v "$name" >/dev/null 2>&1
function Test-CommandAvailable([string]$Name) {
    return $null -ne (Get-Command $Name -ErrorAction SilentlyContinue)
}

# 装前端依赖:pnpm 缺失报错+链接;node_modules 已存在跳过;否则 pnpm install。
function Sync-FrontendDeps {
    if (-not (Test-CommandAvailable "pnpm")) {
        throw "未找到 pnpm,请先安装: https://pnpm.io/installation (可用 `npm i -g pnpm`)"
    }
    if (Test-Path $FrontendNodeModules) {
        Write-Host "前端依赖已就绪,跳过安装。"
        return
    }
    Write-Host "首次运行:安装前端依赖 (pnpm install)..."
    $pnpmExe = Resolve-Command "pnpm"
    # -Wait 同步等待安装完成;失败时 ExitCode 非 0。
    $proc = Start-Process -FilePath $pnpmExe -ArgumentList "install" `
        -WorkingDirectory (Join-Path $RepoRoot "frontend") -NoNewWindow -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        throw "前端依赖安装失败 (pnpm install 退出码 $($proc.ExitCode))"
    }
    Write-Host "  ✓ 前端依赖安装完成"
}

# 装后端依赖:uv 缺失报错+链接;.venv 已存在跳过;否则 uv sync --extra win。
# dev 模式不需要 pyinstaller(那是打包工具),所以不带 --extra dev。
# 平台 extra 固定 win(本脚本只在 Windows 跑);mac/linux 用 dev.sh。
function Sync-BackendDeps {
    if (-not (Test-CommandAvailable "uv")) {
        throw "未找到 uv,请先安装: https://docs.astral.sh/uv/getting-started/installation/"
    }
    if (Test-Path $VenvDir) {
        Write-Host "后端依赖已就绪,跳过安装。"
        return
    }
    Write-Host "首次运行:安装后端依赖 (uv sync --extra win)..."
    $uvExe = Resolve-Command "uv"
    $proc = Start-Process -FilePath $uvExe -ArgumentList "sync", "--extra", "win" `
        -WorkingDirectory $RepoRoot -NoNewWindow -Wait -PassThru
    if ($proc.ExitCode -ne 0) {
        throw "后端依赖安装失败 (uv sync 退出码 $($proc.ExitCode))"
    }
    Write-Host "  ✓ 后端依赖安装完成"
}

# 按 target 分发:all 装两边,frontend/backend 只装对应一边。
function Ensure-Dependencies([string]$Target) {
    switch ($Target) {
        "all"      { Sync-FrontendDeps; Sync-BackendDeps }
        "frontend" { Sync-FrontendDeps }
        "backend"  { Sync-BackendDeps }
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
        # 让 WMI 先过滤可管理的可执行文件，避免每次命令都枚举全系统进程。
        # 仓库路径和 vite/uvicorn 的二次匹配仍在下面完成，不会影响其它服务。
        $procs = Get-CimInstance Win32_Process -Filter "Name = 'node.exe' OR Name = 'python.exe' OR Name = 'uvicorn.exe'" -ErrorAction SilentlyContinue
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

function Test-PortListening([int]$Port) {
    # Get-NetTCPConnection 在部分 Windows 机器上查询空闲端口也需要数秒。
    # start 的常规路径只需知道端口是否空闲，.NET 监听表足够且不需要启动外部命令。
    try {
        $listeners = [System.Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners()
        return @($listeners | Where-Object { $_.Port -eq $Port }).Count -gt 0
    } catch {
        # 极少数平台 API 失败时保守回退，仍保持原有安全行为。
        return @(Get-PortListenerPids -Port $Port).Count -gt 0
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

function Get-ProcessTreeRoots([object[]]$Processes) {
    # 同一棵树只需 taskkill /T 一次；没有受管父进程的项可能是服务根或孤儿进程。
    $managedIds = @{}
    foreach ($process in $Processes) {
        $managedIds[[int]$process.ProcessId] = $true
    }
    return @($Processes | Where-Object { -not $managedIds.ContainsKey([int]$_.ParentProcessId) })
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
        foreach ($p in @(Get-ProcessTreeRoots -Processes $procs)) {
            Write-Host "$TargetName : 杀 PID $($p.ProcessId) ($($p.Name)) + 子进程树"
            & taskkill.exe /F /T /PID $p.ProcessId 2>&1 | Out-Null
        }
        # 只在已经杀过进程时确认残留，避免“无进程的 stop”固定等待。
        # --reload 的 worker 由 taskkill /T 一并终止；若 Windows 尚未回收，短暂重试补杀。
        $deadline = (Get-Date).AddSeconds($StopVerificationTimeoutSec)
        $leftover = @(Find-PixivProcesses -Target $Target)
        while ($leftover.Count -gt 0 -and (Get-Date) -lt $deadline) {
            $leftover = @(Find-PixivProcesses -Target $Target)
            foreach ($p in @(Get-ProcessTreeRoots -Processes $leftover)) {
                Write-Host "$TargetName : 补杀 PID $($p.ProcessId) ($($p.Name))"
                & taskkill.exe /F /T /PID $p.ProcessId 2>&1 | Out-Null
            }
            if ($leftover.Count -gt 0) { Start-Sleep -Milliseconds 100 }
            $leftover = @(Find-PixivProcesses -Target $Target)
        }

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
    if (Test-PortListening -Port $FrontendPort) {
        # 正常启动时端口空闲，不做耗时的命令行扫描。只有端口已占用时才确认归属。
        $listeners = @(Get-PortListenerPids -Port $FrontendPort)
        $existing = @(Find-PixivProcesses -Target "frontend")
        if (@($existing | Where-Object { $listeners -contains $_.ProcessId }).Count -gt 0) {
            Write-Host "前端已启动 (PID: $($existing[0].ProcessId))"
            return $true
        }
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

    Write-Host "  ✓ 前端已启动 (PID: $($proc.Id), port $FrontendPort)"
    return $true
}

function Start-Backend {
    if (Test-PortListening -Port $BackendPort) {
        # 正常启动时端口空闲，不做耗时的命令行扫描。只有端口已占用时才确认归属。
        $listeners = @(Get-PortListenerPids -Port $BackendPort)
        $existing = @(Find-PixivProcesses -Target "backend")
        if (@($existing | Where-Object { $listeners -contains $_.ProcessId }).Count -gt 0) {
            Write-Host "后端已启动 (PID: $($existing[0].ProcessId))"
            return $true
        }
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

    Write-Host "  ✓ 后端已启动 (PID: $($proc.Id), port $BackendPort)"
    return $true
}

function Invoke-Start {
    Ensure-Dirs
    # start 前确保依赖就绪(已装则秒跳过);缺失工具或安装失败会抛错终止。
    Ensure-Dependencies $Target
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
