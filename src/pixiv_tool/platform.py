"""跨平台系统集成小工具。"""

from __future__ import annotations

import platform as _platform
import subprocess
from pathlib import Path


def reveal_in_file_manager(path: Path) -> None:
    """在系统文件管理器中定位文件；文件已不存在时打开其所在目录。

    Windows: explorer /select, 打开所在目录并选中文件
    macOS:   open -R 在 Finder 中显示文件
    Linux:   xdg-open 无"选中文件"语义，直接打开所在目录
    """
    target = path if path.exists() else path.parent
    if not target.exists():
        raise FileNotFoundError(f"路径不存在: {target}")

    system = _platform.system()
    if system == "Windows":
        subprocess.Popen(["explorer", "/select,", str(target)])
    elif system == "Darwin":
        subprocess.Popen(["open", "-R", str(target)])
    else:
        subprocess.Popen(["xdg-open", str(target.parent if target.is_file() else target)])
