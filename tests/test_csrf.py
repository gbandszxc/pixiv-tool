"""Session 校验与 CSRF token 获取回归测试。"""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from pixiv_tool.core.csrf import (
    PIXIV_SELF,
    CsrfExtractionError,
    InvalidSessionError,
    SessionProbe,
    fetch_session_probe,
    normalize_phpsessid,
)


def test_normalize_phpsessid_accepts_value_and_cookie_header():
    assert normalize_phpsessid(" 123_session ") == "123_session"
    assert normalize_phpsessid("foo=bar; PHPSESSID=123_session; baz=qux") == "123_session"


@pytest.mark.parametrize("value", ["", "   ", "abc;def", "abc\ndef"])
def test_normalize_phpsessid_rejects_invalid_input(value):
    with pytest.raises(InvalidSessionError):
        normalize_phpsessid(value)


def _client_with_response(response):
    client = MagicMock()
    client.get = AsyncMock(return_value=response)
    client.close = AsyncMock()
    return client


@pytest.mark.asyncio
async def test_probe_uses_self_api_and_returns_verified_user():
    response = MagicMock(status_code=200)
    response.json.return_value = {
        "token": "csrf-token",
        "userData": {
            "id": "19509348",
            "pixivId": "gbandszxc",
            "name": "测试用户",
            "profileImg": "https://example.test/avatar.png",
        },
    }
    client = _client_with_response(response)

    with patch("pixiv_tool.core.csrf.create_client", return_value=client) as factory:
        probe = await fetch_session_probe("session")

    assert probe == SessionProbe(
        csrf_token="csrf-token",
        is_logged_in=True,
        user={
            "user_id": "19509348",
            "pixiv_id": "gbandszxc",
            "name": "测试用户",
            "profile_img": "https://example.test/avatar.png",
        },
    )
    factory.assert_called_once_with({"PHPSESSID": "session"})
    client.get.assert_awaited_once_with(PIXIV_SELF, timeout=15.0)
    client.close.assert_awaited_once()

@pytest.mark.asyncio
async def test_probe_rejects_anonymous_200_even_when_token_exists():
    """Pixiv 匿名 self 接口也返回 200 + token，不能据此判定已登录。"""
    response = MagicMock(status_code=200)
    response.json.return_value = {"token": "anonymous-token", "userData": None}
    client = _client_with_response(response)

    with patch("pixiv_tool.core.csrf.create_client", return_value=client):
        with pytest.raises(InvalidSessionError, match="无效或已过期"):
            await fetch_session_probe("invalid-session")


@pytest.mark.asyncio
async def test_probe_reports_missing_token_for_verified_user():
    response = MagicMock(status_code=200)
    response.json.return_value = {"userData": {"id": "1"}}
    client = _client_with_response(response)

    with patch("pixiv_tool.core.csrf.create_client", return_value=client):
        with pytest.raises(CsrfExtractionError, match="缺少 csrf token"):
            await fetch_session_probe("session")


@pytest.mark.asyncio
async def test_probe_rejects_auth_error_and_closes_client():
    response = MagicMock(status_code=403)
    client = _client_with_response(response)

    with patch("pixiv_tool.core.csrf.create_client", return_value=client):
        with pytest.raises(InvalidSessionError):
            await fetch_session_probe("session")

    client.close.assert_awaited_once()
