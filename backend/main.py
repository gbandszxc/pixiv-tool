"""
Pixiv Tool 后端入口。
探测可用端口 → 起 uvicorn → 可选起 pywebview 主窗。
"""

from __future__ import annotations

import logging
import os
import socket
import sys
from pathlib import Path


from fastapi import FastAPI
from starlette.responses import FileResponse

logger = logging.getLogger(__name__)

app = FastAPI(title="pixiv-tool", docs_url=None, redoc_url=None)

# 注：dev 模式走 Vite proxy 同源、prod 模式走 pywebview 同源，都不触发 CORS，
# 故不配置 CORSMiddleware。
# 若未来需要跨域（如独立调试前端到不同端口），再加回：
#   from fastapi.middleware.cors import CORSMiddleware
#   app.add_middleware(CORSMiddleware, allow_origins=[...], ...)

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

# PyInstaller frozen 模式下,__file__ 指向 _internal/backend/main.py,
# 但 PyInstaller 把数据文件解压到 sys._MEIPASS(onedir 模式下 == exe 同级 _internal/)。
# spec 里 datas=[(static_dir, "backend/static")] 把静态资源放到 _internal/backend/static,
# 正好和 unfrozen 模式的相对位置一致——所以 frozen 时改用 _MEIPASS 锚定即可。
if getattr(sys, "frozen", False) and hasattr(sys, "_MEIPASS"):
    _STATIC_DIR = Path(sys._MEIPASS) / "backend" / "static"

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

    支持的命令行参数：
        (无参数)         prod 桌面模式(起 pywebview 主窗)
        --no-window     纯 API 模式(仅起后端,dev / 无头调试用)
        --login-window --result-file X   子入口模式:登录窗(SPEC §4.1)。
            prod(PyInstaller frozen)模式下 /api/auth/login 会 spawn 同一个
            pixiv-tool.exe 加这两个参数,走登录窗逻辑;dev 模式由 auth.py
            改成 `python -m backend.auth.login_window`。
    """
    import sys

    # 子入口分发:frozen exe 在 prod 模式下作为登录窗启动器被复用。
    if "--login-window" in sys.argv:
        from backend.auth.login_window import run_login_subprocess_main
        argv = [a for a in sys.argv[1:] if a != "--login-window"]
        # 复用 login_window 自带 argparse(--result-file)
        exit_code = run_login_subprocess_main(_extract_result_file(argv))
        sys.exit(exit_code)

    use_window = "--no-window" not in sys.argv
    start_app(use_window=use_window)


def _extract_result_file(argv: list[str]) -> str:
    """从 argv 里抽 --result-file 值(支持 --result-file X 或 --result-file=X)。"""
    for i, a in enumerate(argv):
        if a == "--result-file" and i + 1 < len(argv):
            return argv[i + 1]
        if a.startswith("--result-file="):
            return a.split("=", 1)[1]
    raise SystemExit("--login-window 必须配 --result-file 参数")


if __name__ == "__main__":
    main()
