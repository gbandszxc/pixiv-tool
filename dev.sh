#!/usr/bin/env bash
# Bash 3.2+ (including macOS); Windows Git Bash delegates toolchain setup to PS.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
original_args=("$@")


help() {
    cat <<'HELP'
Usage: bash dev.sh <command>
       bash dev.sh -h | --help

  dev [start|stop|restart]       Desktop dev service (default: start)
  frontend [start|stop|restart]  Vite service on 9961 (default: start)
  build [release|debug]   Package installers; default: release
  install                 Install frontend dependencies (frozen lockfile)
  check                   Frontend type check/build + cargo check
  test                    Rust unit and integration tests
  logs [app|COMMAND] [-f]  Last 100 lines; -f/--follow watches new lines
                          COMMAND: dev, frontend, build, install, check, test

No arguments show help. Extra/unknown arguments are errors (exit 2).
Commands run from the repository, regardless of your current directory.
Console logs: .dev/logs/<command>.log (overwritten on each invocation).
App logs: data/logs/app.log (debug/dev data only).
macOS/Linux: install the platform prerequisites described in docs/PACKAGING.md.
Windows Git Bash: delegates to dev.ps1 for MSVC/Visual Studio/LLVM setup.
Services run in the background; use stop to end them, logs COMMAND -f to watch.
Examples: bash dev.sh dev start; bash dev.sh frontend restart; bash dev.sh dev stop
HELP
}
fail() {
    if [[ ${worker_mode:-false} == true && -n ${log_path:-} ]]; then printf '%s\n' "$*" >> "$log_path"; fi
    printf '%s\n' "$*" >&2
    exit 1
}
invalid() { printf '%s\n' 'Invalid command or arguments. Run bash dev.sh -h.' >&2; exit 2; }
require() { command -v "$1" >/dev/null 2>&1 || fail "Missing command: $1. Install it and add it to PATH."; }
worker_mode=false
if [[ $# -eq 2 && $1 == __run && ( $2 == dev || $2 == frontend ) ]]; then
    worker_mode=true
    set -- "$2"
fi
if [[ $# -eq 0 ]]; then help; exit 0; fi
if [[ $# -eq 1 && ( $1 == -h || $1 == --help ) ]]; then help; exit 0; fi
command_name=$1
shift
mode=release
source_name=app
follow=false
action=start
case "$command_name" in
    build)
        [[ $# -le 1 ]] || invalid
        mode=${1:-release}
        [[ $mode == release || $mode == debug ]] || invalid
        # An explicitly empty argument is invalid, not the default.
        [[ $# -eq 0 || -n $1 ]] || invalid
        ;;
    logs)
        if [[ $# -gt 0 ]]; then
            last=${!#}
            if [[ $last == -f || $last == --follow ]]; then
                follow=true
                set -- "${@:1:$#-1}"
            fi
        fi
        [[ $# -le 1 ]] || invalid
        source_name=${1:-app}
        [[ $# -eq 0 || -n $1 ]] || invalid
        case "$source_name" in app|dev|frontend|build|install|check|test) ;; *) invalid ;; esac
        ;;
    dev|frontend)
        [[ $# -le 1 ]] || invalid
        action=${1:-start}
        [[ $# -eq 0 || -n $1 ]] || invalid
        case "$action" in start|stop|restart) ;; *) invalid ;; esac
        ;;
    install|check|test) [[ $# -eq 0 ]] || invalid ;;
    *) invalid ;;
esac
# Validate before crossing the Windows native argument boundary: PS 5.1 can
# discard empty arguments, which must never turn an invalid build into release.
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
        exec powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w "$root/dev.ps1")" "${original_args[@]}"
        ;;
esac
if [[ $command_name == logs ]]; then
    if [[ $source_name == app ]]; then path="$root/data/logs/app.log"
    else path="$root/.dev/logs/$source_name.log"; fi
    [[ -f $path ]] || fail "Log not found: $path. Run the corresponding command first."
    printf 'Log: %s\n' "$path"
    if $follow; then exec tail -n 100 -f "$path"; else exec tail -n 100 "$path"; fi
fi
process_stamp() { ps -p "$1" -o lstart=; }
record_path() { printf '%s/.dev/pids/%s.pid' "$root" "$1"; }
read_record() {
    service_pid= service_stamp= service_external= owns_frontend=
    local path line
    path=$(record_path "$1")
    [[ -f $path ]] || return 1
    { IFS= read -r service_pid; IFS= read -r service_stamp; IFS= read -r service_external; IFS= read -r owns_frontend; } < "$path"
    [[ $service_pid =~ ^[0-9]+$ && $service_pid -gt 1 ]] || fail "Invalid service record: $path"
    if [[ $(process_stamp "$service_pid" 2>/dev/null || true) != "$service_stamp" ]]; then
        rm -f -- "$path"
        service_pid=
        return 1
    fi
    line=$(ps -p "$service_pid" -o command=)
    if [[ $service_external == true ]]; then
        [[ $line == *vite/bin/vite.js* || $line == *node_modules/.bin/vite* ]] || fail "Refusing to stop changed process $service_pid."
        frontend_process
        [[ $frontend_pid == "$service_pid" ]] || fail 'Adopted frontend no longer matches this repository.'
    else
        [[ $line == *"$root/dev.sh __run $1"* ]] || fail "Service record does not match its process: $path"
    fi
}
save_record() {
    printf '%s\n%s\n%s\n%s\n' "$2" "$(process_stamp "$2")" "$3" "${4:-false}" > "$(record_path "$1")"
}
frontend_process() {
    local ids id line directory entry
    frontend_pid=
    ids=$(lsof -nP -iTCP:9961 -sTCP:LISTEN -t 2>/dev/null || true)
    for id in $ids; do
        if [[ -n $frontend_pid && $frontend_pid != "$id" ]]; then fail 'Port 9961 has multiple owners.'; fi
        frontend_pid=$id
    done
    [[ -n $frontend_pid ]] || return 0
    line=$(ps -p "$frontend_pid" -o command=)
    directory=
    while IFS= read -r entry; do
        [[ $entry == n* ]] && directory=${entry#n}
    done < <(lsof -a -p "$frontend_pid" -d cwd -Fn 2>/dev/null)
    [[ ( $line == *vite/bin/vite.js* || $line == *node_modules/.bin/vite* ) && $directory == "$root/frontend" ]] ||
        fail "Port 9961 belongs to another program/repository (PID $frontend_pid); refusing to manage it."
}
# Freeze each parent before taking its child snapshot, so no new children escape.
# Force termination matches Windows taskkill /T /F; never kill a whole user session.
kill_tree() {
    local target=$1 child parent snapshot stamp
    stamp=$(process_stamp "$target" 2>/dev/null || true)
    [[ -n $stamp ]] || return 0
    kill -STOP "$target" 2>/dev/null || return 0
    snapshot=$(ps -ax -o pid= -o ppid=)
    while read -r child parent; do
        if [[ $parent == "$target" ]]; then kill_tree "$child"; fi
    done <<< "$snapshot"
    if [[ $(process_stamp "$target" 2>/dev/null || true) == "$stamp" ]]; then
        kill -KILL "$target" 2>/dev/null || true
    fi
}
stop_service() {
    local name=$1 target owned
    if ! read_record "$name"; then
        if [[ $name == frontend ]]; then
            frontend_process
            if [[ -n $frontend_pid ]]; then save_record frontend "$frontend_pid" true; read_record frontend; fi
        fi
    fi
    if [[ -z $service_pid ]]; then printf '%s is not running.\n' "$name"; return; fi
    target=$service_pid
    owned=$owns_frontend
    read_record "$name" || return 0
    kill_tree "$target"
    rm -f -- "$(record_path "$name")"
    rm -f -- "$root/.dev/pids/$name.ready"
    if [[ $name == dev && $owned == true ]]; then stop_service frontend; fi
    if [[ $name == frontend ]]; then
        local attempt
        for attempt in {1..50}; do
            [[ -z $(lsof -nP -iTCP:9961 -sTCP:LISTEN -t 2>/dev/null || true) ]] && break
            sleep 0.2
        done
        [[ -z $(lsof -nP -iTCP:9961 -sTCP:LISTEN -t 2>/dev/null || true) ]] || fail 'Port 9961 did not become free.'
    fi
    printf 'Stopped %s (PID %s).\n' "$name" "$target"
}
start_worker() {
    local name=$1 owned=${2:-false} target attempt
    rm -f -- "$root/.dev/pids/$name.ready"
    nohup bash "$root/dev.sh" __run "$name" > "$root/.dev/logs/$name.stdout.log" 2> "$root/.dev/logs/$name.stderr.log" < /dev/null &
    target=$!
    save_record "$name" "$target" false "$owned"
    for attempt in {1..100}; do
        sleep 0.2
        if ! kill -0 "$target" 2>/dev/null; then
            rm -f -- "$(record_path "$name")"
            if [[ $name == dev && $owned == true ]]; then stop_service frontend; fi
            fail "$name exited during startup; see .dev/logs/$name.log and $name.stderr.log."
        fi
        if [[ $name == dev ]]; then
            if [[ -f $root/.dev/pids/dev.ready && $(< "$root/.dev/pids/dev.ready") == "$target" ]]; then break; fi
        else
            frontend_process
            [[ -n $frontend_pid ]] && break
        fi
    done
    if [[ ( $name == frontend && -z $frontend_pid ) || ( $name == dev && ! -f $root/.dev/pids/dev.ready ) ]]; then
        stop_service "$name"
        fail "$name initialization timed out; see .dev/logs/$name.log."
    fi
    printf 'Started %s (PID %s). Log: .dev/logs/%s.log\n' "$name" "$target" "$name"
    if [[ $name == dev ]]; then printf '%s\n' 'Toolchain initialized; the Tauri window opens after Rust builds. Watch: bash dev.sh logs dev -f'; fi
}
start_service() {
    local name=$1 owned=false
    if read_record "$name"; then printf '%s is already running (PID %s).\n' "$name" "$service_pid"; return; fi
    frontend_process
    if [[ $name == frontend && -n $frontend_pid ]]; then
        save_record frontend "$frontend_pid" true
        printf 'frontend is already running (adopted repository Vite PID %s).\n' "$frontend_pid"
        return
    fi
    require pnpm
    if [[ $name == dev ]]; then
        require cargo
        require cmake
        [[ -x $root/frontend/node_modules/.bin/tauri ]] || fail 'Local Tauri CLI missing. Run bash dev.sh install first.'
        if [[ -z $frontend_pid ]] && ! read_record frontend; then owned=true; fi
        start_service frontend
        start_worker dev "$owned"
    else start_worker frontend; fi
}
if [[ ( $command_name == dev || $command_name == frontend ) && $worker_mode == false ]]; then
    require lsof
    mkdir -p "$root/.dev/pids" "$root/.dev/logs"
    # Atomic mkdir serializes starts/stops across both services.
    lock="$root/.dev/pids/operation.lock"
    if ! mkdir "$lock" 2>/dev/null; then fail 'Another service operation is running; check .dev/pids/operation.lock if it was interrupted.'; fi
    trap 'rmdir "$lock"' EXIT
    case "$action" in stop|restart) stop_service "$command_name" ;; esac
    case "$action" in start|restart) start_service "$command_name" ;; esac
    exit 0
fi
mkdir -p "$root/.dev/logs"
log_path="$root/.dev/logs/$command_name.log"
: > "$log_path"
printf 'Log: %s\n' "$log_path"
case "$command_name" in dev|frontend|build|install|check) require pnpm ;; esac
case "$command_name" in dev|build|check|test) require cargo; require cmake ;; esac
tauri="$root/frontend/node_modules/.bin/tauri"
case "$command_name" in
    dev|build) [[ -x $tauri ]] || fail 'Local Tauri CLI missing. Run bash dev.sh install first.' ;;
esac
run() {
    local directory=$1
    shift
    printf '> %s\n' "$*"
    # pipefail preserves a compiler/package-manager failure despite tee succeeding.
    (cd -- "$directory" && "$@") 2>&1 | tee -a "$log_path"
}
if [[ $worker_mode == true && $command_name == dev ]]; then printf '%s' "$$" > "$root/.dev/pids/dev.ready"; fi
case "$command_name" in
    dev) run "$root" "$tauri" dev --config '{"build":{"beforeDevCommand":""}}' ;;
    frontend) run "$root/frontend" pnpm dev ;;
    build)
        if [[ $mode == debug ]]; then run "$root" "$tauri" build --debug
        else run "$root" "$tauri" build; fi
        ;;
    install) run "$root/frontend" pnpm install --frozen-lockfile ;;
    check)
        run "$root/frontend" pnpm build
        run "$root/src-tauri" cargo check --locked
        ;;
    test) run "$root/src-tauri" cargo test --locked ;;
esac
