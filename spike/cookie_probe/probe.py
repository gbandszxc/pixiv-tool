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
# 浏览器端 JS：提取 x-csrf-token
# ====================================================================
# 经 chrome-devtools 实测（2026-07-19），pixiv 新版 SPA 把 csrf token 藏在：
#   window.__NEXT_DATA__.props.pageProps.dehydratedState.queries[*]
#     .queryKey === ["termsAgreement","status"]
#       .state.data.token
#
# 旧版可能用 meta[name="global-data"]，作为兜底。

EXTRACT_CSRF_JS = """
() => {
    const mask = (s) => s ? s.slice(0, 8) + '...' : '';

    const np = window.__NEXT_DATA__?.props?.pageProps || {};
    const dehydrated = np.dehydratedState;

    // 路径 A：dehydratedState.queries 找 termsAgreement
    let tokenA = '';
    if (dehydrated?.queries) {
        for (const q of dehydrated.queries) {
            const key = JSON.stringify(q.queryKey || q.queryHash || '');
            if (key.includes('termsAgreement')) {
                tokenA = q.state?.data?.token || '';
                if (tokenA) break;
            }
        }
    }

    // 路径 B：旧 pageProps.token（旧版）
    const tokenB = np.token || '';

    // 路径 C：meta global-data（更旧版）
    const meta = document.querySelector('meta[name="global-data"]');
    let tokenC = '';
    if (meta) {
        try {
            const data = JSON.parse(meta.content || '{}');
            tokenC = data.token || '';
        } catch (e) {}
    }

    return {
        url: location.href,
        token_from_dehydrated: mask(tokenA),
        token_from_page_props: mask(tokenB),
        token_from_meta: mask(tokenC),
        has_dehydrated: Boolean(dehydrated),
        dehydrated_query_count: dehydrated?.queries?.length || 0,
    };
}
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


def _dump_first_morsel(cookies: Any, target_name: str) -> str:
    """找到第一个名为 target_name 的 cookie，dump 其 Morsel 的真实结构。

    用来排查 value 总是空的问题——直接打印 repr/属性/方法。
    """
    for jar in cookies:
        items: list[tuple[str, Any]] = []
        if hasattr(jar, "items") and callable(getattr(jar, "items")):
            try:
                items = list(jar.items())
            except Exception:  # noqa: BLE001
                items = []
        for name, morsel in items:
            if name == target_name:
                # 收集所有非 magic 属性
                attrs = {}
                for attr in dir(morsel):
                    if attr.startswith("__"):
                        continue
                    try:
                        val = getattr(morsel, attr)
                    except Exception:  # noqa: BLE001
                        continue
                    if callable(val):
                        continue
                    attrs[attr] = repr(val)[:80]
                # 也试 dict-style 访问
                dict_view = {}
                if hasattr(morsel, "keys"):
                    try:
                        for k in morsel.keys():
                            try:
                                dict_view[k] = repr(morsel[k])[:80]
                            except Exception:  # noqa: BLE001
                                pass
                    except Exception:  # noqa: BLE001
                        pass
                import json as _json
                return (
                    f"type={type(morsel).__name__}, "
                    f"attrs={_json.dumps(attrs, ensure_ascii=False)}, "
                    f"dict_view={_json.dumps(dict_view, ensure_ascii=False)}, "
                    f"repr={repr(morsel)[:200]}"
                )
    return "(not found)"


def _morsel_to_dict(name: str, morsel: Any) -> dict[str, Any]:
    """把一个 SimpleCookie.Morsel（或 dict）转成扁平 dict。

    Morsel 的属性访问规则（容易踩坑）：
        - .key / .value / .coded_value：直接属性
          注意：.value 是 URL-decoded；如果原始 cookie 用 complex 格式，
          .value 可能为空，要 fallback 到 .coded_value
        - httponly / secure / path / domain / expires：reserved 属性，
          用 morsel['httponly'] 访问，返回的是字符串（"" 或 "HttpOnly" 等）
    """
    if isinstance(morsel, dict):
        return {
            "name": name or morsel.get("name", ""),
            "value": morsel.get("value", ""),
            "domain": morsel.get("domain", ""),
            "path": morsel.get("path", "/"),
            "httponly": bool(morsel.get("httponly", False)),
            "secure": bool(morsel.get("secure", False)),
            "expires": morsel.get("expires"),
        }

    # Morsel：value 优先 .value，空了 fallback 到 .coded_value
    value = getattr(morsel, "value", "") or ""
    if not value:
        value = getattr(morsel, "coded_value", "") or ""

    # reserved 属性用 morsel[key] 访问
    def _reserved(key: str) -> Any:
        try:
            val = morsel[key]
        except (KeyError, TypeError):
            return ""
        if isinstance(val, str):
            return val.lower() not in ("", "false", "0", "no")
        return bool(val)

    # domain / path 用 .get() (Morsel 支持 dict-style 默认值)
    def _get_str(key: str, default: str = "") -> str:
        try:
            val = morsel.get(key, default)
        except (KeyError, AttributeError, TypeError):
            val = default
        return val if isinstance(val, str) else (str(val) if val else default)

    expires = _get_str("expires") or None

    return {
        "name": name,
        "value": value,
        "domain": _get_str("domain", ""),
        "path": _get_str("path", "/") or "/",
        "httponly": _reserved("httponly"),
        "secure": _reserved("secure"),
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

    # 诊断：dump 第一个 PHPSESSID Morsel 的真实结构（只为排查 value 空问题）
    _diag = _dump_first_morsel(raw_cookies, "PHPSESSID")
    if _diag:
        result.error += f"\n[morsel diag] {_diag}"

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

    # ---- 验证点 6：csrf via JS ----
    # 注意：loaded 事件触发时 SPA 的 react-query 可能还没注水完，
    # dehydratedState.queries 为空，要延迟重试。
    try:
        js_result = None
        for attempt in range(5):  # 5 次重试，每次间隔 1s
            js_result = window.evaluate_js(EXTRACT_CSRF_JS)
            token = (
                (js_result or {}).get("token_from_dehydrated")
                or (js_result or {}).get("token_from_page_props")
                or (js_result or {}).get("token_from_meta")
                or ""
            ) if isinstance(js_result, dict) else ""
            if token:
                break
            # 还没注水，等 1s 再试
            import time
            time.sleep(1)
        if isinstance(js_result, dict):
            token = (
                js_result.get("token_from_dehydrated")
                or js_result.get("token_from_page_props")
                or js_result.get("token_from_meta")
                or ""
            )
            if token:
                result.csrf_token_found = True
                result.csrf_token_preview = token
            # 记录诊断信息到 error 字段（不算错误，方便排查）
            result.error += (
                f"\n[csrf diag] has_dehydrated={js_result.get('has_dehydrated')}, "
                f"query_count={js_result.get('dehydrated_query_count')}, "
                f"dehydrated='{js_result.get('token_from_dehydrated')}', "
                f"pageProps='{js_result.get('token_from_page_props')}', "
                f"meta='{js_result.get('token_from_meta')}'"
            )
    except Exception as e:  # noqa: BLE001
        result.error += f"\ncsrf 提取失败: {type(e).__name__}: {e}"

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
# 登录态验证（用真实接口）
# ====================================================================
# SPEC 原写的 /ajax/user/self/status 实测 404 不存在，
# 正确接口是 /ajax/user/self，返回 { userData: { id, pixivId, name, ... } }。

CHECK_LOGIN_JS = """
async () => {
    try {
        const r = await fetch('/ajax/user/self?lang=zh', { credentials: 'include' });
        const text = await r.text();
        let data = null;
        try { data = JSON.parse(text); } catch (e) {}
        if (data && data.userData) {
            return {
                ok: true,
                status: r.status,
                user_id: data.userData.id || '',
                pixiv_id: data.userData.pixivId || '',
                name: data.userData.name || '',
            };
        }
        return {
            ok: false,
            status: r.status,
            body_preview: text.slice(0, 200),
            cookie_header_visible: document.cookie.length > 0,
            cookie_count: document.cookie.split(';').length,
        };
    } catch (e) {
        return { ok: false, error: String(e) };
    }
}
"""


def verify_login(window: webview.Window, result: ProbeResult) -> None:
    """通过 /ajax/user/self 接口验证拿到的 cookie 是否真的能登录。"""
    try:
        login_info = window.evaluate_js(CHECK_LOGIN_JS)
    except Exception as e:  # noqa: BLE001
        result.error += f"\nlogin 验证失败: {type(e).__name__}: {e}"
        return

    if isinstance(login_info, dict):
        if login_info.get("ok"):
            result.is_logged_in = True
            result.user_id = login_info.get("user_id", "")
            result.user_pixiv_id = login_info.get("pixiv_id", "")
            result.user_name = login_info.get("name", "")
        else:
            result.is_logged_in = False
            result.error += (
                f"\n[login diag] status={login_info.get('status')}, "
                f"body='{login_info.get('body_preview', '')[:120]}', "
                f"js_cookie_count={login_info.get('cookie_count')}, "
                f"err={login_info.get('error', '')}"
            )


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
            verify_login(window, result)
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
