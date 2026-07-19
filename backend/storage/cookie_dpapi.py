"""
Windows DPAPI cookie 加密存储。
纯 ctypes 调用 crypt32.dll，不依赖 pywin32。
"""

from __future__ import annotations

import ctypes
import ctypes.wintypes
import json
import logging
from pathlib import Path

from backend.storage.cookies import CookieStore

logger = logging.getLogger(__name__)

COOKIE_FILE = Path(__file__).resolve().parent.parent.parent / "config" / "cookies.dat"

# crypt32.dll 常量
CRYPTPROTECT_UI_FORBIDDEN = 0x1
CRYPTPROTECT_LOCAL_MACHINE = 0x0

# ctypes 结构
class DATA_BLOB(ctypes.Structure):
    _fields_ = [("cbData", ctypes.wintypes.DWORD), ("pbData", ctypes.POINTER(ctypes.c_char))]


_crypt32 = ctypes.windll.crypt32  # type: ignore[attr-defined]
_kernel32 = ctypes.windll.kernel32  # type: ignore[attr-defined]


def _encrypt(data: bytes) -> bytes:
    """DPAPI 加密。"""
    input_blob = DATA_BLOB(len(data), ctypes.create_string_buffer(data, len(data)))
    output_blob = DATA_BLOB()
    if not _crypt32.CryptProtectData(
        ctypes.byref(input_blob),
        None, None, None, None,
        CRYPTPROTECT_UI_FORBIDDEN,
        ctypes.byref(output_blob),
    ):
        raise OSError(f"CryptProtectData 失败, error={ctypes.get_last_error()}")
    result = ctypes.string_at(output_blob.pbData, output_blob.cbData)
    _kernel32.LocalFree(output_blob.pbData)
    return result


def _decrypt(data: bytes) -> bytes:
    """DPAPI 解密。"""
    input_blob = DATA_BLOB(len(data), ctypes.create_string_buffer(data, len(data)))
    output_blob = DATA_BLOB()
    if not _crypt32.CryptUnprotectData(
        ctypes.byref(input_blob),
        None, None, None, None,
        CRYPTPROTECT_UI_FORBIDDEN,
        ctypes.byref(output_blob),
    ):
        raise OSError("CryptUnprotectData 失败 — 可能换了 Windows 用户，请重新登录")
    result = ctypes.string_at(output_blob.pbData, output_blob.cbData)
    _kernel32.LocalFree(output_blob.pbData)
    return result


class DpapiCookieStore(CookieStore):
    """Windows DPAPI 实现。"""

    def __init__(self, path: Path = COOKIE_FILE) -> None:
        self._path = path
        self._path.parent.mkdir(parents=True, exist_ok=True)

    def save(self, cookies: dict) -> None:
        raw = json.dumps(cookies, ensure_ascii=False).encode("utf-8")
        encrypted = _encrypt(raw)
        self._path.write_bytes(encrypted)
        logger.info("Cookie 已保存: %s", _mask_path(str(self._path)))

    def load(self) -> dict | None:
        if not self._path.exists():
            return None
        try:
            encrypted = self._path.read_bytes()
            raw = _decrypt(encrypted)
            return json.loads(raw.decode("utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            logger.warning("Cookie 加载失败: %s", exc)
            raise

    def clear(self) -> None:
        if self._path.exists():
            self._path.unlink()
            logger.info("Cookie 已清除")


def _mask_path(p: str) -> str:
    """日志脱敏：仅保留路径。"""
    return p
