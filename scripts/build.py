"""
pixiv-tool 一键构建脚本。

步骤：
  1. 构建前端 (pnpm install + pnpm build)
  2. 复制 frontend/dist/ → src/pixiv_tool/static/
  3. PyInstaller 打包 (onedir 模式)
  4. 输出产物路径

用法：
  python scripts/build.py              # 完整构建
  python scripts/build.py --skip-fe    # 跳过前端构建（适合后端改动时）
"""

from __future__ import annotations

import argparse
import platform
import shutil
import subprocess
import sys
from pathlib import Path

# -------------------------------------------------------------------
# 路径常量
# -------------------------------------------------------------------

REPO_ROOT = Path(__file__).resolve().parent.parent
FRONTEND_DIR = REPO_ROOT / "frontend"
BACKEND_DIR = REPO_ROOT / "src" / "pixiv_tool"
STATIC_DIR = BACKEND_DIR / "static"
DIST_DIR = FRONTEND_DIR / "dist"
SPEC_FILE = REPO_ROOT / "pixiv-tool.spec"
DIST_OUTPUT = REPO_ROOT / "dist" / "pixiv-tool"
VENV_DIR = REPO_ROOT / ".venv"

# uv 安装链接(缺失时打印,避免原始报错糊脸)。
UV_INSTALL_URL = "https://docs.astral.sh/uv/getting-started/installation/"


# -------------------------------------------------------------------
# 环境准备
# -------------------------------------------------------------------


def platform_extra() -> str:
    """当前平台对应的 pyproject.toml extra 名(windows->win 等)。

    pywebview 在 win/macos/linux 三个 extra 里内容相同(都是 pywebview>=5.0,<6.0),
    extra 名仅用于按平台语义标记;这里取当前平台那个。
    """
    system = platform.system()
    if system == "Windows":
        return "win"
    if system == "Darwin":
        return "macos"
    # Linux 及其余一律按 linux extra。
    return "linux"


def venv_python() -> str:
    """返回用于打包的 venv python 路径。

    优先用仓库内 .venv 的解释器(它装了 pyinstaller + pywebview)。
    若当前解释器已在 .venv 内(用户主动用 venv python 调 build.py),直接复用,
    避免路径解析偏差。.venv 不存在时返回当前解释器(调用方应先 ensure_backend_env)。
    """
    current = Path(sys.executable).resolve()
    if VENV_DIR.exists():
        try:
            # samefile 要求路径存在;.venv 存在但 Scripts/bin 不存在时会抛,
            # 落到下面的候选路径判断。
            if current.samefile(VENV_DIR) or VENV_DIR in current.parents:
                return str(current)
        except OSError:
            pass
        if sys.platform == "win32":
            return str(VENV_DIR / "Scripts" / "python.exe")
        return str(VENV_DIR / "bin" / "python")
    return str(current)


def venv_has_pyinstaller() -> bool:
    """检测 .venv 里是否装了 PyInstaller。

    必须用 .venv 的 python 跑检测,而不是当前进程的 python——build.py 常被
    全局 python 调用,而 PyInstaller 装在 .venv 里,当前进程永远 import 不到,
    会误判每次都需要补装。.venv 不存在时返回 False。
    """
    if not VENV_DIR.exists():
        return False
    py = venv_python()
    # -c 跑 importlib 检测;装了返回 0,没装返回 1。
    result = subprocess.run(
        [py, "-c", "import importlib.util, sys; sys.exit(0 if importlib.util.find_spec('PyInstaller') else 1)"],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
    )
    return result.returncode == 0


def ensure_backend_env() -> None:
    """确保后端环境就绪:有 .venv 且装了 pyinstaller。

    门控信号:.venv 不存在,或存在但(在 .venv python 里)import 不到 PyInstaller。
    任一不满足则跑 `uv sync --extra <平台> --extra dev`。
    打包必须带 dev extra(pyinstaller)和平台 extra(pywebview),
    否则打出来的 exe 启动即 ModuleNotFoundError(见 docs/PACKAGING.md §1)。
    """
    print("\n[0/3] 确保后端环境就绪...")
    uv_bin = shutil.which("uv")
    if uv_bin is None:
        sys.exit(f"未找到 uv,请先安装: {UV_INSTALL_URL}")

    need_sync = False
    if not VENV_DIR.exists():
        print("  .venv 不存在,需要安装后端依赖。")
        need_sync = True
    elif not venv_has_pyinstaller():
        print("  .venv 存在但缺少 PyInstaller(dev extra),需要补装。")
        need_sync = True

    if not need_sync:
        print("  ✓ 后端环境已就绪,跳过安装。")
        return

    extra = platform_extra()
    print(f"  安装后端依赖 (uv sync --extra {extra} --extra dev)...")
    result = subprocess.run(
        ["uv", "sync", "--extra", extra, "--extra", "dev"],
        cwd=REPO_ROOT,
        check=False,
    )
    if result.returncode != 0:
        sys.exit(f"后端依赖安装失败 (uv sync 退出码 {result.returncode})")
    print("  ✓ 后端依赖安装完成")


# -------------------------------------------------------------------
# 构建步骤
# -------------------------------------------------------------------


def build_frontend() -> None:
    """构建前端：pnpm install → pnpm build。"""
    print("\n[1/3] 构建前端...")
    # Windows 上 pnpm 是 .CMD shim，subprocess 默认不走 PATHEXT，要让 cmd.exe
    # 解析 PATHEXT 必须传 shell=True。命令是字面量无注入风险。
    # POSIX(macOS/Linux)下 pnpm 是普通可执行文件,直接 exec 即可;强制 shell=True
    # 会走 /bin/sh 解析,引号/变量展开语义不同,没必要也不安全。
    use_shell = sys.platform == "win32"
    for cmd in (
        ["pnpm", "install"],
        ["pnpm", "build"],
    ):
        result = subprocess.run(
            cmd,
            cwd=FRONTEND_DIR,
            check=True,
            shell=use_shell,
        )
        if result.returncode != 0:
            sys.exit(f"前端构建失败: {' '.join(cmd)}")
    print("  ✓ 前端构建完成")


def copy_static() -> None:
    """复制 frontend/dist/ → src/pixiv_tool/static/。"""
    print("\n[2/3] 复制静态资源...")
    if STATIC_DIR.exists():
        shutil.rmtree(STATIC_DIR)
    shutil.copytree(DIST_DIR, STATIC_DIR)
    print(f"  ✓ 已复制到 {STATIC_DIR}")


def run_pyinstaller() -> None:
    """执行 PyInstaller 打包。"""
    print("\n[3/3] PyInstaller 打包...")
    # .spec 文件已经定义了 onedir 模式(通过 COLLECT),不能再传 --onedir/--onefile——
    # PyInstaller 6+ 显式拒绝 makespec 选项与 .spec 共用。
    # --noconfirm 仍然合法(覆盖 dist/pixiv-tool/ 时不问 y/N)。
    # 用 venv python(ensure_backend_env 保证它装了 PyInstaller),
    # 避免用户用全局 python 调 build.py 时找不到 pyinstaller。
    python_bin = venv_python()
    result = subprocess.run(
        [python_bin, "-m", "PyInstaller", str(SPEC_FILE), "--noconfirm"],
        cwd=REPO_ROOT,
        check=True,
    )
    if result.returncode != 0:
        sys.exit("PyInstaller 打包失败")
    print("  ✓ 打包完成")


# -------------------------------------------------------------------
# 入口
# -------------------------------------------------------------------


def main() -> None:
    """执行完整构建流程。"""
    parser = argparse.ArgumentParser(description="pixiv-tool 构建脚本")
    parser.add_argument("--skip-fe", action="store_true", help="跳过前端构建")
    args = parser.parse_args()

    print("=" * 50)
    print("  pixiv-tool 构建")
    print("=" * 50)

    # 打包前确保后端 venv + pyinstaller + pywebview 就绪(已就绪则秒跳过)。
    ensure_backend_env()

    if not args.skip_fe:
        build_frontend()
    else:
        print("\n[1/3] 跳过前端构建 (--skip-fe)")

    copy_static()
    run_pyinstaller()

    print("\n" + "=" * 50)
    print(f"  构建成功！产物路径：{DIST_OUTPUT}")
    print("=" * 50)


if __name__ == "__main__":
    main()
