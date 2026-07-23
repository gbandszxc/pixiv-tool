"""用真实 Chromium 浏览器完成 Pixiv 登录并通过 CDP 读取 Cookie。"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import shutil
import socket
import subprocess
import sys
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

import httpx
from websockets.asyncio.client import connect

from pixiv_tool.core.csrf import InvalidSessionError, fetch_session_probe
from pixiv_tool.storage.paths import CONFIG_DIR

logger = logging.getLogger(__name__)

LOGIN_URL = "https://accounts.pixiv.net/login"
LOGIN_TIMEOUT_SEC = 300
PROFILE_DIR = CONFIG_DIR / "login-browser-profile"


class BrowserNotFoundError(RuntimeError):
    """系统没有可通过 CDP 控制的 Chrome / Edge / Chromium。"""


def find_login_browser() -> Path:
    """返回首个已安装的 Chromium 浏览器。"""
    candidates: list[Path] = []
    if sys.platform == "win32":
        for root in (
            os.environ.get("PROGRAMFILES"),
            os.environ.get("PROGRAMFILES(X86)"),
            os.environ.get("LOCALAPPDATA"),
        ):
            if root:
                candidates.extend(
                    (
                        Path(root) / "Google/Chrome/Application/chrome.exe",
                        Path(root) / "Microsoft/Edge/Application/msedge.exe",
                    )
                )
    elif sys.platform == "darwin":
        candidates.extend(
            Path(path)
            for path in (
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                "/Applications/Chromium.app/Contents/MacOS/Chromium",
            )
        )
    else:
        candidates.extend(
            Path(path)
            for name in ("google-chrome", "microsoft-edge", "chromium", "chromium-browser")
            if (path := shutil.which(name))
        )

    browser = next((path for path in candidates if path.is_file()), None)
    if browser is None:
        raise BrowserNotFoundError("未找到 Chrome、Edge 或 Chromium")
    return browser


def extract_pixiv_cookies(items: list[dict[str, Any]]) -> dict[str, str]:
    """从 CDP Cookie 列表提取 Pixiv 域 Cookie，不记录或打印其值。"""
    return {
        str(item["name"]): str(item["value"])
        for item in items
        if str(item.get("domain", "")).lstrip(".").endswith("pixiv.net")
        and item.get("name")
        and item.get("value")
    }


def has_pixiv_main_target(items: list[dict[str, Any]]) -> bool:
    """登录页跳回 Pixiv 主站后才探测 Session，避免验证码阶段制造额外请求。"""
    return any(
        urlparse(str(item.get("url", ""))).hostname in {"pixiv.net", "www.pixiv.net"}
        for item in items
        if item.get("type") == "page"
    )


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


async def _wait_for_cdp(port: int) -> str:
    url = f"http://127.0.0.1:{port}/json/version"
    async with httpx.AsyncClient(trust_env=False) as client:
        for _ in range(50):
            try:
                response = await client.get(url, timeout=0.3)
                websocket_url = response.json().get("webSocketDebuggerUrl")
                if websocket_url:
                    return str(websocket_url)
            except (httpx.HTTPError, ValueError):
                pass
            await asyncio.sleep(0.1)
    raise RuntimeError("浏览器调试端口启动失败")


async def _cdp_call(websocket: Any, call_id: int, method: str) -> dict[str, Any]:
    await websocket.send(json.dumps({"id": call_id, "method": method}))
    while True:
        message = json.loads(await websocket.recv())
        if message.get("id") == call_id:
            if "error" in message:
                raise RuntimeError(f"CDP {method} 失败：{message['error']}")
            return message.get("result", {})


async def open_browser_login() -> dict[str, Any]:
    """打开隔离的真实浏览器，等待登录成功后返回 Cookie 与用户信息。"""
    browser = find_login_browser()
    PROFILE_DIR.mkdir(parents=True, exist_ok=True)
    port = _free_port()
    command = [
        str(browser),
        f"--remote-debugging-port={port}",
        "--remote-debugging-address=127.0.0.1",
        f"--user-data-dir={PROFILE_DIR}",
        "--no-first-run",
        "--no-default-browser-check",
        f"--app={LOGIN_URL}",
    ]
    process = subprocess.Popen(
        command,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    websocket = None
    try:
        websocket_url = await _wait_for_cdp(port)
        websocket = await connect(websocket_url, open_timeout=3)
        deadline = asyncio.get_running_loop().time() + LOGIN_TIMEOUT_SEC
        call_id = 0

        while asyncio.get_running_loop().time() < deadline:
            if process.poll() is not None:
                return {"status": "cancelled"}

            call_id += 1
            targets = await _cdp_call(websocket, call_id, "Target.getTargets")
            if not has_pixiv_main_target(targets.get("targetInfos", [])):
                await asyncio.sleep(0.5)
                continue

            call_id += 1
            result = await _cdp_call(websocket, call_id, "Storage.getCookies")
            cookies = extract_pixiv_cookies(result.get("cookies", []))
            phpsessid = cookies.get("PHPSESSID", "")
            if phpsessid:
                try:
                    probe = await fetch_session_probe(phpsessid)
                except InvalidSessionError:
                    pass
                else:
                    cookies["x-csrf-token"] = probe.csrf_token
                    return {
                        "status": "success",
                        "cookies": cookies,
                        "user": probe.user or {},
                    }
            await asyncio.sleep(0.5)

        return {"status": "timeout", "error": "登录超时（300s）"}
    except Exception as exc:  # noqa: BLE001
        logger.warning("真实浏览器登录失败：%s", exc)
        return {"status": "error", "error": f"{type(exc).__name__}: {exc}"}
    finally:
        if websocket is not None:
            try:
                call_id = locals().get("call_id", 0) + 1
                await _cdp_call(websocket, call_id, "Browser.close")
            except Exception:  # noqa: BLE001
                pass
            await websocket.close()
        if process.poll() is None:
            process.terminate()
        try:
            await asyncio.to_thread(process.wait, 3)
        except subprocess.TimeoutExpired:
            process.kill()
