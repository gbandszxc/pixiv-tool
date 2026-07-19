"""
PixivClient — httpx 异步 HTTP 客户端 + 限速 + 重试 + 429 暂停。
所有 Pixiv API 请求统一走这里。
"""

from __future__ import annotations

import asyncio
import logging
import time
from typing import Any

import httpx

logger = logging.getLogger(__name__)

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
        self._client = httpx.AsyncClient(
            timeout=httpx.Timeout(REQUEST_TIMEOUT),
            headers={
                "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
                "Referer": "https://www.pixiv.net/",
                "x-csrf-token": cookies.get("x-csrf-token", ""),
            },
            cookies={k: v for k, v in cookies.items() if k != "x-csrf-token"},
        )

    async def close(self) -> None:
        await self._client.aclose()

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
        """获取系列小说内容列表。"""
        data = await self._get(f"{AJAX_URL}/novel/series/{series_id}")
        return data

    async def get_user_novels(self, user_id: int) -> dict[str, Any]:
        """获取用户全部小说列表。"""
        data = await self._get(f"{AJAX_URL}/user/{user_id}/profile/all")
        return data

    # ------------------------------------------------------------------
    # 限速 + 重试核心
    # ------------------------------------------------------------------

    async def _get(self, url: str) -> dict[str, Any]:
        """带限速 + 重试的 GET 请求。"""
        async with self._semaphore:
            await self._pause_event.wait()  # 等 429 暂停解除

            try:
                last_exc: Exception | None = None
                for attempt in range(MAX_RETRIES):
                    start = time.monotonic()
                    try:
                        resp = await self._client.get(url)
                        elapsed = time.monotonic() - start
                        logger.info("GET %s → %d (%.2fs)", _mask_url(url), resp.status_code, elapsed)

                        if resp.status_code == 200:
                            body = resp.json()
                            if body.get("error"):
                                raise PixivClientError(f"API error: {body['message']}")
                            return body.get("body", body)

                        if resp.status_code in (401, 403):
                            raise PixivAuthError(f"认证失败: HTTP {resp.status_code}")

                        if resp.status_code == 404:
                            raise PixivNotFoundError(f"资源不存在: HTTP {resp.status_code}")

                        if resp.status_code == 429:
                            logger.warning("429 Rate Limit — 全队列暂停 %ds", PAUSE_ON_429)
                            self._pause_event.clear()
                            asyncio.get_event_loop().call_later(
                                PAUSE_ON_429, self._pause_event.set
                            )
                            raise PixivRateLimitError("429 Too Many Requests")

                        if resp.status_code >= 500:
                            last_exc = PixivServerError(f"服务端错误: HTTP {resp.status_code}")
                        else:
                            last_exc = PixivClientError(f"HTTP {resp.status_code}")

                    except httpx.TimeoutException:
                        last_exc = PixivClientError(f"请求超时: {url}")
                    except httpx.NetworkError:
                        last_exc = PixivClientError(f"网络错误: {url}")

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


def _mask_url(url: str) -> str:
    """日志脱敏：不泄露 cookie。"""
    return url.split("?")[0] if "?" in url else url
