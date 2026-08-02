"""用户数据目录锚定（分平台）。

dev(venv python)模式（所有平台）: 锚定 <repo_root>/data、<repo_root>/config,
    开发方便,数据落在仓库内。

frozen(PyInstaller 打包)模式分平台:
    - Windows: <exe_dir>/data、<exe_dir>/config —— portable,解压即用,
      升级只要不删 data/ 目录用户数据就保留（V2-03 产品决策,核心需求）。
      和 main._redirect_stdio_if_needed 的 stdout.log 锚定一致。
    - macOS: ~/Library/Application Support/pixiv-tool/data 和 /config。
      不用 <exe_dir> 的原因:mac .app 里 sys.executable 指向
      Foo.app/Contents/MacOS/Foo,其 parent 是 Contents/MacOS/,
      数据会写进 bundle 内部——而代码签名/公证后 bundle 内部只读,写入必失败。
      改用 macOS 习惯的 Application Support 目录。
    - Linux: 遵循 XDG 规范,data 和 config 必须分开:
      $XDG_DATA_HOME/pixiv-tool/data (默认 ~/.local/share/pixiv-tool/data),
      $XDG_CONFIG_HOME/pixiv-tool/config (默认 ~/.config/pixiv-tool/config)。

只用标准库 (sys/os/Path.home()),不引入 platformdirs 等第三方依赖。
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

_APP_NAME = "pixiv-tool"


def _is_frozen() -> bool:
    return getattr(sys, "frozen", False)


def _repo_root() -> Path:
    # paths.py 在 <repo>/src/pixiv_tool/storage/paths.py
    # → parents[0]=storage, [1]=pixiv_tool, [2]=src, [3]=repo
    return Path(__file__).resolve().parents[3]


def _exe_dir() -> Path:
    """frozen 可执行文件所在目录（Windows portable 用）。"""
    return Path(sys.executable).resolve().parent


def _mac_support_root() -> Path:
    """macOS: ~/Library/Application Support/pixiv-tool/。"""
    return Path.home() / "Library" / "Application Support" / _APP_NAME


def _linux_data_root() -> Path:
    """Linux: $XDG_DATA_HOME/pixiv-tool (默认 ~/.local/share/pixiv-tool)。"""
    base = os.environ.get("XDG_DATA_HOME")
    if base:
        return Path(base) / _APP_NAME
    return Path.home() / ".local" / "share" / _APP_NAME


def _linux_config_root() -> Path:
    """Linux: $XDG_CONFIG_HOME/pixiv-tool (默认 ~/.config/pixiv-tool)。"""
    base = os.environ.get("XDG_CONFIG_HOME")
    if base:
        return Path(base) / _APP_NAME
    return Path.home() / ".config" / _APP_NAME


def data_dir() -> Path:
    if not _is_frozen():
        return _repo_root() / "data"
    if sys.platform == "win32":
        # Windows: portable,exe 同级（不变）。
        return _exe_dir() / "data"
    if sys.platform == "darwin":
        # mac: bundle 内部签名只读,落到用户 Application Support。
        return _mac_support_root() / "data"
    # Linux/其它: XDG。
    return _linux_data_root() / "data"


def config_dir() -> Path:
    if not _is_frozen():
        return _repo_root() / "config"
    if sys.platform == "win32":
        # Windows: portable,exe 同级（不变）。
        return _exe_dir() / "config"
    if sys.platform == "darwin":
        # mac: Application Support 下 data/config 同根,目录名约定对齐 Win/Linux。
        return _mac_support_root() / "config"
    # Linux/其它: XDG,config 必须与 data 分开。
    return _linux_config_root() / "config"


def logs_dir() -> Path:
    return data_dir() / "logs"


def default_output_dir() -> Path:
    """默认输出目录：系统下载目录/pixiv-tool（区分平台）。

    Windows/macOS/Linux 统一为 ~/Downloads/pixiv-tool（Linux 不追 XDG 下载
    目录,保持跨平台行为一致;用户可在设置页自改）。
    """
    return Path.home() / "Downloads" / _APP_NAME


# 模块级常量(向后兼容现有 import)
DATA_DIR = data_dir()
CONFIG_DIR = config_dir()
LOGS_DIR = logs_dir()
