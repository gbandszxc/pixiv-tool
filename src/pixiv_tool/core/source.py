"""
NovelSource 抽象 + 3 种实现：SingleNovel / Series / UserNovels。
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from collections.abc import AsyncIterator

from pixiv_tool.core.pixiv_client import PixivClient


class NovelSource(ABC):
    """解析"来源"得到带序号的 novel id 流。"""

    @abstractmethod
    async def resolve(self, client: PixivClient) -> AsyncIterator[tuple[int, int | None]]:
        """yield (novel_id, series_order)，单篇 series_order=None"""
        ...  # pragma: no cover


class SingleNovelSource(NovelSource):
    """单篇小说：直接 yield (id, None)。"""

    def __init__(self, novel_id: int) -> None:
        self.novel_id = novel_id

    async def resolve(self, client: PixivClient) -> AsyncIterator[tuple[int, int | None]]:
        yield (self.novel_id, None)


class SeriesSource(NovelSource):
    """系列小说：按系列顺序 yield (novel_id, series_order)。"""

    def __init__(self, series_id: int) -> None:
        self.series_id = series_id

    async def resolve(self, client: PixivClient) -> AsyncIterator[tuple[int, int | None]]:
        # 实测响应结构(2026-07-21 chrome-devtools):
        #   body.page.seriesContents = [
        #     {"id": "27466576", "series": {"contentOrder": 1}, "title": "..."},
        #     ...
        #   ]
        # body.thumbnails.novel 也有 id + seriesContentOrder,但 page.seriesContents
        # 更稳定(包含所有话,thumbnails 可能有筛选)。
        data = await client.get_series_content(self.series_id)
        page = data.get("page", {}) or {}
        contents = page.get("seriesContents", []) or []

        for item in contents:
            nid_str = item.get("id")
            if nid_str is None:
                continue
            # contentOrder 从 1 开始;缺失时用遍历顺序兜底
            order = item.get("series", {}).get("contentOrder")
            yield (int(nid_str), int(order) if order is not None else None)


class UserNovelsSource(NovelSource):
    """用户全部小说：先获取全部 id,再按系列 + 散篇展开。

    pixiv /ajax/user/{id}/profile/all 返回:
      body.novels = {"novel_id_str": null, ...}  # 全部 novel id(value 全 null)
      body.novelSeries = [{"id":..., "title":...}, ...]  # list 不是 dict

    展开策略:先抓每个 series(用 series 的 contentOrder 作为顺序),
    再抓不属任何 series 的散篇(顺序无意义)。
    """

    def __init__(self, user_id: int) -> None:
        self.user_id = user_id

    async def resolve(self, client: PixivClient) -> AsyncIterator[tuple[int, int | None]]:
        data = await client.get_user_novels(self.user_id)

        # 收集全部 novel id(profile/all 的 novels 是 id→null 映射)
        novels_dict = data.get("novels", {}) or {}
        all_novel_ids: set[int] = set()
        if isinstance(novels_dict, dict):
            for nid_str in novels_dict:
                try:
                    all_novel_ids.add(int(nid_str))
                except ValueError:
                    continue

        # novelSeries 是 list,每项含 id + title
        series_list = data.get("novelSeries", []) or []
        seen_in_series: set[int] = set()

        for s in series_list:
            if not isinstance(s, dict):
                continue
            sid = s.get("id")
            if sid is None:
                continue
            series_source = SeriesSource(int(sid))
            async for nid, order in series_source.resolve(client):
                yield (nid, order)
                seen_in_series.add(nid)

        # 散篇 = 全部 id - 已归入系列的 id
        # 不保证顺序(profile/all 的 dict 顺序通常按发布时间倒序,但 API 没承诺)
        standalone = all_novel_ids - seen_in_series
        for nid in standalone:
            yield (nid, None)
