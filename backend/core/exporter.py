"""
Exporter 抽象 + txt / markdown 实现 + EpubExporter stub。
"""

from __future__ import annotations

import json
import re
from abc import ABC, abstractmethod
from pathlib import Path

from backend.storage.models import NovelData


class Exporter(ABC):
    """输出格式接口。"""

    @abstractmethod
    def export(self, novel: NovelData, target_dir: Path, series_order: int | None = None) -> list[Path]:
        """返回生成的文件路径列表。"""
        ...  # pragma: no cover


def sanitize_filename(name: str) -> str:
    """移除文件名非法字符。"""
    return re.sub(r'[<>:"/\\|?*\x00-\x1f]', '_', name).strip('_. ')


class TxtExporter(Exporter):
    """纯文本输出，多页用空行分隔。"""

    def export(self, novel: NovelData, target_dir: Path, series_order: int | None = None) -> list[Path]:
        target_dir.mkdir(parents=True, exist_ok=True)
        filename = _make_filename(novel, series_order, "txt")
        filepath = target_dir / filename
        filepath.write_text(novel.content, encoding="utf-8")
        return [filepath]


class MarkdownExporter(Exporter):
    """Markdown 输出：章节 → ##、[newpage] → ---、标记清理。"""

    def export(self, novel: NovelData, target_dir: Path, series_order: int | None = None) -> list[Path]:
        target_dir.mkdir(parents=True, exist_ok=True)
        filename = _make_filename(novel, series_order, "md")
        filepath = target_dir / filename
        content = _render_markdown(novel.content)
        filepath.write_text(content, encoding="utf-8")
        return [filepath]


class EpubExporter(Exporter):
    """V2 stub。"""

    def export(self, novel: NovelData, target_dir: Path, series_order: int | None = None) -> list[Path]:
        raise NotImplementedError("EPUB 导出留 V2 实现")


def _render_markdown(text: str) -> str:
    """pixiv 小说标记 → Markdown。"""
    lines = text.split("\n")
    result: list[str] = []
    for line in lines:
        stripped = line.strip()
        # 章节标记
        if stripped.startswith("[chapter:"):
            title = stripped[len("[chapter:"):-1]
            result.append(f"\n## {title}\n")
            continue
        # 换页
        if stripped == "[newpage]":
            result.append("\n---\n")
            continue
        # 跳转标记 — 移除
        if stripped.startswith("[jump:"):
            continue
        # 图片占位符
        if stripped.startswith("[pixivimage:") or stripped.startswith("[uploadedimage:"):
            result.append("<!-- 图片占位 -->\n")
            continue
        # ruby 注音 → base(ruby)
        stripped = re.sub(r'\[rb:([^>]+)>([^\]]+)\]', r'\1(\2)', stripped)
        result.append(line)
    return "\n".join(result)


def _make_filename(novel: NovelData, series_order: int | None, ext: str) -> str:
    """按 SPEC §4.4 命名。"""
    safe_title = sanitize_filename(novel.title)
    if series_order is not None:
        padded = str(series_order).zfill(2 if novel.page_count <= 99 else 3)
        return f"{padded}_{safe_title}.{ext}"
    return f"{safe_title}_{novel.novel_id}.{ext}"


def create_exporters(formats: list[str]) -> list[Exporter]:
    """根据配置选择启用哪些 exporter。"""
    registry: dict[str, type[Exporter]] = {
        "txt": TxtExporter,
        "markdown": MarkdownExporter,
        "epub": EpubExporter,
    }
    return [registry[f]() for f in formats if f in registry]
