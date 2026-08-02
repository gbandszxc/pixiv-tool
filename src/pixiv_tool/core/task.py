"""
Task 状态机 — pending → running ⇄ paused → done/failed/canceled。
"""

from __future__ import annotations

import asyncio
import json
import logging
from datetime import datetime, timezone

from pixiv_tool.storage.db import Database
from pixiv_tool.storage.models import Task

logger = logging.getLogger(__name__)


class TaskManager:
    """任务管理器：创建、暂停、继续、取消、重试。"""

    def __init__(self, db: Database) -> None:
        self.db = db
        self._events: dict[str, asyncio.Event] = {}  # task_id → pause event
        self._cancel_flags: dict[str, asyncio.Event] = {}

    def create_task(self, source_type: str, source_id: str,
                    category: str = "novel") -> Task:
        import uuid
        now = datetime.now(timezone.utc).isoformat()
        task_id = str(uuid.uuid4())
        self.db.insert_task(
            task_id=task_id,
            source_type=source_type,
            source_id=source_id,
            category=category,
            status="pending",
            created_at=now,
            updated_at=now,
        )
        # 初始化 pause event（非暂停状态）
        evt = asyncio.Event()
        evt.set()
        self._events[task_id] = evt
        self._cancel_flags[task_id] = asyncio.Event()

        return Task(
            task_id=task_id,
            source_type=source_type,
            source_id=source_id,
            category=category,
            created_at=now,
            updated_at=now,
        )

    def get_pause_event(self, task_id: str) -> asyncio.Event:
        return self._events.setdefault(task_id, asyncio.Event())

    def get_cancel_flag(self, task_id: str) -> asyncio.Event:
        return self._cancel_flags.setdefault(task_id, asyncio.Event())

    def update_progress(self, task_id: str, **fields) -> None:
        fields["updated_at"] = datetime.now(timezone.utc).isoformat()
        self.db.update_task(task_id, **fields)

    def pause(self, task_id: str) -> None:
        evt = self._events.get(task_id)
        if evt:
            evt.clear()
        self.db.update_task(task_id, status="paused",
                            updated_at=datetime.now(timezone.utc).isoformat())

    def resume(self, task_id: str) -> None:
        evt = self._events.get(task_id)
        if evt:
            evt.set()
        self.db.update_task(task_id, status="running",
                            updated_at=datetime.now(timezone.utc).isoformat())

    def cancel(self, task_id: str) -> None:
        flag = self._cancel_flags.get(task_id)
        if flag:
            flag.set()
        evt = self._events.get(task_id)
        if evt:
            evt.set()  # 解除暂停以允许退出
        self.db.update_task(task_id, status="canceled",
                            updated_at=datetime.now(timezone.utc).isoformat())

    def mark_done(self, task_id: str, total: int, done: int, skipped: int,
                  failed_ids: list[int] | None = None) -> None:
        self.db.update_task(
            task_id,
            status="done",
            total=total,
            done=done,
            skipped=skipped,
            failed_ids=json.dumps(failed_ids or []),
            updated_at=datetime.now(timezone.utc).isoformat(),
        )

    def mark_failed(self, task_id: str, error: str) -> None:
        self.db.update_task(
            task_id,
            status="failed",
            error=error,
            updated_at=datetime.now(timezone.utc).isoformat(),
        )
