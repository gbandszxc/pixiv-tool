"""
历史联合 API：小说 + 插画统一分页查询（全部/单分类）。

删除/打开所在文件夹等变更操作仍走各自分类端点
（/api/novels/*、/api/illustrations/*），本模块只负责只读列表。
"""

from __future__ import annotations

from fastapi import APIRouter, Query

from pixiv_tool.storage.db import Database

router = APIRouter(prefix="/api")

_db = Database()


@router.get("/history")
async def list_history(
    category: str = Query("all"),
    page: int = Query(1, ge=1),
    page_size: int = Query(50, ge=1, le=200),
    keyword: str | None = None,
):
    """分页查询历史记录。category: all | novel | illustration。"""
    if category not in ("all", "novel", "illustration"):
        return {"error": f"未知历史分类: {category}"}
    return _db.list_history(category, page, page_size, keyword)
