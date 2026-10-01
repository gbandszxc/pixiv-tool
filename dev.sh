#!/usr/bin/env bash
# Bash 3.2+ (including macOS); Windows Git Bash delegates toolchain setup to PS.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
original_args=("$@")


help() {
    cat <<'HELP'
Usage: bash dev.sh <command>
       bash dev.sh -h | --help

  dev                     Tauri dev: Vite + Rust hot reload + desktop window
  frontend                Vite only, http://localhost:9961 (no Tauri IPC)
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
Ctrl+C stops foreground commands.
Examples: bash dev.sh dev; bash dev.sh build debug; bash dev.sh logs dev -f
HELP
}
fail() { printf '%s\n' "$*" >&2; exit 1; }
invalid() { printf '%s\n' 'Invalid command or arguments. Run bash dev.sh -h.' >&2; exit 2; }
require() { command -v "$1" >/dev/null 2>&1 || fail "Missing command: $1. Install it and add it to PATH."; }
if [[ $# -eq 0 ]]; then help; exit 0; fi
if [[ $# -eq 1 && ( $1 == -h || $1 == --help ) ]]; then help; exit 0; fi
command_name=$1
shift
mode=release
source_name=app
follow=false
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
    dev|frontend|install|check|test) [[ $# -eq 0 ]] || invalid ;;
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
case "$command_name" in
    dev) run "$root" "$tauri" dev ;;
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
