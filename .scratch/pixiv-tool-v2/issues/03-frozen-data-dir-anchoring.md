# V2-03 — frozen 模式数据目录锚定修正

**What to build:**

统一 frozen（PyInstaller 打包）模式下用户数据的存放位置。当前 4 个模块各自用 `Path(__file__).resolve().parent.parent.parent` 算数据目录，frozen 模式下指向 `_internal/`，导致用户数据进了 bundle 目录（重打包丢失），且和 stdout.log 的锚定逻辑冲突。

**Blocked by:** 建议在 V2-01（src layout 重构）之后做——抽出来的 `paths.py` 要放在新位置 `src/pixiv_tool/storage/paths.py`。独立可做，但路径常量会跟随 V2-01 改。

**Status:** ready-for-agent

## 问题

### 当前不一致的锚定逻辑

| 模块 | 锚定方式 | dev 模式（venv python） | frozen 模式（PyInstaller） |
|---|---|---|---|
| `backend/storage/db.py:17` | `Path(__file__).parent.parent.parent / "data"` | `<repo>/data/app.db` ✓ | `_internal/data/app.db` ✗ |
| `backend/storage/cookie_dpapi.py:18` | `Path(__file__).parent.parent.parent / "config"` | `<repo>/config/cookies.dat` ✓ | `_internal/config/cookies.dat` ✗ |
| `backend/logging_config.py:13` | `Path(__file__).parent.parent / "data" / "logs"` | `<repo>/backend/data/logs/` ✗（多了一级） | `_internal/backend/data/logs/` ✗ |
| `backend/api/settings.py:41` | `Path(__file__).parent.parent.parent.parent / "data" / "logs"` | `<repo>/data/logs/` ✓ | `_internal/data/logs/` ✗ |
| `backend/main.py:_redirect_stdio_if_needed` | `Path(sys.executable).parent / "data" / "logs"` | n/a（dev 不走这条） | `<exe_dir>/data/logs/` ✓（升级安全） |

**核心冲突**：`main.py` 的 stdout 重定向锚定 `<exe_dir>/data/`（dist/pixiv-tool/data/，升级不丢），但 db/cookie 锚定 `_internal/data/`（升级覆盖丢失）。两套逻辑不一致。

### PACKAGING.md 记录错误

我在 `docs/PACKAGING.md §5` 写的"数据存放不在 exe 同级，而在用户目录下"是**错的**——实际 db/cookie 写到了 `_internal/`。需要在该 ticket 里同步更正。

## 方案

### 步骤 1：抽 `paths.py` 统一锚定

新建 `backend/storage/paths.py`（V2-01 后是 `src/pixiv_tool/storage/paths.py`），提供 4 个常量 + 一个 frozen 检测函数：

```python
"""用户数据目录锚定。

frozen(PyInstaller)模式: 锚定 <exe_dir>/data、<exe_dir>/config,
    和 main._redirect_stdio_if_needed 的 stdout.log 一致——
    升级时只要不删 data/ 目录,用户数据保留。
dev(venv python)模式: 锚定 <repo_root>/data、<repo_root>/config。
"""
from __future__ import annotations
import sys
from pathlib import Path


def _is_frozen() -> bool:
    return getattr(sys, "frozen", False)


def _repo_root() -> Path:
    # paths.py 在 <root>/backend/storage/paths.py
    # V2-01 后在 <root>/src/pixiv_tool/storage/paths.py
    return Path(__file__).resolve().parent.parent.parent


def data_dir() -> Path:
    if _is_frozen():
        return Path(sys.executable).resolve().parent / "data"
    return _repo_root() / "data"


def config_dir() -> Path:
    if _is_frozen():
        return Path(sys.executable).resolve().parent / "config"
    return _repo_root() / "config"


def logs_dir() -> Path:
    return data_dir() / "logs"


# 模块级常量(向后兼容现有 import)
DATA_DIR = data_dir()
CONFIG_DIR = config_dir()
LOGS_DIR = logs_dir()
```

### 步骤 2：改 4 个模块引用

| 文件 | 改什么 |
|---|---|
| `backend/storage/db.py:17` | `DB_DIR = Path(__file__)...parent.parent.parent / "data"` → `from .paths import DATA_DIR; DB_DIR = DATA_DIR` |
| `backend/storage/cookie_dpapi.py:18` | `COOKIE_FILE = Path(__file__)...parent.parent.parent / "config" / "cookies.dat"` → `from .paths import CONFIG_DIR; COOKIE_FILE = CONFIG_DIR / "cookies.dat"` |
| `backend/logging_config.py:13-14` | `_LOG_DIR = ...; _LOG_FILE = ...` → `from backend.storage.paths import LOGS_DIR; _LOG_DIR = LOGS_DIR; _LOG_FILE = _LOG_DIR / "app.log"` |
| `backend/api/settings.py:41` | `log_path = Path(__file__)...parent.parent.parent.parent / "data" / "logs" / "app.log"` → `from backend.storage.paths import LOGS_DIR; log_path = LOGS_DIR / "app.log"` |
| `backend/main.py:_redirect_stdio_if_needed` | `log_dir = exe_dir / "data" / "logs"` → `from backend.storage.paths import LOGS_DIR; log_dir = LOGS_DIR`（统一） |

### 步骤 3：更正 PACKAGING.md §5

把"数据存放在用户目录"改成准确描述：

| 数据 | dev 模式位置 | frozen 模式位置 |
|---|---|---|
| 任务/已抓小说 (app.db) | `<repo>/data/app.db` | `<exe_dir>/data/app.db` |
| 登录 cookie (cookies.dat) | `<repo>/config/cookies.dat` | `<exe_dir>/config/cookies.dat` |
| 用户设置 (settings.json) | `<repo>/config/settings.json` | `<exe_dir>/config/settings.json` |
| 运行日志 (app.log / stdout.log) | `<repo>/data/logs/` | `<exe_dir>/data/logs/` |

## Open question（产品决策点，实施前确认）

frozen 模式锚定 `<exe_dir>/data/` 是 portable 模式（数据跟着 exe）。是否要改成 installed 模式（数据放 `%APPDATA%/pixiv-tool/`）？

| 方案 | 优点 | 缺点 |
|---|---|---|
| **A. portable**（`<exe_dir>/data/`，本 ticket 默认） | 解压即用、数据跟着 exe、便携、用户能直接看到 | 升级要用户手动迁移 data/ 目录 |
| **B. installed**（`platformdirs.user_data_dir("pixiv-tool")` → `%APPDATA%/pixiv-tool/`） | 升级不丢数据、符合 Windows 应用规范、多用户隔离 | 便携版用户找不到数据、需新增 `platformdirs` 依赖 |
| **C. 可配置**（A + settings 加"数据目录"项） | 两全 | 实现复杂、V2 可能过度设计 |

**本 ticket 默认方案 A**（最小改动，和现有 stdout.log 一致，不打断 portable 定位）。如果用户选 B/C，本 ticket 范围扩大到加 platformdirs 依赖 + settings 改动。

## Acceptance criteria

- [ ] 新增 `backend/storage/paths.py`（V2-01 后 `src/pixiv_tool/storage/paths.py`），含 `DATA_DIR`/`CONFIG_DIR`/`LOGS_DIR` 三个常量
- [ ] db.py / cookie_dpapi.py / logging_config.py / settings.py / main.py 都 import 这些常量，不再各自算 `__file__.parent.parent.parent`
- [ ] dev 模式：`./scripts/dev.ps1 start` → 抓一篇小说 → 数据落到 `<repo>/data/app.db`（路径不变，向后兼容）
- [ ] frozen 模式：双击 exe → 抓一篇小说 → 数据落到 `dist/pixiv-tool/data/app.db`（exe 同级，**不是** `_internal/data/`）
- [ ] **关键验证**：frozen exe 跑一次产生数据后，删 `_internal/` 之外的所有 bundle 文件重打包（模拟升级），新 exe 启动时旧数据还在（`<exe_dir>/data/` 不被覆盖）
- [ ] `docs/PACKAGING.md §5` 数据存放表更正
- [ ] 90 个测试全过（测试里 db_path 都是 tmp_path fixture，不受影响，但确认下没有硬编码路径）

## 实施建议

1. 先决定 Open question（默认 A，除非用户在 ticket 实施时说改）
2. 建 paths.py
3. 改 5 个文件的 import
4. dev 模式跑一次抓取验证 `<repo>/data/` 落盘
5. 打包 + 双击 frozen exe 跑一次抓取验证 `<exe_dir>/data/` 落盘（不是 `_internal/data/`）
6. 模拟升级：备份 data/ + config/，删 dist 重打包，恢复 data/ + config/，启动验证数据还在
7. 更正 PACKAGING.md

**预计工作量**：1 小时（含打包验证）。

## 风险

- **DPAPI 绑定用户账户**：cookies.dat 是 Windows DPAPI 加密，只能在加密时的同一 Windows 用户账户下解密。换机器/换用户需重新登录。本 ticket 不解决这个（属于设计约束），但 PACKAGING.md §5 要提一句。
- `logging_config.py` 当前多算了一级 parent（`parent.parent` 而非 `parent.parent.parent`），dev 模式落到 `<repo>/backend/data/logs/` 而非 `<repo>/data/logs/`——这是个现存 bug，paths.py 统一后顺便修。
