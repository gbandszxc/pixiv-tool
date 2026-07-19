"""
CookieStore 抽象基类 + 工厂函数。
V1 仅 Windows DPAPI 实现，macOS/Linux 抛 NotImplementedError。
"""

from __future__ import annotations

import sys
from abc import ABC, abstractmethod


class CookieStore(ABC):
    """登录态 cookie 的安全存储接口。"""

    @abstractmethod
    def save(self, cookies: dict) -> None:
        """保存 cookie dict（含 PHPSESSID, x-csrf-token 等）。"""

    @abstractmethod
    def load(self) -> dict | None:
        """读取 cookie，文件不存在或解密失败返回 None 或抛异常。"""
    @abstractmethod
    def clear(self) -> None:
        """删除本地 cookie 文件。"""


def create_cookie_store() -> CookieStore:
    """按平台选择实现。"""
    if sys.platform == "win32":
        from backend.storage.cookie_dpapi import DpapiCookieStore
        return DpapiCookieStore()
    # macOS / Linux: V2 stub
    return _StubCookieStore()


class _StubCookieStore(CookieStore):
    """macOS/Linux 临时 stub，V2 实现 keychain / secretstorage。"""

    def save(self, cookies: dict) -> None:
        raise NotImplementedError("V2 实现 keychain/secretstorage")

    def load(self) -> dict | None:
        raise NotImplementedError("V2 实现 keychain/secretstorage")
    def clear(self) -> None:
        pass  # stub: no-op
