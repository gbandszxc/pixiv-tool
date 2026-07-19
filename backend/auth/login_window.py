"""
pywebview 登录窗 — 复用 spike/cookie_probe/probe.py 成果。
弹出独立窗口加载 Pixiv 登录页，登录成功后提取 cookie + csrf token。
"""

from __future__ import annotations

import json
import logging
import threading
from typing import Any
from http.cookies import SimpleCookie

logger = logging.getLogger(__name__)

LOGIN_URL = "https://accounts.pixiv.net/login"
TARGET_HOST = "www.pixiv.net"

# csrf token 提取 JS — 多路径兜底（A/B/C/D），见 ADR 0005
EXTRACT_CSRF_JS = """
new Promise(function(resolve, reject) {
    try {
        var nd = window.__NEXT_DATA__;
        if (!nd) { resolve({csrf: "", loggedIn: false}); return; }
        var pp = nd.props && nd.props.pageProps;
        if (!pp) { resolve({csrf: "", loggedIn: false}); return; }

        // 路径 A: dehydratedState.queries[*].meta.apiClient.token
        var ds = pp.dehydratedState;
        if (ds && ds.queries) {
            for (var i = 0; i < ds.queries.length; i++) {
                var meta = ds.queries[i] && ds.queries[i].meta;
                if (meta && meta.apiClient && meta.apiClient.token) {
                    resolve({csrf: meta.apiClient.token, loggedIn: true});
                    return;
                }
            }
        }
        // 路径 B: 直接 pageProps.token
        if (pp.token) {
            resolve({csrf: pp.token, loggedIn: true});
            return;
        }
        // 路径 C: state.data.token
        if (ds && ds.queries) {
            for (var j = 0; j < ds.queries.length; j++) {
                var sd = ds.queries[j] && ds.queries[j].state && ds.queries[j].state.data;
                if (sd && sd.token) {
                    resolve({csrf: sd.token, loggedIn: true});
                    return;
                }
            }
        }
        resolve({csrf: "", loggedIn: false});
    } catch(e) { resolve({csrf: "", loggedIn: false}); }
});
"""


def cookies_to_dicts(cookies: Any) -> list[dict[str, Any]]:
    """把 window.get_cookies() 返回值统一成 dict 列表。"""
    flat: list[dict[str, Any]] = []
    for cookie in cookies:
        if isinstance(cookie, SimpleCookie):
            for name, morsel in cookie.items():
                flat.append(_morsel_to_dict(name, morsel))
        elif isinstance(cookie, dict):
            flat.append(cookie)
    return flat


def _morsel_to_dict(name: str, morsel: Any) -> dict[str, Any]:
    """把 SimpleCookie.Morsel 转成扁平 dict。"""
    # Morsel 继承自 dict，必须显式排除
    if isinstance(morsel, dict) and not isinstance(morsel, SimpleCookie.Morsel):
        return morsel
    return {
        "name": name,
        "value": morsel.get("value", ""),
        "domain": morsel.get("domain", ""),
        "path": morsel.get("path", ""),
        "secure": morsel.get("secure", False),
        "httponly": morsel.get("httponly", False),
    }


def open_login_window() -> dict:
    """
    弹出 pywebview 登录窗口。
    返回 {"status": "success"|"cancelled"|"error", "cookies": {...}, "error": "..."}
    """
    import webview  # 懒导入：仅调用时加载 pywebview

    result: dict[str, Any] = {"status": "cancelled", "cookies": None, "error": ""}
    event = threading.Event()

    def on_loaded(window: Any) -> None:
        url = window.get_current_url()
        if TARGET_HOST not in url:
            return

        # 提取 cookies
        raw_cookies = window.get_cookies()
        cookie_dicts = cookies_to_dicts(raw_cookies)
        cookie_map = {c["name"]: c["value"] for c in cookie_dicts}

        phpsessid = cookie_map.get("PHPSESSID")
        if not phpsessid:
            result["status"] = "error"
            result["error"] = "登录成功但未找到 PHPSESSID"
            event.set()
            return

        # 提取 csrf token（callback 模式）
        csrf_result = _extract_csrf(window)
        csrf_token = csrf_result.get("csrf", "")

        result["status"] = "success"
        result["cookies"] = {
            "PHPSESSID": phpsessid,
            "x-csrf-token": csrf_token,
            **{k: v for k, v in cookie_map.items() if k != "PHPSESSID"},
        }
        event.set()

    window = webview.create_window(
        "登录 Pixiv",
        LOGIN_URL,
        width=960,
        height=720,
    )
    window.events.loaded += on_loaded

    def _on_closed() -> None:
        event.set()

    window.events.closed += _on_closed

    # 非阻塞启动：在独立线程中运行 pywebview
    webview_thread = threading.Thread(
        target=lambda: webview.start(private_mode=False, http_server=True),
        daemon=True,
    )
    webview_thread.start()
    event.wait(timeout=300)  # 5 分钟超时

    window.destroy()
    return result


def _extract_csrf(window: Any) -> dict:
    """用 callback 模式提取 csrf token。"""
    import webview  # noqa: WPS433

    result: dict = {}
    event = threading.Event()

    def callback(js_result: Any) -> None:
        nonlocal result
        if isinstance(js_result, str):
            try:
                result = json.loads(js_result)
            except json.JSONDecodeError:
                result = {"csrf": "", "loggedIn": False}
        elif isinstance(js_result, dict):
            result = js_result
        event.set()

    window.evaluate_js(EXTRACT_CSRF_JS, callback=callback)
    event.wait(timeout=10)
    return result
