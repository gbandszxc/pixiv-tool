"""
小说相关 API：分页查询、文件定位、删除。
"""

from __future__ import annotations

import logging
from pathlib import Path

from fastapi import APIRouter, Body, Query
from pydantic import BaseModel, Field

from pixiv_tool.platform import reveal_in_file_manager
from pixiv_tool.storage.db import Database

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api")

_db = Database()


def _unlink_novel_files(novel: dict) -> None:
    """删除单条 novel 记录对应的 txt/md 文件(忽略已不存在)。"""
    for key in ("txt_path", "md_path"):
        p = novel.get(key)
        if p and Path(p).exists():
            try:
                Path(p).unlink()
            except OSError as exc:
                logger.warning("删除文件失败 %s: %s", p, exc)


class BatchDeleteRequest(BaseModel):
    novel_ids: list[int] = Field(default_factory=list)
    delete_file: bool = False


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

    reveal_in_file_manager(Path(path))
    return {"status": "success"}


@router.delete("/novels/{novel_id}")
async def delete_novel(novel_id: int, delete_file: bool = False):
    """删除小说记录（可选删文件）。"""
    novel = _db.get_novel(novel_id)
    if not novel:
        return {"error": "小说不存在"}

    if delete_file:
        _unlink_novel_files(novel)

    _db.delete_novel(novel_id)
    return {"status": "success"}


@router.post("/novels/batch-delete")
async def delete_novels_batch(req: BatchDeleteRequest):
    """批量删除小说记录（可选删文件）。"""
    if not req.novel_ids:
        return {"error": "novel_ids 不能为空"}

    # 先查出要删文件的全部记录(避免删 DB 后丢失 path)
    rows: list[dict] = []
    if req.delete_file:
        for nid in req.novel_ids:
            novel = _db.get_novel(nid)
            if novel:
                rows.append(novel)
        for novel in rows:
            _unlink_novel_files(novel)

    deleted = _db.delete_novels_batch(req.novel_ids)
    logger.info("批量删除 %d 条 novel 记录", deleted)
    return {"status": "success", "deleted": deleted}


@router.delete("/novels")
async def delete_all_novels(delete_file: bool = False):
    """清空全部 novel 记录（可选删文件）。"""
    if delete_file:
        rows = _db.list_novels(page=1, page_size=10000).get("items", [])
        for novel in rows:
            _unlink_novel_files(novel)

    deleted = _db.delete_all_novels()
    logger.info("清空 %d 条 novel 记录", deleted)
    return {"status": "success", "deleted": deleted}
