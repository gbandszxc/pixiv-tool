"""用户数据目录锚定。

frozen(PyInstaller)模式: 锚定 <exe_dir>/data、<exe_dir>/config,
    和 main._redirect_stdio_if_needed 的 stdout.log 一致——
    升级时只要不删 data/ 目录,用户数据保留。
dev(venv python)模式: 锚定 <repo_root>/data、<repo_root>/config。
"""
from __future__ import annotations

import sys
from pathlib import Path


def _is_frozen() -> bool:
    return getattr(sys, "frozen", False)


def _repo_root() -> Path:
    # paths.py 在 <repo>/src/pixiv_tool/storage/paths.py
    # → parents[0]=storage, [1]=pixiv_tool, [2]=src, [3]=repo
    return Path(__file__).resolve().parents[3]


def data_dir() -> Path:
    if _is_frozen():
        return Path(sys.executable).resolve().parent / "data"
    return _repo_root() / "data"


def config_dir() -> Path:
    if _is_frozen():
        return Path(sys.executable).resolve().parent / "config"
    return _repo_root() / "config"


def logs_dir() -> Path:
    return data_dir() / "logs"


# 模块级常量(向后兼容现有 import)
DATA_DIR = data_dir()
CONFIG_DIR = config_dir()
LOGS_DIR = logs_dir()
