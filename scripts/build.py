"""
pixiv-tool 一键构建脚本。

步骤：
  1. 构建前端 (pnpm install + pnpm build)
  2. 复制 frontend/dist/ → backend/static/
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
BACKEND_DIR = REPO_ROOT / "backend"
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
    for cmd in (
        ["pnpm", "install"],
        ["pnpm", "build"],
    ):
        result = subprocess.run(
            cmd,
            cwd=FRONTEND_DIR,
            check=True,
        )
        if result.returncode != 0:
            sys.exit(f"前端构建失败: {' '.join(cmd)}")
    print("  ✓ 前端构建完成")


def copy_static() -> None:
    """复制 frontend/dist/ → backend/static/。"""
    print("\n[2/3] 复制静态资源...")
    if STATIC_DIR.exists():
        shutil.rmtree(STATIC_DIR)
    shutil.copytree(DIST_DIR, STATIC_DIR)
    print(f"  ✓ 已复制到 {STATIC_DIR}")


def run_pyinstaller() -> None:
    """执行 PyInstaller 打包。"""
    print("\n[3/3] PyInstaller 打包...")
    result = subprocess.run(
        [sys.executable, "-m", "PyInstaller", str(SPEC_FILE), "--onedir", "--noconfirm"],
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
