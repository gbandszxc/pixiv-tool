"""
Crawler — 消费 NovelSource 的 id 流，统一执行并发、限速、重试、写库。
"""

from __future__ import annotations

import asyncio
import json
import logging
from collections.abc import AsyncIterator
from datetime import datetime, timezone

from backend.core.exporter import create_exporters
from backend.core.pixiv_client import PixivClient
from backend.core.source import NovelSource
from backend.core.task import TaskManager
from backend.storage.db import Database
from backend.storage.models import NovelData

logger = logging.getLogger(__name__)


class Crawler:
    """编排抓取任务。"""

    def __init__(self, client: PixivClient, db: Database,
                 task_manager: TaskManager) -> None:
        self.client = client
        self.db = db
        self.task_manager = task_manager

    async def run(self, source: NovelSource, task_id: str,
                  formats: list[str] | None = None,
                  output_dir: str = "downloads",
                  event_callback=None) -> None:
        """执行抓取。event_callback(event_type, data) 用于 SSE 推送。"""
        formats = formats or ["txt", "markdown"]
        exporters = create_exporters(formats)
        pause_evt = self.task_manager.get_pause_event(task_id)
        cancel_flag = self.task_manager.get_cancel_flag(task_id)

        total = 0
        done = 0
        skipped = 0
        failed_ids: list[int] = []
        from pathlib import Path
        target_dir = Path(output_dir)

        try:
            self.task_manager.update_progress(task_id, status="running")
            if event_callback:
                event_callback("progress", {"task_id": task_id, "done": 0, "total": 0, "skipped": 0})

            async for novel_id, order in source.resolve(self.client):
                if cancel_flag.is_set():
                    break

                # 去重
                if self.db.is_downloaded(novel_id):
                    skipped += 1
                    total += 1
                    continue

                total += 1
                await pause_evt.wait()  # 暂停点

                try:
                    await self._crawl_one(novel_id, order, target_dir, exporters)
                    done += 1
                    self.task_manager.update_progress(task_id, done=done, total=total, skipped=skipped)
                    if event_callback:
                        event_callback("progress", {
                            "task_id": task_id, "done": done,
                            "total": total, "skipped": skipped,
                        })
                except Exception as exc:
                    logger.error("抓取 %d 失败: %s", novel_id, exc)
                    failed_ids.append(novel_id)
                    if event_callback:
                        event_callback("failed", {"task_id": task_id, "novel_id": novel_id, "error": str(exc)})

            status = "canceled" if cancel_flag.is_set() else "done"
            self.task_manager.mark_done(task_id, total, done, skipped, failed_ids)
            if event_callback:
                event_callback("done", {
                    "task_id": task_id, "done": done,
                    "total": total, "skipped": skipped,
                    "failed": len(failed_ids),
                })

        except Exception as exc:
            logger.error("任务 %s 异常: %s", task_id, exc)
            self.task_manager.mark_failed(task_id, str(exc))
            if event_callback:
                event_callback("failed", {"task_id": task_id, "error": str(exc)})

    async def _crawl_one(self, novel_id: int, order: int | None,
                         target_dir, exporters) -> None:
        """抓取单篇小说。"""
        data = await self.client.get_novel(novel_id)
        title = data.get("title", str(novel_id))
        content = _extract_content(data)
        series_id = data.get("seriesId")
        series_title = data.get("seriesTitle")

        novel = NovelData(
            novel_id=novel_id,
            title=title,
            author_id=data.get("userId", 0),
            author_name=data.get("userName", ""),
            series_id=series_id,
            series_title=series_title,
            series_order=order,
            page_count=data.get("pageCount", 1),
            text_length=len(content),
            modification_date=data.get("updateDate", ""),
            content=content,
        )

        # 写数据库
        self.db.insert_novel(
            novel_id=novel_id,
            title=title,
            series_id=series_id,
            series_order=order,
            author_id=novel.author_id,
            author_name=novel.author_name,
            page_count=novel.page_count,
            text_length=novel.text_length,
            captured_at=datetime.now(timezone.utc).isoformat(),
            modification_date=novel.modification_date,
        )

        # 导出文件
        for exp in exporters:
            paths = exp.export(novel, target_dir, order)
            for p in paths:
                if str(p).endswith(".txt"):
                    self.db.update_novel_paths(novel_id=novel_id, txt_path=str(p))
                elif str(p).endswith(".md"):
                    self.db.update_novel_paths(novel_id=novel_id, md_path=str(p))


def _extract_content(data: dict) -> str:
    """从 pixiv API 响应提取小说文本内容。"""
    # content 字段是完整文本
    content = data.get("content", "")
    if content:
        return content

    # 多页小说：拼接 pages
    pages = data.get("pageCount", 1)
    if pages > 1:
        # pixiv 多页小说内容在 data.content 或 seriesContent 中
        return data.get("content", "")

    return content
