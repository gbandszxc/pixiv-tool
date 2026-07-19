"""
设置相关 API：读取、更新、清除日志。
"""

from __future__ import annotations

import logging
from pathlib import Path

from fastapi import APIRouter

from backend.storage.settings import get_settings

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/settings")


@router.get("")
async def get_config():
    """读取当前配置。"""
    s = get_settings()
    from dataclasses import asdict
    return asdict(s)


@router.put("")
async def update_config(body: dict):
    """更新配置并持久化。"""
    s = get_settings()
    for key in ("output_dir", "output_formats", "language", "theme", "backend_port"):
        if key in body:
            setattr(s, key, body[key])
    s.save()
    return {"status": "success"}


@router.post("/clear-logs")
async def clear_logs():
    """清空 app.log。"""
    log_path = Path(__file__).resolve().parent.parent.parent / "data" / "logs" / "app.log"
    if log_path.exists():
        log_path.write_text("", encoding="utf-8")
    return {"status": "success"}
