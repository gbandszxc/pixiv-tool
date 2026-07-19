"""
认证相关 API：登录状态查询、手动登录、登出。
"""

from __future__ import annotations

import logging

from fastapi import APIRouter

from backend.storage.cookies import create_cookie_store

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/auth")

_store = create_cookie_store()


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
        async with httpx.AsyncClient(timeout=10) as client:
            resp = await client.get(
                "https://www.pixiv.net/ajax/user/self?lang=zh",
                headers={
                    "x-csrf-token": cookies.get("x-csrf-token", ""),
                    "User-Agent": "Mozilla/5.0",
                    "Referer": "https://www.pixiv.net/",
                },
                cookies={k: v for k, v in cookies.items() if k != "x-csrf-token"},
            )
            if resp.status_code != 200:
                _store.clear()
                return {"is_logged_in": False}

            body = resp.json()
            user_data = body.get("body", {}).get("userData", {})
            if not user_data:
                _store.clear()
                return {"is_logged_in": False}

            return {
                "is_logged_in": True,
                "user_id": str(user_data.get("id", "")),
                "pixiv_id": user_data.get("pixivId", ""),
                "name": user_data.get("name", ""),
            }
    except Exception as exc:
        logger.warning("登录态验证失败: %s", exc)
        return {"is_logged_in": False}


@router.post("/login")
async def login():
    """打开 pywebview 登录窗。"""
    try:
        from backend.auth.login_window import open_login_window
        result = open_login_window()
        if result["status"] == "success" and result["cookies"]:
            _store.save(result["cookies"])
            return {"status": "success", "message": "登录成功"}
        return {"status": result["status"], "message": result.get("error", "登录取消")}
    except Exception as exc:
        logger.error("登录异常: %s", exc)
        return {"status": "error", "message": str(exc)}


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
