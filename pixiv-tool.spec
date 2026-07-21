# -*- mode: python ; coding: utf-8 -*-
"""
pixiv-tool PyInstaller spec 文件。

打包模式：onedir
入口：src/pixiv_tool/main.py
包含：前端静态资源 (src/pixiv_tool/static/)
"""

from pathlib import Path

block_cipher = None
ROOT = Path(SPECPATH)

# -------------------------------------------------------------------
# 数据文件：前端构建产物
# -------------------------------------------------------------------

static_dir = ROOT / "src" / "pixiv_tool" / "static"
datas = []
if static_dir.is_dir():
    datas = [(str(static_dir), "pixiv_tool/static")]

# -------------------------------------------------------------------
# Analysis
# -------------------------------------------------------------------

a = Analysis(
    [str(ROOT / "src" / "pixiv_tool" / "main.py")],
    pathex=[str(ROOT)],
    binaries=[],
    datas=datas,
    hiddenimports=[
        "pywebview",
        # pywebview 平台后端是平台判断后才动态 import 的,PyInstaller 静态分析抓不到。
        # V1 仅 Windows,列 edgechromium(WebView2,Win11 默认)+ winforms(fallback)。
        "webview.platforms.edgechromium",
        "webview.platforms.winforms",
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
        # 登录子进程入口在 prod 模式下由主 exe `--login-window` 分发调用,
        # 必须在 bundle 里(静态分析也能找到,但显式声明更稳)。
        "pixiv_tool.auth.login_window",
        "pixiv_tool.storage.cookie_dpapi",
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
        # email/xml/pydoc 之前误排除——fastapi/starlette/pydantic 都间接依赖,
        # 排掉后 frozen exe 一启动就 ModuleNotFoundError。
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
    # console=False:prod 桌面模式,不弹 cmd 黑窗(SPEC §8 dev 同时输出 console 是另一回事)。
    # 调试期可临时改成 True 看启动 traceback。
    console=False,
    icon=str(ROOT / "src" / "pixiv_tool" / "icon.ico") if (ROOT / "src" / "pixiv_tool" / "icon.ico").exists() else None,
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
