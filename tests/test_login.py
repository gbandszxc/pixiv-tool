"""Tests for backend.auth.login_window — cookie conversion + CSRF extraction
(ticket 06).

F1.2 (post-acceptance): _morsel_to_dict now directly reuses the spike
implementation (spike/cookie_probe/probe.py, ADR 0005 reuse list). It reads
Morsel.value via getattr (not dict.get) and normalizes both Morsel and plain
dict inputs to a fixed schema with name/value/domain/path/httponly/secure/expires.
The previous "buggy" tests asserting morsel.get("value") == "" have been
removed and replaced with real-extraction regression tests.

F3.5-F3.7 (post-acceptance): the shallow TestExtractCsrfJS tests that only
checked brace pairing / string length / keyword membership have been replaced
with tests that pin each concrete behavior the Python side (_extract_csrf)
relies on: the three extraction paths (A/B/C), the Promise-not-async contract,
the missing-__NEXT_DATA__ guard, and the try/catch wrapper. Integration tests
feed _extract_csrf a fake window to exercise the real callback plumbing.
"""

from __future__ import annotations

from http.cookies import SimpleCookie, Morsel

import pytest

from backend.auth.login_window import (
    cookies_to_dicts,
    _morsel_to_dict,
    EXTRACT_AND_VERIFY_JS,
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
    """Verify the extraction-strategy contract of EXTRACT_AND_VERIFY_JS.

    These tests intentionally do NOT check syntactic trivia (brace pairing,
    string length) — that was the shallow-water mark called out in F3.5.
    Instead each test pins one concrete behavior the JS must exhibit so the
    Python side (extract_login_result + _evaluate_js_with_retry) can rely on it.
    """

    def test_csrf_js_implements_path_a_meta_api_client(self):
        """Path A (primary): queries[*].meta.apiClient.token.

        Pixiv's Next.js dehydrates the GraphQL client's auth token under
        meta.apiClient.token — this is the path spike probe.py confirmed.
        """
        assert "meta.apiClient.token" in EXTRACT_AND_VERIFY_JS

    def test_csrf_js_implements_path_b_state_data_token(self):
        """Path B: queries[*].state.data.token.

        After a client-side refetch, React Query stores the response under
        state.data rather than meta — the JS must cover this too.
        """
        assert "state.data.token" in EXTRACT_AND_VERIFY_JS or "state?.data?.token" in EXTRACT_AND_VERIFY_JS

    def test_csrf_js_implements_path_c_page_props_token(self):
        """Path C (fallback): pageProps.token.

        Older / unauthenticated variants expose the token directly on
        pageProps; the JS must try this when path A yields nothing.
        """
        assert "np.token" in EXTRACT_AND_VERIFY_JS or "pageProps.token" in EXTRACT_AND_VERIFY_JS

    def test_csrf_js_implements_path_d_global_data_meta(self):
        """Path D (last-resort fallback): meta[name='global-data'].

        Some pixiv page variants inject the token into a <meta> element.
        spike probe.py's EXTRACT_AND_VERIFY_JS confirms this path is needed.
        """
        assert 'global-data' in EXTRACT_AND_VERIFY_JS

    def test_csrf_js_uses_promise_not_async_function(self):
        """pywebview evaluate_js (callback mode) needs a Promise, not an async fn.

        pywebview's evaluate_js callback receives the resolved value of the
        expression. `new Promise(...)` resolves to the dict; an `async`
        function would return a Promise but pywebview may not await it.
        """
        assert "new Promise" in EXTRACT_AND_VERIFY_JS
        # No bare `async function` or `async (` definitions allowed.
        stripped = EXTRACT_AND_VERIFY_JS
        assert "async function" not in stripped
        assert "async (" not in stripped

    def test_csrf_js_uses_optional_chaining_for_next_data(self):
        """window.__NEXT_DATA__ access uses optional chaining.

        When __NEXT_DATA__ is absent, optional chaining (?.) yields undefined
        silently instead of throwing. Combined with the .catch() handler,
        this guarantees the callback always fires (ADR 0005 bug #5).
        """
        assert "?." in EXTRACT_AND_VERIFY_JS
        assert "__NEXT_DATA__" in EXTRACT_AND_VERIFY_JS

    def test_csrf_js_calls_ajax_user_self_for_login_verification(self):
        """Login state is verified via fetch('/ajax/user/self').

        The csrf token alone proves nothing — the JS must actually call
        /ajax/user/self with the token + cookies to confirm the session is
        live. This is what distinguishes 'token present' from 'logged in'.
        """
        assert "/ajax/user/self" in EXTRACT_AND_VERIFY_JS
        assert "x-csrf-token" in EXTRACT_AND_VERIFY_JS

    def test_csrf_js_always_resolves_never_rejects(self):
        """Both the success path and error path call resolve().

        _evaluate_js_with_retry waits on a threading.Event fired by the
        callback. If the JS rejected, the callback would never fire and
        we'd hit the per-attempt timeout. The .catch() must resolve().
        """
        # success path
        assert "resolve({" in EXTRACT_AND_VERIFY_JS
        # error path (.catch resolves with diagnostic)
        assert ".catch" in EXTRACT_AND_VERIFY_JS


class TestEvaluateJsWithRetry:
    """Integration test of _evaluate_js_with_retry callback handling.

    Feeds the function a fake `window` whose evaluate_js immediately invokes
    the callback. Exercises the real Python plumbing: callback parsing,
    threading.Event wait, dict-vs-string handling, retry loop.
    """

    def test_returns_dict_when_callback_fires_with_token_and_login(self):
        """Successful extraction returns the full JS result dict.

        The retry loop exits as soon as token + login.ok are both present.
        """
        from backend.auth.login_window import _evaluate_js_with_retry

        class FakeWindow:
            def evaluate_js(self, _js, callback):
                callback({
                    "token": "abc123",
                    "login": {"ok": True, "user_id": "19509348"},
                })

        result = _evaluate_js_with_retry(FakeWindow(), "/* js */")
        assert result is not None
        assert result["token"] == "abc123"
        assert result["login"]["ok"] is True

    def test_retries_when_token_not_yet_hydrated(self):
        """First attempt's token='' triggers retry (react-query not hydrated yet).

        Second attempt returns a real token → success. Mirrors spike's
        behavior when the dehydrated state isn't yet populated on the
        first loaded event.
        """
        from backend.auth.login_window import _evaluate_js_with_retry

        attempts = {"n": 0}

        class FakeWindow:
            def evaluate_js(self, _js, callback):
                attempts["n"] += 1
                if attempts["n"] == 1:
                    # token not yet hydrated
                    callback({"token": "", "login": {"ok": False}})
                else:
                    callback({"token": "tok", "login": {"ok": True}})

        result = _evaluate_js_with_retry(FakeWindow(), "/* js */", attempts=5, per_attempt_timeout=2.0)
        assert result is not None
        assert attempts["n"] == 2
        assert result["token"] == "tok"

    def test_no_retry_when_token_present_but_login_failed(self):
        """token present + login.ok=False returns immediately (no retry).

        Rationale: token present means extraction succeeded; login.ok=False
        means the session is genuinely invalid. Retrying would mask the
        real failure with a misleading 'extraction failed' error.
        """
        from backend.auth.login_window import _evaluate_js_with_retry

        attempts = {"n": 0}

        class FakeWindow:
            def evaluate_js(self, _js, callback):
                attempts["n"] += 1
                callback({"token": "tok", "login": {"ok": False, "status": 401}})

        result = _evaluate_js_with_retry(FakeWindow(), "/* js */", attempts=5, per_attempt_timeout=2.0)
        assert result is not None
        assert attempts["n"] == 1  # returned immediately, no retry
        assert result["login"]["ok"] is False

    def test_returns_none_when_all_attempts_fail(self):
        """No token + login after all retries returns None.

        extract_login_result turns this into a user-facing error.
        """
        from backend.auth.login_window import _evaluate_js_with_retry

        class FakeWindow:
            def evaluate_js(self, _js, callback):
                callback({"token": "", "login": {"ok": False}})

        result = _evaluate_js_with_retry(FakeWindow(), "/* js */", attempts=2, per_attempt_timeout=1.0)
        assert result is None

    def test_returns_none_on_callback_timeout(self):
        """If the callback never fires, retry exhausts and returns None."""
        from backend.auth.login_window import _evaluate_js_with_retry

        class FakeWindow:
            def evaluate_js(self, _js, callback):
                pass  # never invoke callback

        result = _evaluate_js_with_retry(FakeWindow(), "/* js */", attempts=2, per_attempt_timeout=0.2)
        assert result is None

    def test_returns_none_on_non_dict_callback(self):
        """Non-dict callback (string/None) is treated as failed attempt."""
        from backend.auth.login_window import _evaluate_js_with_retry

        class FakeWindow:
            def evaluate_js(self, _js, callback):
                callback("garbage string")

        result = _evaluate_js_with_retry(FakeWindow(), "/* js */", attempts=2, per_attempt_timeout=1.0)
        assert result is None


class TestExtractLoginResult:
    """extract_login_result combines cookie extraction + JS verification."""

    def test_success_returns_cookies_with_phpsessid_and_csrf(self):
        """PHPSESSID cookie + verified token → success dict."""
        from http.cookies import SimpleCookie
        from backend.auth.login_window import extract_login_result

        cookie = SimpleCookie()
        cookie["PHPSESSID"] = "19509348_session"

        class FakeWindow:
            def get_cookies(self):
                return [cookie]
            def evaluate_js(self, _js, callback):
                callback({
                    "token": "csrf_tok_123",
                    "login": {"ok": True, "user_id": "19509348"},
                })

        result = extract_login_result(FakeWindow())
        assert result["status"] == "success"
        assert result["cookies"]["PHPSESSID"] == "19509348_session"
        assert result["cookies"]["x-csrf-token"] == "csrf_tok_123"

    def test_error_when_phpsessid_missing(self):
        """No PHPSESSID cookie → error, even if JS succeeded."""
        from backend.auth.login_window import extract_login_result

        class FakeWindow:
            def get_cookies(self):
                return []
            def evaluate_js(self, _js, callback):
                callback({"token": "tok", "login": {"ok": True}})

        result = extract_login_result(FakeWindow())
        assert result["status"] == "error"
        assert "PHPSESSID" in result["error"]

    def test_error_when_js_verification_fails(self):
        """PHPSESSID present but login.ok=False → error with diagnostic."""
        from http.cookies import SimpleCookie
        from backend.auth.login_window import extract_login_result

        cookie = SimpleCookie()
        cookie["PHPSESSID"] = "sess"

        class FakeWindow:
            def get_cookies(self):
                return [cookie]
            def evaluate_js(self, _js, callback):
                callback({"token": "tok", "login": {"ok": False, "status": 401}})

        result = extract_login_result(FakeWindow())
        assert result["status"] == "error"
        assert "401" in result["error"] or "登录态验证失败" in result["error"]


class TestLoginWindowSpikeRegression:
    """End-to-end regression for ADR 0005 #4 (morsel.get('value') bug).

    Uses the exact PHPSESSID shape that the spike probe.py extracted
    during R1 verification (HttpOnly + Secure, user_id-prefixed value).
    """

    def test_login_window_cookies_extraction_with_real_spike_data(self):
        """cookies_to_dicts preserves the full PHPSESSID value.

        Before F1.2 this returned value='' because the buggy path called
        morsel.get('value', '') — Morsel's dict-view only contains
        reserved keys, so 'value' was always missing. With the spike
        implementation reused (getattr-based), the real value flows
        through unchanged.
        """
        from backend.auth.login_window import cookies_to_dicts
        from http.cookies import SimpleCookie

        sc = SimpleCookie()
        sc["PHPSESSID"] = "19509348_mZU7ZB4pwPtL1y0npggwwL1HbiMNP77S"
        sc["PHPSESSID"]["httponly"] = True
        sc["PHPSESSID"]["secure"] = True
        sc["PHPSESSID"]["domain"] = ".pixiv.net"
        sc["PHPSESSID"]["path"] = "/"

        result = cookies_to_dicts([sc])
        php = next(c for c in result if c["name"] == "PHPSESSID")

        assert php["value"] == "19509348_mZU7ZB4pwPtL1y0npggwwL1HbiMNP77S"
        assert php["httponly"] is True
        assert php["secure"] is True
        assert php["domain"] == ".pixiv.net"
