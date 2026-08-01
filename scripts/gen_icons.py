"""从 docs/icon/raw_icon.png 生成各平台应用图标，替换 PyInstaller 默认图标。

用法（无需给项目加依赖）：
    uv run --with pillow python scripts/gen_icons.py

产物：
    src/pixiv_tool/icon.ico   Windows 多尺寸 16-256
    src/pixiv_tool/icon.icns  macOS 全尺寸 16-1024（PNG 块，与 iconutil 产物一致）
"""

from __future__ import annotations

import io
import struct
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
RAW = ROOT / "docs" / "icon" / "raw_icon.png"
OUT_DIR = ROOT / "src" / "pixiv_tool"

# ICO 最多 256，Pillow 按 sizes 依次缩放生成
ICO_SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]

# (块类型, 像素尺寸) —— 与 macOS iconutil iconset 产出的 10 块一致
ICNS_CHUNKS = [
    ("icp4", 16), ("icp5", 32), ("ic11", 32), ("ic12", 64),
    ("ic07", 128), ("ic08", 256), ("ic13", 256), ("ic14", 512),
    ("ic09", 512), ("ic10", 1024),
]


def main() -> None:
    img = Image.open(RAW).convert("RGBA")
    # 源图非正方形时中心裁剪为正方形，避免拉伸变形
    w, h = img.size
    if w != h:
        side = min(w, h)
        img = img.crop(((w - side) // 2, (h - side) // 2, (w + side) // 2, (h + side) // 2))

    OUT_DIR.mkdir(parents=True, exist_ok=True)

    ico_path = OUT_DIR / "icon.ico"
    img.save(ico_path, format="ICO", sizes=ICO_SIZES)
    print(f"ok {ico_path} ({len(ICO_SIZES)} sizes)")

    icns_path = OUT_DIR / "icon.icns"
    chunks = b""
    for kind, size in ICNS_CHUNKS:
        buf = io.BytesIO()
        img.resize((size, size), Image.LANCZOS).save(buf, format="PNG")
        data = buf.getvalue()
        chunks += kind.encode("ascii") + struct.pack(">I", len(data) + 8) + data
    icns_path.write_bytes(b"icns" + struct.pack(">I", len(chunks) + 8) + chunks)
    print(f"ok {icns_path} ({len(ICNS_CHUNKS)} chunks)")

    # 自校验：回读确认结构合法
    with Image.open(ico_path) as check:
        assert set(check.info["sizes"]) == set(ICO_SIZES), check.info["sizes"]
    raw = icns_path.read_bytes()
    assert raw[:4] == b"icns" and struct.unpack(">I", raw[4:8])[0] == len(raw)
    i, got = 8, []
    while i < len(raw):
        kind = raw[i : i + 4].decode()
        ln = struct.unpack(">I", raw[i + 4 : i + 8])[0]
        got.append((kind, ln - 8))
        i += ln
    assert i == len(raw) and [k for k, _ in got] == [k for k, _ in ICNS_CHUNKS], got
    print("verify ok: ico 7 sizes, icns 10 chunks, containers well-formed")


if __name__ == "__main__":
    main()
