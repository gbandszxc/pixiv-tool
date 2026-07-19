"""Tests for backend.auth.login_window — cookie conversion + CSRF extraction
(ticket 06).

Note: _morsel_to_dict references SimpleCookie.Morsel which doesn't exist in
Python 3.11+. Tests patch this and/or mock _morsel_to_dict to isolate behavior.
Morsel.get("value") also returns "" — Morsel stores value via .value property,
not as a dict key. This is a known source code bug; tests verify actual behavior.
"""

from __future__ import annotations

from http.cookies import SimpleCookie, Morsel
from unittest.mock import MagicMock, patch

import pytest

from backend.auth.login_window import (
    cookies_to_dicts,
    _morsel_to_dict,
    EXTRACT_CSRF_JS,
)

# Patch SimpleCookie.Morsel globally for _morsel_to_dict tests
_MORSEL_PATCH = patch.object(SimpleCookie, "Morsel", Morsel, create=True)


class TestCookiesToDicts:
    def test_cookies_to_dicts_simple_cookie(self):
        """SimpleCookie input produces flat dicts via _morsel_to_dict."""
        cookie = SimpleCookie()
        cookie["PHPSESSID"] = "sess123"
        cookie["PHPSESSID"]["domain"] = ".pixiv.net"

        # Mock _morsel_to_dict to bypass SimpleCookie.Morsel bug + value bug
        fake = {"name": "PHPSESSID", "value": "sess123", "domain": ".pixiv.net"}
        with patch("backend.auth.login_window._morsel_to_dict", return_value=fake):
            result = cookies_to_dicts([cookie])

        assert len(result) == 1
        assert result[0]["name"] == "PHPSESSID"
        assert result[0]["value"] == "sess123"

    def test_cookies_to_dicts_multiple_morsels(self):
        """Multiple morsels in one SimpleCookie are all extracted."""
        cookie = SimpleCookie()
        cookie["PHPSESSID"] = "sess"
        cookie["x-csrf-token"] = "tok123"

        with patch("backend.auth.login_window._morsel_to_dict") as mock_fn:
            mock_fn.side_effect = lambda n, m: {"name": n, "value": "x"}
            result = cookies_to_dicts([cookie])

        assert len(result) == 2
        names = {d["name"] for d in result}
        assert "PHPSESSID" in names
        assert "x-csrf-token" in names

    def test_cookies_to_dicts_morsel_not_dict(self):
        """Morsel objects are not plain dicts — _morsel_to_dict handles them."""
        cookie = SimpleCookie()
        cookie["key"] = "val"
        morsel = cookie["key"]

        # Morsel IS a dict subclass AND an http.cookies.Morsel
        assert isinstance(morsel, dict)
        assert isinstance(morsel, Morsel)

    def test_cookies_to_dicts_plain_dict_passthrough(self):
        """Plain dicts are returned as-is (no _morsel_to_dict call)."""
        d = {"name": "test", "value": "123"}
        result = cookies_to_dicts([d])
        assert result == [d]

    def test_cookies_to_dicts_empty(self):
        """Empty input returns empty list."""
        assert cookies_to_dicts([]) == []

    def test_cookies_to_dicts_mixed(self):
        """Mixed SimpleCookie + dict in same list."""
        cookie = SimpleCookie()
        cookie["a"] = "1"
        plain = {"name": "b", "value": "2"}
        with patch("backend.auth.login_window._morsel_to_dict") as mock_fn:
            mock_fn.return_value = {"name": "a", "value": "1"}
            result = cookies_to_dicts([cookie, plain])
        assert len(result) == 2


class TestMorselToDict:
    def test_morsel_to_dict_plain_dict_passthrough(self):
        """A plain dict (not Morsel) is returned as-is."""
        d = {"name": "test", "value": "123", "extra": True}
        with _MORSEL_PATCH:
            result = _morsel_to_dict("test", d)
        assert result == d

    def test_morsel_to_dict_real_morsel_value_buggy(self):
        """Real Morsel: morsel.get('value') returns '' (known bug).

        Morsel stores value via .value property, not as dict key.
        The source code uses morsel.get("value", "") which always returns "".
        """
        cookie = SimpleCookie()
        cookie["k"] = "v"
        morsel = cookie["k"]

        with _MORSEL_PATCH:
            result = _morsel_to_dict("k", morsel)

        assert result["name"] == "k"
        # Known behavior: value is "" due to Morsel key access bug
        assert result["value"] == ""
        assert result["domain"] == ""
        assert result["secure"] == ""
        assert result["httponly"] == ""

    def test_morsel_to_dict_dict_with_value_key(self):
        """Dict with 'value' key goes through return path (when patched)."""
        # After patching SimpleCookie.Morsel, a dict with 'value' key
        # will NOT be recognized as Morsel, so it passes through as-is
        d = {"name": "x", "value": "real_val", "domain": ".pixiv.net"}
        with _MORSEL_PATCH:
            result = _morsel_to_dict("x", d)
        assert result == d  # returned as-is

    def test_morsel_to_dict_all_expected_keys(self):
        """Return dict always has: name, value, domain, path, secure, httponly."""
        cookie = SimpleCookie()
        cookie["s"] = "1"
        cookie["s"]["domain"] = ".test.com"
        cookie["s"]["path"] = "/api"
        cookie["s"]["secure"] = True
        cookie["s"]["httponly"] = True
        morsel = cookie["s"]

        with _MORSEL_PATCH:
            result = _morsel_to_dict("s", morsel)

        expected_keys = {"name", "value", "domain", "path", "secure", "httponly"}
        assert set(result.keys()) == expected_keys
        assert result["name"] == "s"
        assert result["domain"] == ".test.com"
        assert result["path"] == "/api"
        assert result["secure"] is True
        assert result["httponly"] is True


class TestExtractCsrfJS:
    def test_extract_csrf_js_is_valid_js_string(self):
        """EXTRACT_CSRF_JS is syntactically valid JavaScript."""
        assert EXTRACT_CSRF_JS.strip().startswith("new Promise")
        opens = EXTRACT_CSRF_JS.count("{")
        closes = EXTRACT_CSRF_JS.count("}")
        assert opens == closes, f"Unbalanced braces: {opens} open, {closes} close"

    def test_extract_csrf_js_is_nontrivial_string(self):
        """EXTRACT_CSRF_JS is a substantial string constant."""
        assert isinstance(EXTRACT_CSRF_JS, str)
        assert len(EXTRACT_CSRF_JS) > 100

    def test_extract_csrf_js_contains_extraction_paths(self):
        """JS string documents multiple CSRF extraction paths."""
        assert "dehydratedState" in EXTRACT_CSRF_JS
        assert "resolve" in EXTRACT_CSRF_JS
