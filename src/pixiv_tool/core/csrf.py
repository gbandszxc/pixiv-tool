"""Pixiv csrf token 提取 + 登录态探测。

为什么需要
----------
Pixiv Web API(/ajax/...)每个写操作都要带 x-csrf-token。token 本身是公开的
(首页 HTML 里就有),登录态由 PHPSESSID cookie 决定。

手动导入 Cookie 的场景下,用户从浏览器 DevTools 只能方便地拿到 PHPSESSID,
csrf token 藏在 __NEXT_DATA__ / meta global-data 里,手动复制很麻烦。
本模块用 PHPSESSID 请求 pixiv 首页 HTML,自动解析出 csrf token —— 这样
用户只需填一个 PHPSESSID。

token 存放位置(2026-07-23 chrome-devtools 实测):
    <meta name="global-data" id="meta-global-data"
          content='{"token":"f6f3a6b0...","services":{...}}'>
content 用单引号包裹,是 JSON 字符串。token 在顶层 data["token"]。
"""

from __future__ import annotations

import html as html_mod
import json
import logging
import re
from dataclasses import dataclass

from pixiv_tool.core.http_factory import create_client

logger = logging.getLogger(__name__)

PIXIV_HOME = "https://www.pixiv.net/"

# 匹配 <meta name="global-data" ... content='...'>。
# pixiv 的真实结构(2026-07-23 实测):name 用双引号且出现在 content 之前,
# content 用单引号包裹 JSON(JSON 内部是双引号,所以 content 必须用单引号避免冲突)。
# content 值的捕获组必须配对同一个引号:
#   单引号包裹 → content='([^']*)'(匹配到下一个单引号)
#   双引号包裹 → content="([^"]*)"
# 关键: 正则末尾必须显式匹配 '>' 收尾,否则 [^>]*? 跨越属性时回溯歧义会漏匹配。
# 约束: 此正则要求 name 在 content 之前(与 pixiv 真实结构一致),若 Pixiv 改成
# content 在前,需同步调整。__NEXT_DATA__ 兜底路径会再尝试一次。
_GLOBAL_DATA_RE = re.compile(
    r"""<meta\s+[^>]*?name=["']global-data["'][^>]*?content='([^']*)'>"""
    r"""|<meta\s+[^>]*?name=["']global-data["'][^>]*?content="([^"]*)">""",
    re.IGNORECASE,
)


class CsrfExtractionError(Exception):
    """csrf token 提取失败(首页请求失败 / meta 标签缺失 / JSON 解析失败)。"""


class InvalidSessionError(Exception):
    """PHPSESSID 无效或已过期(首页返回登录墙)。"""


@dataclass
class SessionProbe:
    """fetch_session_probe 的结果:csrf token + 是否登录态。

    is_logged_in 由首页是否含登录态特征判定(匿名也能拿到 token,所以 token
    存在 ≠ 已登录;必须额外判断)。登录态判据见下方实现。
    """

    csrf_token: str
    is_logged_in: bool


async def fetch_session_probe(phpsessid: str) -> SessionProbe:
    """用 PHPSESSID 请求 pixiv 首页,解析 csrf token + 判断登录态。

    Raises:
        InvalidSessionError: 首页返回非 200(被风控/拦截/重定向到登录)。
        CsrfExtractionError: 首页正常但解析不出 csrf token(Pixiv 改版)。

    返回的 SessionProbe.is_logged_in 仅作参考,真正登录态验证仍由
    /api/auth/status(调 /ajax/user/self)做。这里只是给手动导入一个
    即时反馈,避免用户导入了无效 cookie 还以为成功了。
    """
    phpsessid = phpsessid.strip()
    if not phpsessid:
        raise InvalidSessionError("PHPSESSID 为空")

    cookies = {"PHPSESSID": phpsessid}
    client = create_client(cookies)
    try:
        resp = await client.get(PIXIV_HOME, timeout=15.0)
    except Exception as exc:
        # curl_cffi 对无效 PHPSESSID 会触发重定向循环(TooManyRedirects,
        # pixiv 反复把无效 session 重定向到登录页)。这是无效 cookie 的典型信号,
        # 归为 InvalidSessionError 让用户看到"cookie 无效"而非技术报错。
        msg = str(exc)
        if "redirect" in msg.lower() or "TooManyRedirects" in type(exc).__name__:
            raise InvalidSessionError(
                "PHPSESSID 无效或已过期(Pixiv 反复重定向到登录页)"
            ) from exc
        raise  # 其余网络错误向上抛,由调用方兜底处理
    finally:
        await client.close()

    if resp.status_code != 200:
        # 防御风控拦截(403/5xx)。注意:有效的匿名访问也会返回 200 匿名首页,
        # 所以 200 不代表登录态,只代表请求没被拦截。
        raise InvalidSessionError(
            f"请求 pixiv 首页失败: HTTP {resp.status_code}(可能被风控或 PHPSESSID 无效)"
        )

    html_text = resp.text
    token = _extract_csrf_from_html(html_text)
    if not token:
        raise CsrfExtractionError(
            "首页 HTML 未找到 csrf token(Pixiv 页面结构可能已改版)"
        )

    # 登录态判据:登录后的首页 SSR 会注入用户数据;匿名首页不含。
    # 用全局变量名 "globalInitData" 是否含 "userData" 判定(旧版 Pixiv),
    # 以及 meta global-data 里是否含 "userData" 键(新版)。
    is_logged_in = _detect_login_state(html_text)
    logger.info("session 探测: token 已获取, 登录态=%s", is_logged_in)

    return SessionProbe(csrf_token=token, is_logged_in=is_logged_in)


def _extract_csrf_from_html(html_text: str) -> str | None:
    """从首页 HTML 提取 csrf token。

    优先 meta global-data(当前主路径);失败时回退到 __NEXT_DATA__
    (login_window.py 的 JS 路径,作为 HTML 解析的兜底)。
    """
    # 主路径:meta global-data。双分支正则:group(1)=单引号包裹,group(2)=双引号包裹
    m = _GLOBAL_DATA_RE.search(html_text)
    if m:
        raw = m.group(1) if m.group(1) is not None else m.group(2)
        decoded = html_mod.unescape(raw)
        try:
            data = json.loads(decoded)
            token = data.get("token")
            if isinstance(token, str) and token:
                return token
        except (json.JSONDecodeError, ValueError) as exc:
            logger.warning("meta global-data JSON 解析失败: %s", exc)

    # 兜底:__NEXT_DATA__ 的 serverSerializedPreloadedState.api.token
    # (login_window.py 路径 E 的等价 HTML 提取)
    next_m = re.search(
        r'<script[^>]*id="__NEXT_DATA__"[^>]*>([^<]*)</script>', html_text
    )
    if next_m:
        try:
            nd = json.loads(html_mod.unescape(next_m.group(1)))
            token = (
                nd.get("props", {})
                .get("pageProps", {})
                .get("serverSerializedPreloadedState", {})
                .get("api", {})
                .get("token")
            )
            if isinstance(token, str) and token:
                return token
        except (json.JSONDecodeError, ValueError, AttributeError):
            pass

    return None


def _detect_login_state(html_text: str) -> bool:
    """粗略判断首页是否登录态。仅用于即时反馈,不作为最终判据。"""
    # 登录后的 pixiv 首页 SSR 含用户 ID 注入特征,匿名首页没有。
    # 用 "userData" 出现作为信号(meta global-data 或 __NEXT_DATA__ 里)。
    return "userData" in html_text
