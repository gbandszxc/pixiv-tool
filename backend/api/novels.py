"""
小说相关 API：分页查询、文件定位、删除。
"""

from __future__ import annotations

import logging
import os
import platform
import subprocess
from pathlib import Path

from fastapi import APIRouter, Query

from backend.storage.db import Database

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api")

_db = Database()


@router.get("/novels")
async def list_novels(
    page: int = Query(1, ge=1),
    page_size: int = Query(50, ge=1, le=200),
    series_id: int | None = None,
    author_id: int | None = None,
    keyword: str | None = None,
):
    """分页查询已抓小说。"""
    return _db.list_novels(page, page_size, series_id, author_id, keyword)


@router.get("/novels/{novel_id}/file")
async def get_novel_file(novel_id: int):
    """返回小说文件路径。"""
    novel = _db.get_novel(novel_id)
    if not novel:
        return {"error": "小说不存在"}
    path = novel.get("txt_path") or novel.get("md_path")
    return {"path": path}


@router.post("/novels/{novel_id}/open")
async def open_novel_file(novel_id: int):
    """在系统资源管理器中打开文件所在目录。"""
    novel = _db.get_novel(novel_id)
    if not novel:
        return {"error": "小说不存在"}
    path = novel.get("txt_path") or novel.get("md_path")
    if not path or not Path(path).exists():
        return {"error": "文件不存在"}

    system = platform.system()
    if system == "Windows":
        subprocess.Popen(["explorer", "/select,", str(path)])
    elif system == "Darwin":
        subprocess.Popen(["open", "-R", str(path)])
    else:
        subprocess.Popen(["xdg-open", str(Path(path).parent)])

    return {"status": "success"}


@router.delete("/novels/{novel_id}")
async def delete_novel(novel_id: int, delete_file: bool = False):
    """删除小说记录（可选删文件）。"""
    novel = _db.get_novel(novel_id)
    if not novel:
        return {"error": "小说不存在"}

    if delete_file:
        for key in ("txt_path", "md_path"):
            p = novel.get(key)
            if p and Path(p).exists():
                Path(p).unlink()

    _db.delete_novel(novel_id)
    return {"status": "success"}
