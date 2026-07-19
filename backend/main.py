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


def start_app(use_window: bool = True) -> None:
    """启动 App：探测端口 → 起 uvicorn → （prod 模式）起 pywebview 主窗。

    SPEC §3.1 进程拓扑：
        1. find_available_port() 探测 [9962, 9999]
        2. uvicorn.Server 在独立 daemon 线程跑（用 Server 而非 uvicorn.run
           避免 sys.exit 拖垮主进程）
        3. 主线程 pywebview.create_window 加载 http://127.0.0.1:<port>/
        4. webview.start() 阻塞；窗口关闭即整体退出

    Args:
        use_window: True=prod 桌面模式（起 pywebview）；False=纯 API 模式
            （只起后端，供 dev / 无头调试 / CI 使用）。
    """
    import time
    import threading

    import uvicorn  # noqa: WPS433 – 懒导入避免顶层副作用

    port = find_available_port()
    os.environ["PIXIV_TOOL_PORT"] = str(port)
    logger.info("Pixiv Tool 后端启动于 http://127.0.0.1:%d", port)

    config = uvicorn.Config(
        app,
        host="127.0.0.1",
        port=port,
        log_level="info",
    )
    server = uvicorn.Server(config)

    if not use_window:
        # dev / 纯 API 模式：主线程直接跑后端
        server.run()
        return

    # prod 模式：后端跑在 daemon 线程，主线程跑 pywebview
    backend_thread = threading.Thread(target=server.run, daemon=True)
    backend_thread.start()

    # 等后端 ready（最多 5 秒），避免 pywebview 加载到空端口
    for _ in range(50):
        if server.started:
            break
        time.sleep(0.1)

    import webview  # noqa: WPS433 – 懒导入：仅 prod 模式需要

    webview.create_window(
        "Pixiv Tool",
        f"http://127.0.0.1:{port}/",
        width=1200,
        height=800,
    )
    webview.start()  # 阻塞，直到用户关窗
    # 窗口关闭 → 通知 uvicorn 退出，daemon 线程随之结束
    server.should_exit = True


def main() -> None:
    """默认入口：prod 桌面模式（起 pywebview 主窗）。

    传 --no-window 参数切到纯 API 模式（仅起后端）。
    """
    import sys

    use_window = "--no-window" not in sys.argv
    start_app(use_window=use_window)


if __name__ == "__main__":
    main()
