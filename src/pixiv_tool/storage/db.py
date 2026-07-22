"""
SQLite 数据库连接管理 + schema 初始化。
线程安全，FastAPI async 兼容。
"""

from __future__ import annotations

import json
import logging
import sqlite3
import threading
from contextlib import contextmanager
from pathlib import Path

from .paths import DATA_DIR

logger = logging.getLogger(__name__)

DB_DIR = DATA_DIR
DB_PATH = DB_DIR / "app.db"

_SCHEMA_SQL = """
CREATE TABLE IF NOT EXISTS novels (
    novel_id          INTEGER PRIMARY KEY,
    title             TEXT NOT NULL,
    series_id         INTEGER,
    series_order      INTEGER,
    author_id         INTEGER NOT NULL,
    author_name       TEXT,
    page_count        INTEGER,
    text_length       INTEGER,
    captured_at       TEXT NOT NULL,
    modification_date TEXT,
    txt_path          TEXT,
    md_path           TEXT,
    status            TEXT NOT NULL DEFAULT 'ok'
);
CREATE INDEX IF NOT EXISTS idx_novels_series ON novels(series_id);
CREATE INDEX IF NOT EXISTS idx_novels_author ON novels(author_id);

CREATE TABLE IF NOT EXISTS tasks (
    task_id     TEXT PRIMARY KEY,
    source_type TEXT NOT NULL,
    source_id   TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',
    total       INTEGER DEFAULT 0,
    done        INTEGER DEFAULT 0,
    skipped     INTEGER DEFAULT 0,
    failed_ids  TEXT DEFAULT '[]',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    error       TEXT
);
"""


class Database:
    """线程安全的 SQLite 封装。"""

    def __init__(self, db_path: Path = DB_PATH) -> None:
        self._db_path = db_path
        self._local = threading.local()
        DB_DIR.mkdir(parents=True, exist_ok=True)
        self._init_schema()

    def _get_conn(self) -> sqlite3.Connection:
        conn = getattr(self._local, "conn", None)
        if conn is None:
            conn = sqlite3.connect(str(self._db_path))
            conn.row_factory = sqlite3.Row
            conn.execute("PRAGMA journal_mode=WAL")
            conn.execute("PRAGMA foreign_keys=ON")
            self._local.conn = conn
        return conn

    @contextmanager
    def _transaction(self):
        conn = self._get_conn()
        try:
            yield conn
            conn.commit()
        except Exception:
            conn.rollback()
            raise

    def _init_schema(self) -> None:
        conn = self._get_conn()
        conn.executescript(_SCHEMA_SQL)
        logger.info("数据库 schema 初始化完成: %s", self._db_path)

    # ------------------------------------------------------------------
    # Novel CRUD
    # ------------------------------------------------------------------

    def insert_novel(self, novel_id: int, title: str, series_id: int | None,
                     series_order: int | None, author_id: int,
                     author_name: str | None = None, page_count: int | None = None,
                     text_length: int | None = None, captured_at: str = "",
                     modification_date: str | None = None,
                     txt_path: str | None = None, md_path: str | None = None,
                     status: str = "ok") -> None:
        with self._transaction() as conn:
            conn.execute(
                """INSERT OR REPLACE INTO novels
                   (novel_id, title, series_id, series_order, author_id, author_name,
                    page_count, text_length, captured_at, modification_date, txt_path, md_path, status)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (novel_id, title, series_id, series_order, author_id, author_name,
                 page_count, text_length, captured_at, modification_date, txt_path, md_path, status),
            )

    def update_novel_paths(self, novel_id: int,
                           txt_path: str | None = None,
                           md_path: str | None = None) -> None:
        """更新 novels 表的 txt_path / md_path。

        crawler 每导出一个文件就调一次,记录输出路径。只更新非 None 的字段。
        """
        fields: dict[str, str] = {}
        if txt_path is not None:
            fields["txt_path"] = txt_path
        if md_path is not None:
            fields["md_path"] = md_path
        if not fields:
            return
        set_clause = ", ".join(f"{k} = ?" for k in fields)
        values = list(fields.values()) + [novel_id]
        with self._transaction() as conn:
            conn.execute(
                f"UPDATE novels SET {set_clause} WHERE novel_id = ?", values
            )

    def get_novel(self, novel_id: int) -> dict | None:
        row = self._get_conn().execute(
            "SELECT * FROM novels WHERE novel_id = ?", (novel_id,)
        ).fetchone()
        return dict(row) if row else None

    def is_downloaded(self, novel_id: int) -> bool:
        row = self._get_conn().execute(
            "SELECT 1 FROM novels WHERE novel_id = ?", (novel_id,)
        ).fetchone()
        return row is not None

    def list_novels(self, page: int = 1, page_size: int = 50,
                    series_id: int | None = None, author_id: int | None = None,
                    keyword: str | None = None) -> dict:
        conditions: list[str] = []
        params: list = []
        if series_id is not None:
            conditions.append("series_id = ?")
            params.append(series_id)
        if author_id is not None:
            conditions.append("author_id = ?")
            params.append(author_id)
        if keyword:
            conditions.append("title LIKE ?")
            params.append(f"%{keyword}%")

        where = (" WHERE " + " AND ".join(conditions)) if conditions else ""
        conn = self._get_conn()

        total = conn.execute(f"SELECT COUNT(*) FROM novels{where}", params).fetchone()[0]
        offset = (page - 1) * page_size
        rows = conn.execute(
            f"SELECT * FROM novels{where} ORDER BY captured_at DESC LIMIT ? OFFSET ?",
            params + [page_size, offset],
        ).fetchall()

        return {
            "items": [dict(r) for r in rows],
            "total": total,
            "page": page,
            "page_size": page_size,
        }

    def delete_novel(self, novel_id: int) -> None:
        with self._transaction() as conn:
            conn.execute("DELETE FROM novels WHERE novel_id = ?", (novel_id,))

    def delete_novels_batch(self, novel_ids: list[int]) -> int:
        """批量删除 novel 记录,返回删除条数。"""
        if not novel_ids:
            return 0
        placeholders = ",".join("?" * len(novel_ids))
        with self._transaction() as conn:
            cur = conn.execute(
                f"DELETE FROM novels WHERE novel_id IN ({placeholders})",
                novel_ids,
            )
            return cur.rowcount or 0

    def delete_all_novels(self) -> int:
        """清空 novels 表,返回删除条数。"""
        with self._transaction() as conn:
            count = conn.execute("SELECT COUNT(*) FROM novels").fetchone()[0]
            conn.execute("DELETE FROM novels")
            return count

    # ------------------------------------------------------------------
    # Task CRUD
    # ------------------------------------------------------------------

    def insert_task(self, task_id: str, source_type: str, source_id: str,
                    status: str = "pending", created_at: str = "",
                    updated_at: str = "") -> None:
        with self._transaction() as conn:
            conn.execute(
                """INSERT INTO tasks (task_id, source_type, source_id, status, created_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?)""",
                (task_id, source_type, source_id, status, created_at, updated_at),
            )

    def update_task(self, task_id: str, **fields) -> None:
        if not fields:
            return
        set_clause = ", ".join(f"{k} = ?" for k in fields)
        values = list(fields.values()) + [task_id]
        with self._transaction() as conn:
            conn.execute(f"UPDATE tasks SET {set_clause} WHERE task_id = ?", values)

    def get_task(self, task_id: str) -> dict | None:
        row = self._get_conn().execute(
            "SELECT * FROM tasks WHERE task_id = ?", (task_id,)
        ).fetchone()
        return dict(row) if row else None

    def list_tasks(self) -> list[dict]:
        rows = self._get_conn().execute(
            "SELECT * FROM tasks ORDER BY created_at DESC"
        ).fetchall()
        return [dict(r) for r in rows]
