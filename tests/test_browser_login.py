"""真实 Chromium 登录链路的最小协议回归测试。"""

from __future__ import annotations

from unittest.mock import MagicMock

import pytest

from pixiv_tool.auth import browser_login
from pixiv_tool.core.csrf import SessionProbe


def test_extract_pixiv_cookies_filters_other_domains_and_empty_values():
    cookies = browser_login.extract_pixiv_cookies(
        [
            {"name": "PHPSESSID", "value": "session", "domain": ".pixiv.net"},
            {"name": "device_token", "value": "device", "domain": "accounts.pixiv.net"},
            {"name": "_GRECAPTCHA", "value": "secret", "domain": ".recaptcha.net"},
            {"name": "empty", "value": "", "domain": ".pixiv.net"},
        ]
    )

    assert cookies == {"PHPSESSID": "session", "device_token": "device"}


def test_has_pixiv_main_target_rejects_accounts_login_page():
    assert not browser_login.has_pixiv_main_target(
        [{"type": "page", "url": "https://accounts.pixiv.net/login"}]
    )
    assert browser_login.has_pixiv_main_target(
        [{"type": "page", "url": "https://www.pixiv.net/"}]
    )


@pytest.mark.asyncio
async def test_open_browser_login_reads_cookie_then_uses_authoritative_probe(
    monkeypatch, tmp_path
):
    class FakeProcess:
        def __init__(self):
            self.terminated = False

        def poll(self):
            return None if not self.terminated else 0

        def terminate(self):
            self.terminated = True

        def kill(self):
            self.terminated = True

        def wait(self, _timeout=None):
            self.terminated = True
            return 0

    class FakeWebSocket:
        async def close(self):
            pass

    process = FakeProcess()
    websocket = FakeWebSocket()
    methods = []

    async def fake_cdp_call(_websocket, _call_id, method):
        methods.append(method)
        if method == "Target.getTargets":
            return {
                "targetInfos": [
                    {"type": "page", "url": "https://www.pixiv.net/"}
                ]
            }
        if method == "Storage.getCookies":
            return {
                "cookies": [
                    {
                        "name": "PHPSESSID",
                        "value": "browser-session",
                        "domain": ".pixiv.net",
                    }
                ]
            }
        return {}

    async def fake_probe(value):
        assert value == "browser-session"
        return SessionProbe(
            csrf_token="csrf",
            is_logged_in=True,
            user={"user_id": "1", "pixiv_id": "tester", "name": "Tester"},
        )

    async def fake_connect(_url, **_kwargs):
        return websocket

    monkeypatch.setattr(browser_login, "PROFILE_DIR", tmp_path / "profile")
    monkeypatch.setattr(browser_login, "find_login_browser", lambda: tmp_path / "chrome")
    monkeypatch.setattr(browser_login, "_free_port", lambda: 12345)
    monkeypatch.setattr(browser_login, "_wait_for_cdp", AsyncValue("ws://localhost"))
    monkeypatch.setattr(browser_login, "connect", fake_connect)
    monkeypatch.setattr(browser_login, "_cdp_call", fake_cdp_call)
    monkeypatch.setattr(browser_login, "fetch_session_probe", fake_probe)
    monkeypatch.setattr(browser_login.subprocess, "Popen", lambda *_a, **_kw: process)

    result = await browser_login.open_browser_login()

    assert result["status"] == "success"
    assert result["cookies"] == {
        "PHPSESSID": "browser-session",
        "x-csrf-token": "csrf",
    }
    assert result["user"]["pixiv_id"] == "tester"
    assert methods == ["Target.getTargets", "Storage.getCookies", "Browser.close"]


class AsyncValue:
    def __init__(self, value):
        self.value = value

    def __call__(self, *_args, **_kwargs):
        async def resolve():
            return self.value

        return resolve()
