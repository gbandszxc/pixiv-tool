"""CookieStore 抽象基类 + 平台工厂。"""

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
        from pixiv_tool.storage.cookie_dpapi import DpapiCookieStore
        return DpapiCookieStore()
    if sys.platform == "darwin":
        from pixiv_tool.storage.cookie_keychain import KeychainCookieStore
        return KeychainCookieStore()
    # Linux 暂无 Secret Service 实现。
    return _StubCookieStore()


class _StubCookieStore(CookieStore):
    """Linux 临时 stub，后续接 Secret Service。"""

    def save(self, cookies: dict) -> None:
        raise NotImplementedError("Linux Secret Service 尚未实现")

    def load(self) -> dict | None:
        raise NotImplementedError("Linux Secret Service 尚未实现")
    def clear(self) -> None:
        pass  # stub: no-op
