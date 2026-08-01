"""
设置相关 API：读取、更新、清除日志、系统目录选择。
"""

from __future__ import annotations

import logging
import os
from pathlib import Path

from fastapi import APIRouter, HTTPException

from pixiv_tool.storage.paths import DATA_DIR, LOGS_DIR
from pixiv_tool.storage.settings import get_settings

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/settings")

# Windows 不允许出现在路径中的字符（冒号/斜杠除外，它们是盘符/分隔符）
_ILLEGAL_CHARS = '*?"<>|'


@router.get("")
async def get_config():
    """读取当前配置。"""
    s = get_settings()
    from dataclasses import asdict
    return asdict(s)


def _validate_output_dir(value: str) -> None:
    """校验输出目录，非法时抛 ValueError。"""
    if not isinstance(value, str):
        raise ValueError("输出目录必须是字符串")
    if not value.strip():
        raise ValueError("输出目录不能为空")
    if any(ord(c) < 32 for c in value):
        raise ValueError("输出目录包含非法控制字符")
    if os.name == "nt":
        found = "".join(dict.fromkeys(c for c in value if c in _ILLEGAL_CHARS))
        if found:
            raise ValueError(f"输出目录包含非法字符: {found}")
    path = Path(value)
    if not path.is_absolute():
        path = DATA_DIR / path
    if path.exists() and not path.is_dir():
        raise ValueError("输出目录指向一个已有文件")
    try:
        path.mkdir(parents=True, exist_ok=True)
    except OSError as exc:
        raise ValueError(f"无法创建输出目录: {exc}") from exc


def _pick_directory() -> str | None:
    """弹系统原生目录选择对话框，返回所选路径；取消或不可用时返回 None。"""
    # 优先 pywebview（桌面窗口存在时用其原生对话框）
    try:
        import webview

        if webview.windows:
            result = webview.windows[0].create_file_dialog(webview.FOLDER_DIALOG)
            if result:
                return str(result[0])
    except Exception:
        logger.debug("pywebview 目录选择不可用，回退 tkinter", exc_info=True)

    # 兜底 tkinter（dev 模式无 pywebview 窗口）
    try:
        import tkinter as tk
        from tkinter import filedialog

        root = tk.Tk()
        root.withdraw()
        try:
            return filedialog.askdirectory(parent=root, title="选择输出目录") or None
        finally:
            root.destroy()
    except Exception:
        logger.debug("tkinter 目录选择失败，返回 None", exc_info=True)
        return None


@router.put("")
async def update_config(body: dict):
    """更新配置并持久化。"""
    s = get_settings()
    if "output_dir" in body:
        try:
            _validate_output_dir(body["output_dir"])
        except ValueError as exc:
            raise HTTPException(status_code=400, detail=str(exc)) from exc
    for key in ("output_dir", "output_formats", "language", "theme", "backend_port"):
        if key in body:
            setattr(s, key, body[key])
    s.save()
    return {"status": "success"}


@router.post("/select-directory")
async def select_directory() -> dict:
    """弹系统原生目录选择器，返回所选路径（取消时为 null）。"""
    return {"path": _pick_directory()}


@router.post("/clear-logs")
async def clear_logs():
    """清空 app.log。"""
    log_path = LOGS_DIR / "app.log"
    if log_path.exists():
        log_path.write_text("", encoding="utf-8")
    return {"status": "success"}
