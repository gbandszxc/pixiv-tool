"""
Logging 配置 — root logger。
FileHandler → data/logs/app.log（UTF-8, INFO）。
dev 模式额外 StreamHandler 到 console。
"""

from __future__ import annotations

import logging
import sys
from pathlib import Path

_LOG_DIR = Path(__file__).resolve().parent.parent / "data" / "logs"
_LOG_FILE = _LOG_DIR / "app.log"

# 敏感字段脱敏 filter
_SENSITIVE_PATTERNS = ("PHPSESSID", "phpsessid", "device_token")


class _SensitiveFilter(logging.Filter):
    """自动掩码 cookie 敏感值（仅前 8 位 + masked）。"""

    def filter(self, record: logging.LogRecord) -> bool:
        msg = record.getMessage()
        for pat in _SENSITIVE_PATTERNS:
            if pat.lower() in msg.lower():
                # 简单掩码：找到值并截断
                record.msg = _mask_values(record.msg)
                record.args = None
        return True


def _mask_values(text: str) -> str:
    """粗略掩码：把看起来像 token 的长字符串截断。"""
    import re
    return re.sub(
        r'(["\']?\w{8,})(\w+)(["\']?)',
        lambda m: m.group(1) + "(masked)",
        text,
    )


def setup_logging(dev_mode: bool = False) -> None:
    """配置 root logger。"""
    _LOG_DIR.mkdir(parents=True, exist_ok=True)

    root = logging.getLogger()
    root.setLevel(logging.INFO)

    # 清除已有 handler（避免重复）
    root.handlers.clear()

    fmt = logging.Formatter("%(asctime)s [%(levelname)s] %(name)s: %(message)s")

    # FileHandler
    fh = logging.FileHandler(str(_LOG_FILE), encoding="utf-8")
    fh.setLevel(logging.INFO)
    fh.setFormatter(fmt)
    fh.addFilter(_SensitiveFilter())
    root.addHandler(fh)

    # dev 模式：StreamHandler
    if dev_mode or "--dev" in sys.argv:
        sh = logging.StreamHandler(sys.stdout)
        sh.setLevel(logging.DEBUG)
        sh.setFormatter(fmt)
        root.addHandler(sh)
