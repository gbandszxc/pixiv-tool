"""macOS Keychain 登录态存储。"""

from __future__ import annotations

import json

from pixiv_tool.storage.cookies import CookieStore

_SERVICE = "pixiv-tool.cookies"
_ACCOUNT = "default"


class KeychainCookieStore(CookieStore):
    """通过 macOS Keychain 加密保存 Cookie。"""

    def save(self, cookies: dict) -> None:
        import keyring

        keyring.set_password(
            _SERVICE,
            _ACCOUNT,
            json.dumps(cookies, ensure_ascii=False, separators=(",", ":")),
        )

    def load(self) -> dict | None:
        import keyring

        payload = keyring.get_password(_SERVICE, _ACCOUNT)
        if payload is None:
            return None
        try:
            cookies = json.loads(payload)
        except (json.JSONDecodeError, TypeError) as exc:
            raise RuntimeError("macOS Keychain 中的登录态已损坏，请重新登录") from exc
        if not isinstance(cookies, dict):
            raise RuntimeError("macOS Keychain 中的登录态格式无效，请重新登录")
        return cookies

    def clear(self) -> None:
        import keyring
        from keyring.errors import PasswordDeleteError

        try:
            keyring.delete_password(_SERVICE, _ACCOUNT)
        except PasswordDeleteError:
            pass

