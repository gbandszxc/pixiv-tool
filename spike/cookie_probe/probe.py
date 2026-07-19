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

验证流程：
    1. 弹 pywebview 窗口加载 pixiv 登录页（http_server=True, private_mode=False）
    2. 用户手动登录（输账密 / 过验证码 / 过 2FA）
    3. 登录成功后页面重定向到 www.pixiv.net
    4. 通过 expose 的 probe() 函数被前端按钮调用，或 loaded 事件触发：
       - 打印所有 cookie（验证返回类型）
       - 标记 PHPSESSID 是否存在 + 是否标 HttpOnly
       - 提取 x-csrf-token（从 window.__NEXT_DATA__ 或 meta 标签）
    5. 把 cookie dump 到 result.json，方便事后比对

运行：
    uv run python probe.py
"""

from __future__ import annotations

import json
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Any

import webview


# ====================================================================
# 配置
# ====================================================================

# 登录起始页（accounts.pixiv.net 跳转到 www.pixiv.net）
LOGIN_URL = "https://accounts.pixiv.net/login"

# 登录成功后落地的域（取 cookie 用）
TARGET_URL = "https://www.pixiv.net/"

# 结果输出文件
RESULT_FILE = Path(__file__).parent / "result.json"

# pywebview HTTP server 端口（cookie 持久化需要 http_server=True）
# 等价 sh：export PYWEBVIEW_HTTP_PORT=17729
HTTP_PORT = 17729


# ====================================================================
# 结果记录
# ====================================================================


@dataclass
class ProbeResult:
    """spike 验证矩阵的数据载体，对应 README.md 的验证点表格。"""

    # 验证点 1：返回类型
    get_cookies_return_type: str = ""
    cookies_count: int = 0

    # 验证点 2：PHPSESSID（HttpOnly）
    phpsessid_found: bool = False
    phpsessid_is_httponly: bool | None = None
    phpsessid_is_secure: bool | None = None
    phpsessid_value_preview: str = ""  # 仅前 8 位，避免完整泄漏到日志

    # 验证点 3：其他关键 cookie
    device_token_found: bool = False
    privacy_policy_agreement_found: bool = False

    # 验证点 4：持久化（重读一次 cookie 是否一致）
    persisted_across_read: bool | None = None

    # 验证点 5：当前 URL（确认重定向到了 www.pixiv.net）
    current_url: str = ""

    # 验证点 6：x-csrf-token
    csrf_token_found: bool = False
    csrf_token_preview: str = ""

    # 所有 cookie 的明细（name / domain / path / httponly / secure / expires）
    cookies_detail: list[dict[str, Any]] = []

    # 原始错误（若有）
    error: str = ""


# ====================================================================
# 浏览器端 JS：提取 x-csrf-token
# ====================================================================
# Pixiv 把 token 放在两处之一：
#   1. <meta name="global-data" ...> 的 JSON 内容（旧版）
#   2. window.__NEXT_DATA__.props.pageProps.token（新版）
# spike 两个都试，看哪个能拿到。

EXTRACT_CSRF_JS = """
() => {
    const mask = (s) => s ? s.slice(0, 8) + '...' : '';

    // 方法 A：__NEXT_DATA__
    const nextData = window.__NEXT_DATA__ || null;
    const tokenA = nextData?.props?.pageProps?.token || '';

    // 方法 B：meta global-data
    const meta = document.querySelector('meta[name="global-data"]');
    let tokenB = '';
    if (meta) {
        try {
            const data = JSON.parse(meta.content || '{}');
            tokenB = data.token || '';
        } catch (e) {}
    }

    // 方法 C：直接找 meta[name="csp-token"] 或类似
    const tokenC = (document.querySelector('meta[name="csp-token"]') || {}).content || '';

    return {
        url: location.href,
        token_from_next_data: mask(tokenA),
        token_from_meta: mask(tokenB),
        token_from_csp_meta: mask(tokenC),
        has_next_data: Boolean(nextData),
        next_data_keys: nextData ? Object.keys(nextData.props?.pageProps || {}) : [],
    };
}
"""


# ====================================================================
# pywebview JS API（前端按钮可调）
# ====================================================================


class ProbeApi:
    """暴露给前端按钮的 Python 接口。

    pywebview 的 js_api 方法在主线程被调用时执行，此时 window 对象活跃，
    get_cookies() 可用。这是绕开"窗口关闭后 KeyError: master"的正确姿势。
    """

    def __init__(self) -> None:
        self.result = ProbeResult()
        # pywebview 注入的 window 实例（start 前为 None，由 setup_api 注入）
        self._window: webview.Window | None = None

    def bind_window(self, window: webview.Window) -> None:
        self._window = window

    # ---- 前端调用入口 ----

    def probe(self) -> dict[str, Any]:
        """前端"开始探测"按钮调用。

        返回 JSON-serializable dict，前端可显示探测摘要；
        同时把完整 ProbeResult 写到 result.json。
        """
        if self._window is None:
            return {"error": "window 未绑定"}

        try:
            self._collect_cookies()
            self._collect_csrf()
            self._verify_persistence()
            self._write_result()
            return self._summary()
        except Exception as e:  # noqa: BLE001 - spike 要捕获一切异常落到 result.json
            self.result.error = f"{type(e).__name__}: {e}"
            self._write_result()
            return {"error": str(e)}

    def navigate_to_pixiv(self) -> dict[str, str]:
        """前端"我已登录，跳到 pixiv.net"按钮调用。"""
        if self._window is None:
            return {"error": "window 未绑定"}
        self._window.load_url(TARGET_URL)
        return {"status": "loading", "url": TARGET_URL}

    def close(self) -> None:
        """前端"关闭"按钮调用。"""
        if self._window is None:
            return
        self._window.destroy()

    # ---- 内部：cookie 收集 ----

    def _collect_cookies(self) -> None:
        """验证点 1/2/3/5：读取并分析 cookie。"""
        cookies = self._window.get_cookies()
        # pywebview 返回 SimpleCookie 实例列表（或类似 dict-like 对象）
        self.result.get_cookies_return_type = type(cookies).__name__
        self.result.current_url = self._window.get_current_url() or ""

        details: list[dict[str, Any]] = []
        for c in cookies:
            # SimpleCookie 的 morsel：用 output() / .value / 各属性
            # pywebview 可能返回 SimpleCookie 实例或 dict，两种都兼容
            detail = self._cookie_to_dict(c)
            details.append(detail)

            name = detail["name"]
            if name == "PHPSESSID":
                self.result.phpsessid_found = True
                self.result.phpsessid_is_httponly = detail.get("httponly")
                self.result.phpsessid_is_secure = detail.get("secure")
                val = detail.get("value", "")
                self.result.phpsessid_value_preview = val[:8] + "..." if val else ""
            elif name == "device_token":
                self.result.device_token_found = True
            elif name == "privacy_policy_agreement":
                self.result.privacy_policy_agreement_found = True

        self.result.cookies_count = len(details)
        # 脱敏后存明细（PHPSESSID 只留前 8 位）
        self.result.cookies_detail = [self._mask_sensitive(d) for d in details]

    @staticmethod
    def _cookie_to_dict(c: Any) -> dict[str, Any]:
        """把 pywebview 返回的 cookie 对象统一成 dict。

        SimpleCookie.Morsel 走 .key / .value / .coded_value + 属性；
        pywebview 内部可能直接给 dict。两种都兼容。
        """
        if isinstance(c, dict):
            return {
                "name": c.get("name") or c.get("key", ""),
                "value": c.get("value", ""),
                "domain": c.get("domain", ""),
                "path": c.get("path", ""),
                "httponly": c.get("httponly", False),
                "secure": c.get("secure", False),
                "expires": c.get("expires"),
            }
        # SimpleCookie.Morsel 或类似
        name = getattr(c, "key", None) or getattr(c, "name", "")
        value = getattr(c, "value", "") or ""
        return {
            "name": str(name),
            "value": value,
            "domain": getattr(c, "domain", "") or getattr(c, "domain", ""),
            "path": getattr(c, "path", "") or "",
            "httponly": bool(getattr(c, "httponly", False)),
            "secure": bool(getattr(c, "secure", False)),
            "expires": getattr(c, "expires", None),
        }

    @staticmethod
    def _mask_sensitive(d: dict[str, Any]) -> dict[str, Any]:
        """脱敏：PHPSESSID 只留前 8 位，避免完整泄漏到 result.json。"""
        if d.get("name") == "PHPSESSID" and d.get("value"):
            d = dict(d)
            d["value"] = d["value"][:8] + "...(masked)"
        return d

    # ---- 内部：csrf 提取 ----

    def _collect_csrf(self) -> None:
        """验证点 6：执行 JS 提取 x-csrf-token。"""
        try:
            result = self._window.evaluate_js(EXTRACT_CSRF_JS)
        except Exception as e:  # noqa: BLE001
            self.result.error += f"\ncsrf 提取失败: {e}"
            return

        if not isinstance(result, dict):
            return

        # 三个 token 来源任一非空即视为成功
        token = (
            result.get("token_from_next_data")
            or result.get("token_from_meta")
            or result.get("token_from_csp_meta")
            or ""
        )
        if token:
            self.result.csrf_token_found = True
            self.result.csrf_token_preview = token

    # ---- 内部：持久化验证 ----

    def _verify_persistence(self) -> None:
        """验证点 4：再读一次 cookie，比对 PHPSESSID 是否一致。

        验证 pywebview 在 private_mode=False + http_server=True 下，
        cookie 真的被持久化存储（不是只在内存里）。
        """
        first = self.result.cookies_detail
        try:
            cookies2 = self._window.get_cookies()
        except Exception:  # noqa: BLE001
            self.result.persisted_across_read = None
            return

        php1 = next((c["value"] for c in first if c["name"] == "PHPSESSID"), None)
        php2 = ""
        for c in cookies2:
            d = self._cookie_to_dict(c)
            if d["name"] == "PHPSESSID":
                php2 = d["value"]
                break

        # 持久化读到的 PHPSESSID（注意 first 里被 mask 了，要按前缀比对）
        if php1 and php2:
            prefix1 = php1.split("...")[0] if "..." in php1 else php1
            self.result.persisted_across_read = php2.startswith(prefix1)
        else:
            self.result.persisted_across_read = False

    # ---- 内部：输出 ----

    def _write_result(self) -> None:
        RESULT_FILE.write_text(
            json.dumps(asdict(self.result), ensure_ascii=False, indent=2),
            encoding="utf-8",
        )

    def _summary(self) -> dict[str, Any]:
        r = self.result
        return {
            "return_type": r.get_cookies_return_type,
            "cookies_count": r.cookies_count,
            "phpsessid_found": r.phpsessid_found,
            "phpsessid_is_httponly": r.phpsessid_is_httponly,
            "phpsessid_preview": r.phpsessid_value_preview,
            "device_token_found": r.device_token_found,
            "privacy_policy_agreement_found": r.privacy_policy_agreement_found,
            "csrf_token_found": r.csrf_token_found,
            "csrf_token_preview": r.csrf_token_preview,
            "current_url": r.current_url,
            "persisted_across_read": r.persisted_across_read,
            "result_file": str(RESULT_FILE),
        }


# ====================================================================
# UI（HTML 内嵌，省去额外文件）
# ====================================================================

HTML = """<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<title>Cookie Probe · R1 Spike</title>
<style>
body { font-family: -apple-system, 'Segoe UI', sans-serif; padding: 24px; max-width: 640px; margin: 0 auto; }
h1 { font-size: 18px; }
.step { background:#f6f8fa; padding:12px 16px; border-radius:6px; margin:12px 0; }
.step ol { margin:8px 0; padding-left:20px; }
button { padding:8px 16px; margin-right:8px; cursor:pointer; font-size:14px; }
button.primary { background:#0969da; color:white; border:none; border-radius:4px; }
button.secondary { background:#ffffff; color:#24292f; border:1px solid #d0d7de; border-radius:4px; }
pre { background:#f6f8fa; padding:12px; border-radius:6px; overflow:auto; max-height:300px; font-size:12px; }
.hint { color:#57606a; font-size:13px; }
</style>
</head>
<body>
<h1>Cookie Probe · R1 Spike</h1>

<div class="step">
<strong>步骤 1：登录 Pixiv</strong>
<p class="hint">在上方窗口登录 pixiv（输账密 / 过验证码 / 过 2FA）。登录成功后会自动重定向到 <code>www.pixiv.net</code>。</p>
<p class="hint">如果还停在 <code>accounts.pixiv.net</code>，点下面的按钮手动跳转。</p>
<button class="secondary" onclick="onNavigate()">我已登录，跳到 pixiv.net</button>
</div>

<div class="step">
<strong>步骤 2：开始探测</strong>
<p class="hint">确认窗口地址栏已是 <code>www.pixiv.net/</code> 后点此按钮，读取所有 cookie + 提取 x-csrf-token。</p>
<button class="primary" onclick="onProbe()">开始探测 cookie</button>
</div>

<div class="step">
<strong>结果</strong>
<pre id="result">（等待探测）</pre>
<p class="hint">完整明细已写到 spike/cookie_probe/result.json</p>
<button class="secondary" onclick="onClose()">关闭窗口</button>
</div>

<script>
function setResult(obj) {
    document.getElementById('result').textContent = JSON.stringify(obj, null, 2);
}

async function onNavigate() {
    setResult({ status: 'loading...' });
    const r = await window.pywebview.api.navigate_to_pixiv();
    setResult(r);
}

async function onProbe() {
    setResult({ status: 'probing...' });
    const r = await window.pywebview.api.probe();
    setResult(r);
}

async function onClose() {
    await window.pywebview.api.close();
}
</script>
</body>
</html>"""


# ====================================================================
# 入口
# ====================================================================


def main() -> None:
    """启动 pywebview 探测窗口。

    关键参数说明（注释完整以便转 sh 时对照）：
        - url=HTML              加载内嵌 UI
        - js_api=api             暴露 ProbeApi 给前端按钮
        - private_mode=False     关闭隐私模式，cookie 才能持久化
        - http_server=True       启用内置 HTTP server（cookie 持久化的前提）
        - http_port=HTTP_PORT    固定端口，保证下次启动读到同一份 cookie
    """
    api = ProbeApi()

    # 主窗口：先加载 pixiv 登录页，登录后会重定向到 www.pixiv.net
    login_window = webview.create_window(
        title="Cookie Probe · 登录 Pixiv",
        url=LOGIN_URL,
        js_api=api,
        width=900,
        height=700,
    )
    api.bind_window(login_window)

    # 第二个窗口：探测控制 UI
    ui_window = webview.create_window(
        title="Cookie Probe · 控制",
        url=HTML,
        js_api=api,
        width=680,
        height=560,
        x=120,
        y=120,
    )
    api.bind_window(ui_window)

    # private_mode=False + http_server=True：cookie 才会持久化
    # 等价 sh 注释：
    #   webview.start(http_server=True, private_mode=False, http_port=17729)
    webview.start(
        private_mode=False,
        http_server=True,
        http_port=HTTP_PORT,
    )


if __name__ == "__main__":
    main()
