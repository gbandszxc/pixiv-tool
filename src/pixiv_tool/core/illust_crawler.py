"""
IllustCrawler — 消费 IllustSource 的 id 流，统一执行并发、限速、重试、写库。

与 Crawler(小说)平行。差异:
  - 单幅作品按页下载原图(meta.pages[].image_urls.original,一律原图)
  - ugoira 动图的"原图"是 zip(/ajax/illust/{id}/ugoira_meta 的 zip_urls.original)
  - 输出目录(扁平,artworkId 在文件名中天然去重):
      单作品   → {output}/pic/{标题}_{id}_p{N}.{ext}
      用户全集 → {output}/pic/users/{作者}_{userId}/{标题}_{id}_p{N}.{ext}
"""

from __future__ import annotations

import asyncio
import json
import logging
import re
import time
from datetime import datetime, timezone
from pathlib import Path

from pixiv_tool.core.exporter import sanitize_filename
from pixiv_tool.core.illust_source import IllustSource
from pixiv_tool.core.pixiv_client import PixivClient, PixivClientError
from pixiv_tool.core.task import TaskManager
from pixiv_tool.storage.db import Database
from pixiv_tool.storage.paths import DATA_DIR

logger = logging.getLogger(__name__)

UGOIRA_TYPE = 2


class IllustCrawler:
    """编排插画抓取任务。"""

    def __init__(self, client: PixivClient, db: Database,
                 task_manager: TaskManager) -> None:
        self.client = client
        self.db = db
        self.task_manager = task_manager

    async def run(self, source: IllustSource, task_id: str,
                  output_dir: str = "downloads",
                  user_id: int | None = None,
                  max_wait_seconds: int = 180,
                  event_callback=None) -> None:
        """执行抓取。user_id 非空时按用户全集目录布局(pic/users/)。

        max_wait_seconds: 任务最大运行时长（不含暂停时间），超过自动标记失败。
        """
        pause_evt = self.task_manager.get_pause_event(task_id)
        cancel_flag = self.task_manager.get_cancel_flag(task_id)

        total = 0
        done = 0
        skipped = 0
        failed_ids: list[int] = []
        started_at = time.monotonic()
        paused_total = 0.0
        timed_out = False

        # 锚定相对 output_dir 到 DATA_DIR(与小说 Crawler 一致)。
        out = Path(output_dir)
        if not out.is_absolute():
            out = DATA_DIR / out
        base = out / "pic"
        if user_id is not None:
            base = base / "users" / await self._user_dir_name(user_id)

        try:
            self.task_manager.update_progress(task_id, status="running")
            if event_callback:
                event_callback("progress", {"task_id": task_id, "done": 0, "total": 0, "skipped": 0})

            async for artwork_id in source.resolve(self.client):
                if cancel_flag.is_set():
                    break

                if self.db.is_illust_downloaded(artwork_id):
                    skipped += 1
                    total += 1
                    continue

                total += 1
                pause_start = time.monotonic()
                await pause_evt.wait()  # 暂停点
                paused_total += time.monotonic() - pause_start

                # 最大等待时间：超过自动失败（暂停时长不计入）
                if time.monotonic() - started_at - paused_total > max_wait_seconds:
                    timed_out = True
                    break

                try:
                    await self._crawl_one(artwork_id, base)
                    done += 1
                    self.task_manager.update_progress(
                        task_id, done=done, total=total, skipped=skipped)
                    if event_callback:
                        event_callback("progress", {
                            "task_id": task_id, "done": done,
                            "total": total, "skipped": skipped,
                        })
                except Exception as exc:
                    logger.error("抓取插画 %d 失败: %s", artwork_id, exc)
                    failed_ids.append(artwork_id)
                    if event_callback:
                        event_callback("failed", {
                            "task_id": task_id, "novel_id": artwork_id, "error": str(exc),
                        })

            if timed_out:
                msg = f"任务超过最大等待时间（{max_wait_seconds}s）"
                logger.warning("任务 %s %s", task_id, msg)
                self.task_manager.mark_failed(task_id, msg)
                if event_callback:
                    event_callback("failed", {"task_id": task_id, "error": msg})
                return

            status = "canceled" if cancel_flag.is_set() else "done"
            self.task_manager.mark_done(task_id, total, done, skipped, failed_ids)
            if event_callback:
                event_callback("done", {
                    "task_id": task_id, "done": done,
                    "total": total, "skipped": skipped,
                    "failed": len(failed_ids),
                })

        except Exception as exc:
            logger.error("任务 %s 异常: %s", task_id, exc)
            self.task_manager.mark_failed(task_id, str(exc))
            if event_callback:
                event_callback("failed", {"task_id": task_id, "error": str(exc)})

    async def _user_dir_name(self, user_id: int) -> str:
        """用户全集目录名:{作者名}_{userId},拿不到作者名退回 user_{userId}。"""
        try:
            info = await self.client.get_user_info(user_id)
            name = info.get("name") or info.get("account")
            if name:
                return f"{sanitize_filename(name)}_{user_id}"
        except PixivClientError as exc:
            logger.warning("获取用户 %d 信息失败,目录退回 userId: %s", user_id, exc)
        return f"user_{user_id}"

    async def _crawl_one(self, artwork_id: int, base: Path) -> list[Path]:
        """抓取单幅插画:解析各页原图 URL → 下载 → 写库。"""
        data = await self.client.get_illust(artwork_id)
        title = data.get("title") or str(artwork_id)
        illust_type = int(data.get("illustType", 0))
        page_count = int(data.get("pageCount", 1))
        author_id = int(data.get("userId", 0) or 0)
        author_name = data.get("userName", "")

        safe = sanitize_filename(title) or str(artwork_id)
        base.mkdir(parents=True, exist_ok=True)

        saved: list[Path] = []
        if illust_type == UGOIRA_TYPE:
            saved.append(await self._download_ugoira(artwork_id, base, safe))
        else:
            urls = _collect_page_urls(data, page_count)
            if not urls:
                raise PixivClientError(f"作品 {artwork_id} 没有可用原图 URL")
            for idx, url in enumerate(urls):
                if not url:
                    continue
                content = await self.client.download_bytes(url)
                fp = base / f"{safe}_{artwork_id}_p{idx}.{_url_ext(url)}"
                fp.write_bytes(content)
                saved.append(fp)

        self.db.insert_illustration(
            artwork_id=artwork_id,
            title=title,
            author_id=author_id,
            author_name=author_name,
            illust_type=illust_type,
            page_count=page_count,
            saved_paths=json.dumps([str(p) for p in saved]),
            captured_at=datetime.now(timezone.utc).isoformat(),
        )
        logger.info("插画 %d 已保存 %d 个文件", artwork_id, len(saved))
        return saved

    async def _download_ugoira(self, artwork_id: int, base: Path, safe: str) -> Path:
        """ugoira 动图原图 = zip 帧序列。

        实测(2026-08-02 登录态浏览器):/ajax/illust/{id}/ugoira_meta 返回
          body.originalSrc = 原始尺寸 zip(1920x1080 等)
          body.src         = 展示用缩放 zip(600x600)
        "一律按原图下载" → 取 originalSrc;旧结构 zip_urls.original 作兜底。
        """
        meta = await self.client.get_ugoira_meta(artwork_id)
        zip_url = meta.get("originalSrc") or (meta.get("zip_urls") or {}).get("original")
        if not zip_url:
            raise PixivClientError(f"ugoira 作品 {artwork_id} 无 zip 原图")
        content = await self.client.download_bytes(zip_url)
        fp = base / f"{safe}_{artwork_id}_ugoira.zip"
        fp.write_bytes(content)
        return fp


def _collect_page_urls(data: dict, page_count: int) -> list[str]:
    """收集作品各页原图 URL(登录态 meta 优先,缺失时从 p0 推导)。"""
    meta_pages = (data.get("meta") or {}).get("pages") or []
    if meta_pages:
        urls: list[str] = []
        for p in meta_pages:
            img = p.get("image_urls") or {}
            urls.append(img.get("original") or p.get("original") or "")
        if urls:
            return urls

    p0 = (data.get("urls") or {}).get("original") or ""
    if not p0:
        return []
    return [p0] + [_derive_page_url(p0, i) for i in range(1, page_count)]


def _derive_page_url(p0_url: str, page: int) -> str:
    """多图作品从 p0 直链推导第 N 页原图:_p0.{ext} → _p{N}.{ext}。

    仅作为 meta 缺失(匿名/接口变动)时的兜底;带 hash 的 URL(如
    147635069-499627f2..._p0.png)同样适用,因为只替换末尾页码。
    """
    return re.sub(r"_p0(\.[A-Za-z0-9]+)$", f"_p{page}\\1", p0_url)


def _url_ext(url: str) -> str:
    """从 URL 取扩展名(不含查询串)。"""
    return url.rsplit(".", 1)[-1].split("?", 1)[0].split("#", 1)[0] or "bin"
