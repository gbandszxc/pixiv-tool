"""Tests for backend.core — source, exporter, task manager, pixiv client
(tickets 08, 09, 10, 11, 12, 13)."""

from __future__ import annotations

import asyncio
import json
from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from backend.storage.models import NovelData
from backend.core.exporter import (
    TxtExporter,
    MarkdownExporter,
    sanitize_filename,
    _render_markdown,
    _make_filename,
)
from backend.core.source import SingleNovelSource, SeriesSource
from backend.core.task import TaskManager
from backend.core.pixiv_client import (
    PixivClient,
    PixivServerError,
    PixivRateLimitError,
)


# ── Source tests (ticket 08) ─────────────────────────────────────────────────


class TestSingleNovelSource:
    @pytest.mark.asyncio
    async def test_single_novel_source_yields_id_none(self):
        """SingleNovelSource yields (id, None)."""
        src = SingleNovelSource(novel_id=42)
        mock_client = MagicMock()
        results = []
        async for nid, order in src.resolve(mock_client):
            results.append((nid, order))
        assert results == [(42, None)]

    @pytest.mark.asyncio
    async def test_single_novel_source_different_id(self):
        """Different novel_id yields correctly."""
        src = SingleNovelSource(novel_id=99999)
        mock_client = MagicMock()
        results = []
        async for nid, order in src.resolve(mock_client):
            results.append((nid, order))
        assert results == [(99999, None)]


class TestSeriesSource:
    @pytest.mark.asyncio
    async def test_series_source_resolves_ids_dict(self):
        """SeriesSource resolves IDs from dict response."""
        src = SeriesSource(series_id=100)
        mock_client = AsyncMock()
        mock_client.get_series_content.return_value = {
            "seriesMapping": {"2": 2001, "1": 2002}
        }
        results = []
        async for nid, order in src.resolve(mock_client):
            results.append((nid, order))
        # sorted by key: "1"→2002 order=1, "2"→2001 order=2
        assert results == [(2002, 1), (2001, 2)]

    @pytest.mark.asyncio
    async def test_series_source_resolves_ids_list(self):
        """SeriesSource resolves IDs from list response."""
        src = SeriesSource(series_id=200)
        mock_client = AsyncMock()
        mock_client.get_series_content.return_value = {
            "novels": [{"id": 3001}, {"id": 3002}]
        }
        results = []
        async for nid, order in src.resolve(mock_client):
            results.append((nid, order))
        assert results == [(3001, 1), (3002, 2)]


# ── Exporter tests (tickets 09, 10, 11, 12) ──────────────────────────────────


class TestSanitizeFilename:
    def test_sanitize_filename_removes_illegal_chars(self):
        """Illegal chars replaced with underscore."""
        assert sanitize_filename('a/b:c\\d*e?f"g<h>i') == "a_b_c_d_e_f_g_h_i"

    def test_sanitize_filename_strips_edge_chars(self):
        """Leading/trailing dots and underscores stripped."""
        assert sanitize_filename("...hello...") == "hello"

    def test_sanitize_filename_preserves_unicode(self):
        """Unicode characters preserved."""
        assert sanitize_filename("テスト小説") == "テスト小説"


class TestTxtExporter:
    def test_exporter_txt_creates_file(self, tmp_path, sample_novel_data):
        """TxtExporter writes content to .txt file."""
        exporter = TxtExporter()
        paths = exporter.export(sample_novel_data, tmp_path)
        assert len(paths) == 1
        assert paths[0].suffix == ".txt"
        content = paths[0].read_text(encoding="utf-8")
        assert str(sample_novel_data.novel_id) in str(paths[0]) or sample_novel_data.title in content
        assert len(content) > 0

    def test_exporter_txt_with_series_order(self, tmp_path, sample_novel_data):
        """Series order prefixes the filename."""
        exporter = TxtExporter()
        paths = exporter.export(sample_novel_data, tmp_path, series_order=3)
        assert paths[0].name.startswith("03_")


class TestMarkdownExporter:
    def test_exporter_markdown_renders_chapters(self, tmp_path, sample_novel_data):
        """[chapter:X] → ## X in markdown."""
        exporter = MarkdownExporter()
        paths = exporter.export(sample_novel_data, tmp_path)
        content = paths[0].read_text(encoding="utf-8")
        assert "## 第二章" in content

    def test_exporter_markdown_renders_newpage(self, tmp_path, sample_novel_data):
        """[newpage] → horizontal rule."""
        exporter = MarkdownExporter()
        paths = exporter.export(sample_novel_data, tmp_path)
        content = paths[0].read_text(encoding="utf-8")
        assert "---" in content

    def test_exporter_markdown_removes_jump(self, tmp_path, sample_novel_data):
        """[jump:N] removed from output."""
        exporter = MarkdownExporter()
        paths = exporter.export(sample_novel_data, tmp_path)
        content = paths[0].read_text(encoding="utf-8")
        assert "[jump:" not in content


class TestExportDirCreation:
    def test_exporter_creates_series_dir(self, tmp_path, sample_novel_data):
        """Export creates target directory if it doesn't exist."""
        target = tmp_path / "series" / "sub"
        exporter = TxtExporter()
        paths = exporter.export(sample_novel_data, target)
        assert target.exists()
        assert paths[0].parent == target


class TestMakeFilename:
    def test_make_filename_single(self):
        """Single novel: title_id.ext."""
        n = NovelData(novel_id=42, title="My Novel", page_count=1)
        assert _make_filename(n, None, "txt") == "My Novel_42.txt"

    def test_make_filename_series(self):
        """Series: padded_order_title.ext."""
        n = NovelData(novel_id=42, title="My Novel", page_count=5)
        assert _make_filename(n, 3, "md") == "03_My Novel.md"


# ── TaskManager tests (ticket 11) ────────────────────────────────────────────


class TestTaskManager:
    def test_task_manager_state_transitions(self, tmp_db):
        """pending → running → paused → running → done."""
        tm = TaskManager(tmp_db)
        task = tm.create_task("single", "1234")
        assert task.status == "pending"

        # pending → running
        tmp_db.update_task(task.task_id, status="running")
        assert tmp_db.get_task(task.task_id)["status"] == "running"

        # running → paused
        tm.pause(task.task_id)
        assert tmp_db.get_task(task.task_id)["status"] == "paused"

        # paused → running
        tm.resume(task.task_id)
        assert tmp_db.get_task(task.task_id)["status"] == "running"

        # running → done
        tm.mark_done(task.task_id, total=1, done=1, skipped=0)
        assert tmp_db.get_task(task.task_id)["status"] == "done"

    def test_task_manager_cancel(self, tmp_db):
        """Cancel sets cancel flag and status to canceled."""
        tm = TaskManager(tmp_db)
        task = tm.create_task("single", "5678")
        tm.cancel(task.task_id)
        assert tmp_db.get_task(task.task_id)["status"] == "canceled"
        flag = tm.get_cancel_flag(task.task_id)
        assert flag.is_set()

    def test_task_manager_pause_clears_event(self, tmp_db):
        """Pause clears the pause event (blocks execution)."""
        tm = TaskManager(tmp_db)
        task = tm.create_task("single", "1111")
        evt = tm.get_pause_event(task.task_id)
        assert evt.is_set()  # initially not paused
        tm.pause(task.task_id)
        assert not evt.is_set()

    def test_task_manager_resume_sets_event(self, tmp_db):
        """Resume sets the pause event (unblocks execution)."""
        tm = TaskManager(tmp_db)
        task = tm.create_task("single", "2222")
        tm.pause(task.task_id)
        tm.resume(task.task_id)
        evt = tm.get_pause_event(task.task_id)
        assert evt.is_set()

    def test_task_manager_mark_failed(self, tmp_db):
        """mark_failed sets status and error."""
        tm = TaskManager(tmp_db)
        task = tm.create_task("single", "3333")
        tm.mark_failed(task.task_id, "something broke")
        t = tmp_db.get_task(task.task_id)
        assert t["status"] == "failed"
        assert t["error"] == "something broke"


# ── PixivClient tests (ticket 13) ────────────────────────────────────────────


class TestPixivClient:
    @pytest.mark.asyncio
    async def test_pixiv_client_retry_on_5xx(self):
        """On 500 then 200, client retries and returns data."""
        mock_resp_500 = MagicMock()
        mock_resp_500.status_code = 500
        mock_resp_500.json.return_value = {}

        mock_resp_200 = MagicMock()
        mock_resp_200.status_code = 200
        mock_resp_200.json.return_value = {"body": {"id": 42}}

        with patch("backend.core.pixiv_client.httpx.AsyncClient") as MockClient:
            instance = MockClient.return_value
            instance.get = AsyncMock(side_effect=[mock_resp_500, mock_resp_200])
            instance.aclose = AsyncMock()

            client = PixivClient(cookies={"PHPSESSID": "abc"})
            try:
                with patch("backend.core.pixiv_client.asyncio.sleep", new_callable=AsyncMock):
                    result = await client._get("https://example.com/api")
                assert result == {"id": 42}
            finally:
                await client.close()

    @pytest.mark.asyncio
    async def test_pixiv_client_pause_on_429(self):
        """429 triggers pause event clear (queue pause)."""
        mock_resp_429 = MagicMock()
        mock_resp_429.status_code = 429

        with patch("backend.core.pixiv_client.httpx.AsyncClient") as MockClient:
            instance = MockClient.return_value
            instance.get = AsyncMock(return_value=mock_resp_429)
            instance.aclose = AsyncMock()

            client = PixivClient(cookies={"PHPSESSID": "abc"})
            try:
                with patch("backend.core.pixiv_client.asyncio.sleep", new_callable=AsyncMock):
                    with pytest.raises(PixivRateLimitError):
                        await client._get("https://example.com/api")
                # Pause event should be cleared after 429
                assert not client._pause_event.is_set()
            finally:
                await client.close()

    @pytest.mark.asyncio
    async def test_pixiv_client_auth_error(self):
        """401 raises PixivAuthError."""
        mock_resp = MagicMock()
        mock_resp.status_code = 401

        with patch("backend.core.pixiv_client.httpx.AsyncClient") as MockClient:
            instance = MockClient.return_value
            instance.get = AsyncMock(return_value=mock_resp)
            instance.aclose = AsyncMock()

            client = PixivClient(cookies={})
            try:
                with pytest.raises(Exception):  # PixivAuthError
                    await client._get("https://example.com/api")
            finally:
                await client.close()
