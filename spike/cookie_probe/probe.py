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
    cookies_detail: list[dict[str, Any]] = field(default_factory=list)
    error: str = ""


# ====================================================================
# 浏览器端 JS：提取 x-csrf-token
# ====================================================================
# Pixiv 把 token 放在两处之一：
#   1. window.__NEXT_DATA__.props.pageProps.token（新版 SPA）
#   2. <meta name="global-data"> 的 JSON 内容（旧版）
# 两个都试，谁非空用谁。

EXTRACT_CSRF_JS = """
() => {
    const mask = (s) => s ? s.slice(0, 8) + '...' : '';

    const nextData = window.__NEXT_DATA__ || null;
    const tokenA = nextData?.props?.pageProps?.token || '';

    const meta = document.querySelector('meta[name="global-data"]');
    let tokenB = '';
    if (meta) {
        try {
            const data = JSON.parse(meta.content || '{}');
            tokenB = data.token || '';
        } catch (e) {}
    }

    const tokenC = (document.querySelector('meta[name="csp-token"]') || {}).content || '';

    return {
        url: location.href,
        token_from_next_data: mask(tokenA),
        token_from_meta: mask(tokenB),
        token_from_csp_meta: mask(tokenC),
        has_next_data: Boolean(nextData),
        next_data_keys: nextData ? Object.keys(nextData.props?.pageProps || {}).slice(0, 10) : [],
    };
}
"""


# ====================================================================
# Cookie 处理工具
# ====================================================================


def cookie_to_dict(c: Any) -> dict[str, Any]:
    """把 pywebview 返回的 cookie 对象统一成 dict。

    SimpleCookie.Morsel 走 .key / .value + 属性；
    pywebview 内部可能直接给 dict。两种都兼容。
    """
    if isinstance(c, dict):
        return {
            "name": c.get("name") or c.get("key", ""),
            "value": c.get("value", ""),
            "domain": c.get("domain", ""),
            "path": c.get("path", "/"),
            "httponly": bool(c.get("httponly", False)),
            "secure": bool(c.get("secure", False)),
            "expires": c.get("expires"),
        }

    # SimpleCookie.Morsel 或类似对象
    name = getattr(c, "key", None) or getattr(c, "name", "")
    return {
        "name": str(name),
        "value": getattr(c, "value", "") or "",
        "domain": getattr(c, "domain", "") or "",
        "path": getattr(c, "path", "") or "/",
        "httponly": bool(getattr(c, "httponly", False)),
        "secure": bool(getattr(c, "secure", False)),
        "expires": getattr(c, "expires", None),
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
    cookies = window.get_cookies()
    result.get_cookies_return_type = type(cookies).__name__
    result.current_url = window.get_current_url() or ""

    details: list[dict[str, Any]] = []
    for c in cookies:
        detail = cookie_to_dict(c)
        details.append(detail)

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
    try:
        js_result = window.evaluate_js(EXTRACT_CSRF_JS)
        if isinstance(js_result, dict):
            token = (
                js_result.get("token_from_next_data")
                or js_result.get("token_from_meta")
                or js_result.get("token_from_csp_meta")
                or ""
            )
            if token:
                result.csrf_token_found = True
                result.csrf_token_preview = token
    except Exception as e:  # noqa: BLE001
        result.error += f"\ncsrf 提取失败: {type(e).__name__}: {e}"

    # ---- 验证点 4：持久化（再读一次对比 PHPSESSID 前缀）----
    try:
        cookies2 = window.get_cookies()
        php2 = ""
        for c in cookies2:
            d = cookie_to_dict(c)
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
    # 验证点 4
    if r.persisted_across_read is not None:
        print(f"[8] 持久化重读一致: {'✅' if r.persisted_across_read else '❌'}")
    else:
        print(f"[8] 持久化重读一致: ? (跳过)")

    print(f"\n完整明细已写入: {RESULT_FILE}")

    # R1 结论
    passed = r.phpsessid_found and (r.phpsessid_is_httponly is not False) and r.csrf_token_found
    print(f"\n{'=' * 60}")
    print(f"R1 结论: {'✅ 通过 — pywebview 能读 HttpOnly PHPSESSID，A 方案成立' if passed else '❌ 失败 — 需退 C 兜底（手动粘 PHPSESSID）'}")
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
