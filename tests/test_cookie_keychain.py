"""macOS Keychain CookieStore 的平台无关契约测试。"""

from __future__ import annotations

import sys
from types import ModuleType

from pixiv_tool.storage.cookie_keychain import KeychainCookieStore


def test_keychain_store_round_trip_and_clear(monkeypatch):
    values = {}

    class PasswordDeleteError(Exception):
        pass

    keyring = ModuleType("keyring")
    keyring.set_password = lambda service, account, value: values.__setitem__(
        (service, account), value
    )
    keyring.get_password = lambda service, account: values.get((service, account))

    def delete_password(service, account):
        if values.pop((service, account), None) is None:
            raise PasswordDeleteError

    keyring.delete_password = delete_password
    errors = ModuleType("keyring.errors")
    errors.PasswordDeleteError = PasswordDeleteError
    monkeypatch.setitem(sys.modules, "keyring", keyring)
    monkeypatch.setitem(sys.modules, "keyring.errors", errors)

    store = KeychainCookieStore()
    cookies = {"PHPSESSID": "session", "x-csrf-token": "csrf"}
    assert store.load() is None
    store.save(cookies)
    assert store.load() == cookies
    store.clear()
    assert store.load() is None
    store.clear()
