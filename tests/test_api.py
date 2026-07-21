"""Tests for backend.api — health, ping, novels, settings, auth
(tickets 01, 05, 07, 14, 15)."""

from __future__ import annotations

from unittest.mock import MagicMock, patch

import pytest
from fastapi.testclient import TestClient


@pytest.fixture()
def client(tmp_path, monkeypatch):
    """TestClient with monkeypatched module-level DB and settings."""
    from backend.storage.db import Database
    from backend.storage import settings as settings_mod

    # Patch settings to use tmp
    config_dir = tmp_path / "config"
    config_dir.mkdir()
    settings_file = config_dir / "settings.json"
    monkeypatch.setattr(settings_mod, "CONFIG_DIR", config_dir)
    monkeypatch.setattr(settings_mod, "SETTINGS_FILE", settings_file)
    monkeypatch.setattr(settings_mod, "_instance", None)

    # Patch Database to use tmp
    db_path = tmp_path / "test.db"
    db = Database(db_path=db_path)

    # Patch module-level DB instances in routers
    import backend.api.novels as novels_mod
    import backend.api.tasks as tasks_mod
    monkeypatch.setattr(novels_mod, "_db", db)
    monkeypatch.setattr(tasks_mod, "_db", db)

    # Patch cookie store to stub
    stub_store = MagicMock()
    stub_store.load.return_value = None
    stub_store.clear.return_value = None
    stub_store.save.return_value = None

    import backend.api.auth as auth_mod
    import backend.api.tasks as tasks_mod2
    monkeypatch.setattr(auth_mod, "_store", stub_store)
    monkeypatch.setattr(tasks_mod2, "_store", stub_store)

    # Patch settings get_settings
    monkeypatch.setattr(
        "backend.api.settings.get_settings",
        lambda: settings_mod.get_settings(),
    )

    # Import app AFTER patches. All routers (auth/novels/tasks/settings/system)
    # are already included in backend.main:app at import time, so the TestClient
    # sees the same route table as production.
    from backend.main import app
    return TestClient(app), db, stub_store, settings_mod, config_dir


# ── Health / Ping (tickets 01, 05) ──────────────────────────────────────────


class TestHealth:
    def test_health_endpoint(self, client):
        """GET /api/health returns ok."""
        tc, *_ = client

        resp = tc.get("/api/health")
        assert resp.status_code == 200
        assert resp.json() == {"status": "ok"}

    def test_ping_endpoint(self, client):
        """GET /api/ping returns pong timestamp."""
        tc, *_ = client
        resp = tc.get("/api/ping")
        assert resp.status_code == 200
        body = resp.json()
        assert "pong" in body


# ── Novels API (ticket 07) ──────────────────────────────────────────────────


class TestNovelsAPI:
    def test_list_novels_empty(self, client):
        """GET /api/novels with no data returns empty."""
        tc, db, *_ = client
        resp = tc.get("/api/novels")
        assert resp.status_code == 200
        body = resp.json()
        assert body["items"] == []
        assert body["total"] == 0

    def test_list_novels_with_data(self, client):
        """GET /api/novels returns inserted novels."""
        tc, db, *_ = client
        db.insert_novel(
            novel_id=5001, title="Test Novel", series_id=None,
            series_order=None, author_id=1, captured_at="2025-01-01",
        )
        resp = tc.get("/api/novels")
        body = resp.json()
        assert len(body["items"]) == 1
        assert body["items"][0]["novel_id"] == 5001

    def test_list_novels_pagination(self, client):
        """Pagination parameters work."""
        tc, db, *_ = client
        for i in range(5):
            db.insert_novel(
                novel_id=6000 + i, title=f"N{i}", series_id=None,
                series_order=None, author_id=1, captured_at=f"2025-01-{i+1:02d}",
            )
        resp = tc.get("/api/novels?page=1&page_size=2")
        body = resp.json()
        assert len(body["items"]) == 2
        assert body["total"] == 5


# ── Settings API (ticket 14) ─────────────────────────────────────────────────


class TestSettingsAPI:
    def test_settings_get(self, client):
        """GET /api/settings returns config dict."""
        tc, *_ = client
        resp = tc.get("/api/settings")
        assert resp.status_code == 200
        body = resp.json()
        assert "output_dir" in body
        assert "language" in body

    def test_settings_put(self, client):
        """PUT /api/settings updates and persists."""
        tc, db, _, settings_mod, config_dir = client
        resp = tc.put("/api/settings", json={"language": "ja"})
        assert resp.status_code == 200
        assert resp.json()["status"] == "success"
        # Verify persistence
        settings_mod._instance = None
        s = settings_mod.get_settings()
        assert s.language == "ja"

    def test_settings_get_put_round_trip(self, client):
        """Get → put → get round trip."""
        tc, *_ = client
        tc.put("/api/settings", json={"theme": "dark"})
        resp = tc.get("/api/settings")
        assert resp.json()["theme"] == "dark"


# ── Auth API (ticket 15) ─────────────────────────────────────────────────────


class TestAuthAPI:
    def test_auth_status_not_logged_in(self, client):
        """GET /api/auth/status returns is_logged_in false when no cookies."""
        tc, db, stub_store, *_ = client
        stub_store.load.return_value = None
        resp = tc.get("/api/auth/status")
        assert resp.status_code == 200
        assert resp.json()["is_logged_in"] is False

    def test_auth_status_not_logged_in_on_exception(self, client):
        """GET /api/auth/status returns false when store raises."""
        tc, db, stub_store, *_ = client
        stub_store.load.side_effect = NotImplementedError("stub")
        resp = tc.get("/api/auth/status")
        assert resp.json()["is_logged_in"] is False

    def test_auth_status_returns_profile_image(self, client, monkeypatch):
        """GET /api/auth/status maps Pixiv userData.profileImg for the sidebar."""
        tc, db, stub_store, *_ = client
        stub_store.load.return_value = {"PHPSESSID": "session", "x-csrf-token": "csrf"}

        class FakeResponse:
            status_code = 200

            @staticmethod
            def json():
                return {
                    "userData": {
                        "id": "19509348",
                        "pixivId": "gbandszxc",
                        "name": "用户名称",
                        "profileImg": "https://i.pximg.net/user-profile/img.png",
                    }
                }

        class FakeAsyncClient:
            def __init__(self, **_kwargs):
                pass

            async def __aenter__(self):
                return self

            async def __aexit__(self, *_args):
                return False

            @staticmethod
            async def get(*_args, **_kwargs):
                return FakeResponse()

        monkeypatch.setattr("httpx.AsyncClient", FakeAsyncClient)

        resp = tc.get("/api/auth/status")

        assert resp.status_code == 200
        assert resp.json() == {
            "is_logged_in": True,
            "user_id": "19509348",
            "pixiv_id": "gbandszxc",
            "name": "用户名称",
            "profile_img": "https://i.pximg.net/user-profile/img.png",
        }

    def test_auth_logout(self, client):
        """POST /api/auth/logout clears store."""
        tc, db, stub_store, *_ = client
        resp = tc.post("/api/auth/logout")
        assert resp.status_code == 200
        stub_store.clear.assert_called_once()
