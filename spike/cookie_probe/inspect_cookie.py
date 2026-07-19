"""
Spike · 诊断脚本：dump pywebview cookie 对象的真实结构
======================================================

关联：R1 spike · v2 失败原因排查

现象：
    v2 探测到 26 个 cookie 对象，但 cookie_to_dict() 映射后 name/value/domain
    全部为空字符串。说明 pywebview 返回的不是 SimpleCookie.Morsel 也不是 dict，
    我猜测的属性名（.key/.value/.domain）匹配不上。

策略：
    不靠猜——直接把第一个 cookie 对象的真实类型、所有属性、dict 化结果、
    repr 都 dump 出来，看清楚再修映射。

用法：
    uv run python inspect_cookie.py
    （同样会弹 pywebview 窗口，登录后自动 dump）
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import webview


LOGIN_URL = "https://accounts.pixiv.net/login"
TARGET_HOST = "www.pixiv.net"
OUT_FILE = Path(__file__).parent / "cookie_inspect.json"


def inspect_one_cookie(c: Any, index: int) -> dict[str, Any]:
    """把一个 cookie 对象的所有可能信息 dump 出来。

    探测顺序（从最可靠到最 hacky）：
        1. type 名
        2. repr / str
        3. __dict__（实例属性）
        4. dir() 里所有非下划线属性 + 它们的值
        5. 是否可迭代（list/tuple/dict-like）
        6. as dict / output() 等显式方法的结果
    """
    info: dict[str, Any] = {
        "index": index,
        "type": type(c).__name__,
        "module": type(c).__module__,
        "repr": repr(c)[:500],
        "str": str(c)[:500],
    }

    # ---- 实例属性 __dict__ ----
    try:
        d = getattr(c, "__dict__", None)
        if d:
            info["instance_dict"] = {
                k: _safe(v) for k, v in d.items() if not k.startswith("_")
            }
    except Exception as e:  # noqa: BLE001
        info["instance_dict_error"] = str(e)

    # ---- dir() 公开属性 ----
    public_attrs: dict[str, Any] = {}
    for name in dir(c):
        if name.startswith("_"):
            continue
        try:
            value = getattr(c, name)
        except Exception:  # noqa: BLE001
            continue
        # 跳过方法/函数（只关心数据）
        if callable(value) and not isinstance(value, (str, int, bool, type(None))):
            continue
        public_attrs[name] = _safe(value)
    info["public_attrs"] = public_attrs

    # ---- 试 dict(c)（如果是 mapping）----
    try:
        info["as_dict"] = {str(k): _safe(v) for k, v in dict(c).items()}  # type: ignore[arg-type]
    except Exception:  # noqa: BLE001
        pass

    # ---- 试 .output()（SimpleCookie.Morsel 方法）----
    for method in ("output", "js_attrs", "Dump"):
        if hasattr(c, method):
            try:
                result = getattr(c, method)()
                info[f"call_{method}"] = _safe(result)
            except Exception as e:  # noqa: BLE001
                info[f"call_{method}_error"] = str(e)

    # ---- 试常见 key 名 ----
    for key in ("key", "name", "value", "coded_value", "domain", "path",
                "httponly", "secure", "expires", "max_age", "samesite"):
        if hasattr(c, key):
            try:
                info[f"attr_{key}"] = _safe(getattr(c, key))
            except Exception:  # noqa: BLE001
                pass

    return info


def _safe(v: Any) -> Any:
    """把任意值转成 JSON 可序列化形式。"""
    if isinstance(v, (str, int, float, bool, type(None))):
        return v
    if isinstance(v, (list, tuple)):
        return [_safe(x) for x in v][:50]
    if isinstance(v, dict):
        return {str(k): _safe(val) for k, val in list(v.items())[:50]}
    return repr(v)[:200]


def main() -> None:
    """启动登录窗，loaded 事件触发 dump。"""
    state = {"done": False}

    def on_loaded(window: webview.Window) -> None:
        if state["done"]:
            return
        url = window.get_current_url() or ""
        if TARGET_HOST not in url:
            return
        state["done"] = True

        print(f"\n[inspect] URL={url}, 开始 dump cookie 结构 ...")
        cookies = window.get_cookies()

        out = {
            "count": len(cookies),
            "container_type": type(cookies).__name__,
            "first_three": [],
        }
        # 只看前 3 个，足够推断结构
        for i, c in enumerate(cookies[:3]):
            out["first_three"].append(inspect_one_cookie(c, i))

        OUT_FILE.write_text(json.dumps(out, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"\n[inspect] 类型={type(cookies[0]).__name__}, 总数={len(cookies)}")
        print(f"[inspect] 详情写入 {OUT_FILE}")
        print(f"[inspect] 窗口保持打开，手动关闭。")

    win = webview.create_window(
        title="Cookie Inspect · 登录 Pixiv（自动 dump）",
        url=LOGIN_URL,
        width=960,
        height=720,
    )
    win.events.loaded += on_loaded
    webview.start(private_mode=False, http_server=True, http_port=17729)


if __name__ == "__main__":
    main()
