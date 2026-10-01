# Compatible with Windows PowerShell 5.1 and PowerShell 7.
# Keep this file ASCII so Windows PowerShell does not require a UTF-8 BOM.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = $PSScriptRoot
$cliArgs = @($args)

function Show-Help {
    @'
Usage: ./dev.ps1 <command>
       ./dev.ps1 -h | --help

  dev [start|stop|restart]       Desktop dev service (default: start)
  frontend [start|stop|restart]  Vite service on 9961 (default: start)
  build [release|debug]   Package installers; default: release
  install                 Install frontend dependencies (frozen lockfile)
  check                   Frontend type check/build + cargo check
  test                    Rust unit and integration tests (offline)
  test-live               Live pixiv API tests (needs a local login; serial)
  logs [app|COMMAND] [-f]  Last 100 lines; -f/--follow watches new lines
                          COMMAND: dev, frontend, build, install, check, test, test-live

No arguments show help. Extra/unknown arguments are errors (exit 2).
Commands run from the repository, regardless of your current directory.
Console logs: .dev/logs/<command>.log (overwritten on each invocation).
App logs: data/logs/app.log (debug/dev data only).
Windows: auto-configure MSVC, Visual Studio and LLVM. Set LIBCLANG_PATH
or VSINSTALLDIR to override discovery. Services run in the background.
Use stop to end services; logs COMMAND -f watches their output.
Examples: ./dev.ps1 dev start; ./dev.ps1 frontend restart; ./dev.ps1 dev stop
'@ | Write-Host
}

function Require-Command([string]$name) {
    if (-not (Get-Command $name -ErrorAction SilentlyContinue)) {
        throw "Missing command: $name. Install it and add it to PATH."
    }
}

function Initialize-Rust {
    Require-Command cargo
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { return }
    Require-Command rustup
    $env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-msvc'
    $toolchains = & rustup toolchain list
    if ($LASTEXITCODE -ne 0) { throw 'Cannot list Rust toolchains.' }
    if (-not ($toolchains -match '^stable-x86_64-pc-windows-msvc(?:\s|$)')) {
        throw 'Run: rustup toolchain install stable-x86_64-pc-windows-msvc'
    }
    if (-not $env:LIBCLANG_PATH) {
        $candidates = @(
            "$env:USERPROFILE/scoop/apps/llvm/current/bin",
            "$env:ProgramFiles/LLVM/bin"
        )
        $env:LIBCLANG_PATH = $candidates | Where-Object {
            Test-Path -LiteralPath (Join-Path $_ 'libclang.dll')
        } | Select-Object -First 1
    }
    if (-not $env:LIBCLANG_PATH -or -not (Test-Path -LiteralPath (Join-Path $env:LIBCLANG_PATH 'libclang.dll'))) {
        throw 'Set LIBCLANG_PATH to the LLVM bin directory containing libclang.dll.'
    }
    $env:CMAKE_GENERATOR = 'Visual Studio 17 2022'
    # vcvars expands PATH inside cmd.exe (8191-character command-line limit).
    # Keep inherited tool directories once, including an already initialized VS shell.
    foreach ($name in @('PATH', 'INCLUDE', 'LIB', 'LIBPATH', '__VSCMD_PREINIT_PATH')) {
        $value = [Environment]::GetEnvironmentVariable($name, 'Process')
        if ($value) {
            $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
            $entries = @($value.Split(';') | Where-Object { $seen.Add($_.TrimEnd([char[]]@('\', '/'))) })
            [Environment]::SetEnvironmentVariable($name, ($entries -join ';'), 'Process')
        }
    }
    Require-Command cmake
    if ($env:VSCMD_ARG_TGT_ARCH -eq 'x64' -and $env:VCToolsInstallDir -and $env:WindowsSdkDir -and $env:LIB -and $env:VSINSTALLDIR -and
        $env:VCToolsInstallDir.StartsWith($env:VSINSTALLDIR.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
        $tools = Join-Path $env:VCToolsInstallDir 'bin/Hostx64/x64'
        if ((Test-Path -LiteralPath (Join-Path $tools 'cl.exe')) -and (Test-Path -LiteralPath (Join-Path $tools 'link.exe'))) {
            return
        }
    }
    # Import the complete VS environment, not just link.exe's directory.
    if ($env:VSINSTALLDIR) {
        $vs = $env:VSINSTALLDIR
    } else {
        $vswhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
        if (-not (Test-Path -LiteralPath $vswhere)) { throw 'Install Visual Studio 2022 C++ Build Tools (vswhere missing).' }
        $vs = & $vswhere -latest -version '[17.0,18.0)' -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($LASTEXITCODE -ne 0 -or -not $vs) { throw 'Visual Studio 2022 C++ Build Tools not found.' }
    }
    $vcvars = Join-Path $vs 'VC/Auxiliary/Build/vcvars64.bat'
    if (-not (Test-Path -LiteralPath $vcvars)) { throw "Missing VS environment script: $vcvars" }
    $environment = & $env:ComSpec /d /c "call `"$vcvars`" >nul && set"
    if ($LASTEXITCODE -ne 0) { throw 'Cannot initialize the Visual Studio environment.' }
    foreach ($line in $environment) {
        if ($line -match '^([^=]+)=(.*)$') {
            [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process')
        }
    }
    Require-Command cmake
}

function Invoke-Logged([string]$executable, [string[]]$arguments, [string]$directory) {
    Write-Host "> $executable $($arguments -join ' ')"
    Push-Location -LiteralPath $directory
    $writer = [IO.StreamWriter]::new($script:logPath, $true, [Text.UTF8Encoding]::new($false))
    try {
        # PS 5.1 converts native stderr into ErrorRecords. Preserve the native
        # exit code instead of treating compiler progress as a terminating error.
        $ErrorActionPreference = 'Continue'
        & $executable @arguments 2>&1 | ForEach-Object {
            # Empty native stderr lines stringify as the exception type on 5.1.
            $text = if ($_ -is [Management.Automation.ErrorRecord]) { $_.Exception.Message } else { "$_" }
            $writer.WriteLine($text)
            $writer.Flush()
            Write-Host $text
        }
        $code = $LASTEXITCODE
    } finally {
        $writer.Dispose()
        Pop-Location
    }
    if ($code -ne 0) { exit $code }
}
function Get-ServiceRecord([string]$name) {
    $path = Join-Path $root ".dev/pids/$name.json"
    if (-not (Test-Path -LiteralPath $path)) { return $null }
    $record = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    if ("$($record.pid)" -notmatch '^[1-9][0-9]*$') { throw "Invalid service record: $path" }
    $process = Get-CimInstance Win32_Process -Filter "ProcessId=$($record.pid)"
    if (-not $process -or "$($process.CreationDate.ToUniversalTime().Ticks)" -ne "$($record.created)") {
        Remove-Item -LiteralPath $path
        return $null
    }
    $line = "$($process.CommandLine)".Replace('\', '/')
    if ($record.external) {
        if ($line -notmatch 'vite/bin/vite\.js') { throw "Refusing to stop changed process $($record.pid)." }
        $verified = Get-FrontendProcess
        if (-not $verified -or $verified.ProcessId -ne $record.pid) { throw 'Adopted frontend no longer matches this repository.' }
    } elseif ($line.IndexOf("$root/dev.ps1".Replace('\', '/'), [StringComparison]::OrdinalIgnoreCase) -lt 0 -or $line -notmatch "__run $name(?:\s|$)") {
        throw "Service record does not match its process: $path"
    }
    return $record
}

function Save-ServiceRecord([string]$name, $process, [bool]$external, [bool]$ownsFrontend = $false) {
    $record = @{
        pid = $process.ProcessId
        created = "$($process.CreationDate.ToUniversalTime().Ticks)"
        external = $external
        ownsFrontend = $ownsFrontend
    }
    [IO.File]::WriteAllText((Join-Path $root ".dev/pids/$name.json"), ($record | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
}

function Get-FrontendProcess {
    $listeners = @(Get-NetTCPConnection -LocalPort 9961 -State Listen -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess -Unique)
    if ($listeners.Count -eq 0) { return $null }
    if ($listeners.Count -ne 1) { throw 'Port 9961 has multiple owners; refusing to manage it.' }
    $process = Get-CimInstance Win32_Process -Filter "ProcessId=$($listeners[0])"
    if (-not $process -or "$($process.CommandLine)".Replace('\', '/') -notmatch 'vite/bin/vite\.js') {
        throw "Port 9961 belongs to another program (PID $($listeners[0])); refusing to stop it."
    }
    # Vite started through pnpm can have only relative paths in its command line.
    # Verify both its served source and access to this repository's package file.
    $response = Invoke-WebRequest 'http://localhost:9961/src/main.ts' -UseBasicParsing -TimeoutSec 3
    if ($response.Content -notmatch 'sourceMappingURL=data:application/json;base64,([A-Za-z0-9+/=]+)') {
        throw 'Port 9961 is not a verifiable repository Vite server.'
    }
    $map = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($matches[1])) | ConvertFrom-Json
    $sourceText = [IO.File]::ReadAllText((Join-Path $root 'frontend/src/main.ts')).Replace("`r`n", "`n").Trim()
    if (-not (@($map.sourcesContent) | Where-Object { $_.Replace("`r`n", "`n").Trim() -ceq $sourceText })) {
        throw 'Port 9961 serves another repository; refusing to manage it.'
    }
    $packagePath = "$root/frontend/package.json".Replace('\', '/')
    $package = Invoke-WebRequest ("http://localhost:9961/@fs/" + [Uri]::EscapeUriString($packagePath)) -UseBasicParsing -TimeoutSec 3
    if (($package.Content | ConvertFrom-Json).name -ne 'pixiv-tool-frontend') {
        throw 'Port 9961 cannot serve this repository package; refusing to manage it.'
    }
    return $process
}

function Stop-ServiceTree([string]$name) {
    $record = Get-ServiceRecord $name
    if (-not $record -and $name -eq 'frontend') {
        $vite = Get-FrontendProcess
        if ($vite) { Save-ServiceRecord 'frontend' $vite $true; $record = Get-ServiceRecord 'frontend' }
    }
    if (-not $record) { Write-Host "$name is not running."; return }
    # Re-check identity immediately before terminating the entire owned tree.
    $record = Get-ServiceRecord $name
    if (-not $record) { return }
    & taskkill.exe /PID $record.pid /T /F | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Cannot stop $name (PID $($record.pid))." }
    Remove-Item -LiteralPath (Join-Path $root ".dev/pids/$name.json")
    if ($name -eq 'dev' -and $record.ownsFrontend) { Stop-ServiceTree 'frontend' }
    Remove-Item -LiteralPath (Join-Path $root ".dev/pids/$name.ready") -ErrorAction SilentlyContinue
    if ($name -eq 'frontend') {
        $deadline = (Get-Date).AddSeconds(10)
        while (Get-NetTCPConnection -LocalPort 9961 -State Listen -ErrorAction SilentlyContinue) {
            if ((Get-Date) -ge $deadline) { throw 'Port 9961 did not become free after stopping frontend.' }
            Start-Sleep -Milliseconds 200
        }
    }
    Write-Host "Stopped $name (PID $($record.pid))."
}

function Start-ServiceWorker([string]$name, [bool]$ownsFrontend = $false) {
    $executable = (Get-Process -Id $PID).Path
    $readyPath = Join-Path $root ".dev/pids/$name.ready"
    Remove-Item -LiteralPath $readyPath -ErrorAction SilentlyContinue
    $worker = Start-Process -FilePath $executable -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$root/dev.ps1`"", '__run', $name) -WorkingDirectory $root -WindowStyle Hidden -RedirectStandardOutput (Join-Path $root ".dev/logs/$name.stdout.log") -RedirectStandardError (Join-Path $root ".dev/logs/$name.stderr.log") -PassThru
    $process = Get-CimInstance Win32_Process -Filter "ProcessId=$($worker.Id)"
    if (-not $process) { throw "$name worker exited during startup; see .dev/logs/$name.stderr.log." }
    Save-ServiceRecord $name $process $false $ownsFrontend
    $deadline = (Get-Date).AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 300
        if ($worker.HasExited) {
            Remove-Item -LiteralPath (Join-Path $root ".dev/pids/$name.json") -ErrorAction SilentlyContinue
            $detail = [string](Get-Content -LiteralPath (Join-Path $root ".dev/logs/$name.stderr.log") -Raw)
            throw "$name exited during startup: $($detail.Trim()). See .dev/logs/$name.log."
        }
        if ($name -eq 'dev') {
            if ((Test-Path -LiteralPath $readyPath) -and (Get-Content -LiteralPath $readyPath -Raw) -eq "$($worker.Id)") { break }
        } elseif ((Get-NetTCPConnection -LocalPort 9961 -State Listen -ErrorAction SilentlyContinue) -and (Get-FrontendProcess)) { break }
        if ((Get-Date) -ge $deadline) { Stop-ServiceTree $name; throw "$name initialization timed out; see .dev/logs/$name.log." }
    } while ($true)
    Write-Host "Started $name (PID $($worker.Id)). Log: .dev/logs/$name.log"
    if ($name -eq 'dev') { Write-Host 'Toolchain initialized; the Tauri window opens after Rust builds. Watch: ./dev.ps1 logs dev -f' }
}

function Start-ManagedService([string]$name) {
    $existing = Get-ServiceRecord $name
    if ($existing) { Write-Host "$name is already running (PID $($existing.pid))."; return }
    $vite = Get-FrontendProcess
    if ($name -eq 'frontend' -and $vite) {
        Save-ServiceRecord 'frontend' $vite $true
        Write-Host "frontend is already running (adopted repository Vite PID $($vite.ProcessId))."
        return
    }
    Require-Command pnpm
    if ($name -eq 'dev') {
        # Initialize VS only inside the isolated worker, never in the caller's shell.
        Require-Command cargo
        Require-Command rustup
        Require-Command cmake
        if (-not (Test-Path -LiteralPath (Join-Path $root 'frontend/node_modules/.bin/tauri.cmd'))) { throw 'Local Tauri CLI missing. Run ./dev.ps1 install first.' }
        $frontendRecord = Get-ServiceRecord 'frontend'
        $ownsFrontend = -not $vite -and -not $frontendRecord
        Start-ManagedService 'frontend'
        try { Start-ServiceWorker 'dev' $ownsFrontend }
        catch { if ($ownsFrontend) { Stop-ServiceTree 'frontend' }; throw }
    } else { Start-ServiceWorker 'frontend' }
}

function Manage-Service([string]$name, [string]$action) {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Use dev.sh for service management on macOS/Linux.' }
    New-Item -ItemType Directory -Force -Path (Join-Path $root '.dev/pids'), (Join-Path $root '.dev/logs') | Out-Null
    $hash = [Security.Cryptography.SHA256]::Create()
    try { $key = [BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($root.ToLowerInvariant()))).Replace('-', '') }
    finally { $hash.Dispose() }
    $mutex = [Threading.Mutex]::new($false, "Local\pixiv-dev-$key")
    $locked = $false
    try {
        try { $locked = $mutex.WaitOne(30000) } catch [Threading.AbandonedMutexException] { $locked = $true }
        if (-not $locked) { throw 'Another service operation is still running.' }
        if ($action -in @('stop', 'restart')) { Stop-ServiceTree $name }
        if ($action -in @('start', 'restart')) { Start-ManagedService $name }
    } finally {
        if ($locked) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
    }
}

$workerMode = $cliArgs.Count -eq 2 -and $cliArgs[0] -ceq '__run' -and $cliArgs[1] -cin @('dev', 'frontend')
if ($workerMode) { $cliArgs = @($cliArgs[1]) }

if ($cliArgs.Count -eq 0 -or ($cliArgs.Count -eq 1 -and $cliArgs[0] -cin @('-h', '--help'))) {
    Show-Help
    exit 0
}
$command = $cliArgs[0]
$valid = @('dev', 'frontend', 'build', 'install', 'check', 'test', 'test-live', 'logs')
$invalid = $command -cnotin $valid
$mode = 'release'
$source = 'app'
$follow = $false
$action = 'start'
if ($command -cin @('dev', 'frontend')) {
    if ($cliArgs.Count -gt 2) { $invalid = $true }
    if ($cliArgs.Count -eq 2) { $action = $cliArgs[1] }
    if ($action -cnotin @('start', 'stop', 'restart')) { $invalid = $true }
} elseif ($command -ceq 'build') {
    if ($cliArgs.Count -gt 2) { $invalid = $true }
    if ($cliArgs.Count -eq 2) { $mode = $cliArgs[1] }
    if ($mode -cnotin @('release', 'debug')) { $invalid = $true }
} elseif ($command -ceq 'logs') {
    $rest = @($cliArgs | Select-Object -Skip 1)
    if ($rest.Count -gt 0 -and $rest[-1] -cin @('-f', '--follow')) {
        $follow = $true
        $rest = @($rest | Select-Object -First ($rest.Count - 1))
    }
    if ($rest.Count -gt 1) { $invalid = $true }
    if ($rest.Count -eq 1) { $source = $rest[0] }
    if ($source -cnotin @('app', 'dev', 'frontend', 'build', 'install', 'check', 'test', 'test-live')) { $invalid = $true }
} elseif ($cliArgs.Count -ne 1) {
    $invalid = $true
}
if ($invalid) {
    [Console]::Error.WriteLine('Invalid command or arguments. Run ./dev.ps1 -h.')
    exit 2
}

try {
    if ($command -cin @('dev', 'frontend') -and -not $workerMode) {
        Manage-Service $command $action
        exit 0
    }
    if ($command -ceq 'logs') {
        $path = if ($source -ceq 'app') { Join-Path $root 'data/logs/app.log' } else { Join-Path $root ".dev/logs/$source.log" }
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Log not found: $path. Run the corresponding command first." }
        Write-Host "Log: $path"
        if ($follow) { Get-Content -LiteralPath $path -Encoding UTF8 -Tail 100 -Wait }
        else { Get-Content -LiteralPath $path -Encoding UTF8 -Tail 100 }
        exit 0
    }
    $logDir = Join-Path $root '.dev/logs'
    New-Item -ItemType Directory -Force -Path $logDir | Out-Null
    $script:logPath = Join-Path $logDir "$command.log"
    # Explicit UTF-8, including on Windows PowerShell 5.1.
    [IO.File]::WriteAllText($script:logPath, '', [Text.UTF8Encoding]::new($false))
    Write-Host "Log: $script:logPath"
    $frontend = Join-Path $root 'frontend'
    $backend = Join-Path $root 'src-tauri'
    if ($command -cin @('dev', 'frontend', 'build', 'install', 'check')) { Require-Command pnpm }
    if ($command -cin @('dev', 'build', 'check', 'test', 'test-live')) { Initialize-Rust }
    $tauri = Join-Path $frontend 'node_modules/.bin/tauri'
    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) { $tauri += '.cmd' }
    if ($command -cin @('dev', 'build') -and -not (Test-Path -LiteralPath $tauri)) { throw 'Local Tauri CLI missing. Run ./dev.ps1 install first.' }
    switch -CaseSensitive ($command) {
        'dev' {
            # A file avoids cmd.exe stripping quotes from inline JSON on PS 5.1.
            $devConfig = Join-Path $root '.dev/tauri-dev.json'
            [IO.File]::WriteAllText($devConfig, '{"build":{"beforeDevCommand":""}}', [Text.UTF8Encoding]::new($false))
            [IO.File]::WriteAllText((Join-Path $root '.dev/pids/dev.ready'), "$PID")
            Invoke-Logged $tauri @('dev', '--config', $devConfig) $root
        }
        'frontend' { Invoke-Logged 'pnpm' @('dev') $frontend }
        'build' {
            $buildArgs = @('build')
            if ($mode -ceq 'debug') { $buildArgs += '--debug' }
            Invoke-Logged $tauri $buildArgs $root
        }
        'install' { Invoke-Logged 'pnpm' @('install', '--frozen-lockfile') $frontend }
        'check' {
            Invoke-Logged 'pnpm' @('build') $frontend
            Invoke-Logged 'cargo' @('check', '--locked') $backend
        }
        'test' { Invoke-Logged 'cargo' @('test', '--locked') $backend }
        # Live pixiv API tests: slow + network + real login state, so opt-in.
        # Serial (test-threads=1) keeps the shared session under pixiv rate limits.
        'test-live' { Invoke-Logged 'cargo' @('test', '--locked', '--test', 'pixiv_api', '--', '--ignored', '--test-threads=1') $backend }
    }
    exit 0
} catch {
    if ($workerMode -and (Get-Variable logPath -Scope Script -ErrorAction SilentlyContinue)) {
        [IO.File]::AppendAllText($script:logPath, ($_.Exception.Message + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))
    }
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
