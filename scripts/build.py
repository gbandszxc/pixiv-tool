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
    result = subprocess.run(
        [sys.executable, "-m", "PyInstaller", str(SPEC_FILE), "--noconfirm"],
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
