"""
Spike · R1 风险验证：pywebview 能否读取 HttpOnly 的 PHPSESSID cookie
====================================================================

关联：SPEC §4.1 / ADR 0002 / 风险登记 R1

背景：
    Pixiv Web API 需要 PHPSESSID（HttpOnly，JS document.cookie 读不到）+ x-csrf-token。
    旧 PowerShell 脚本靠 playwright-cli persistent profile 复用浏览器 cookie；
    本工具改走 pywebview 嵌入式 WebView2 登录窗。必须先验证 pywebview 的
    window.get_cookies() 能不能把 HttpOnly cookie 拿出来——拿不到就要退 C 方案
    （手动粘 PHPSESSID）。

设计（v2 · 自动提取）：
    上一版用两个窗口（登录窗 + 控制面板），控制面板加载内嵌 HTML 时与
    http_server=True 冲突报 404。改为单窗口 + loaded 事件自动探测：
    1. 单窗口加载 https://accounts.pixiv.net/login
    2. loaded 事件每次触发时检查 URL
    3. URL 跳到 www.pixiv.net 视为登录成功，自动提取 cookie + csrf
    4. 结果打印到终端（用户复制回贴）+ 写 result.json

运行：
    uv run python probe.py
"""

from __future__ import annotations

import json
import threading
from dataclasses import dataclass, asdict, field
from pathlib import Path
from typing import Any

import webview


# ====================================================================
# 配置
# ====================================================================

LOGIN_URL = "https://accounts.pixiv.net/login"
TARGET_HOST = "www.pixiv.net"
RESULT_FILE = Path(__file__).parent / "result.json"
HTTP_PORT = 17729


# ====================================================================
# 结果记录
# ====================================================================


@dataclass
class ProbeResult:
    """spike 验证矩阵的数据载体，对应 README.md 的验证点表格。"""

    get_cookies_return_type: str = ""
    cookies_count: int = 0
    phpsessid_found: bool = False
    phpsessid_is_httponly: bool | None = None
    phpsessid_is_secure: bool | None = None
    phpsessid_value_preview: str = ""  # 仅前 8 位
    device_token_found: bool = False
    privacy_policy_agreement_found: bool = False
    persisted_across_read: bool | None = None
    current_url: str = ""
    csrf_token_found: bool = False
    csrf_token_preview: str = ""
    # 验证点 7：通过 /ajax/user/self 验证 cookie 是否真的有效
    is_logged_in: bool = False
    user_id: str = ""
    user_pixiv_id: str = ""
    user_name: str = ""
    cookies_detail: list[dict[str, Any]] = field(default_factory=list)
    error: str = ""


# ====================================================================
# 浏览器端 JS：一次性提取 csrf token + 验证登录态
# ====================================================================
# 经 chrome-devtools 实测（2026-07-19），pixiv 新版 SPA 的 csrf token 藏在：
#   __NEXT_DATA__.props.pageProps.dehydratedState.queries[*].meta.apiClient.token
# 注意不是 .state.data.token——react-query 跑一会儿后 state.data 会被刷新，
# 但 meta 字段是稳定的（pixiv apiClient 自定义塞进去的）。
#
# 登录态验证：fetch /ajax/user/self 带 x-csrf-token + cookie，返回 userData 即登录。
#
# 单次 JS 调用完成两件事，避免多次 evaluate_js 往返 + 时序问题。

EXTRACT_AND_VERIFY_JS = """
new Promise((resolve) => {
    const mask = (s) => s ? s.slice(0, 12) + '...' : '';

    // ---- 1. 提取 csrf token（多路径兜底）----
    const np = window.__NEXT_DATA__?.props?.pageProps || {};
    const dehydrated = np.dehydratedState;

    let token = '';
    const tokenSources = [];

    // 路径 A（新版主路径）：queries[*].meta.apiClient.token
    if (dehydrated?.queries) {
        for (const q of dehydrated.queries) {
            const t = q.meta?.apiClient?.token;
            if (t) {
                token = t;
                tokenSources.push('queries.meta.apiClient.token');
                break;
            }
        }
    }

    // 路径 B：queries[*].state.data.token（旧 dehydrated 数据，刷新前）
    if (!token && dehydrated?.queries) {
        for (const q of dehydrated.queries) {
            const t = q.state?.data?.token;
            if (t) {
                token = t;
                tokenSources.push('queries.state.data.token');
                break;
            }
        }
    }

    // 路径 C：旧 pageProps.token
    if (!token) {
        const t = np.token || '';
        if (t) {
            token = t;
            tokenSources.push('pageProps.token');
        }
    }

    // 路径 D：meta[name="global-data"]
    if (!token) {
        const metaEl = document.querySelector('meta[name="global-data"]');
        if (metaEl) {
            try {
                const data = JSON.parse(metaEl.content || '{}');
                if (data.token) {
                    token = data.token;
                    tokenSources.push('meta.global-data');
                }
            } catch (e) {}
        }
    }

    // ---- 2. 验证登录态（异步 fetch）----
    fetch('/ajax/user/self?lang=zh', {
        credentials: 'include',
        headers: token ? { 'x-csrf-token': token } : {},
    }).then(r => r.text().then(text => ({ status: r.status, text }))).then(({ status, text }) => {
        let data = null;
        try { data = JSON.parse(text); } catch (e) {}
        let loginInfo;
        if (data && data.userData) {
            loginInfo = {
                ok: true,
                status: status,
                user_id: data.userData.id || '',
                pixiv_id: data.userData.pixivId || '',
                name: data.userData.name || '',
            };
        } else {
            loginInfo = {
                ok: false,
                status: status,
                body_preview: text.slice(0, 200),
                js_cookie_count: document.cookie ? document.cookie.split(';').length : 0,
            };
        }
        resolve({
            url: location.href,
            token_preview: mask(token),
            token_sources_tried: tokenSources,
            has_dehydrated: Boolean(dehydrated),
            dehydrated_query_count: dehydrated?.queries?.length || 0,
            login: loginInfo,
        });
    }).catch(err => {
        resolve({
            url: location.href,
            token_preview: mask(token),
            token_sources_tried: tokenSources,
            has_dehydrated: Boolean(dehydrated),
            dehydrated_query_count: dehydrated?.queries?.length || 0,
            login: { ok: false, error: String(err) },
        });
    });
});
"""


# ====================================================================
# Cookie 处理工具
# ====================================================================


def cookies_to_dicts(cookies: Any) -> list[dict[str, Any]]:
    """把 window.get_cookies() 返回值统一成 dict 列表。

    pywebview 返回 list[SimpleCookie]（每个元素本身是一个 dict-like 容器，
    可能含一个或多个 Morsel）。必须遍历每个 SimpleCookie.items() 取出
    (name, Morsel) 对。直接读 SimpleCookie 上的属性会拿到空值。

    参考：
        - https://pywebview.flowrl.com/api/  window.get_cookies()
        - https://docs.python.org/3/library/http.cookies.html  Morsel
    """
    flat: list[dict[str, Any]] = []
    for jar in cookies:
        # 每个 jar 可能是 SimpleCookie（dict-like）、dict、或单个 Morsel
        items: list[tuple[str, Any]] = []
        if hasattr(jar, "items") and callable(getattr(jar, "items")):
            # SimpleCookie 或 dict —— 拿到 (name, morsel) 对
            try:
                items = list(jar.items())
            except Exception:  # noqa: BLE001
                items = []
        if not items:
            # 退化：jar 本身可能是 Morsel
            key = getattr(jar, "key", None)
            if key:
                items = [(key, jar)]

        for name, morsel in items:
            flat.append(_morsel_to_dict(name, morsel))
    return flat


def _morsel_to_dict(name: str, morsel: Any) -> dict[str, Any]:
    """把一个 SimpleCookie.Morsel（或 dict）转成扁平 dict。

    Morsel 的属性访问规则（实测自 pywebview 5.4 + Python 3.13）：
        - 直接属性：.key / .value / .coded_value（_value 是底层存储）
        - dict-style（reserved）：morsel['httponly'] / morsel['secure'] /
          morsel['expires'] / morsel['domain'] / morsel['path']
          返回字符串，"httponly" 键返回 "True"/"False" 字符串
        - repr 形如：<Morsel: PHPSESSID=xxx; Domain=.pixiv.net; HttpOnly; Secure>

    所有访问都包 try/except，单个失败不连累整个 dict。
    """
    # ---- dict 路径（pywebview 偶尔返回 dict）----
    # 注意：SimpleCookie.Morsel 继承自 dict，所以必须先排除 Morsel
    # 否则所有 Morsel 都走 dict 路径，而 Morsel 的 dict-view 里没有 "value" 键
    # （只有 expires/path/domain/httponly 等 reserved 键），会读出空 value。
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

    # ---- Morsel 路径：value 取直接属性，多重 fallback ----
    def _attr(key: str) -> str:
        # 直接属性优先，然后试 coded_value，最后试 _value
        for k in (key, f"_{key}"):
            try:
                v = getattr(morsel, k, "")
                if v:
                    return str(v)
            except Exception:  # noqa: BLE001
                continue
        return ""

    value = _attr("value") or _attr("coded_value")

    # ---- dict-style reserved 属性 ----
    def _reserved_str(key: str) -> str:
        try:
            v = morsel[key]
        except (KeyError, TypeError, AttributeError):
            return ""
        return str(v) if v is not None else ""

    def _reserved_bool(key: str) -> bool:
        v = _reserved_str(key)
        # Morsel 里 reserved 标志可能存 "True"/"False" 字符串，也可能空
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


def mask_sensitive(d: dict[str, Any]) -> dict[str, Any]:
    """脱敏：PHPSESSID 只留前 8 位，避免完整泄漏到 result.json。"""
    if d.get("name") == "PHPSESSID" and d.get("value"):
        d = dict(d)
        d["value"] = d["value"][:8] + "...(masked)"
    return d


# ====================================================================
# 核心探测逻辑
# ====================================================================


def run_probe(window: webview.Window) -> ProbeResult:
    """从活跃 window 提取 cookie + csrf，返回 ProbeResult。

    必须在 window 活跃时调用——关闭后再调会抛 KeyError: 'master'。
    """
    result = ProbeResult()

    # ---- 验证点 1/2/3/5：cookie ----
    raw_cookies = window.get_cookies()
    result.get_cookies_return_type = type(raw_cookies).__name__
    result.current_url = window.get_current_url() or ""

    details = cookies_to_dicts(raw_cookies)
    for detail in details:
        name = detail["name"]
        if name == "PHPSESSID":
            result.phpsessid_found = True
            result.phpsessid_is_httponly = detail.get("httponly")
            result.phpsessid_is_secure = detail.get("secure")
            val = detail.get("value", "")
            result.phpsessid_value_preview = val[:8] + "..." if val else ""
        elif name == "device_token":
            result.device_token_found = True
        elif name == "privacy_policy_agreement":
            result.privacy_policy_agreement_found = True

    result.cookies_count = len(details)
    result.cookies_detail = [mask_sensitive(d) for d in details]

    # ---- 验证点 6+7：csrf token + 登录态（合并一次 JS 调用，callback 模式）----
    # pywebview 的 evaluate_js 同步模式不会 await Promise，async 函数返回 None。
    # 必须用 callback 模式：传 callback，Promise resolve 后会回调我们。
    # 用 threading.Event 把异步回调转成同步阻塞，方便在 loaded handler 里继续处理。
    import time

    try:
        js_result = None
        last_diag = ""
        for attempt in range(5):
            done = threading.Event()
            box: dict[str, Any] = {}

            def _cb(res: Any) -> None:
                box["result"] = res
                done.set()

            window.evaluate_js(EXTRACT_AND_VERIFY_JS, callback=_cb)

            # 等待 Promise resolve，最长 8 秒（fetch + 网络可能慢）
            if not done.wait(timeout=8):
                last_diag = f"attempt {attempt+1}: callback 超时未触发"
                continue

            js_result = box.get("result")
            if not isinstance(js_result, dict):
                last_diag = (
                    f"attempt {attempt+1}: callback 返回 {type(js_result).__name__}"
                    f" = {repr(js_result)[:100]}"
                )
                time.sleep(1)
                continue

            token_preview = js_result.get("token_preview", "")
            login_ok = js_result.get("login", {}).get("ok", False)
            if token_preview and login_ok:
                break
            last_diag = (
                f"attempt {attempt+1}: token='{token_preview}', "
                f"login_ok={login_ok}, "
                f"sources={js_result.get('token_sources_tried')}, "
                f"has_dehydrated={js_result.get('has_dehydrated')}, "
                f"q_count={js_result.get('dehydrated_query_count')}, "
                f"login={js_result.get('login')}"
            )
            time.sleep(1)

        if isinstance(js_result, dict):
            token = js_result.get("token_preview", "")
            if token:
                result.csrf_token_found = True
                result.csrf_token_preview = token
            login = js_result.get("login", {})
            if login.get("ok"):
                result.is_logged_in = True
                result.user_id = login.get("user_id", "")
                result.user_pixiv_id = login.get("pixiv_id", "")
                result.user_name = login.get("name", "")
            # 仅失败时记录诊断
            if not (token and login.get("ok")):
                result.error += f"\n[csrf+login diag] {last_diag}"
        else:
            result.error += f"\n[csrf+login diag] js_result 非 dict: {last_diag}"
    except Exception as e:  # noqa: BLE001
        result.error += f"\ncsrf/login 提取失败: {type(e).__name__}: {e}"

    # ---- 验证点 4：持久化（再读一次对比 PHPSESSID 前缀）----
    try:
        cookies2 = cookies_to_dicts(window.get_cookies())
        php2 = ""
        for d in cookies2:
            if d["name"] == "PHPSESSID":
                php2 = d["value"]
                break
        if result.phpsessid_value_preview and php2:
            prefix = result.phpsessid_value_preview.split("...")[0]
            result.persisted_across_read = php2.startswith(prefix)
        else:
            result.persisted_across_read = False
    except Exception:  # noqa: BLE001
        result.persisted_across_read = None

    return result


# ====================================================================
# 登录态验证已合并到 EXTRACT_AND_VERIFY_JS（一次 JS 调用完成 token + login）
# ====================================================================
# SPEC 原写的 /ajax/user/self/status 实测 404 不存在，
# 正确接口是 /ajax/user/self，返回 { userData: { id, pixivId, name, ... } }。


# ====================================================================
# 终端输出
# ====================================================================


def print_report(r: ProbeResult) -> None:
    """把结果格式化打印到终端，方便用户复制回贴。"""
    line = "=" * 60
    print(f"\n{line}")
    print("COOKIE PROBE · 探测结果")
    print(line)

    # 验证点 1
    print(f"\n[1] get_cookies() 返回类型: {r.get_cookies_return_type}")
    # 验证点 2
    print(f"[2] Cookie 总数: {r.cookies_count}")
    # 验证点 3 (PHPSESSID)
    print(f"[3] PHPSESSID:")
    print(f"    找到:        {'✅' if r.phpsessid_found else '❌'}")
    if r.phpsessid_found:
        print(f"    HttpOnly:    {'✅' if r.phpsessid_is_httponly else '❌' if r.phpsessid_is_httponly is False else '?'}")
        print(f"    Secure:      {'✅' if r.phpsessid_is_secure else '❌' if r.phpsessid_is_secure is False else '?'}")
        print(f"    值前 8 位:   {r.phpsessid_value_preview}")
    # 验证点 3 其他
    print(f"[4] device_token:             {'✅' if r.device_token_found else '❌'}")
    print(f"[5] privacy_policy_agreement: {'✅' if r.privacy_policy_agreement_found else '❌'}")
    # 验证点 5
    print(f"[6] 当前 URL: {r.current_url}")
    # 验证点 6 (csrf)
    print(f"[7] x-csrf-token:")
    print(f"    找到:        {'✅' if r.csrf_token_found else '❌'}")
    if r.csrf_token_found:
        print(f"    值前 8 位:   {r.csrf_token_preview}")
    # 验证点 7：登录态
    print(f"[8] /ajax/user/self 登录态:")
    print(f"    is_logged_in: {'✅' if r.is_logged_in else '❌'}")
    if r.is_logged_in:
        print(f"    user_id:      {r.user_id}")
        print(f"    pixiv_id:     {r.user_pixiv_id}")
        print(f"    name:         {r.user_name}")
    # 验证点 4
    if r.persisted_across_read is not None:
        print(f"[9] 持久化重读一致: {'✅' if r.persisted_across_read else '❌'}")
    else:
        print(f"[9] 持久化重读一致: ? (跳过)")

    print(f"\n完整明细已写入: {RESULT_FILE}")

    # R1 结论：cookie 拿到 + 登录态确认 + csrf 拿到
    passed = (
        r.phpsessid_found
        and r.phpsessid_is_httponly is True
        and r.is_logged_in
        and r.csrf_token_found
    )
    print(f"\n{'=' * 60}")
    if passed:
        print("R1 结论: ✅ 通过 — pywebview 能读 HttpOnly PHPSESSID，A 方案成立")
    else:
        print("R1 结论: ❌ 失败 — 见上面各项，决定是否退 C 兜底")
    print(f"{'=' * 60}\n")


# ====================================================================
# 主入口
# ====================================================================


def make_loaded_handler() -> Any:
    """构造 loaded 事件处理器。

    用闭包持有 extracted 标志位，避免重复提取（loaded 事件每次页面导航都触发）。
    """
    state = {"extracted": False}

    def on_loaded(window: webview.Window) -> None:
        url = window.get_current_url() or ""
        # 还没登录到 www.pixiv.net，跳过
        if TARGET_HOST not in url:
            return
        # 已经提取过，跳过
        if state["extracted"]:
            return
        state["extracted"] = True

        print(f"\n[spike] 检测到登录成功，URL: {url}")
        print(f"[spike] 开始提取 cookie + csrf ...")

        try:
            result = run_probe(window)
        except Exception as e:  # noqa: BLE001
            result = ProbeResult(error=f"{type(e).__name__}: {e}")

        RESULT_FILE.write_text(
            json.dumps(asdict(result), ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
        print_report(result)
        print("[spike] 提取完成。窗口保持打开，可手动关闭。")

    return on_loaded


def main() -> None:
    """启动 pywebview 登录窗，loaded 事件自动探测。

    关键参数：
        - private_mode=False  关闭隐私模式，cookie 才能持久化到 WebView2 数据目录
        - http_server=True    启用内置 HTTP server（pywebview 官方 cookie 示例要求）
        - http_port=HTTP_PORT 固定端口，跨会话复用 cookie 存储
    """
    login_window = webview.create_window(
        title="Cookie Probe · 登录 Pixiv（登录后自动探测，无需点按钮）",
        url=LOGIN_URL,
        width=960,
        height=720,
    )
    login_window.events.loaded += make_loaded_handler()

    webview.start(
        private_mode=False,
        http_server=True,
        http_port=HTTP_PORT,
    )


if __name__ == "__main__":
    main()
