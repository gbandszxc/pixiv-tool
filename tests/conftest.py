"""Shared fixtures for pixiv-tool tests."""

from __future__ import annotations

import json
import sys
from pathlib import Path
from unittest.mock import MagicMock

import pytest

# Ensure backend package is importable
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))


@pytest.fixture()
def tmp_db(tmp_path):
    """Temporary SQLite Database — auto-cleanup."""
    from backend.storage.db import Database

    db_path = tmp_path / "test.db"
    db = Database(db_path=db_path)
    return db


@pytest.fixture()
def sample_novel_data():
    """Sample NovelData for exporter tests."""
    from backend.storage.models import NovelData

    return NovelData(
        novel_id=12345,
        title="テスト小説",
        author_id=9999,
        author_name="テスト作者",
        series_id=100,
        series_title="テストシリーズ",
        series_order=1,
        page_count=3,
        text_length=500,
        modification_date="2025-01-01",
        content="第一章\n[chapter:第二章]\n正文\n[newpage]\n[jump:3]\n第二页",
    )


@pytest.fixture()
def sample_task_data():
    """Sample task fields for task tests."""
    return {
        "task_id": "test-task-001",
        "source_type": "single",
        "source_id": "12345",
        "status": "pending",
        "created_at": "2025-01-01T00:00:00+00:00",
        "updated_at": "2025-01-01T00:00:00+00:00",
    }


@pytest.fixture()
def tmp_settings(tmp_path, monkeypatch):
    """Temporary Settings with monkeypatched file paths."""
    from backend.storage import settings as settings_mod

    config_dir = tmp_path / "config"
    config_dir.mkdir()
    settings_file = config_dir / "settings.json"

    monkeypatch.setattr(settings_mod, "CONFIG_DIR", config_dir)
    monkeypatch.setattr(settings_mod, "SETTINGS_FILE", settings_file)
    # Reset singleton so it picks up new paths
    monkeypatch.setattr(settings_mod, "_instance", None)

    return settings_mod, settings_file, config_dir
