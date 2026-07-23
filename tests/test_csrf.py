"""Tests for pixiv_tool.core.csrf — csrf token 提取 + session 探测。

重点验证 _extract_csrf_from_html 的正则鲁棒性:pixiv 首页 meta global-data
用单引号包裹 content(JSON 内部是双引号),正则必须正确配对引号 + 显式收尾 '>'。
"""

from __future__ import annotations

import asyncio
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from pixiv_tool.core.csrf import (
    CsrfExtractionError,
    InvalidSessionError,
    SessionProbe,
    _detect_login_state,
    _extract_csrf_from_html,
    fetch_session_probe,
)


class TestExtractCsrfFromHtml:
    def test_extracts_from_meta_global_data_single_quote(self):
        """真实结构:name 用双引号,content 用单引号包裹 JSON。"""
        html = (
            '<meta name="global-data" id="meta-global-data" '
            "content='{\"token\":\"abc123def456\",\"services\":{\"booth\":\"x\"}}'>"
        )
        assert _extract_csrf_from_html(html) == "abc123def456"

    def test_extracts_from_meta_global_data_double_quote_escaped(self):
        """双引号 content 的 HTML 实体转义形式(&quot;)。"""
        html = (
            '<meta name="global-data" content="{&quot;token&quot;:&quot;xyz789&quot;}">'
        )
        assert _extract_csrf_from_html(html) == "xyz789"

    def test_returns_none_when_no_global_data(self):
        """首页被风控拦截(无 meta global-data)时返回 None,由上层抛 CsrfExtractionError。"""
        assert _extract_csrf_from_html("<html><body>blocked</body></html>") is None

    def test_returns_none_when_token_missing(self):
        """meta 存在但 JSON 里没有 token 字段(Pixiv 改版)。"""
        html = "<meta name='global-data' content='{\"services\":{}}'>"
        assert _extract_csrf_from_html(html) is None

    def test_returns_none_when_token_not_string(self):
        """token 不是字符串(异常结构)。"""
        html = "<meta name='global-data' content='{\"token\":123}'>"
        assert _extract_csrf_from_html(html) is None

    def test_extracts_from_next_data_fallback(self):
        """meta 缺失时回退到 __NEXT_DATA__ 的 serverSerializedPreloadedState.api.token。"""
        import json as _json

        nd = {
            "props": {
                "pageProps": {
                    "serverSerializedPreloadedState": {"api": {"token": "next_fallback_tok"}}
                }
            }
        }
        html = f'<script id="__NEXT_DATA__" type="application/json">{_json.dumps(nd)}</script>'
        assert _extract_csrf_from_html(html) == "next_fallback_tok"

    def test_meta_takes_precedence_over_next_data(self):
        """两条路径都在时,meta global-data 优先。"""
        import json as _json

        nd = {"props": {"pageProps": {"serverSerializedPreloadedState": {"api": {"token": "next"}}}}}
        html = (
            f"<script id='__NEXT_DATA__'>{_json.dumps(nd)}</script>"
            "<meta name='global-data' content='{\"token\":\"meta_wins\"}'>"
        )
        assert _extract_csrf_from_html(html) == "meta_wins"


class TestDetectLoginState:
    def test_logged_in_html_has_userdata(self):
        """登录态首页 SSR 含 userData 注入。"""
        assert _detect_login_state("...\"userData\":{\"id\":\"123\"}...") is True

    def test_anonymous_html_no_userdata(self):
        """匿名首页不含 userData。"""
        assert _detect_login_state("<html>anonymous homepage</html>") is False


class TestFetchSessionProbe:
    @pytest.mark.asyncio
    async def test_strips_and_validates_empty(self):
        """空 PHPSESSID 抛 InvalidSessionError。"""
        with pytest.raises(InvalidSessionError):
            await fetch_session_probe("   ")

    @pytest.mark.asyncio
    async def test_redirect_loop_treated_as_invalid_session(self):
        """无效 PHPSESSID 触发重定向循环 → InvalidSessionError(非裸技术异常)。"""
        fake_client = MagicMock()
        fake_client.get = AsyncMock(
            side_effect=Exception("TooManyRedirects: Maximum (30) redirects followed")
        )
        fake_client.close = AsyncMock()
        with patch("pixiv_tool.core.csrf.create_client", return_value=fake_client):
            with pytest.raises(InvalidSessionError, match="无效或已过期"):
                await fetch_session_probe("bad_session")

    @pytest.mark.asyncio
    async def test_non_200_raises_invalid_session(self):
        """首页 403(风控) → InvalidSessionError。"""
        fake_resp = MagicMock()
        fake_resp.status_code = 403
        fake_client = MagicMock()
        fake_client.get = AsyncMock(return_value=fake_resp)
        fake_client.close = AsyncMock()
        with patch("pixiv_tool.core.csrf.create_client", return_value=fake_client):
            with pytest.raises(InvalidSessionError, match="HTTP 403"):
                await fetch_session_probe("some_session")

    @pytest.mark.asyncio
    async def test_success_extracts_token_and_login_state(self):
        """200 + 含 token + userData → SessionProbe(token, is_logged_in=True)。"""
        fake_resp = MagicMock()
        fake_resp.status_code = 200
        fake_resp.text = (
            '<meta name="global-data" content=\'{"token":"tok_abc","userData":{"id":"1"}}\'>'
        )
        fake_client = MagicMock()
        fake_client.get = AsyncMock(return_value=fake_resp)
        fake_client.close = AsyncMock()
        with patch("pixiv_tool.core.csrf.create_client", return_value=fake_client):
            probe = await fetch_session_probe("valid_session")
        assert isinstance(probe, SessionProbe)
        assert probe.csrf_token == "tok_abc"
        assert probe.is_logged_in is True

    @pytest.mark.asyncio
    async def test_success_anonymous_no_userdata(self):
        """200 + token 但无 userData → is_logged_in=False(token 公开,不代表登录)。"""
        fake_resp = MagicMock()
        fake_resp.status_code = 200
        fake_resp.text = '<meta name="global-data" content=\'{"token":"tok_anon"}\'>'
        fake_client = MagicMock()
        fake_client.get = AsyncMock(return_value=fake_resp)
        fake_client.close = AsyncMock()
        with patch("pixiv_tool.core.csrf.create_client", return_value=fake_client):
            probe = await fetch_session_probe("anon_session")
        assert probe.csrf_token == "tok_anon"
        assert probe.is_logged_in is False

    @pytest.mark.asyncio
    async def test_missing_token_raises_csrf_error(self):
        """200 但解析不出 token(Pixiv 改版) → CsrfExtractionError。"""
        fake_resp = MagicMock()
        fake_resp.status_code = 200
        fake_resp.text = "<html>pixiv redesigned, no token here</html>"
        fake_client = MagicMock()
        fake_client.get = AsyncMock(return_value=fake_resp)
        fake_client.close = AsyncMock()
        with patch("pixiv_tool.core.csrf.create_client", return_value=fake_client):
            with pytest.raises(CsrfExtractionError, match="未找到 csrf token"):
                await fetch_session_probe("some_session")
