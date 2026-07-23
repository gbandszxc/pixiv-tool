# -*- mode: python ; coding: utf-8 -*-
"""
pixiv-tool PyInstaller spec 文件。

打包模式：onedir
入口：src/pixiv_tool/main.py
包含：前端静态资源 (src/pixiv_tool/static/)
"""

from pathlib import Path
import sys

block_cipher = None
ROOT = Path(SPECPATH)

# -------------------------------------------------------------------
# curl_cffi 的 PyInstaller hook 目录。
#
# curl_cffi 依赖 libcurl-impersonate 原生库(TLS 指纹伪装),wheel 放在
# curl_cffi/lib/ 下,PyInstaller 静态分析抓不到运行期 dlopen 的动态库,
# 必须用 hook 里的 collect_dynamic_libs 显式收集。漏打后 frozen exe 发请求
# 会静默降级到 httpx(无 TLS 伪装),风控重新触发。详见
# pyinstaller-hooks/hook-curl_cffi.py。
# -------------------------------------------------------------------
HOOKS_DIR = str(ROOT / "pyinstaller-hooks")

# -------------------------------------------------------------------
# 平台相关 hiddenimports
#
# pywebview 平台后端是 sys.platform 判断后才动态 import 的,PyInstaller 静态
# 分析抓不到,必须显式声明。但**平台后端是平台专属的**:
#   - webview.platforms.edgechromium / winforms 仅 Windows 存在
#   - webview.platforms.cocoa 仅 macOS 存在(依赖 PyObjC)
#   - webview.platforms.webkitgtk / gtk 仅 Linux 存在(依赖 GTK)
# 把别平台的后端声明进 hiddenimports,PyInstaller 编译该模块时会因 import
# 平台专属符号(如 mac 上 webview.platforms.winforms 引 clr/pythonnet)而崩。
#
# pixiv_tool.storage.cookie_dpapi 同理:模块顶层 `import ctypes.wintypes` +
# `ctypes.windll`(mac/linux 的 ctypes 没有 windll 属性),只能 Windows 编译。
# 运行期 cookies.py 的 create_cookie_store() 已经按 sys.platform 惰性 import,
# 所以 mac/linux 不需要这个模块在 bundle 里。
# -------------------------------------------------------------------

if sys.platform == "win32":
    pywebview_backends = [
        "webview.platforms.edgechromium",
        "webview.platforms.winforms",
    ]
    platform_hidden = ["pixiv_tool.storage.cookie_dpapi"]
elif sys.platform == "darwin":
    pywebview_backends = ["webview.platforms.cocoa"]
    platform_hidden = []  # mac cookie 存储是 stub,不需要 dpapi 模块
else:  # linux
    pywebview_backends = ["webview.platforms.webkitgtk", "webview.platforms.gtk"]
    platform_hidden = []

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
        # pywebview 平台后端 + 平台专属模块按 sys.platform 展开(见文件顶部条件)。
        *pywebview_backends,
        *platform_hidden,
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
        # curl_cffi:浏览器 TLS 指纹伪装后端。动态库由 hook-curl_cffi.py 收集,
        # 这里声明纯 Python 模块确保运行期 import 成功。core.http_factory 在
        # curl_cffi 不可用时会降级到 httpx,但正常打包应该都能进 bundle。
        "curl_cffi",
        "curl_cffi.requests",
        "curl_cffi.requests.async_session",
        "curl_cffi.requests.exceptions",
        # 登录子进程入口在 prod 模式下由主 exe `--login-window` 分发调用,
        # 必须在 bundle 里(静态分析也能找到,但显式声明更稳)。
        "pixiv_tool.auth.login_window",
        "pixiv_tool.core.http_factory",
    ],
    hookspath=[HOOKS_DIR],
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

# -------------------------------------------------------------------
# BUNDLE（macOS .app,仅 darwin）
#
# Windows 上 EXE(onedir) 就是最终产物;macOS 上要让 Finder 把它识别为可双击
# 的 .app bundle,必须再加一层 BUNDLE。COLLECT 产物是 pixiv-tool/ 目录,BUNDLE
# 把它打包成 Pixiv Tool.app/。
#
# 图标:mac 用 .icns(Windows 的 .ico 在 mac 上不被 BUNDLE 接受)。src/pixiv_tool/
# 下当前 icon.ico / icon.icns 都可能不存在(美术资源未就绪),统一 .exists() 守卫,
# 不存在就传 None(PyInstaller 用默认图标)。**不要**在这里生成图标资源。
# -------------------------------------------------------------------

if sys.platform == "darwin":
    _icns_path = ROOT / "src" / "pixiv_tool" / "icon.icns"
    app = BUNDLE(
        coll,
        name="Pixiv Tool.app",
        icon=str(_icns_path) if _icns_path.exists() else None,
        bundle_identifier="com.pixivtool.app",
        info_plist={
            "CFBundleDisplayName": "Pixiv Tool",
            "NSHighResolutionCapable": True,
            "LSMinimumSystemVersion": "10.13",
        },
    )
