"""
pywebview 登录窗 — 子进程模式。

pywebview 的 GUI 事件循环必须在主线程跑(Windows EdgeChromium/WinForms
硬约束)。但 FastAPI 的 endpoint 跑在 uvicorn 的 asyncio loop 线程里,
不能直接调 `webview.start()`。

解法:把登录窗拆成独立 Python 子进程,在子进程的主线程跑 pywebview。
主进程通过临时 JSON 文件接收结果。协议:

    python -m pixiv_tool.auth.login_window --result-file <path>

退出码:
    0 = 登录成功,result file 含 {"status":"success","cookies":{...}}
    1 = 用户关窗取消,result file 含 {"status":"cancelled"}
    2 = 异常,result file 含 {"status":"error","error":"..."}

pywebview 调用模式直接复用 spike/cookie_probe/probe.py(ADR 0005 复用清单):
    - 主线程 webview.start(private_mode=False, http_server=True, http_port=17729)
    - loaded 事件防重入(state["extracted"] 闭包标志)
    - EXTRACT_AND_VERIFY_JS 一次完成 csrf + /ajax/user/self 登录验证
    - evaluate_js callback 模式 + threading.Event 同步包装,5 次重试
"""

from __future__ import annotations

import argparse
import json
import logging
import sys
import threading
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

LOGIN_URL = "https://accounts.pixiv.net/login"
TARGET_HOST = "www.pixiv.net"
HTTP_PORT = 17729  # spike 实测可跨会话持久化 cookie 的固定端口

# ====================================================================
# JS:csrf 提取 + 登录态验证(一次 evaluate_js 完成)
# ====================================================================
# 复刻自 spike/cookie_probe/probe.py 的 EXTRACT_AND_VERIFY_JS。
# 4 路径 csrf 兜底 + fetch /ajax/user/self 验证 cookie 真生效。
#
# csrf token 藏在(ADR 0005 bug #2):
#   __NEXT_DATA__.props.pageProps.dehydratedState.queries[*].meta.apiClient.token
# 不是 pageProps.token(空),也不是 state.data.token(react-query 刷新后消失)。

EXTRACT_AND_VERIFY_JS = """
new Promise((resolve) => {
    const mask = (s) => s ? s.slice(0, 12) + '...' : '';

    // ---- 1. 提取 csrf token(5 路径兜底)----
    // react-query hydrate 是异步的,loaded 事件触发时数据可能还没就绪。
    // 用短 polling 等到 token 出现，避免登录成功后仍被 CSRF 兜底长时间阻塞。
    //
    // 路径优先级(2026-07-21 chrome-devtools 实测):
    //   E(当前主路径):serverSerializedPreloadedState 的 JSON 字符串里 .api.token
    //   A(spike 2026-07-19 主路径,现已失效):queries[*].meta.apiClient.token
    //   B(spike fallback):queries[*].state.data.token
    //   C:pageProps.token
    //   D:meta[name="global-data"]
    function extractToken() {
        const np = window.__NEXT_DATA__?.props?.pageProps || {};
        const dehydrated = np.dehydratedState;

        // 路径 E(当前主路径,2026-07-21):serverSerializedPreloadedState.api.token
        // pixiv 改版后 csrf token 从 react-query meta 移到了服务端预加载状态字符串
        const serialized = np.serverSerializedPreloadedState;
        if (typeof serialized === 'string') {
            try {
                const parsed = JSON.parse(serialized);
                if (parsed?.api?.token) {
                    return { token: parsed.api.token, source: 'serverSerializedPreloadedState.api.token' };
                }
            } catch (e) {}
        }

        // 路径 A(spike 2026-07-19 主路径):queries[*].meta.apiClient.token
        if (dehydrated?.queries) {
            for (const q of dehydrated.queries) {
                const t = q.meta?.apiClient?.token;
                if (t) return { token: t, source: 'queries.meta.apiClient.token' };
            }
        }
        // 路径 B:queries[*].state.data.token
        if (dehydrated?.queries) {
            for (const q of dehydrated.queries) {
                const t = q.state?.data?.token;
                if (t) return { token: t, source: 'queries.state.data.token' };
            }
        }
        // 路径 C:旧 pageProps.token
        if (np.token) return { token: np.token, source: 'pageProps.token' };
        // 路径 D:meta[name="global-data"]
        const metaEl = document.querySelector('meta[name="global-data"]');
        if (metaEl) {
            try {
                const data = JSON.parse(metaEl.content || '{}');
                if (data.token) return { token: data.token, source: 'meta.global-data' };
            } catch (e) {}
        }
        return null;
    }

    const tokenSources = [];
    let token = '';
    const pollingStart = Date.now();
    const POLLING_TIMEOUT_MS = 1500;
    const POLLING_INTERVAL_MS = 100;

    function pollToken() {
        const found = extractToken();
        if (found) {
            token = found.token;
            tokenSources.push(found.source);
            verifyLogin();
            return;
        }
        if (Date.now() - pollingStart > POLLING_TIMEOUT_MS) {
            // token 超时未拿到,仍继续验证登录态(/ajax/user/self 不需要 token)
            verifyLogin();
            return;
        }
        setTimeout(pollToken, POLLING_INTERVAL_MS);
    }

    // ---- 2. 验证登录态(异步 fetch)----
    function verifyLogin() {
        fetch('/ajax/user/self?lang=zh', {
            credentials: 'include',
            headers: token ? { 'x-csrf-token': token } : {},
        }).then(r => r.text().then(text => ({ status: r.status, text }))).then(({ status, text }) => {
            let data = null;
            try { data = JSON.parse(text); } catch (e) {}
            let loginInfo;
            // /ajax/user/self 返回顶层就是 {userData: {...}}(spike probe.py 实测),
            // 不是 {body: {userData: ...}}。注意和后端 auth_status 里 httpx 路径不同。
            if (data && data.userData) {
                loginInfo = {
                    ok: true,
                    status: status,
                    user_id: data.userData.id || '',
                    pixiv_id: data.userData.pixivId || '',
                    name: data.userData.name || '',
                    profile_img: data.userData.profileImg || data.userData.profileImgBig || '',
                };
            } else {
                loginInfo = {
                    ok: false,
                    status: status,
                    body_preview: text.slice(0, 200),
                };
            }
            resolve({
                url: location.href,
                token: token,
                token_sources_tried: tokenSources,
                login: loginInfo,
            });
        }).catch(err => {
            resolve({
                url: location.href,
                token: token,
                token_sources_tried: tokenSources,
                login: { ok: false, error: String(err) },
            });
        });
    }

    pollToken();
});
"""


# ====================================================================
# Cookie 序列化(已正确复用 spike,保留现状)
# ====================================================================


def cookies_to_dicts(cookies: Any) -> list[dict[str, Any]]:
    """把 window.get_cookies() 返回值统一成 dict 列表。

    pywebview 返回 list[SimpleCookie](每个元素本身是一个 dict-like 容器,
    可能含一个或多个 Morsel)。必须遍历每个 SimpleCookie.items() 取出
    (name, Morsel) 对。直接读 SimpleCookie 上的属性会拿到空值。

    复刻自 spike/cookie_probe/probe.py(ADR 0005 复用清单)。
    """
    flat: list[dict[str, Any]] = []
    for jar in cookies:
        items: list[tuple[str, Any]] = []
        if hasattr(jar, "items") and callable(getattr(jar, "items")):
            try:
                items = list(jar.items())
            except Exception:  # noqa: BLE001
                items = []
        if not items:
            key = getattr(jar, "key", None)
            if key:
                items = [(key, jar)]

        for name, morsel in items:
            flat.append(_morsel_to_dict(name, morsel))
    return flat


def _morsel_to_dict(name: str, morsel: Any) -> dict[str, Any]:
    """Morsel → 扁平 dict。

    关键:Morsel 继承自 dict,但 dict-view 里没有 "value" 键。
    必须用 getattr(morsel, "value") 而非 morsel.get("value")(ADR 0005 bug #4)。
    """
    from http.cookies import Morsel

    if isinstance(morsel, dict) and not isinstance(morsel, Morsel):
        return {
            "name": name or morsel.get("name", ""),
            "value": morsel.get("value", ""),
            "domain": morsel.get("domain", ""),
            "path": morsel.get("path", "/"),
            "httponly": bool(morsel.get("httponly", False)),
            "secure": bool(morsel.get("secure", False)),
            "expires": morsel.get("expires"),
        }

    def _attr(key: str) -> str:
        for k in (key, f"_{key}"):
            try:
                v = getattr(morsel, k, "")
                if v:
                    return str(v)
            except Exception:  # noqa: BLE001
                continue
        return ""

    value = _attr("value") or _attr("coded_value")

    def _reserved_str(key: str) -> str:
        try:
            v = morsel[key]
        except (KeyError, TypeError, AttributeError):
            return ""
        return str(v) if v is not None else ""

    def _reserved_bool(key: str) -> bool:
        v = _reserved_str(key)
        return v.lower() in ("true", "1", "yes", key)

    expires = _reserved_str("expires") or None

    return {
        "name": name,
        "value": value,
        "domain": _reserved_str("domain"),
        "path": _reserved_str("path") or "/",
        "httponly": _reserved_bool("httponly"),
        "secure": _reserved_bool("secure"),
        "expires": expires,
    }


# ====================================================================
# 核心:在 window 活跃时提取 cookie + csrf + 登录态
# ====================================================================


def _evaluate_js_with_retry(window: Any, script: str, attempts: int = 2, per_attempt_timeout: float = 3.0) -> dict | None:
    """evaluate_js callback 模式 + threading.Event 同步包装。

    pywebview 同步 evaluate_js 不 await Promise(ADR 0005 bug #5),
    必须 callback 模式。当前 Pixiv 页面会在 JS 内短轮询 token；Python 端
    仅补两次短尝试，避免登录成功后的信息回显被 CSRF 兜底长时间阻塞。

    重试规则:
        - callback 超时/非 dict 返回 → 重试(可能是网络或 pywebview 内部时序问题)
        - token 还没拿到 → 重试(react-query 可能还没 hydrate)
        - token 拿到了但 login.ok=False → **立即返回**(不重试,登录态真失败)
        - token 拿到了且 login.ok=True → 立即返回(成功)
    """
    import time

    last_diag = ""
    for attempt in range(attempts):
        done = threading.Event()
        box: dict[str, Any] = {}

        def _cb(res: Any) -> None:
            box["result"] = res
            done.set()

        window.evaluate_js(script, callback=_cb)

        if not done.wait(timeout=per_attempt_timeout):
            last_diag = f"attempt {attempt + 1}: callback 超时未触发"
            continue

        js_result = box.get("result")
        if not isinstance(js_result, dict):
            last_diag = f"attempt {attempt + 1}: callback 返回 {type(js_result).__name__}"
            time.sleep(1)
            continue

        token = js_result.get("token", "")
        login_info = js_result.get("login", {})
        login_ok = login_info.get("ok", False)

        # token 已拿到 → 不管 login 是否 ok 都返回(避免重试掩盖真失败)
        if token:
            return js_result

        last_diag = (
            f"attempt {attempt + 1}: token=空, "
            f"login_ok={login_ok}, "
            f"sources={js_result.get('token_sources_tried')}, "
            f"login={login_info}"
        )
        time.sleep(1)

    logger.warning("JS 提取重试 %d 次均未拿到 token,最后诊断: %s", attempts, last_diag)
    return None


def extract_login_result(window: Any) -> dict[str, Any]:
    """从活跃 window 提取 cookie + csrf + 登录态,返回结构化结果。

    返回:
        {"status": "success", "cookies": {...}} 成功
        {"status": "error", "error": "..."} 失败
    """
    raw_cookies = window.get_cookies()
    cookie_dicts = cookies_to_dicts(raw_cookies)
    cookie_map = {c["name"]: c["value"] for c in cookie_dicts if c.get("value")}

    phpsessid = cookie_map.get("PHPSESSID")
    if not phpsessid:
        return {"status": "error", "error": "登录成功但未找到 PHPSESSID"}

    js_result = _evaluate_js_with_retry(window, EXTRACT_AND_VERIFY_JS)
    if not js_result:
        return {"status": "error", "error": "csrf token 提取失败(重试 5 次均未拿到 token,react-query 可能未 hydrate)"}

    csrf_token = js_result.get("token", "")
    login = js_result.get("login", {})
    if not login.get("ok"):
        # token 拿到了但 /ajax/user/self 验证失败——cookie 无效或会话过期
        return {
            "status": "error",
            "error": (
                f"登录态验证失败: status={login.get('status')}, "
                f"body={login.get('body_preview', '')[:120]}"
            ),
        }

    # 组装 cookie dict(主进程会 DPAPI 加密落盘)
    cookies = {
        "PHPSESSID": phpsessid,
        "x-csrf-token": csrf_token,
        **{k: v for k, v in cookie_map.items() if k != "PHPSESSID"},
    }
    return {
        "status": "success",
        "cookies": cookies,
        "user": {
            "user_id": str(login.get("user_id", "")),
            "pixiv_id": login.get("pixiv_id", ""),
            "name": login.get("name", ""),
            "profile_img": login.get("profile_img", ""),
        },
    }


# ====================================================================
# 子进程主入口
# ====================================================================


def _write_result(result_file: Path, payload: dict) -> None:
    """把结果 JSON 写到 result_file(原子写,避免主进程读到半成品)。"""
    tmp = result_file.with_suffix(result_file.suffix + ".tmp")
    tmp.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
    tmp.replace(result_file)


def run_login_subprocess_main(result_file: str) -> int:
    """子进程主入口:在主线程跑 pywebview,登录后写 result_file。

    Returns:
        0=success, 1=cancelled(用户关窗), 2=error
    """
    import webview  # 子进程主线程,合法

    result_path = Path(result_file)
    state: dict[str, Any] = {"extracted": False}  # loaded 防重入标志
    exit_code = {"value": 1}  # 默认 cancelled(用户关窗)

    def on_loaded(window: Any) -> None:
        url = window.get_current_url() or ""
        if TARGET_HOST not in url:
            return  # 还没跳到 www.pixiv.net,等下次 loaded
        if state["extracted"]:
            return  # 已经提取过,避免重复(spike 模式)
        state["extracted"] = True

        try:
            result = extract_login_result(window)
        except Exception as exc:  # noqa: BLE001
            result = {"status": "error", "error": f"{type(exc).__name__}: {exc}"}

        _write_result(result_path, result)
        exit_code["value"] = 0 if result["status"] == "success" else 2

        # 给主进程一点时间确认文件落盘,然后关窗退出
        threading.Timer(0.5, lambda: window.destroy()).start()

    def on_closed() -> None:
        # 用户主动关窗,exit_code 保持默认 1(除非 on_loaded 已改成 0/2)
        pass

    try:
        window = webview.create_window(
            "登录 Pixiv",
            LOGIN_URL,
            width=960,
            height=720,
        )
        window.events.loaded += on_loaded
        window.events.closed += on_closed

        # 主线程阻塞跑 pywebview(spike 模式,合法)
        webview.start(
            private_mode=False,
            http_server=True,
            http_port=HTTP_PORT,
        )
    except Exception as exc:  # noqa: BLE001
        _write_result(result_path, {"status": "error", "error": f"{type(exc).__name__}: {exc}"})
        return 2

    return exit_code["value"]


def _parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="pixiv_tool.auth.login_window")
    parser.add_argument(
        "--result-file",
        required=True,
        help="登录结果 JSON 写入路径(主进程提供临时文件)",
    )
    return parser.parse_args(argv)


if __name__ == "__main__":
    args = _parse_args(sys.argv[1:])
    sys.exit(run_login_subprocess_main(args.result_file))
