"""
认证相关 API：登录状态查询、手动登录、登出。
"""

from __future__ import annotations

import asyncio
import json
import logging
import subprocess
import sys
import tempfile
from pathlib import Path

from fastapi import APIRouter

from pixiv_tool.storage.cookies import create_cookie_store

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/auth")

_store = create_cookie_store()

# 登录子进程最长允许跑 5 分钟(用户可能慢慢输密码)
_LOGIN_TIMEOUT_SEC = 300
# 启动阶段只允许一次短暂的远程登录态验证，避免失效 Cookie 或网络异常拖慢 UI。
_AUTH_STATUS_TIMEOUT_SEC = 2.0


@router.get("/status")
async def auth_status():
    """查询当前登录态。"""
    try:
        cookies = _store.load()
    except NotImplementedError:
        # macOS/Linux: stub
        return {"is_logged_in": False}
    except Exception:
        return {"is_logged_in": False}

    if not cookies:
        return {"is_logged_in": False}

    # 验证 cookie 有效性：调 /ajax/user/self
    try:
        import httpx
        async with httpx.AsyncClient(timeout=_AUTH_STATUS_TIMEOUT_SEC) as client:
            async with asyncio.timeout(_AUTH_STATUS_TIMEOUT_SEC):
                resp = await client.get(
                    "https://www.pixiv.net/ajax/user/self?lang=zh",
                    headers={
                        "x-csrf-token": cookies.get("x-csrf-token", ""),
                        "User-Agent": "Mozilla/5.0",
                        "Referer": "https://www.pixiv.net/",
                    },
                    cookies={k: v for k, v in cookies.items() if k != "x-csrf-token"},
                )
            if resp.status_code in (401, 403):
                _store.clear()
                return {"is_logged_in": False}
            if resp.status_code != 200:
                logger.warning("登录态验证返回非认证错误: status=%s", resp.status_code)
                return {"is_logged_in": False}

            body = resp.json()
            # Pixiv 当前返回顶层 userData；保留旧结构兼容以应对页面/API 变体。
            user_data = body.get("userData") or body.get("body", {}).get("userData") or {}
            if not user_data:
                logger.warning("登录态验证响应缺少 userData")
                return {"is_logged_in": False}

            return {
                "is_logged_in": True,
                "user_id": str(user_data.get("id", "")),
                "pixiv_id": user_data.get("pixivId", ""),
                "name": user_data.get("name", ""),
                "profile_img": user_data.get("profileImg") or user_data.get("profileImgBig", ""),
            }
    except TimeoutError:
        logger.warning("登录态验证超时（%.1fs）", _AUTH_STATUS_TIMEOUT_SEC)
        return {"is_logged_in": False}
    except Exception as exc:
        logger.warning("登录态验证失败: %s", exc)
        return {"is_logged_in": False}


@router.get("/diag-version")
async def diag_version():
    """诊断端点:报告 worker 实际加载的代码版本。

    frozen(PyInstaller 打包)环境下 inspect.getsource 拿不到源码——
    代码已被编译进 PYZ 归档,没有 .py 源文件。这时直接报 frozen 标志即可。
    """
    import sys

    frozen = getattr(sys, "frozen", False)
    if frozen:
        return {
            "frozen": True,
            "executable": sys.executable,
            "login_window_file": "(bundled)",
            "auth_file": "(bundled)",
            "auth_has_diag_version": True,
        }

    from pixiv_tool.auth import login_window as lw_mod
    from pixiv_tool.api import auth as auth_mod
    import inspect
    lw_src = inspect.getsource(lw_mod)
    auth_src = inspect.getsource(auth_mod)
    return {
        "frozen": False,
        "login_window_file": lw_mod.__file__,
        "login_window_has_run_subprocess_main": "run_login_subprocess_main" in lw_src,
        "login_window_has_lambda_webview": "lambda: webview.start" in lw_src or "lambda: webview_thread" in lw_src,
        "auth_file": auth_mod.__file__,
        "auth_has_asyncio_to_thread": "asyncio.to_thread" in auth_src,
        "auth_has_diag_version": True,
    }


@router.post("/login")
async def login():
    """打开 pywebview 登录窗(子进程模式)。

    pywebview 必须在主线程跑,但本 endpoint 跑在 uvicorn asyncio loop 线程,
    不能直接调 webview.start()。改用子进程:在独立 Python 解释器的主线程
    跑 pywebview,通过临时 JSON 文件传回 cookie。

    await asyncio.to_thread 避免阻塞 event loop(否则 SSE/其他请求会卡 5 分钟)。
    """
    # 诊断:确认 worker 跑的是新代码(frozen 模式没有源码可 inspect,跳过)
    if not getattr(sys, "frozen", False):
        from pixiv_tool.auth import login_window as lw_mod
        import inspect
        lw_src = inspect.getsource(lw_mod)
        has_lambda = 'lambda' in lw_src and 'webview' in lw_src
        has_run_subprocess = 'run_login_subprocess_main' in lw_src
        logger.warning("DIAG login_window: has_lambda=%s has_run_subprocess_main=%s file=%s",
                       has_lambda, has_run_subprocess, lw_mod.__file__)
    else:
        logger.info("login: running under frozen executable (skip source diag)")
    result = await asyncio.to_thread(_spawn_login_subprocess)
    if result["status"] == "success" and result.get("cookies"):
        _store.save(result["cookies"])
        return {"status": "success", "message": "登录成功"}
    return {"status": result["status"], "message": result.get("error", "登录取消")}


def _spawn_login_subprocess() -> dict:
    """spawn 登录子进程,等结束,读 result file 返回 dict。"""
    # 临时文件让子进程写结果。NamedTemporaryFile 在 Windows 上 delete=True
    # 时不能被另一个进程打开,所以 delete=False,主进程读完手动删。
    with tempfile.NamedTemporaryFile(
        suffix=".json", prefix="pixiv-login-", delete=False
    ) as tf:
        result_path = Path(tf.name)

    try:
        # frozen(prod 打包):sys.executable 就是 pixiv-tool.exe,
        # 单入口 bootloader 不识别 -m,改用 --login-window 作为子入口分发。
        # dev:sys.executable 是 venv 里的 python.exe,走 -m 加载模块。
        if getattr(sys, "frozen", False):
            cmd = [sys.executable, "--login-window", "--result-file", str(result_path)]
        else:
            cmd = [
                sys.executable,
                "-m",
                "pixiv_tool.auth.login_window",
                "--result-file",
                str(result_path),
            ]
        proc = subprocess.run(
            cmd,
            timeout=_LOGIN_TIMEOUT_SEC,
            # 子进程的 stdout/stderr 直接丢弃。
            # 不能用默认(继承父进程)——当 uvicorn 的 stdout 被 dev.ps1 重定向到
            # 日志文件时,子进程继承的也是 pipe,而 subprocess.run 不读 pipe,
            # pywebview 大量 libpng warning 把 pipe 写满就死锁。
            # 也不能用 PIPE(必须主动读);DEVNULL 最干净。
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        logger.info("登录子进程退出码: %d", proc.returncode)
    except subprocess.TimeoutExpired:
        return {"status": "timeout", "error": f"登录超时({_LOGIN_TIMEOUT_SEC}s)"}
    except Exception as exc:  # noqa: BLE001
        logger.error("spawn 登录子进程失败: %s", exc)
        return {"status": "error", "error": f"{type(exc).__name__}: {exc}"}

    # 读 result file(子进程无论成功失败都会写)
    try:
        payload = json.loads(result_path.read_text(encoding="utf-8"))
    except Exception as exc:  # noqa: BLE001
        logger.error("读取登录结果失败(r=%s): %s", proc.returncode, exc)
        return {
            "status": "error",
            "error": f"子进程退出码 {proc.returncode},结果文件解析失败",
        }
    finally:
        try:
            result_path.unlink(missing_ok=True)
        except Exception:  # noqa: BLE001
            pass

    return payload


@router.post("/login/manual")
async def login_manual(PHPSESSID: str, csrf_token: str = ""):
    """手动提交 PHPSESSID（C 兜底）。"""
    cookies = {"PHPSESSID": PHPSESSID, "x-csrf-token": csrf_token}
    _store.save(cookies)
    return {"status": "success", "message": "Cookie 已保存"}


@router.post("/logout")
async def logout():
    """清空本地 cookie。"""
    _store.clear()
    return {"status": "success"}
