"""
Pixiv Tool 后端入口。
探测可用端口 → 起 uvicorn → 可选起 pywebview 主窗。
"""

from __future__ import annotations

import logging
import os
import socket
from pathlib import Path


from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from starlette.responses import FileResponse

logger = logging.getLogger(__name__)

app = FastAPI(title="pixiv-tool", docs_url=None, redoc_url=None)

# CORS: dev 模式允许 localhost:9961
app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:9961", "http://127.0.0.1:9961"],
    allow_methods=["*"],
    allow_headers=["*"],
)

# 注册所有 API 路由
from backend.api.auth import router as auth_router
from backend.api.novels import router as novels_router
from backend.api.tasks import router as tasks_router
from backend.api.settings import router as settings_router
from backend.api.system import router as system_router

app.include_router(system_router)
app.include_router(auth_router)
app.include_router(novels_router)
app.include_router(tasks_router)
app.include_router(settings_router)

# -------------------------------------------------------------------
# 路由
# -------------------------------------------------------------------


@app.get("/api/health")
async def health():
    return {"status": "ok"}


# -------------------------------------------------------------------
# 静态资源挂载（生产模式）
# -------------------------------------------------------------------

_STATIC_DIR = Path(__file__).resolve().parent / "static"

if _STATIC_DIR.is_dir():
    _index_html = _STATIC_DIR / "index.html"

    if _index_html.exists():
        @app.get("/{full_path:path}")
        async def serve_spa(full_path: str):
            """SPA fallback：非 /api 路径全部返回 index.html。"""
            file_path = _STATIC_DIR / full_path
            if file_path.is_file():
                return FileResponse(file_path)
            return FileResponse(_index_html)



# -------------------------------------------------------------------
# 端口探测
# -------------------------------------------------------------------


class PortAllocationError(Exception):
    """所有端口均被占用。"""


def find_available_port(start: int = 9962, end: int = 9999) -> int:
    """在 [start, end] 范围内探测可用端口，回退到 [10000, 19999]。"""
    for port_range in ((start, end), (10000, 19999)):
        for port in range(port_range[0], port_range[1] + 1):
            if _is_port_free(port):
                return port
    raise PortAllocationError("所有候选端口均被占用，请关闭占用进程后重试")


def _is_port_free(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        try:
            s.bind(("127.0.0.1", port))
            return True
        except OSError:
            return False



# -------------------------------------------------------------------
# 入口
# -------------------------------------------------------------------


def main() -> None:
    """启动 FastAPI 应用。"""
    import uvicorn  # noqa: WPS433 – 懒导入避免顶层副作用

    port = find_available_port()
    os.environ["PIXIV_TOOL_PORT"] = str(port)
    logger.info("Pixiv Tool 后端启动于 http://127.0.0.1:%d", port)

    uvicorn.run(
        app,
        host="127.0.0.1",
        port=port,
        log_level="info",
    )


if __name__ == "__main__":
    main()
