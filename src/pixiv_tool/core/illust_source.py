"""
IllustSource 抽象 + 2 种实现：SingleIllust / UserIllusts。

与 NovelSource 平行的插画来源解析。插画没有"系列抓取"(pixiv 系列插画
仍是一个多页作品),所以只有单作品 + 用户全集两类。
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from collections.abc import AsyncIterator

from pixiv_tool.core.pixiv_client import PixivClient


class IllustSource(ABC):
    """解析"来源"得到插画/漫画作品 id 流。"""

    @abstractmethod
    async def resolve(self, client: PixivClient) -> AsyncIterator[int]:
        """yield artwork_id。"""
        ...  # pragma: no cover

    async def resolve_total(self, client: PixivClient) -> int | None:
        """预取任务分母（进度显示用）；无法确定时返回 None。

        用户全集 = 作品总数，单作品 = 页数。调用方在进入 resolve 循环前
        调用，结果可缓存供 resolve 复用（避免重复请求）。
        """
        return None

    def cached_illust(self) -> dict | None:
        """resolve_total 预取的插画元数据（单作品源），供爬虫复用。"""
        return None


class SingleIllustSource(IllustSource):
    """单幅插画：直接 yield (id)。分母 = 页数（1 张图 = 1）。"""

    def __init__(self, illust_id: int) -> None:
        self.illust_id = illust_id
        self._illust: dict | None = None

    async def resolve_total(self, client: PixivClient) -> int | None:
        self._illust = await client.get_illust(self.illust_id)
        return max(1, int(self._illust.get("pageCount", 1) or 1))

    def cached_illust(self) -> dict | None:
        return self._illust

    async def resolve(self, client: PixivClient) -> AsyncIterator[int]:
        yield self.illust_id


class UserIllustsSource(IllustSource):
    """用户全部插画/漫画作品。

    /ajax/user/{id}/profile/all 返回:
      body.illusts = {"illust_id_str": null, ...}  # 插画(含 ugoira)
      body.manga   = {"illust_id_str": null, ...}  # 漫画

    顺序:先插画后漫画,各自保持接口返回顺序(通常按发布时间倒序)。
    匿名访问时两个 map 都为空(noLoginData 掩码),需登录态。
    """

    def __init__(self, user_id: int) -> None:
        self.user_id = user_id
        self._profile: dict | None = None

    async def resolve_total(self, client: PixivClient) -> int | None:
        self._profile = await client.get_user_profile_all(self.user_id)
        return len(self._ids("illusts")) + len(self._ids("manga"))

    async def resolve(self, client: PixivClient) -> AsyncIterator[int]:
        if self._profile is None:
            self._profile = await client.get_user_profile_all(self.user_id)

        for artwork_id in [*self._ids("illusts"), *self._ids("manga")]:
            yield artwork_id

    def _ids(self, key: str) -> list[int]:
        mapping = (self._profile or {}).get(key, {}) or {}
        ids: list[int] = []
        if isinstance(mapping, dict):
            for id_str in mapping:
                try:
                    ids.append(int(id_str))
                except (TypeError, ValueError):
                    continue
        return ids
