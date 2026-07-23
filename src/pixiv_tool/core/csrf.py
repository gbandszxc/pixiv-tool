"""Pixiv Session 校验与 CSRF token 获取。"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from http.cookies import CookieError, SimpleCookie

from pixiv_tool.core.http_factory import create_client

logger = logging.getLogger(__name__)

PIXIV_SELF = "https://www.pixiv.net/ajax/user/self?lang=zh"


class CsrfExtractionError(Exception):
    """登录态有效，但 Pixiv 响应中缺少 CSRF token。"""


class InvalidSessionError(Exception):
    """PHPSESSID 无效或已过期。"""


@dataclass
class SessionProbe:
    csrf_token: str
    is_logged_in: bool
    user: dict[str, str] | None = None


def normalize_phpsessid(value: str) -> str:
    """兼容纯值、``PHPSESSID=值`` 和 Cookie 请求头格式。"""
    text = value.strip()
    if len(text) > 4096:
        raise InvalidSessionError("PHPSESSID 格式不正确")
    if "PHPSESSID=" in text:
        cookies = SimpleCookie()
        try:
            cookies.load(text)
        except CookieError as exc:
            raise InvalidSessionError("PHPSESSID 格式不正确") from exc
        morsel = cookies.get("PHPSESSID")
        text = morsel.value if morsel else ""

    if not text:
        raise InvalidSessionError("PHPSESSID 不能为空")
    if any(char in text for char in "\r\n;\0"):
        raise InvalidSessionError("PHPSESSID 格式不正确")
    return text


async def fetch_session_probe(phpsessid: str) -> SessionProbe:
    """用 Pixiv 当前用户接口一次完成 Session、token 与用户信息校验。"""
    phpsessid = normalize_phpsessid(phpsessid)
    client = create_client({"PHPSESSID": phpsessid})
    try:
        resp = await client.get(PIXIV_SELF, timeout=15.0)
    except Exception as exc:
        message = str(exc)
        if "redirect" in message.lower() or "TooManyRedirects" in type(exc).__name__:
            raise InvalidSessionError("PHPSESSID 无效或已过期") from exc
        raise
    finally:
        await client.close()

    if resp.status_code in (401, 403):
        raise InvalidSessionError("PHPSESSID 无效或已过期")
    if resp.status_code != 200:
        raise InvalidSessionError(
            f"Pixiv 登录态校验失败：HTTP {resp.status_code}"
        )

    try:
        body = resp.json()
    except Exception as exc:  # noqa: BLE001
        raise CsrfExtractionError("Pixiv 登录态响应不是有效 JSON") from exc

    user_data = body.get("userData")
    if not isinstance(user_data, dict) or not user_data.get("id"):
        # Chrome 实测：匿名 /ajax/user/self 同样返回 HTTP 200 和 token，
        # 只有 userData 能证明 PHPSESSID 真正有效。
        raise InvalidSessionError("PHPSESSID 无效或已过期")

    token = body.get("token")
    if not isinstance(token, str) or not token:
        raise CsrfExtractionError("登录态有效，但 Pixiv 响应缺少 csrf token")

    user = {
        "user_id": str(user_data.get("id", "")),
        "pixiv_id": str(user_data.get("pixivId", "")),
        "name": str(user_data.get("name", "")),
        "profile_img": str(
            user_data.get("profileImg") or user_data.get("profileImgBig") or ""
        ),
    }
    logger.info("Session 校验成功：user_id=%s", user["user_id"])
    return SessionProbe(csrf_token=token, is_logged_in=True, user=user)
