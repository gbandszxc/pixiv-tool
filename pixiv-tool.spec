# -*- mode: python ; coding: utf-8 -*-
"""
pixiv-tool PyInstaller spec 文件。

打包模式：onedir
入口：backend/main.py
包含：前端静态资源 (backend/static/)
"""

from pathlib import Path

block_cipher = None
ROOT = Path(SPECPATH)

# -------------------------------------------------------------------
# 数据文件：前端构建产物
# -------------------------------------------------------------------

static_dir = ROOT / "backend" / "static"
datas = []
if static_dir.is_dir():
    datas = [(str(static_dir), "backend/static")]

# -------------------------------------------------------------------
# Analysis
# -------------------------------------------------------------------

a = Analysis(
    [str(ROOT / "backend" / "main.py")],
    pathex=[str(ROOT)],
    binaries=[],
    datas=datas,
    hiddenimports=[
        "pywebview",
        "uvicorn",
        "uvicorn.logging",
        "uvicorn.loops",
        "uvicorn.loops.auto",
        "uvicorn.protocols",
        "uvicorn.protocols.http",
        "uvicorn.protocols.http.auto",
        "uvicorn.protocols.websockets",
        "uvicorn.protocols.websockets.auto",
        "uvicorn.lifespan",
        "uvicorn.lifespan.on",
        "fastapi",
        "starlette",
        "httpx",
        "httpx._transports",
        "httpx._transports.default",
    ],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        "numpy",
        "scipy",
        "pandas",
        "matplotlib",
        "tkinter",
        "unittest",
        "test",
        "email",
        "xml",
        "pydoc",
    ],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
)

# -------------------------------------------------------------------
# PYZ（Python 字节码归档）
# -------------------------------------------------------------------

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

# -------------------------------------------------------------------
# EXE（入口可执行文件）
# -------------------------------------------------------------------

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="pixiv-tool",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    console=True,
    icon=str(ROOT / "backend" / "icon.ico") if (ROOT / "backend" / "icon.ico").exists() else None,
)

# -------------------------------------------------------------------
# COLLECT（onedir 输出）
# -------------------------------------------------------------------

coll = COLLECT(
    exe,
    a.binaries,
    a.zipfiles,
    a.datas,
    strip=False,
    upx=True,
    upx_exclude=[],
    name="pixiv-tool",
)
