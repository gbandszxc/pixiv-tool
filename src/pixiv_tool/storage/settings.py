"""
Settings 单例 — JSON 配置管理。
dataclass + 工厂，支持默认值填充、损坏恢复、持久化。
"""

from __future__ import annotations

import json
import logging
import shutil
from dataclasses import dataclass, field, asdict
from pathlib import Path
from threading import Lock

from .paths import CONFIG_DIR, default_output_dir

logger = logging.getLogger(__name__)

SETTINGS_FILE = CONFIG_DIR / "settings.json"

_DEFAULTS: dict = {
    # 默认输出目录：系统下载目录/pixiv-tool（区分平台，见 paths.default_output_dir）。
    "output_dir": default_output_dir,
    "output_formats": ["txt", "markdown"],
    "language": "zh-CN",
    "theme": "auto",
    "backend_port": None,
    # 任务最大等待时间（秒）：运行超过该时长自动标记失败，不含暂停时间。
    "max_wait_seconds": 180,
}


@dataclass
class Settings:
    output_dir: str = field(default_factory=lambda: str(default_output_dir()))
    output_formats: list[str] = field(default_factory=lambda: ["txt", "markdown"])
    language: str = "zh-CN"
    theme: str = "auto"
    backend_port: int | None = None
    max_wait_seconds: int = 180

    def save(self) -> None:
        """持久化到 settings.json。"""
        SETTINGS_FILE.parent.mkdir(parents=True, exist_ok=True)
        SETTINGS_FILE.write_text(
            json.dumps(asdict(self), ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
        logger.info("Settings 已保存")


_instance: Settings | None = None
_lock = Lock()


def get_settings() -> Settings:
    """获取 Settings 单例。"""
    global _instance  # noqa: WPS420
    if _instance is None:
        with _lock:
            if _instance is None:
                _instance = _load_or_create()
    return _instance


def _load_or_create() -> Settings:
    """从文件加载，不存在则创建默认，损坏则备份后重建。"""
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)

    if not SETTINGS_FILE.exists():
        s = Settings()
        s.save()
        logger.info("创建默认 settings.json")
        return s

    try:
        data = json.loads(SETTINGS_FILE.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, UnicodeDecodeError) as exc:
        # 损坏文件备份
        ts = Path(SETTINGS_FILE).stat().st_mtime_ns
        backup = SETTINGS_FILE.with_suffix(f".json.corrupt-{ts}")
        shutil.copy2(SETTINGS_FILE, backup)
        logger.warning("settings.json 损坏，已备份为 %s，使用默认值", backup.name)
        s = Settings()
        s.save()
        return s

    # 旧默认值迁移：早期默认是相对路径 "downloads"（锚定 data 目录），
    # 2026-08 起默认改为系统下载目录/pixiv-tool。存了旧默认字面量的
    # 配置直接迁移；用户显式改过的路径不动。
    if data.get("output_dir") == "downloads":
        data["output_dir"] = str(default_output_dir())

    # 默认值填充（向后兼容；output_dir 的默认是 callable，惰性求值）
    for key, default in _DEFAULTS.items():
        if key not in data:
            data[key] = default() if callable(default) else default

    return Settings(**{k: data[k] for k in _DEFAULTS if k in data})
