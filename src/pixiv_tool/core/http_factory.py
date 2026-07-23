"""统一的 Pixiv HTTP 客户端工厂 —— 浏览器指纹伪装。

为什么需要这个模块
-------------------
登录窗走 pywebview(真浏览器),天然不触发 Pixiv 风控;但登录后所有业务请求
(验证登录态、抓小说)走的是裸 httpx,它的 TLS 指纹(CPython OpenSSL)、HTTP/1.1
协议、残缺 header(User-Agent 断在 AppleWebKit/537.36)全方位不像浏览器,
容易被 Pixiv 识别为爬虫触发风控。ADR 0002 第 28 行写过"A 方案风控最低,
真浏览器指纹",但这个认识当时只落实在登录窗,业务请求退化成了裸 httpx。

本模块的对策
------------
优先用 curl_cffi(impersonate=chrome):一次性解决 TLS ClientHello(JA3/JA4)
+ HTTP/2 + cipher/GREASE 顺序 + 浏览器默认 header 顺序。impersonate 还会自动
补全 UA / sec-ch-ua / sec-fetch-* 等浏览器特征 header。

降级策略:curl_cffi 不可用时(如打包动态库缺失)回退到 httpx,并手动补一套
完整 Chrome header(挡得住基于 header 的浅层风控,挡不住 TLS 指纹)。两层解耦、
可独立验证、可单独回退。

对外只暴露 ``create_client(cookies)``,返回的 ``HTTPClient`` 抹平 curl_cffi 与
httpx 的接口差异,上层(pixiv_client / auth_status)无需感知后端实现。
"""

from __future__ import annotations

import logging
from typing import Any, Protocol, runtime_checkable

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# 浏览器画像常量(降级到 httpx 时使用;curl_cffi 的 impersonate 会自动补这些)
# ---------------------------------------------------------------------------

# 完整 Chrome UA。之前 pixiv_client 用的是 "...AppleWebKit/537.36"(断尾),
# auth_status 更差只有 "Mozilla/5.0" —— 这种残缺 UA 本身就是强风控信号。
CHROME_UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
    "AppleWebKit/537.36 (KHTML, like Gecko) "
    "Chrome/124.0.0.0 Safari/537.36"
)

# curl_cffi 的 impersonate 也会发这套,但 httpx 后端必须手动补齐:
# 缺 Accept-Language / sec-ch-ua / sec-fetch-* 是爬虫最经典的暴露点。
BROWSER_HEADERS: dict[str, str] = {
    "User-Agent": CHROME_UA,
    "Accept": "application/json, text/plain, */*",
    "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.8,en-US;q=0.7",
    "Referer": "https://www.pixiv.net/",
    "sec-ch-ua": '"Chromium";v="124", "Google Chrome";v="124", "Not-A.Brand";v="99"',
    "sec-ch-ua-mobile": "?0",
    "sec-ch-ua-platform": '"Windows"',
    "Sec-Fetch-Site": "same-origin",
    "Sec-Fetch-Mode": "cors",
    "Sec-Fetch-Dest": "empty",
}

# impersonate 目标浏览器。选 chrome124 而非裸 "chrome":风控会检测过期指纹,
# 显式版本号比 "chrome"(随库默认)更可控。升级 curl_cffi 时同步评估。
IMPERSONATE_TARGET = "chrome124"

# 运行时探测:curl_cffi 是否可用。模块级探测一次,frozen 后若动态库缺失
# (collect_dynamic_libs 没打进去)会在这里降级到 httpx,而不是让每个请求崩。
try:
    from curl_cffi.requests import AsyncSession as _CurlAsyncSession  # type: ignore
    from curl_cffi.requests.exceptions import RequestException as _CurlRequestError  # type: ignore

    _HAS_CURL_CFFI = True
    logger.debug("HTTP 后端:curl_cffi(impersonate=%s)", IMPERSONATE_TARGET)
except Exception as exc:  # ImportError / 动态库加载失败都走这里
    _HAS_CURL_CFFI = False
    _CurlAsyncSession = None  # type: ignore
    _CurlRequestError = Exception  # type: ignore
    logger.warning("curl_cffi 不可用(%s),降级到 httpx —— 仅补 header,无 TLS 指纹伪装", exc)


@runtime_checkable
class _SupportsAclose(Protocol):
    """curl_cffi.AsyncSession 和 httpx.AsyncClient 都有 aclose。"""

    async def aclose(self) -> None: ...


class HTTPClient:
    """抹平 curl_cffi / httpx 差异的薄封装。

    对外只暴露 ``get`` / ``close``,响应对象只用到 ``status_code`` / ``json()``。
    pixiv_client._get 的重试逻辑只依赖这两个字段,因此后端切换对它透明。

    cookie 处理:x-csrf-token 必须进 header(Pixiv 专用,不进 cookie jar);
    其余 cookie(含 PHPSESSID)进 session jar。这套分离逻辑两处都在重复,
    现在统一在这里做一次。
    """

    def __init__(self, cookies: dict[str, str]) -> None:
        csrf_token = cookies.get("x-csrf-token", "")
        jar_cookies = {k: v for k, v in cookies.items() if k != "x-csrf-token"}

        # curl_cffi 的 impersonate 会自动补 UA / sec-ch-ua / sec-fetch-* 等,
        # 我们只额外注入 csrf token(浏览器默认不发这个,Pixiv 后端校验它)。
        session_headers = {"x-csrf-token": csrf_token}

        self._backend = "curl_cffi" if _HAS_CURL_CFFI else "httpx"
        if _HAS_CURL_CFFI:
            # impersonate=True 时不要传 default_headers=False —— 我们要它补全浏览器
            # header,只覆盖/追加 csrf。
            self._impl: Any = _CurlAsyncSession(  # type: ignore[misc]
                impersonate=IMPERSONATE_TARGET,
                headers=session_headers,
                cookies=jar_cookies,
                timeout=15.0,
            )
        else:
            import httpx

            # 降级路径:手动合并完整浏览器 header + csrf。
            merged = {**BROWSER_HEADERS, **session_headers}
            self._impl = httpx.AsyncClient(
                timeout=httpx.Timeout(15.0),
                headers=merged,
                cookies=jar_cookies,
            )

    @property
    def backend(self) -> str:
        """当前后端名,供诊断/日志确认伪装是否生效。"""
        return self._backend

    async def get(self, url: str, **kwargs: Any) -> Any:
        """发起 GET。透传 timeout 等覆盖参数。"""
        return await self._impl.get(url, **kwargs)

    async def close(self) -> None:
        """关闭底层 session/client。幂等。"""
        impl = getattr(self, "_impl", None)
        if impl is None:
            return
        close = getattr(impl, "aclose", None)
        if close is not None:
            await close()
        self._impl = None


def create_client(cookies: dict[str, str]) -> HTTPClient:
    """创建带浏览器指纹伪装的 HTTP 客户端。

    单一入口:pixiv_client(抓取)和 auth_status(登录态验证)都走这里,
    消除之前两处 header 不一致(status 的 UA 只有 "Mozilla/5.0")、
    且 status 每次新建裸 client 的无防护问题。
    """
    return HTTPClient(cookies)
