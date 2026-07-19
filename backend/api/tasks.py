"""
任务相关 API：创建任务、列表、详情、SSE 进度流、暂停/继续/取消/重试。
"""

from __future__ import annotations

import asyncio
import json
import logging
from datetime import datetime, timezone

from fastapi import APIRouter
from fastapi.responses import StreamingResponse

from backend.core.crawler import Crawler
from backend.core.source import SingleNovelSource, SeriesSource, UserNovelsSource
from backend.core.task import TaskManager
from backend.core.pixiv_client import PixivClient
from backend.storage.db import Database
from backend.storage.cookies import create_cookie_store
from backend.storage.settings import get_settings

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/tasks")

_db = Database()
_task_manager = TaskManager(_db)
_store = create_cookie_store()


@router.post("")
async def create_task(body: dict):
    """创建抓取任务。"""
    source_type = body.get("source_type", "single")
    source_id = body.get("source_id", "")
    formats = body.get("formats", ["txt", "markdown"])

    task = _task_manager.create_task(source_type, source_id)

    # 获取登录 cookie
    try:
        cookies = _store.load() or {}
    except NotImplementedError:
        return {"error": "当前平台不支持登录（V2 实现 keychain/secretstorage）"}

    if not cookies.get("PHPSESSID"):
        return {"error": "未登录，请先登录"}

    client = PixivClient(cookies)
    settings = get_settings()

    # 选择 Source
    source_map = {
        "single": lambda: SingleNovelSource(int(source_id)),
        "series": lambda: SeriesSource(int(source_id)),
        "user": lambda: UserNovelsSource(int(source_id)),
    }
    source_fn = source_map.get(source_type)
    if not source_fn:
        return {"error": f"未知来源类型: {source_type}"}

    # 后台执行抓取
    crawler = Crawler(client, _db, _task_manager)

    async def _run():
        try:
            await crawler.run(
                source_fn(), task.task_id, formats,
                output_dir=settings.output_dir,
                event_callback=lambda *a: None,  # SSE 推送通过事件流
            )
        finally:
            await client.close()

    asyncio.create_task(_run())
    return {"task_id": task.task_id, "status": "pending"}


@router.get("")
async def list_tasks():
    """任务列表。"""
    return {"items": _db.list_tasks()}


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
    """重试失败的篇目。"""
    task = _db.get_task(task_id)
    if not task:
        return {"error": "任务不存在"}

    failed_ids = json.loads(task.get("failed_ids", "[]"))
    if not failed_ids:
        return {"error": "没有失败项"}

    # 创建新任务只抓失败的
    new_task = _task_manager.create_task(task["source_type"], ",".join(str(i) for i in failed_ids))

    try:
        cookies = _store.load() or {}
    except NotImplementedError:
        return {"error": "当前平台不支持登录"}

    client = PixivClient(cookies)
    settings = get_settings()

    # 用 SingleNovelSource 逐个抓
    from backend.core.source import SingleNovelSource
    source = SingleNovelSource(failed_ids[0])  # 简化：单个重试

    crawler = Crawler(client, _db, _task_manager)

    async def _run():
        try:
            for nid in failed_ids:
                s = SingleNovelSource(nid)
                await crawler.run(s, new_task.task_id, output_dir=settings.output_dir)
        finally:
            await client.close()

    asyncio.create_task(_run())
    return {"task_id": new_task.task_id, "status": "pending"}
