"""
PixivClient — 异步 HTTP 客户端 + 限速 + 重试 + 429 暂停。
所有 Pixiv API 请求统一走这里。

底层 HTTP 后端(TLS 指纹伪装)由 core.http_factory 统一管理:优先 curl_cffi
(impersonate=chrome,解决 JA3/JA4 + HTTP/2 + 浏览器 header),降级 httpx。
本类只关心限速/重试/错误分类,不感知后端实现。
"""

from __future__ import annotations

import asyncio
import logging
import time
from typing import Any

from pixiv_tool.core.http_factory import create_client

logger = logging.getLogger(__name__)

# 网络层异常元组:_get 用 `except _NETWORK_ERRORS` 统一捕获超时/连接错误,
# 与后端解耦。优先用 curl_cffi 的 RequestException(超时/网络都是它的子类);
# 降级到 httpx 时用它的 TimeoutException + TransportError(NetworkError 的基类)。
try:
    from curl_cffi.requests.exceptions import RequestException as _CurlRequestError

    _NETWORK_ERRORS: tuple[type[BaseException], ...] = (_CurlRequestError,)
except ImportError:
    import httpx as _httpx

    _NETWORK_ERRORS = (_httpx.TimeoutException, _httpx.TransportError)

# V1 写死常量（不暴露给用户）
CONCURRENCY = 2
REQUEST_INTERVAL = 0.4  # 秒
REQUEST_TIMEOUT = 15.0  # 秒
MAX_RETRIES = 3
RETRY_BACKOFF = [1.0, 2.0, 4.0]
PAUSE_ON_429 = 60.0  # 秒

BASE_URL = "https://www.pixiv.net"
AJAX_URL = f"{BASE_URL}/ajax"


class PixivClientError(Exception):
    """Pixiv API 错误基类。"""


class PixivAuthError(PixivClientError):
    """401/403 认证错误。"""


class PixivNotFoundError(PixivClientError):
    """404 资源不存在。"""


class PixivRateLimitError(PixivClientError):
    """429 请求过多。"""


class PixivServerError(PixivClientError):
    """5xx 服务端错误。"""


class PixivClient:
    """带限速 + 重试的 Pixiv HTTP 客户端。"""

    def __init__(self, cookies: dict[str, str]) -> None:
        self._cookies = cookies
        self._semaphore = asyncio.Semaphore(CONCURRENCY)
        self._pause_event = asyncio.Event()
        self._pause_event.set()  # 初始不暂停
        # 客户端创建统一走 http_factory:TLS 指纹伪装 + cookie 分离 + 浏览器 header
        # 都在那里做。这里只持有引用用于限速/重试。
        self._client = create_client(cookies)
        logger.debug("PixivClient 使用 HTTP 后端: %s", self._client.backend)

    @property
    def backend(self) -> str:
        """当前 HTTP 后端(curl_cffi / httpx),供诊断确认伪装是否生效。"""
        return self._client.backend

    async def close(self) -> None:
        await self._client.close()

    # ------------------------------------------------------------------
    # API 方法
    # ------------------------------------------------------------------

    async def get_user_self(self) -> dict[str, Any]:
        """验证登录态：GET /ajax/user/self?lang=zh"""
        data = await self._get(f"{AJAX_URL}/user/self?lang=zh")
        return data.get("userData", {})

    async def get_novel(self, novel_id: int) -> dict[str, Any]:
        """获取单篇小说元数据 + 内容。"""
        data = await self._get(f"{AJAX_URL}/novel/{novel_id}")
        return data

    async def get_series_content(self, series_id: int) -> dict[str, Any]:
        """获取系列小说内容列表。

        实测接口路径(2026-07-21 chrome-devtools):
            /ajax/novel/series_content/{id}?limit=30&last_order=0&order_by=asc
        返回 body.page.seriesContents[] —— 含 id + series.contentOrder。
        注意不是 /ajax/novel/series/{id}(那个是系列元信息,无小说列表)。
        limit=30 是单次返回上限,系列超过 30 话需要分页(last_order 增量)。
        V1 假设系列 ≤ 30 话,V2 加分页。
        """
        data = await self._get(
            f"{AJAX_URL}/novel/series_content/{series_id}"
            f"?limit=30&last_order=0&order_by=asc&lang=zh"
        )
        return data

    async def get_user_novels(self, user_id: int) -> dict[str, Any]:
        """获取用户全部小说列表(novel id + novelSeries 元信息)。

        实测接口(2026-07-21 chrome-devtools):
            /ajax/user/{id}/profile/all?sensitiveFilterMode=userSetting&lang=zh
        返回 body.novels = {"novel_id_str": null, ...}(只有 id,value 全 null)
              body.novelSeries = [{"id":..., "title":...}, ...](list,不是 dict)
        """
        data = await self._get(
            f"{AJAX_URL}/user/{user_id}/profile/all"
            f"?sensitiveFilterMode=userSetting&lang=zh"
        )
        return data

    async def get_illust(self, illust_id: int) -> dict[str, Any]:
        """获取单幅插画元数据(含各页原图 URL)。

        实测接口(2026-08-02 浏览器):
            GET /ajax/illust/{id}
        登录态返回 body.urls.original(p0 原图直链) + body.meta.pages[]
        (每项含 image_urls.original, 多图作品各页原图);illustType 2(ugoira)
        的原始文件是 zip,另走 get_ugoira_meta。匿名访问时 meta 被掩码,
        只有 urls.original,所以多图/用户全集抓取必须携带有效登录态。
        """
        return await self._get(f"{AJAX_URL}/illust/{illust_id}")

    async def get_user_info(self, user_id: int) -> dict[str, Any]:
        """获取用户公开信息(用于插画用户全集目录命名)。

        GET /ajax/user/{id} 返回 body.name / body.account 等。匿名可读,
        拿不到时调用方回退用 userId。
        """
        return await self._get(f"{AJAX_URL}/user/{user_id}")

    async def get_ugoira_meta(self, illust_id: int) -> dict[str, Any]:
        """获取 ugoira(动图)元数据:帧列表 + 原图 zip 直链。

        实测接口:GET /ajax/illust/{id}/ugoira_meta
        返回 body.zip_urls.original = img-zip-ugoira 的 zip 原图。
        需登录态;匿名不可见。
        """
        return await self._get(f"{AJAX_URL}/illust/{illust_id}/ugoira_meta")

    async def get_user_profile_all(self, user_id: int) -> dict[str, Any]:
        """获取用户主页全部作品 id(插画 + 漫画 + 小说)。

        与 get_user_novels 同一接口。body.illusts / body.manga 是
        {illust_id_str: null} 映射(插画/漫画作品);body.novels 是小说。
        匿名访问时内容为空(noLoginData 掩码),需登录态。
        """
        return await self._get(
            f"{AJAX_URL}/user/{user_id}/profile/all"
            f"?sensitiveFilterMode=userSetting&lang=zh"
        )

    async def download_bytes(self, url: str) -> bytes:
        """下载二进制内容(插画原图 / ugoira zip)。

        与 _get 共用限速/重试/429 暂停,额外带 Referer 头:
        i.pximg.net 防盗链校验 Referer 必须是 www.pixiv.net(实测 2026-08-02)。
        """
        resp = await self._request(
            url, headers={"Referer": "https://www.pixiv.net/"}
        )
        content = getattr(resp, "content", None)
        if content is None:
            raise PixivClientError(f"响应无内容: {_mask_url(url)}")
        return content

    # ------------------------------------------------------------------
    # 限速 + 重试核心
    # ------------------------------------------------------------------

    async def _get(self, url: str) -> dict[str, Any]:
        """带限速 + 重试的 JSON GET 请求。"""
        resp = await self._request(url)
        body = resp.json()
        if body.get("error"):
            raise PixivClientError(f"API error: {body['message']}")
        return body.get("body", body)

    async def _request(self, url: str, *, headers: dict[str, str] | None = None):
        """带限速 + 重试的 GET,返回响应对象(JSON 与二进制共用)。"""
        async with self._semaphore:
            await self._pause_event.wait()  # 等 429 暂停解除

            try:
                last_exc: Exception | None = None
                for attempt in range(MAX_RETRIES):
                    start = time.monotonic()
                    try:
                        resp = await self._client.get(url, headers=headers or {})
                        elapsed = time.monotonic() - start
                        logger.info("GET %s → %d (%.2fs)", _mask_url(url), resp.status_code, elapsed)

                        if resp.status_code == 200:
                            return resp

                        if resp.status_code in (401, 403):
                            raise PixivAuthError(f"认证失败: HTTP {resp.status_code}")

                        if resp.status_code == 404:
                            raise PixivNotFoundError(f"资源不存在: HTTP {resp.status_code}")

                        if resp.status_code == 429:
                            logger.warning("429 Rate Limit — 全队列暂停 %ds", PAUSE_ON_429)
                            self._pause_event.clear()
                            asyncio.create_task(self._resume_after_429(PAUSE_ON_429))
                            raise PixivRateLimitError("429 Too Many Requests")

                        if resp.status_code >= 500:
                            last_exc = PixivServerError(f"服务端错误: HTTP {resp.status_code}")
                        else:
                            last_exc = PixivClientError(f"HTTP {resp.status_code}")

                    except _NETWORK_ERRORS as exc:
                        # curl_cffi 的 RequestException 或 httpx 的 Timeout/NetworkError。
                        # 超时和连接错误都归到这里统一重试,不再区分(两者都属"可重试的网络抖动")。
                        last_exc = PixivClientError(f"网络错误: {exc}: {url}")

                    if attempt < MAX_RETRIES - 1:
                        wait = RETRY_BACKOFF[min(attempt, len(RETRY_BACKOFF) - 1)]
                        logger.info("重试 %d/%d，等待 %.1fs", attempt + 1, MAX_RETRIES, wait)
                        await asyncio.sleep(wait)

                raise last_exc or PixivClientError("请求失败")
            finally:
                # 关键：sleep 必须在 semaphore 持有期间执行，保证每请求 0.4s 间隔真正生效。
                # 原代码此行缩进在 async with 块外，永远不执行 → 限速失效，触发 pixiv 风控（SPEC R2）。
                # 用 try/finally 包裹重试循环，确保即使 429 / auth / 404 等异常路径也会限速。
                await asyncio.sleep(REQUEST_INTERVAL)

    async def _resume_after_429(self, seconds: float) -> None:
        """429 暂停结束后恢复请求队列。

        替代已弃用的 ``asyncio.get_event_loop().call_later()`` —— Python 3.12+
        中 ``get_event_loop()`` 在没有运行中事件循环时弃用/报错，而此处调用方
        一定在事件循环内，所以用 ``asyncio.create_task`` 调度一个延时 set 的
        协程更 Pythonic 且前向兼容。
        """
        await asyncio.sleep(seconds)
        self._pause_event.set()
        logger.info("429 暂停结束，恢复请求")


def _mask_url(url: str) -> str:
    """日志脱敏：不泄露 cookie。"""
    return url.split("?")[0] if "?" in url else url
