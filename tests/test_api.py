"""Tests for pixiv_tool.api — health, ping, novels, settings, auth
(tickets 01, 05, 07, 14, 15)."""

from __future__ import annotations

import asyncio
import json
import os
from unittest.mock import MagicMock, patch

import pytest
from fastapi.testclient import TestClient


@pytest.fixture()
def client(tmp_path, monkeypatch):
    """TestClient with monkeypatched module-level DB and settings."""
    from pixiv_tool.storage.db import Database
    from pixiv_tool.storage import settings as settings_mod

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
    import pixiv_tool.api.novels as novels_mod
    import pixiv_tool.api.tasks as tasks_mod
    import pixiv_tool.api.illustrations as illust_mod
    import pixiv_tool.api.history as history_mod
    monkeypatch.setattr(novels_mod, "_db", db)
    monkeypatch.setattr(tasks_mod, "_db", db)
    monkeypatch.setattr(illust_mod, "_db", db)
    monkeypatch.setattr(history_mod, "_db", db)

    # Patch cookie store to stub
    stub_store = MagicMock()
    stub_store.load.return_value = None
    stub_store.clear.return_value = None
    stub_store.save.return_value = None

    import pixiv_tool.api.auth as auth_mod
    import pixiv_tool.api.tasks as tasks_mod2
    monkeypatch.setattr(auth_mod, "_store", stub_store)
    monkeypatch.setattr(tasks_mod2, "_store", stub_store)

    # Patch settings get_settings
    monkeypatch.setattr(
        "pixiv_tool.api.settings.get_settings",
        lambda: settings_mod.get_settings(),
    )

    # Import app AFTER patches. All routers (auth/novels/tasks/settings/system)
    # are already included in pixiv_tool.main:app at import time, so the TestClient
    # sees the same route table as production.
    from pixiv_tool.main import app
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


# ── Illustrations API (历史页插画兼容) ───────────────────────────────────────


class TestIllustrationsAPI:
    def _insert(self, db, artwork_id: int, title: str, saved_paths: str = "[]",
                captured_at: str = "2025-01-01", **kw):
        db.insert_illustration(
            artwork_id=artwork_id, title=title, author_id=1,
            saved_paths=saved_paths, captured_at=captured_at, **kw,
        )

    def test_list_illustrations_empty(self, client):
        """GET /api/illustrations with no data returns empty."""
        tc, *_ = client
        resp = tc.get("/api/illustrations")
        assert resp.status_code == 200
        body = resp.json()
        assert body["items"] == []
        assert body["total"] == 0

    def test_list_illustrations_with_data(self, client):
        """GET /api/illustrations returns inserted illustrations."""
        tc, db, *_ = client
        self._insert(db, 7001, "Test Illust")
        resp = tc.get("/api/illustrations")
        body = resp.json()
        assert len(body["items"]) == 1
        assert body["items"][0]["artwork_id"] == 7001

    def test_list_illustrations_keyword_filter(self, client):
        """keyword 按标题模糊过滤。"""
        tc, db, *_ = client
        self._insert(db, 7001, "夏色の風景")
        self._insert(db, 7002, "冬の街")
        resp = tc.get("/api/illustrations", params={"keyword": "夏"})
        body = resp.json()
        assert body["total"] == 1
        assert body["items"][0]["artwork_id"] == 7001

    def test_open_illustration_folder_windows(self, client, tmp_path, monkeypatch):
        """Windows 下用 explorer /select 定位第一个已保存文件。"""
        import pixiv_tool.platform as platform_mod
        tc, db, *_ = client
        img = tmp_path / "pic" / "test_7001_p0.png"
        img.parent.mkdir(parents=True)
        img.write_bytes(b"png")
        self._insert(db, 7001, "Test", saved_paths=json.dumps([str(img)]))

        popen = MagicMock()
        monkeypatch.setattr(platform_mod, "subprocess", MagicMock(Popen=popen))
        monkeypatch.setattr(platform_mod._platform, "system", lambda: "Windows")

        resp = tc.post("/api/illustrations/7001/open")
        assert resp.status_code == 200
        assert resp.json() == {"status": "success"}
        popen.assert_called_once_with(["explorer", "/select,", str(img)])

    def test_open_illustration_folder_macos(self, client, tmp_path, monkeypatch):
        """macOS 用 open -R 定位。"""
        import pixiv_tool.platform as platform_mod
        tc, db, *_ = client
        img = tmp_path / "pic" / "test_7001_p0.png"
        img.parent.mkdir(parents=True)
        img.write_bytes(b"png")
        self._insert(db, 7001, "Test", saved_paths=json.dumps([str(img)]))

        popen = MagicMock()
        monkeypatch.setattr(platform_mod, "subprocess", MagicMock(Popen=popen))
        monkeypatch.setattr(platform_mod._platform, "system", lambda: "Darwin")

        resp = tc.post("/api/illustrations/7001/open")
        assert resp.json() == {"status": "success"}
        popen.assert_called_once_with(["open", "-R", str(img)])

    def test_open_illustration_folder_linux_opens_dir(self, client, tmp_path, monkeypatch):
        """Linux 用 xdg-open 打开所在目录。"""
        import pixiv_tool.platform as platform_mod
        tc, db, *_ = client
        img = tmp_path / "pic" / "test_7001_p0.png"
        img.parent.mkdir(parents=True)
        img.write_bytes(b"png")
        self._insert(db, 7001, "Test", saved_paths=json.dumps([str(img)]))

        popen = MagicMock()
        monkeypatch.setattr(platform_mod, "subprocess", MagicMock(Popen=popen))
        monkeypatch.setattr(platform_mod._platform, "system", lambda: "Linux")

        resp = tc.post("/api/illustrations/7001/open")
        assert resp.json() == {"status": "success"}
        popen.assert_called_once_with(["xdg-open", str(img.parent)])

    def test_open_illustration_missing_file_falls_back_to_parent(self, client, tmp_path, monkeypatch):
        """文件已不存在时打开其父目录（若父目录存在）。"""
        import pixiv_tool.platform as platform_mod
        tc, db, *_ = client
        missing = tmp_path / "pic" / "gone_7001_p0.png"
        missing.parent.mkdir(parents=True)
        self._insert(db, 7001, "Test", saved_paths=json.dumps([str(missing)]))

        popen = MagicMock()
        monkeypatch.setattr(platform_mod, "subprocess", MagicMock(Popen=popen))
        monkeypatch.setattr(platform_mod._platform, "system", lambda: "Windows")

        resp = tc.post("/api/illustrations/7001/open")
        assert resp.json() == {"status": "success"}
        popen.assert_called_once_with(["explorer", "/select,", str(missing.parent)])

    def test_open_illustration_not_found(self, client):
        """记录不存在返回 error。"""
        tc, *_ = client
        resp = tc.post("/api/illustrations/9999/open")
        assert resp.json()["error"]

    def test_open_illustration_no_saved_files(self, client):
        """saved_paths 为空返回 error。"""
        tc, db, *_ = client
        self._insert(db, 7001, "Test")
        resp = tc.post("/api/illustrations/7001/open")
        assert resp.json()["error"]

    def test_delete_illustration_with_file(self, client, tmp_path):
        """delete_file=true 时删除记录并删文件。"""
        tc, db, *_ = client
        img = tmp_path / "pic" / "test_7001_p0.png"
        img.parent.mkdir(parents=True)
        img.write_bytes(b"png")
        self._insert(db, 7001, "Test", saved_paths=json.dumps([str(img)]))

        resp = tc.delete("/api/illustrations/7001", params={"delete_file": True})
        assert resp.json() == {"status": "success"}
        assert db.get_illustration(7001) is None
        assert not img.exists()

    def test_batch_delete_illustrations(self, client):
        """POST /api/illustrations/batch-delete 批量删除。"""
        tc, db, *_ = client
        self._insert(db, 7001, "A")
        self._insert(db, 7002, "B")
        resp = tc.post("/api/illustrations/batch-delete", json={"illustration_ids": [7001, 7002]})
        assert resp.json() == {"status": "success", "deleted": 2}
        assert db.list_illustrations()["total"] == 0

    def test_batch_delete_illustrations_rejects_empty(self, client):
        """空列表被拒绝。"""
        tc, *_ = client
        resp = tc.post("/api/illustrations/batch-delete", json={"illustration_ids": []})
        assert resp.json()["error"]

    def test_delete_all_illustrations(self, client):
        """DELETE /api/illustrations 清空。"""
        tc, db, *_ = client
        self._insert(db, 7001, "A")
        self._insert(db, 7002, "B")
        resp = tc.delete("/api/illustrations")
        assert resp.json() == {"status": "success", "deleted": 2}
        assert db.list_illustrations()["total"] == 0


# ── History API (全部/小说/插画 联合查询) ────────────────────────────────────


class TestHistoryAPI:
    def _seed(self, db):
        db.insert_novel(
            novel_id=6001, title="N 小说", series_id=100, series_order=1,
            author_id=1, author_name="作者A", page_count=3,
            captured_at="2025-01-01T00:00:00+00:00",
        )
        db.insert_illustration(
            artwork_id=7001, title="I 插画", author_id=1, author_name="作者B",
            illust_type=0, page_count=2,
            captured_at="2025-01-02T00:00:00+00:00",
        )
        db.insert_illustration(
            artwork_id=7002, title="N 插画二号", author_id=1, author_name="作者C",
            illust_type=2, page_count=1,
            captured_at="2025-01-03T00:00:00+00:00",
        )

    def test_history_empty(self, client):
        """GET /api/history with no data returns empty."""
        tc, *_ = client
        resp = tc.get("/api/history")
        assert resp.status_code == 200
        body = resp.json()
        assert body["items"] == []
        assert body["total"] == 0

    def test_history_all_merges_and_sorts(self, client):
        """全部：两表合并，按 captured_at 倒序。"""
        tc, db, *_ = client
        self._seed(db)
        resp = tc.get("/api/history")
        body = resp.json()
        assert body["total"] == 3
        # 倒序：I 插画二号(01-03) > I 插画(01-02) > N 小说(01-01)
        assert [row["id"] for row in body["items"]] == [7002, 7001, 6001]
        assert [row["category"] for row in body["items"]] == [
            "illustration", "illustration", "novel",
        ]
        assert body["items"][0]["pages"] == 1
        assert body["items"][2]["pages"] == 3
        assert body["items"][2]["series_id"] == 100
        assert body["items"][2]["illust_type"] is None
        assert body["items"][0]["illust_type"] == 2

    def test_history_category_filter(self, client):
        """category=novel / illustration 只返回对应分类。"""
        tc, db, *_ = client
        self._seed(db)
        resp = tc.get("/api/history", params={"category": "novel"})
        body = resp.json()
        assert body["total"] == 1
        assert body["items"][0]["category"] == "novel"

        resp = tc.get("/api/history", params={"category": "illustration"})
        body = resp.json()
        assert body["total"] == 2
        assert all(r["category"] == "illustration" for r in body["items"])

    def test_history_keyword_filters_both_tables(self, client):
        """keyword 跨两表模糊匹配标题。"""
        tc, db, *_ = client
        self._seed(db)
        resp = tc.get("/api/history", params={"keyword": "N "})
        body = resp.json()
        assert body["total"] == 2
        assert {r["id"] for r in body["items"]} == {6001, 7002}

    def test_history_pagination(self, client):
        """分页参数生效。"""
        tc, db, *_ = client
        self._seed(db)
        resp = tc.get("/api/history", params={"page": 1, "page_size": 2})
        body = resp.json()
        assert len(body["items"]) == 2
        assert body["total"] == 3

    def test_history_invalid_category_rejected(self, client):
        """非法 category 返回 error。"""
        tc, *_ = client
        resp = tc.get("/api/history", params={"category": "music"})
        assert resp.json()["error"]


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

    def test_settings_max_wait_default_and_round_trip(self, client):
        """GET 返回 max_wait_seconds 默认 180；PUT 可修改并持久化。"""
        tc, db, _, settings_mod, _ = client
        resp = tc.get("/api/settings")
        assert resp.json()["max_wait_seconds"] == 180

        resp = tc.put("/api/settings", json={"max_wait_seconds": 300})
        assert resp.status_code == 200
        settings_mod._instance = None
        assert settings_mod.get_settings().max_wait_seconds == 300

    def test_put_max_wait_out_of_range_rejected(self, client):
        """max_wait_seconds 超出 30~86400 → 400。"""
        tc, *_ = client
        for bad in (10, 100000, "180", True):
            resp = tc.put("/api/settings", json={"max_wait_seconds": bad})
            assert resp.status_code == 400, f"{bad!r} should be rejected"

    def test_put_output_dir_non_str_rejected(self, client):
        """非字符串 output_dir → 400。"""
        tc, *_ = client
        resp = tc.put("/api/settings", json={"output_dir": 123})
        assert resp.status_code == 400
        assert "detail" in resp.json()

    def test_put_output_dir_empty_rejected(self, client):
        """空/纯空白 output_dir → 400。"""
        tc, *_ = client
        resp = tc.put("/api/settings", json={"output_dir": "   "})
        assert resp.status_code == 400
        assert "detail" in resp.json()

    def test_put_output_dir_control_char_rejected(self, client):
        """含控制字符的 output_dir → 400。"""
        tc, *_ = client
        resp = tc.put("/api/settings", json={"output_dir": "dl\x01dir"})
        assert resp.status_code == 400
        assert "detail" in resp.json()

    @pytest.mark.skipif(os.name != "nt", reason="非法字符校验仅 Windows 生效")
    def test_put_output_dir_illegal_char_rejected(self, client):
        """Windows 下含 ? 的 output_dir → 400。"""
        tc, *_ = client
        resp = tc.put("/api/settings", json={"output_dir": "dl?dir"})
        assert resp.status_code == 400
        assert "detail" in resp.json()

    def test_put_output_dir_existing_file_rejected(self, client, tmp_path):
        """output_dir 指向已有文件 → 400。"""
        tc, *_ = client
        existing = tmp_path / "a_file"
        existing.write_text("x", encoding="utf-8")
        resp = tc.put("/api/settings", json={"output_dir": str(existing)})
        assert resp.status_code == 400
        assert "detail" in resp.json()

    def test_put_output_dir_uncreatable_rejected(self, client, tmp_path):
        """父路径是文件导致无法创建 → 400。"""
        tc, *_ = client
        blocker = tmp_path / "a_file"
        blocker.write_text("x", encoding="utf-8")
        resp = tc.put("/api/settings", json={"output_dir": str(blocker / "sub")})
        assert resp.status_code == 400
        assert "detail" in resp.json()

    def test_put_output_dir_valid_creates_dir(self, client, tmp_path):
        """合法绝对路径 → 200，目录被创建且设置持久化。"""
        tc, db, _, settings_mod, config_dir = client
        target = tmp_path / "new_downloads"
        resp = tc.put("/api/settings", json={"output_dir": str(target)})
        assert resp.status_code == 200
        assert resp.json()["status"] == "success"
        assert target.is_dir()
        settings_mod._instance = None
        assert settings_mod.get_settings().output_dir == str(target)

    def test_select_directory_picked(self, client, monkeypatch):
        """select-directory 返回选中路径。"""
        tc, *_ = client
        monkeypatch.setattr(
            "pixiv_tool.api.settings._pick_directory", lambda: "/tmp/picked"
        )
        resp = tc.post("/api/settings/select-directory")
        assert resp.status_code == 200
        assert resp.json() == {"path": "/tmp/picked"}

    def test_select_directory_cancelled(self, client, monkeypatch):
        """select-directory 取消时返回 null。"""
        tc, *_ = client
        monkeypatch.setattr("pixiv_tool.api.settings._pick_directory", lambda: None)
        resp = tc.post("/api/settings/select-directory")
        assert resp.status_code == 200
        assert resp.json() == {"path": None}


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

        # auth_status 现在走 http_factory.create_client(返回 HTTPClient,非 async with)。
        # 伪 client 需提供 async get + async close。
        class FakeHTTPClient:
            backend = "test"

            def __init__(self, *_args, **_kwargs):
                pass

            @staticmethod
            async def get(*_args, **_kwargs):
                return FakeResponse()

            @staticmethod
            async def close():
                pass

        monkeypatch.setattr("pixiv_tool.core.http_factory.create_client", lambda _c: FakeHTTPClient())

        resp = tc.get("/api/auth/status")

        assert resp.status_code == 200
        assert resp.json() == {
            "is_logged_in": True,
            "user_id": "19509348",
            "pixiv_id": "gbandszxc",
            "name": "用户名称",
            "profile_img": "https://i.pximg.net/user-profile/img.png",
        }

    def test_auth_status_accepts_nested_user_data_and_large_avatar(self, client, monkeypatch):
        """兼容 Pixiv 响应变体，并在小头像缺失时回退大头像。"""
        tc, db, stub_store, *_ = client
        stub_store.load.return_value = {"PHPSESSID": "session"}

        class FakeResponse:
            status_code = 200

            @staticmethod
            def json():
                return {
                    "body": {
                        "userData": {
                            "id": "1",
                            "pixivId": "pixiv-user",
                            "name": "Pixiv User",
                            "profileImgBig": "https://i.pximg.net/user-profile/large.png",
                        }
                    }
                }

        class FakeHTTPClient:
            backend = "test"

            def __init__(self, *_args, **_kwargs):
                pass

            @staticmethod
            async def get(*_args, **_kwargs):
                return FakeResponse()

            @staticmethod
            async def close():
                pass

        monkeypatch.setattr("pixiv_tool.core.http_factory.create_client", lambda _c: FakeHTTPClient())

        resp = tc.get("/api/auth/status")

        assert resp.status_code == 200
        assert resp.json()["profile_img"] == "https://i.pximg.net/user-profile/large.png"

    def test_auth_status_timeout_keeps_saved_cookies(self, client, monkeypatch):
        """网络超时只快速显示未登录，不应丢失可供后续重试的本地 Cookie。"""
        tc, db, stub_store, *_ = client
        stub_store.load.return_value = {"PHPSESSID": "session"}

        class FakeHTTPClient:
            backend = "test"

            def __init__(self, *_args, **_kwargs):
                pass

            @staticmethod
            async def get(*_args, **_kwargs):
                await asyncio.sleep(0.05)

            @staticmethod
            async def close():
                pass

        import pixiv_tool.api.auth as auth_mod
        monkeypatch.setattr("pixiv_tool.core.http_factory.create_client", lambda _c: FakeHTTPClient())
        monkeypatch.setattr(auth_mod, "_AUTH_STATUS_TIMEOUT_SEC", 0.01)

        resp = tc.get("/api/auth/status")

        assert resp.status_code == 200
        assert resp.json() == {"is_logged_in": False}
        stub_store.clear.assert_not_called()

    def test_auth_status_unauthorized_clears_saved_cookies(self, client, monkeypatch):
        """仅明确的 401/403 认证失败会清理本地 Cookie。"""
        tc, db, stub_store, *_ = client
        stub_store.load.return_value = {"PHPSESSID": "session"}

        class FakeResponse:
            status_code = 401

        class FakeHTTPClient:
            backend = "test"

            def __init__(self, *_args, **_kwargs):
                pass

            @staticmethod
            async def get(*_args, **_kwargs):
                return FakeResponse()

            @staticmethod
            async def close():
                pass

        monkeypatch.setattr("pixiv_tool.core.http_factory.create_client", lambda _c: FakeHTTPClient())

        resp = tc.get("/api/auth/status")

        assert resp.status_code == 200
        assert resp.json() == {"is_logged_in": False}
        stub_store.clear.assert_called_once()

    def test_auth_logout(self, client):
        """POST /api/auth/logout clears store."""
        tc, db, stub_store, *_ = client
        resp = tc.post("/api/auth/logout")
        assert resp.status_code == 200
        stub_store.clear.assert_called_once()

    def test_login_returns_verified_user_without_second_status_request(self, client, monkeypatch):
        """真实浏览器已验证的用户资料直接回传。"""
        tc, db, stub_store, *_ = client
        from unittest.mock import AsyncMock

        monkeypatch.setattr(
            "pixiv_tool.auth.browser_login.open_browser_login",
            AsyncMock(return_value={
                "status": "success",
                "cookies": {"PHPSESSID": "session", "x-csrf-token": "csrf"},
                "user": {
                    "user_id": "19509348",
                    "pixiv_id": "gbandszxc",
                    "name": "测试用户",
                    "profile_img": "https://i.pximg.net/user-profile/avatar.png",
                },
            }),
        )

        resp = tc.post("/api/auth/login")

        assert resp.status_code == 200
        assert resp.json()["status"] == "success"
        assert resp.json()["user"]["pixiv_id"] == "gbandszxc"
        stub_store.save.assert_called_once_with({"PHPSESSID": "session", "x-csrf-token": "csrf"})


# ── 手动 Session 登录 ───────────────────────────────────────────────────────


class TestManualLoginAPI:
    def test_manual_login_rejects_empty_phpsessid(self, client):
        """空 PHPSESSID → 400。"""
        tc, *_ = client
        resp = tc.post("/api/auth/login/manual", data={"PHPSESSID": "  "})
        assert resp.status_code == 400

    def test_manual_login_auto_fills_csrf_from_probe(self, client, monkeypatch):
        """后端用 PHPSESSID 调 self 接口校验并获取 token。"""
        tc, db, stub_store, *_ = client

        # auth.py 在函数内 `from pixiv_tool.core.csrf import fetch_session_probe`,
        # 所以 patch csrf 模块属性即可影响每次调用(每次请求重新 import)。
        import pixiv_tool.core.csrf as csrf_real

        async def fake_probe(php):
            assert php == "session_xyz"
            return csrf_real.SessionProbe(
                csrf_token="auto_filled_csrf",
                is_logged_in=True,
                user={"user_id": "1", "pixiv_id": "tester", "name": "Tester"},
            )

        monkeypatch.setattr(csrf_real, "fetch_session_probe", fake_probe)

        resp = tc.post("/api/auth/login/manual", data={"PHPSESSID": "session_xyz"})

        assert resp.status_code == 200
        body = resp.json()
        assert body["status"] == "success"
        assert body["user"]["pixiv_id"] == "tester"
        # 存进 store 的 cookies 含自动补的 csrf
        stub_store.save.assert_called_once_with(
            {"PHPSESSID": "session_xyz", "x-csrf-token": "auto_filled_csrf"}
        )

    def test_manual_login_accepts_cookie_header_format(self, client, monkeypatch):
        tc, db, stub_store, *_ = client
        import pixiv_tool.core.csrf as csrf_real

        async def fake_probe(php):
            assert php == "session_abc"
            return csrf_real.SessionProbe("csrf", True)

        monkeypatch.setattr(csrf_real, "fetch_session_probe", fake_probe)
        resp = tc.post(
            "/api/auth/login/manual",
            data={"PHPSESSID": "foo=bar; PHPSESSID=session_abc; baz=qux"},
        )

        assert resp.status_code == 200
        stub_store.save.assert_called_once_with(
            {"PHPSESSID": "session_abc", "x-csrf-token": "csrf"}
        )

    def test_manual_login_invalid_session_returns_400(self, client, monkeypatch):
        """无效 PHPSESSID(重定向循环) → 400 + 明确错误。"""
        tc, *_ = client
        import pixiv_tool.core.csrf as csrf_real

        async def fake_probe(php):
            raise csrf_real.InvalidSessionError("PHPSESSID 无效或已过期")

        monkeypatch.setattr(csrf_real, "fetch_session_probe", fake_probe)

        resp = tc.post("/api/auth/login/manual", data={"PHPSESSID": "bad"})

        assert resp.status_code == 400
        assert "无效或已过期" in resp.json()["detail"]

    def test_manual_login_csrf_extraction_failure_returns_502(self, client, monkeypatch):
        """首页能访问但解析不出 csrf(Pixiv 改版) → 502。"""
        tc, *_ = client
        import pixiv_tool.core.csrf as csrf_real

        async def fake_probe(php):
            raise csrf_real.CsrfExtractionError("未找到 csrf token")

        monkeypatch.setattr(csrf_real, "fetch_session_probe", fake_probe)

        resp = tc.post("/api/auth/login/manual", data={"PHPSESSID": "ok_session"})

        assert resp.status_code == 502
        assert "csrf token" in resp.json()["detail"]


# ── Tasks API (任务删除) ─────────────────────────────────────────────────────


class TestTasksAPI:
    @staticmethod
    def _insert_task(db, task_id: str, status: str) -> None:
        db.insert_task(
            task_id=task_id,
            source_type="single",
            source_id="123",
            status=status,
            created_at="2025-01-01",
            updated_at="2025-01-01",
        )

    def test_delete_single_terminal_task(self, client):
        tc, db, *_ = client
        self._insert_task(db, "done-task", "done")

        resp = tc.delete("/api/tasks/done-task")

        assert resp.status_code == 200
        assert resp.json() == {"deleted": 1}
        assert db.get_task("done-task") is None

    def test_delete_task_not_found(self, client):
        tc, *_ = client

        resp = tc.delete("/api/tasks/missing")

        assert resp.status_code == 404

    def test_batch_delete_includes_active_tasks(self, client):
        """混合批次（终态 + 进行中）全部删除，进行中任务先被取消。"""
        tc, db, *_ = client
        self._insert_task(db, "done-task", "done")
        self._insert_task(db, "running-task", "running")

        resp = tc.request("DELETE", "/api/tasks", json={"task_ids": ["done-task", "running-task"]})

        assert resp.status_code == 200
        assert resp.json() == {"deleted": 2}
        assert db.get_task("done-task") is None
        assert db.get_task("running-task") is None

    def test_delete_single_active_task(self, client):
        """进行中任务可单独删除（先取消再删记录）。"""
        tc, db, *_ = client
        self._insert_task(db, "pending-task", "pending")

        resp = tc.delete("/api/tasks/pending-task")

        assert resp.status_code == 200
        assert resp.json() == {"deleted": 1}
        assert db.get_task("pending-task") is None

    def test_batch_delete_rejects_empty_selection(self, client):
        tc, *_ = client

        resp = tc.request("DELETE", "/api/tasks", json={"task_ids": []})

        assert resp.status_code == 422

    def test_clear_completed_tasks_keeps_other_terminal_states(self, client):
        tc, db, *_ = client
        self._insert_task(db, "done-task", "done")
        self._insert_task(db, "failed-task", "failed")
        self._insert_task(db, "canceled-task", "canceled")

        resp = tc.delete("/api/tasks/completed")

        assert resp.status_code == 200
        assert resp.json() == {"deleted": 1}
        assert db.get_task("done-task") is None
        assert db.get_task("failed-task") is not None
        assert db.get_task("canceled-task") is not None
