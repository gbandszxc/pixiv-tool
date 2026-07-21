"""数据模型 — dataclass 定义。"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class Novel:
    novel_id: int
    title: str
    series_id: int | None = None
    series_order: int | None = None
    author_id: int = 0
    author_name: str | None = None
    page_count: int | None = None
    text_length: int | None = None
    captured_at: str = ""
    modification_date: str | None = None
    txt_path: str | None = None
    md_path: str | None = None
    status: str = "ok"


@dataclass
class Task:
    task_id: str
    source_type: str  # 'single' | 'series' | 'user'
    source_id: str
    status: str = "pending"  # pending|running|paused|done|failed|canceled
    total: int = 0
    done: int = 0
    skipped: int = 0
    failed_ids: list[int] = field(default_factory=list)
    created_at: str = ""
    updated_at: str = ""
    error: str | None = None


@dataclass
class NovelData:
    """抓取接口返回的小说完整数据（供 Exporter 使用）。"""
    novel_id: int
    title: str
    author_id: int = 0
    author_name: str = ""
    series_id: int | None = None
    series_title: str | None = None
    series_order: int | None = None
    page_count: int = 1
    text_length: int = 0
    modification_date: str = ""
    content: str = ""  # 完整文本（多页拼接后）
