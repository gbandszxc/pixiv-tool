"""
系统探活 / 调试端点（ticket 05 acceptance）。

- GET /api/ping          返回 {"pong": "<ISO8601 UTC timestamp>"}
- GET /api/test/events   返回 text/event-stream，连推 3 条 SSE progress
                         + 1 条 done（间隔 1s），用于前端 SSE 链路验证。
"""

from __future__ import annotations

import logging
from asyncio import sleep
from datetime import datetime, timezone

from fastapi import APIRouter
from fastapi.responses import StreamingResponse

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api")


@router.get("/ping")
async def ping() -> dict[str, str]:
    """返回当前 UTC 时间，用于前后端连通性探活。"""
    return {"pong": datetime.now(timezone.utc).isoformat()}


@router.get("/test/events")
async def test_events() -> StreamingResponse:
    """SSE 链路自检：连推 3 条 progress（间隔 1s）+ 1 条 done。

    事件格式遵循 SPEC §7.6（event + data 两行，空行分隔）。
    """
    async def gen():
        for i in range(1, 4):
            yield f'event: progress\ndata: {{"n": {i}}}\n\n'
            await sleep(1)
        yield 'event: done\ndata: {"total": 3}\n\n'

    return StreamingResponse(gen(), media_type="text/event-stream")
