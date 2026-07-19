"""API 路由注册。"""

from fastapi import APIRouter

router = APIRouter(prefix="/api")


@router.get("/ping")
async def ping():
    from datetime import datetime, timezone
    return {"pong": datetime.now(timezone.utc).isoformat()}
