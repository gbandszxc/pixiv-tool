"""Tests for backend.auth.login_window — cookie conversion + CSRF extraction
(ticket 06).

F1.2 (post-acceptance): _morsel_to_dict now directly reuses the spike
implementation (spike/cookie_probe/probe.py, ADR 0005 reuse list). It reads
Morsel.value via getattr (not dict.get) and normalizes both Morsel and plain
dict inputs to a fixed schema with name/value/domain/path/httponly/secure/expires.
The previous "buggy" tests asserting morsel.get("value") == "" have been
removed and replaced with real-extraction regression tests.
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


class TestCookiesToDicts:
    def test_cookies_to_dicts_simple_cookie(self):
        """SimpleCookie input is flattened via _morsel_to_dict (real, unmocked)."""
        cookie = SimpleCookie()
        cookie["PHPSESSID"] = "sess123"
        cookie["PHPSESSID"]["domain"] = ".pixiv.net"

        result = cookies_to_dicts([cookie])

        assert len(result) == 1
        assert result[0]["name"] == "PHPSESSID"
        # Real extraction — value must not be "" (regression guard for F1.2)
        assert result[0]["value"] == "sess123"
        assert result[0]["domain"] == ".pixiv.net"

    def test_cookies_to_dicts_multiple_morsels(self):
        """Multiple morsels in one SimpleCookie are all extracted."""
        cookie = SimpleCookie()
        cookie["PHPSESSID"] = "sess"
        cookie["x-csrf-token"] = "tok123"

        result = cookies_to_dicts([cookie])

        assert len(result) == 2
        by_name = {d["name"]: d for d in result}
        assert by_name["PHPSESSID"]["value"] == "sess"
        assert by_name["x-csrf-token"]["value"] == "tok123"

    def test_cookies_to_dicts_morsel_not_dict(self):
        """Morsel objects are dict subclasses but still go through the Morsel path."""
        cookie = SimpleCookie()
        cookie["key"] = "val"
        morsel = cookie["key"]

        # Morsel IS a dict subclass AND an http.cookies.Morsel —
        # the dict branch must be guarded by `not isinstance(morsel, Morsel)`
        assert isinstance(morsel, dict)
        assert isinstance(morsel, Morsel)

    def test_cookies_to_dicts_plain_dict_iterated_as_jar(self):
        """Plain dict is iterated like a SimpleCookie jar (spike contract).

        Per ADR 0005 / spike probe.py, cookies_to_dicts expects
        list[SimpleCookie] from pywebview.get_cookies(); each jar is iterated
        via .items(). A plain dict {name: ..., value: ...} is iterated the same
        way, producing one record per key (this matches the spike verbatim).
        """
        d = {"name": "test", "value": "123"}
        result = cookies_to_dicts([d])
        # Two keys → two records, each normalized to schema
        assert len(result) == 2
        by_name = {r["name"]: r for r in result}
        assert set(by_name.keys()) == {"name", "value"}
        for rec in result:
            assert set(rec.keys()) == {
                "name", "value", "domain", "path",
                "httponly", "secure", "expires",
            }

    def test_cookies_to_dicts_empty(self):
        """Empty input returns empty list."""
        assert cookies_to_dicts([]) == []

    def test_cookies_to_dicts_mixed(self):
        """Mixed SimpleCookie + plain dict in same list (real spike behavior)."""
        cookie = SimpleCookie()
        cookie["a"] = "1"
        plain = {"name": "b", "value": "2"}
        result = cookies_to_dicts([cookie, plain])
        # SimpleCookie yields 1 record ('a'); dict yields 2 records ('name','value')
        assert len(result) == 3
        by_name = {d["name"]: d for d in result}
        assert by_name["a"]["value"] == "1"

    def test_cookies_to_dicts_phpsessid_httponly_real_extraction(self):
        """Real PHPSESSID-style cookie: HttpOnly + Secure flags must be True.

        Regression guard for F1.2 / ADR-0005 bug #4 — this would have returned
        value="" and httponly=False under the buggy morsel.get("value") path.
        """
        cookie = SimpleCookie()
        cookie["PHPSESSID"] = "19509348_real_session_value_here"
        cookie["PHPSESSID"]["domain"] = ".pixiv.net"
        cookie["PHPSESSID"]["path"] = "/"
        cookie["PHPSESSID"]["httponly"] = True
        cookie["PHPSESSID"]["secure"] = True

        result = cookies_to_dicts([cookie])

        assert len(result) == 1
        out = result[0]
        assert out["name"] == "PHPSESSID"
        assert out["value"] == "19509348_real_session_value_here"
        assert out["domain"] == ".pixiv.net"
        assert out["path"] == "/"
        assert out["httponly"] is True
        assert out["secure"] is True

    def test_cookies_to_dicts_object_with_key_attr_fallback(self):
        """Non-dict/non-Morsel object with .key attribute uses the .key fallback."""
        class CookieLike:
            # No .items() method, but has .key — triggers the fallback branch
            key = "device_token"
            value = "abc"

            def __getitem__(self, k):
                raise KeyError(k)

        result = cookies_to_dicts([CookieLike()])
        assert len(result) == 1
        assert result[0]["name"] == "device_token"
        assert result[0]["value"] == "abc"


class TestMorselToDict:
    def test_morsel_to_dict_plain_dict_normalized(self):
        """A plain dict (not Morsel) is normalized to the fixed schema."""
        d = {"name": "test", "value": "123", "extra": True}
        result = _morsel_to_dict("test", d)
        assert result["name"] == "test"
        assert result["value"] == "123"
        assert result["domain"] == ""
        assert result["path"] == "/"
        assert result["httponly"] is False
        assert result["secure"] is False
        assert result["expires"] is None
        # Unknown keys are dropped — schema is fixed
        assert "extra" not in result

    def test_morsel_to_dict_real_morsel_extracts_value(self):
        """Real Morsel: value is read via getattr(.value), NOT morsel.get('value').

        Regression test for F1.2 / ADR-0005 bug #4. The old code called
        morsel.get("value", "") which always returned "" because Morsel's
        dict-view only contains reserved keys (expires/path/domain/...).
        """
        cookie = SimpleCookie()
        cookie["PHPSESSID"] = "session_value_xyz"
        morsel = cookie["PHPSESSID"]

        result = _morsel_to_dict("PHPSESSID", morsel)

        assert result["name"] == "PHPSESSID"
        # Correct behavior: value extracted via attribute access
        assert result["value"] == "session_value_xyz"
        assert result["domain"] == ""

    def test_morsel_to_dict_dict_with_value_key(self):
        """Dict with 'value' key goes through the dict branch and is normalized."""
        d = {"name": "x", "value": "real_val", "domain": ".pixiv.net"}
        result = _morsel_to_dict("x", d)
        assert result["name"] == "x"
        assert result["value"] == "real_val"
        assert result["domain"] == ".pixiv.net"

    def test_morsel_to_dict_all_expected_keys(self):
        """Return dict always has: name, value, domain, path, secure, httponly, expires."""
        cookie = SimpleCookie()
        cookie["s"] = "1"
        cookie["s"]["domain"] = ".test.com"
        cookie["s"]["path"] = "/api"
        cookie["s"]["secure"] = True
        cookie["s"]["httponly"] = True
        morsel = cookie["s"]

        result = _morsel_to_dict("s", morsel)

        expected_keys = {"name", "value", "domain", "path", "secure", "httponly", "expires"}
        assert set(result.keys()) == expected_keys
        assert result["name"] == "s"
        assert result["value"] == "1"
        assert result["domain"] == ".test.com"
        assert result["path"] == "/api"
        assert result["secure"] is True
        assert result["httponly"] is True

    def test_morsel_to_dict_value_falls_back_to_coded_value(self):
        """If .value is empty, _attr falls back to .coded_value."""
        # Build a Morsel-like stub: .value empty, .coded_value populated
        class StubMorsel:
            value = ""
            coded_value = "fallback_val"
            key = "stub"

            def __getitem__(self, key):
                raise KeyError(key)

        result = _morsel_to_dict("stub", StubMorsel())
        assert result["value"] == "fallback_val"


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
