"""
插画相关 API：分页查询、文件定位、删除。

与 novels.py 平行，数据表 illustrations（saved_paths 为 JSON 文件路径数组）。
"""

from __future__ import annotations

import json
import logging
from pathlib import Path

from fastapi import APIRouter, Query
from pydantic import BaseModel, Field

from pixiv_tool.platform import reveal_in_file_manager
from pixiv_tool.storage.db import Database

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api")

_db = Database()


def _saved_paths(illustration: dict) -> list[Path]:
    """解析 saved_paths JSON 为 Path 列表（坏数据返回空）。"""
    try:
        return [Path(p) for p in json.loads(illustration.get("saved_paths") or "[]")]
    except (ValueError, TypeError):
        return []


def _unlink_illust_files(illustration: dict) -> None:
    """删除单条插画记录对应的文件(忽略已不存在)。"""
    for p in _saved_paths(illustration):
        if p.exists():
            try:
                p.unlink()
            except OSError as exc:
                logger.warning("删除文件失败 %s: %s", p, exc)


class BatchDeleteRequest(BaseModel):
    illustration_ids: list[int] = Field(default_factory=list)
    delete_file: bool = False


@router.get("/illustrations")
async def list_illustrations(
    page: int = Query(1, ge=1),
    page_size: int = Query(50, ge=1, le=200),
    author_id: int | None = None,
    keyword: str | None = None,
):
    """分页查询已抓插画。"""
    return _db.list_illustrations(page, page_size, author_id, keyword)


@router.post("/illustrations/{artwork_id}/open")
async def open_illustration_folder(artwork_id: int):
    """在系统文件管理器中打开作品所在目录。"""
    illust = _db.get_illustration(artwork_id)
    if not illust:
        return {"error": "插画记录不存在"}
    paths = _saved_paths(illust)
    if not paths:
        return {"error": "没有已保存的文件"}
    try:
        reveal_in_file_manager(paths[0])
    except FileNotFoundError as exc:
        return {"error": str(exc)}
    return {"status": "success"}


@router.delete("/illustrations/{artwork_id}")
async def delete_illustration(artwork_id: int, delete_file: bool = False):
    """删除插画记录（可选删文件）。"""
    illust = _db.get_illustration(artwork_id)
    if not illust:
        return {"error": "插画记录不存在"}
    if delete_file:
        _unlink_illust_files(illust)
    _db.delete_illustration(artwork_id)
    return {"status": "success"}


@router.post("/illustrations/batch-delete")
async def delete_illustrations_batch(req: BatchDeleteRequest):
    """批量删除插画记录（可选删文件）。"""
    if not req.illustration_ids:
        return {"error": "illustration_ids 不能为空"}

    # 先查出要删文件的全部记录(避免删 DB 后丢失 path)
    rows: list[dict] = []
    if req.delete_file:
        for aid in req.illustration_ids:
            illust = _db.get_illustration(aid)
            if illust:
                rows.append(illust)
        for illust in rows:
            _unlink_illust_files(illust)

    deleted = _db.delete_illustrations_batch(req.illustration_ids)
    logger.info("批量删除 %d 条插画记录", deleted)
    return {"status": "success", "deleted": deleted}


@router.delete("/illustrations")
async def delete_all_illustrations(delete_file: bool = False):
    """清空全部插画记录（可选删文件）。"""
    if delete_file:
        rows = _db.list_illustrations(page=1, page_size=10000).get("items", [])
        for illust in rows:
            _unlink_illust_files(illust)

    deleted = _db.delete_all_illustrations()
    logger.info("清空 %d 条插画记录", deleted)
    return {"status": "success", "deleted": deleted}
