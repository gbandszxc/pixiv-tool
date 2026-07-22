"""Tests for pixiv_tool.storage — Database (ticket 02/03) + Settings (ticket 04)."""

from __future__ import annotations

import json
import pytest

from pixiv_tool.storage.db import Database
from pixiv_tool.storage.settings import Settings


# ── Database tests (tickets 02, 03) ──────────────────────────────────────────


class TestDatabaseSchema:
    def test_schema_creation_idempotent(self, tmp_path):
        """CREATE IF NOT EXISTS — second init on same file must not error."""
        db_path = tmp_path / "idempotent.db"
        db1 = Database(db_path=db_path)
        db2 = Database(db_path=db_path)
        # Both should work; verify tables exist
        tables = db2._get_conn().execute(
            "SELECT name FROM sqlite_master WHERE type='table'"
        ).fetchall()
        table_names = {t[0] for t in tables}
        assert "novels" in table_names
        assert "tasks" in table_names


class TestNovelCRUD:
    def test_insert_and_get_novel(self, tmp_db):
        """Round-trip: insert then get returns identical data."""
        tmp_db.insert_novel(
            novel_id=1001,
            title="Sample Novel",
            series_id=None,
            series_order=None,
            author_id=42,
            author_name="Author",
            page_count=5,
            text_length=1200,
            captured_at="2025-06-01",
        )
        row = tmp_db.get_novel(1001)
        assert row is not None
        assert row["novel_id"] == 1001
        assert row["title"] == "Sample Novel"
        assert row["author_id"] == 42

    def test_is_downloaded_true_after_insert(self, tmp_db):
        """is_downloaded returns True after insert."""
        assert tmp_db.is_downloaded(9999) is False
        tmp_db.insert_novel(
            novel_id=9999, title="T", series_id=None, series_order=None,
            author_id=1, captured_at="2025-01-01",
        )
        assert tmp_db.is_downloaded(9999) is True

    def test_list_novels_pagination(self, tmp_db):
        """Verify page and page_size parameters work correctly."""
        for i in range(12):
            tmp_db.insert_novel(
                novel_id=2000 + i, title=f"Novel {i}", series_id=None,
                series_order=None, author_id=1, captured_at=f"2025-01-{i+1:02d}",
            )
        result = tmp_db.list_novels(page=1, page_size=5)
        assert len(result["items"]) == 5
        assert result["total"] == 12
        assert result["page"] == 1
        assert result["page_size"] == 5

        result2 = tmp_db.list_novels(page=3, page_size=5)
        assert len(result2["items"]) == 2  # 12 - 10 = 2 remaining

    def test_delete_novel(self, tmp_db):
        """Delete removes the novel."""
        tmp_db.insert_novel(
            novel_id=3000, title="D", series_id=None, series_order=None,
            author_id=1, captured_at="2025-01-01",
        )
        assert tmp_db.is_downloaded(3000) is True
        tmp_db.delete_novel(3000)
        assert tmp_db.is_downloaded(3000) is False

    def test_insert_or_replace_novel(self, tmp_db):
        """INSERT OR REPLACE updates existing record."""
        tmp_db.insert_novel(
            novel_id=4000, title="Old", series_id=None, series_order=None,
            author_id=1, captured_at="2025-01-01",
        )
        tmp_db.insert_novel(
            novel_id=4000, title="New", series_id=None, series_order=None,
            author_id=2, captured_at="2025-06-01",
        )
        row = tmp_db.get_novel(4000)
        assert row["title"] == "New"
        assert row["author_id"] == 2

    def test_update_novel_paths_writes_txt_and_md(self, tmp_db):
        """update_novel_paths writes txt_path / md_path independently.

        Regression: crawler previously called db.update_task(novel_id=...)
        which targets the tasks table (and missed the required task_id arg),
        so the call raised and only the first exporter's file survived.
        """
        tmp_db.insert_novel(
            novel_id=5000, title="Paths", series_id=None, series_order=None,
            author_id=1, captured_at="2025-01-01",
        )
        # 初始为 None
        row = tmp_db.get_novel(5000)
        assert row["txt_path"] is None
        assert row["md_path"] is None

        # 模拟 crawler 的两次调用(每个 exporter 一次)
        tmp_db.update_novel_paths(novel_id=5000, txt_path="/dl/novel.txt")
        tmp_db.update_novel_paths(novel_id=5000, md_path="/dl/novel.md")

        row2 = tmp_db.get_novel(5000)
        assert row2["txt_path"] == "/dl/novel.txt"
        assert row2["md_path"] == "/dl/novel.md"

    def test_update_novel_paths_noop_when_both_none(self, tmp_db):
        """No-op when called with no fields — avoids empty UPDATE."""
        tmp_db.insert_novel(
            novel_id=5001, title="Noop", series_id=None, series_order=None,
            author_id=1, captured_at="2025-01-01",
        )
        # 不应抛异常
        tmp_db.update_novel_paths(novel_id=5001)
        row = tmp_db.get_novel(5001)
        assert row["txt_path"] is None
        assert row["md_path"] is None


class TestTaskCRUD:
    def test_insert_task_and_update(self, tmp_db):
        """Create task then update status — verify transitions."""
        tmp_db.insert_task(
            task_id="t-001", source_type="single", source_id="1234",
            status="pending", created_at="2025-01-01", updated_at="2025-01-01",
        )
        task = tmp_db.get_task("t-001")
        assert task is not None
        assert task["status"] == "pending"

        tmp_db.update_task("t-001", status="running", updated_at="2025-01-02")
        task2 = tmp_db.get_task("t-001")
        assert task2["status"] == "running"

    def test_list_tasks(self, tmp_db):
        """list_tasks returns all tasks."""
        tmp_db.insert_task(
            task_id="t-100", source_type="series", source_id="500",
            created_at="2025-01-01", updated_at="2025-01-01",
        )
        tmp_db.insert_task(
            task_id="t-101", source_type="user", source_id="600",
            created_at="2025-01-02", updated_at="2025-01-02",
        )
        tasks = tmp_db.list_tasks()
        assert len(tasks) == 2

    def test_delete_terminal_tasks_is_atomic(self, tmp_db):
        """终态任务可删除；混入进行中任务时整个批次不产生副作用。"""
        for task_id, status in (("done", "done"), ("running", "running")):
            tmp_db.insert_task(
                task_id=task_id,
                source_type="single",
                source_id="1",
                status=status,
                created_at="2025-01-01",
                updated_at="2025-01-01",
            )

        deleted, missing, non_terminal = tmp_db.delete_terminal_tasks(["done", "running"])

        assert deleted == 0
        assert missing == set()
        assert non_terminal == {"running"}
        assert tmp_db.get_task("done") is not None
        assert tmp_db.get_task("running") is not None

    def test_delete_terminal_tasks_and_completed_tasks(self, tmp_db):
        """单批删除所有终态，清理完成任务只影响 done。"""
        for task_id, status in (("done", "done"), ("failed", "failed"), ("canceled", "canceled")):
            tmp_db.insert_task(
                task_id=task_id,
                source_type="single",
                source_id="1",
                status=status,
                created_at="2025-01-01",
                updated_at="2025-01-01",
            )

        deleted, missing, non_terminal = tmp_db.delete_terminal_tasks(["failed", "canceled"])
        assert (deleted, missing, non_terminal) == (2, set(), set())
        assert tmp_db.get_task("failed") is None
        assert tmp_db.get_task("canceled") is None
        assert tmp_db.delete_completed_tasks() == 1
        assert tmp_db.get_task("done") is None


# ── Settings tests (ticket 04) ───────────────────────────────────────────────


class TestSettings:
    def test_settings_defaults(self, tmp_settings):
        """New settings.json gets default values."""
        settings_mod, settings_file, _ = tmp_settings
        s = settings_mod.get_settings()
        assert s.output_dir == "downloads"
        assert "txt" in s.output_formats
        assert s.language == "zh-CN"
        assert s.theme == "auto"

    def test_settings_backward_compat(self, tmp_path, monkeypatch):
        """Missing fields are filled with defaults on load."""
        from pixiv_tool.storage import settings as settings_mod

        config_dir = tmp_path / "cfg"
        config_dir.mkdir()
        settings_file = config_dir / "settings.json"
        # Write partial data (missing 'theme')
        settings_file.write_text(
            json.dumps({"output_dir": "custom", "language": "en"}),
            encoding="utf-8",
        )
        monkeypatch.setattr(settings_mod, "CONFIG_DIR", config_dir)
        monkeypatch.setattr(settings_mod, "SETTINGS_FILE", settings_file)
        monkeypatch.setattr(settings_mod, "_instance", None)

        s = settings_mod.get_settings()
        assert s.output_dir == "custom"
        assert s.language == "en"
        assert s.theme == "auto"  # filled from defaults

    def test_settings_corrupt_recovery(self, tmp_path, monkeypatch):
        """Corrupt JSON → backup created + defaults restored."""
        from pixiv_tool.storage import settings as settings_mod

        config_dir = tmp_path / "cfg2"
        config_dir.mkdir()
        settings_file = config_dir / "settings.json"
        settings_file.write_text("{corrupt json!!!", encoding="utf-8")

        monkeypatch.setattr(settings_mod, "CONFIG_DIR", config_dir)
        monkeypatch.setattr(settings_mod, "SETTINGS_FILE", settings_file)
        monkeypatch.setattr(settings_mod, "_instance", None)

        s = settings_mod.get_settings()
        # Default values restored
        assert s.output_dir == "downloads"
        # Backup file created
        backups = list(config_dir.glob("*.json.corrupt-*"))
        assert len(backups) == 1

    def test_settings_round_trip(self, tmp_settings):
        """Save and load preserves values."""
        settings_mod, settings_file, _ = tmp_settings
        s = settings_mod.get_settings()
        s.output_dir = "my_output"
        s.language = "ja"
        s.save()

        # Reload from disk
        monkeypatch_reset = True
        settings_mod._instance = None
        s2 = settings_mod.get_settings()
        assert s2.output_dir == "my_output"
        assert s2.language == "ja"

    def test_settings_singleton(self, tmp_settings):
        """get_settings returns the same instance."""
        settings_mod, _, _ = tmp_settings
        s1 = settings_mod.get_settings()
        s2 = settings_mod.get_settings()
        assert s1 is s2
