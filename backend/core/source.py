"""
NovelSource 抽象 + 3 种实现：SingleNovel / Series / UserNovels。
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from collections.abc import AsyncIterator

from backend.core.pixiv_client import PixivClient


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
    """系列小说：按系列顺序 yield。"""

    def __init__(self, series_id: int) -> None:
        self.series_id = series_id

    async def resolve(self, client: PixivClient) -> AsyncIterator[tuple[int, int | None]]:
        data = await client.get_series_content(self.series_id)
        # pixiv 系列 API 返回 seriesContentMapping
        novels = data.get("seriesMapping", data.get("novels", []))
        if isinstance(novels, dict):
            # dict key 是 order
            for idx, (order_str, novel_id) in enumerate(
                sorted(novels.items(), key=lambda x: int(x[0]))
            ):
                yield (int(novel_id), idx + 1)
        elif isinstance(novels, list):
            for idx, item in enumerate(novels):
                nid = item.get("id", item) if isinstance(item, dict) else item
                yield (int(nid), idx + 1)
        else:
            # fallback: data 本身可能包含 ids
            ids = data.get("ids", [])
            for idx, nid in enumerate(ids):
                yield (int(nid), idx + 1)


class UserNovelsSource(NovelSource):
    """用户全部小说：先获取全部 id，再按系列 + 散篇展开。"""

    def __init__(self, user_id: int) -> None:
        self.user_id = user_id

    async def resolve(self, client: PixivClient) -> AsyncIterator[tuple[int, int | None]]:
        data = await client.get_user_novels(self.user_id)

        novel_ids: list[int] = []
        series_list: list[dict] = []

        # profile/all 返回 novels dict 和 novelSeries dict
        novels_dict = data.get("novels", {})
        if isinstance(novels_dict, dict):
            for nid in novels_dict:
                novel_ids.append(int(nid))

        series_dict = data.get("novelSeries", {})
        if isinstance(series_dict, dict):
            for sid, info in series_dict.items():
                series_list.append({"series_id": int(sid), "title": info.get("title", "")})

        # 对每个 series 展开
        for s in series_list:
            series_source = SeriesSource(s["series_id"])
            async for nid, order in series_source.resolve(client):
                yield (nid, order)
                # 从 novel_ids 中移除已归入系列的
                if nid in novel_ids:
                    novel_ids.remove(nid)

        # 散篇
        for nid in novel_ids:
            yield (nid, None)
