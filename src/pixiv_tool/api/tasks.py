"""
任务相关 API：创建任务、列表、详情、SSE 进度流、暂停/继续/取消/重试。
"""

from __future__ import annotations

import asyncio
import json
import logging
from datetime import datetime, timezone

from fastapi import APIRouter, HTTPException
from fastapi.responses import StreamingResponse

from pixiv_tool.core.crawler import Crawler
from pixiv_tool.core.illust_crawler import IllustCrawler
from pixiv_tool.core.source import SingleNovelSource, SeriesSource, UserNovelsSource
from pixiv_tool.core.illust_source import SingleIllustSource, UserIllustsSource
from pixiv_tool.core.task import TaskManager
from pixiv_tool.core.pixiv_client import PixivClient
from pixiv_tool.storage.db import Database, TERMINAL_TASK_STATUSES
from pixiv_tool.storage.cookies import create_cookie_store
from pixiv_tool.storage.settings import get_settings

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/tasks")

_db = Database()
_task_manager = TaskManager(_db)
_store = create_cookie_store()

NOVEL_SOURCE_MAP = {
    "single": lambda sid: SingleNovelSource(int(sid)),
    "series": lambda sid: SeriesSource(int(sid)),
    "user": lambda sid: UserNovelsSource(int(sid)),
}
ILLUST_SOURCE_MAP = {
    "single": lambda sid: SingleIllustSource(int(sid)),
    "user": lambda sid: UserIllustsSource(int(sid)),
}


@router.post("")
async def create_task(body: dict):
    """创建抓取任务。category: 'novel'(默认) | 'illustration'。"""
    source_type = body.get("source_type", "single")
    source_id = body.get("source_id", "")
    formats = body.get("formats", ["txt", "markdown"])
    category = body.get("category", "novel")

    if category not in ("novel", "illustration"):
        return {"error": f"未知任务分类: {category}"}
    source_map = NOVEL_SOURCE_MAP if category == "novel" else ILLUST_SOURCE_MAP
    source_fn = source_map.get(source_type)
    if not source_fn:
        return {"error": f"未知来源类型: {source_type}"}

    task = _task_manager.create_task(source_type, source_id, category=category)

    # 获取登录 cookie
    try:
        cookies = _store.load() or {}
    except NotImplementedError:
        return {"error": "当前平台不支持登录（V2 实现 keychain/secretstorage）"}

    if not cookies.get("PHPSESSID"):
        return {"error": "未登录，请先登录"}

    client = PixivClient(cookies)
    settings = get_settings()

    # 后台执行抓取
    if category == "novel":
        crawler = Crawler(client, _db, _task_manager)
    else:
        crawler = IllustCrawler(client, _db, _task_manager)

    user_id = int(source_id) if (source_type == "user" and category == "illustration") else None

    async def _run():
        try:
            if category == "novel":
                await crawler.run(
                    source_fn(source_id), task.task_id, formats,
                    output_dir=settings.output_dir,
                    max_wait_seconds=settings.max_wait_seconds,
                    event_callback=lambda *a: None,  # SSE 推送通过事件流
                )
            else:
                await crawler.run(
                    source_fn(source_id), task.task_id,
                    output_dir=settings.output_dir,
                    user_id=user_id,
                    max_wait_seconds=settings.max_wait_seconds,
                    event_callback=lambda *a: None,
                )
        finally:
            await client.close()

    asyncio.create_task(_run())
    return {"task_id": task.task_id, "status": "pending"}


@router.get("")
async def list_tasks(category: str | None = None):
    """任务列表，可按 category(novel/illustration) 过滤。"""
    if category is not None and category not in ("novel", "illustration"):
        return {"error": f"未知任务分类: {category}"}
    return {"items": _db.list_tasks(category=category)}


def _delete_task_ids(task_ids: list[str]) -> dict[str, int]:
    """删除任务记录（含进行中任务）。

    进行中任务先 cancel：设置 cancel flag，后台循环在下一个作品处退出，
    然后删除记录。正在下载的当前作品会下完（与手动取消行为一致）。
    """
    if not task_ids:
        raise HTTPException(status_code=422, detail="task_ids 不能为空")

    for tid in task_ids:
        task = _db.get_task(tid)
        if task and task.get("status") not in TERMINAL_TASK_STATUSES:
            _task_manager.cancel(tid)

    deleted, missing_ids = _db.delete_tasks(task_ids)
    if missing_ids:
        raise HTTPException(status_code=404, detail="任务不存在")
    return {"deleted": deleted}


@router.delete("")
async def delete_tasks(body: dict):
    """批量删除任务记录（含进行中任务，先取消）。"""
    task_ids = body.get("task_ids", [])
    if not isinstance(task_ids, list) or not all(isinstance(task_id, str) for task_id in task_ids):
        raise HTTPException(status_code=422, detail="task_ids 必须是字符串数组")
    return _delete_task_ids(task_ids)


@router.delete("/completed")
async def delete_completed_tasks():
    """清除全部已完成任务记录。"""
    return {"deleted": _db.delete_completed_tasks()}


@router.delete("/{task_id}")
async def delete_task(task_id: str):
    """删除单个任务记录（含进行中任务，先取消）。"""
    return _delete_task_ids([task_id])


@router.get("/{task_id}")
async def get_task(task_id: str):
    """任务详情。"""
    task = _db.get_task(task_id)
    if not task:
        return {"error": "任务不存在"}
    return task


@router.get("/{task_id}/events")
async def task_events(task_id: str):
    """SSE 进度流。"""
    async def event_stream():
        # 简化实现：轮询数据库状态变化
        last_status = None
        while True:
            task = _db.get_task(task_id)
            if not task:
                break

            current_status = task["status"]
            if current_status != last_status:
                data = json.dumps({
                    "task_id": task_id,
                    "status": current_status,
                    "done": task["done"],
                    "total": task["total"],
                    "skipped": task["skipped"],
                })
                yield f"event: progress\ndata: {data}\n\n"
                last_status = current_status

            if current_status in ("done", "failed", "canceled"):
                data = json.dumps({
                    "task_id": task_id,
                    "done": task["done"],
                    "total": task["total"],
                    "failed": len(json.loads(task["failed_ids"] or "[]")),
                    "skipped": task["skipped"],
                })
                yield f"event: done\ndata: {data}\n\n"
                break

            await asyncio.sleep(0.5)

    return StreamingResponse(
        event_stream(),
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
    )


@router.post("/{task_id}/pause")
async def pause_task(task_id: str):
    _task_manager.pause(task_id)
    return {"status": "paused"}


@router.post("/{task_id}/resume")
async def resume_task(task_id: str):
    _task_manager.resume(task_id)
    return {"status": "running"}


@router.post("/{task_id}/cancel")
async def cancel_task(task_id: str):
    _task_manager.cancel(task_id)
    return {"status": "canceled"}


@router.post("/{task_id}/retry-failed")
async def retry_failed(task_id: str):
    """重试失败的篇目/作品。"""
    task = _db.get_task(task_id)
    if not task:
        return {"error": "任务不存在"}

    failed_ids = json.loads(task.get("failed_ids", "[]"))
    if not failed_ids:
        return {"error": "没有失败项"}

    category = task.get("category", "novel")
    # 创建新任务只抓失败的
    new_task = _task_manager.create_task(
        task["source_type"], ",".join(str(i) for i in failed_ids), category=category)

    try:
        cookies = _store.load() or {}
    except NotImplementedError:
        return {"error": "当前平台不支持登录"}

    client = PixivClient(cookies)
    settings = get_settings()

    if category == "novel":
        from pixiv_tool.core.source import SingleNovelSource
        crawler: object = Crawler(client, _db, _task_manager)

        async def _run():
            try:
                for nid in failed_ids:
                    await crawler.run(SingleNovelSource(nid), new_task.task_id,
                                      output_dir=settings.output_dir,
                                      max_wait_seconds=settings.max_wait_seconds)
            finally:
                await client.close()
    else:
        from pixiv_tool.core.illust_source import SingleIllustSource
        crawler = IllustCrawler(client, _db, _task_manager)

        async def _run():
            try:
                for nid in failed_ids:
                    await crawler.run(SingleIllustSource(nid), new_task.task_id,
                                      output_dir=settings.output_dir,
                                      max_wait_seconds=settings.max_wait_seconds)
            finally:
                await client.close()

    asyncio.create_task(_run())
    return {"task_id": new_task.task_id, "status": "pending"}
