# V2-01 — src layout 重构（F2.3 + F2.4 合并）

**Status:** done（2026-07-22 实施，commit `7c01908` + `acf9a54`）

**What to build:**

把 `backend/` 整体迁到 `src/pixiv_tool/`（snake_case 包名），采用 PEP 470 标准 src layout。同步改 66 处 import + 7 处连锁配置（pyproject/spec/dev.ps1/build.py/conftest/venv/docs）。

**为什么合并 F2.3 + F2.4**：SESSION-2026-07-19 §2.2 把 pyproject.toml 位置（F2.3）和 tests/ 位置（F2.4）当两个独立 cleanup 项，但两者在 src layout 迁移里是同一次机械改动——拆开反而要改两次 import、调两次 spec。合并成一个 ticket 一次做完。

**为什么是 `pixiv_tool` 不是 `backend`**：
- src layout 的核心价值是"强制安装才能 import"（防止根目录运行时意外 import 到未安装代码）。放进 `src/` 后包名应规范化（PEP 8 snake_case），`backend` 不是合法的 Python 语义包名。
- `pixiv_tool` 对应 `pyproject.toml` 的 `name = "pixiv-tool"`，约定俗成。
- **避开 `backend→api` 命名冲突**：`backend/api/` 已作为子包存在（放 auth.py/novels.py 等路由），直接 rename backend→api 会产生 `api/api/` 嵌套。

**Blocked by:** None — 独立可立即开始

**Status:** ready-for-agent

## 目标布局

```
当前:                               目标:
backend/                            src/
├── __init__.py                     └── pixiv_tool/
├── api/                                ├── __init__.py
│   ├── auth.py                         ├── api/
│   ├── novels.py                       │   ├── auth.py
│   ├── tasks.py                        │   ├── novels.py
│   ├── settings.py                     │   ├── tasks.py
│   └── system.py                       │   ├── settings.py
├── auth/                               │   └── system.py
│   └── login_window.py                 ├── auth/
├── core/                               │   └── login_window.py
│   ├── crawler.py                      ├── core/
│   ├── exporter.py                     │   ├── crawler.py
│   ├── pixiv_client.py                 │   ├── exporter.py
│   ├── source.py                       │   ├── pixiv_client.py
│   └── task.py                         │   ├── source.py
├── storage/                            │   └── task.py
│   ├── cookie_dpapi.py                 ├── storage/
│   ├── cookies.py                      │   ├── cookie_dpapi.py
│   ├── db.py                           │   ├── cookies.py
│   ├── models.py                       │   ├── db.py
│   └── settings.py                     │   ├── models.py
├── logging_config.py                   │   └── settings.py
├── main.py                             ├── logging_config.py
└── static/  (build 产物,gitignored)    ├── main.py
                                        └── static/  (build 产物,gitignored)
tests/                              tests/   ← 保留 root,只改 import
├── conftest.py                     ├── conftest.py
├── test_api.py                     ├── test_api.py
├── test_core.py                    ├── test_core.py
├── test_login.py                   ├── test_login.py
└── test_storage.py                 └── test_storage.py
pyproject.toml (root)              pyproject.toml (root)
```

**tests 位置决策**：保留 root `tests/`，不跟随移动。
- src layout 下 root tests/ 是社区主流（pytest 默认发现 + conftest sys.path 最简）
- F2.4 原描述"移到 backend/tests/"是基于旧布局的设想，src layout 后该约定失效
- SPEC §3.4 目录结构图需同步更新这个约定

## import 改写规则（机械替换）

全仓库 `.py` 文件（排除 `.venv/`、`.scratch/`、`spike/`、`build/`、`dist/`）：

| 原模式 | 新模式 |
|---|---|
| `from backend.` | `from pixiv_tool.` |
| `from backend import` | `from pixiv_tool import` |
| `import backend` | `import pixiv_tool` |
| `import backend.` | `import pixiv_tool.` |
| `backend.main:app`（uvicorn CLI / spec） | `pixiv_tool.main:app` |

**66 处分布**（Explore agent 实测）：
- `tests/test_core.py` 15 处
- `tests/test_login.py` 12 处
- `backend/main.py` 12 处
- `backend/api/tasks.py` 8 处
- `tests/test_api.py` 10 处
- `backend/core/crawler.py` 6 处
- `backend/api/auth.py` 5 处
- `tests/test_storage.py` 5 处
- `tests/conftest.py` 4 处
- `backend/api/*` + `storage/*` + `core/*` 散落若干

## 7 处连锁改动点

| 文件 | 行 | 改什么 |
|---|---|---|
| `pyproject.toml` | 24 | `[tool.hatch.build.targets.wheel] packages = ["backend"]` → `["src/pixiv_tool"]` |
| `pixiv-tool.spec` | 7 | docstring `入口：backend/main.py` → `入口：src/pixiv_tool/main.py` |
| `pixiv-tool.spec` | 19 | `static_dir = ROOT / "backend" / "static"` → `ROOT / "src" / "pixiv_tool" / "static"` |
| `pixiv-tool.spec` | 22 | `datas = [(str(static_dir), "backend/static")]` → 目标 `"pixiv_tool/static"`（注意 bundle 路径也要改，否则 frozen 找不到） |
| `pixiv-tool.spec` | 29 | `Analysis([str(ROOT / "backend" / "main.py")]` → `ROOT / "src" / "pixiv_tool" / "main.py"` |
| `pixiv-tool.spec` | 57 | hiddenimport `"backend.auth.login_window"` → `"pixiv_tool.auth.login_window"` |
| `pixiv-tool.spec` | 58 | hiddenimport `"backend.storage.cookie_dpapi"` → `"pixiv_tool.storage.cookie_dpapi"` |
| `pixiv-tool.spec` | 103 | `icon=str(ROOT / "backend" / "icon.ico")` → `ROOT / "src" / "pixiv_tool" / "icon.ico"`（如果 icon 存在；不存在保持 None） |
| `scripts/dev.ps1` | 280 | 注释 `# sh: ... uvicorn backend.main:app` → `pixiv_tool.main:app` |
| `scripts/dev.ps1` | 282 | `Start-Process ... "run", "uvicorn", "backend.main:app", ...` → `"pixiv_tool.main:app"` |
| `scripts/build.py` | 29 | `BACKEND_DIR = REPO_ROOT / "backend"` → `REPO_ROOT / "src" / "pixiv_tool"` |
| `scripts/build.py` | 30 | `STATIC_DIR = BACKEND_DIR / "static"`（自动跟随，不用改） |
| `tests/conftest.py` | 13 | `sys.path.insert(0, str(Path(__file__).resolve().parent.parent))` → 改成指向 `src`（或干脆删掉，靠 editable install） |
| `backend/main.py` | 54, 60, 62 | `_STATIC_DIR = Path(__file__).resolve().parent / "static"` 和 frozen 分支的 `_MEIPASS / "backend" / "static"` 都要改 → `"pixiv_tool" / "static"` |
| `.venv` | — | `uv sync --extra win --extra dev` 重建 editable install（自动） |

**容易漏的点**：
1. `pixiv-tool.spec` 的 `datas` 第二个参数是 bundle 内目标路径，不是源路径——必须和 `main.py:_STATIC_DIR` 的 `_MEIPASS` 拼接路径一致，否则 frozen SPA 404（之前踩过的坑）
2. `backend/main.py` frozen 分支 `sys._MEIPASS / "backend" / "static"` 也要改成 `"pixiv_tool" / "static"`
3. `conftest.py` 的 `sys.path.insert` 在 src layout + editable install 下其实多余，但留着不影响；建议改成指向 `src` 或删掉

## docs 同步

- `docs/SPEC.md §3.4` 目录结构图：`backend/` → `src/pixiv_tool/`
- `docs/PACKAGING.md §3` 产物结构、§8 spec 配置表：路径引用同步
- `AGENTS.md` 技术栈表：Python 模块路径示例（如有）
- `docs/SPEC.md §8.1` dev 端口/启动说明里 `uvicorn backend.main:app` → `pixiv_tool.main:app`

## Acceptance criteria

- [x] 90 个测试全过（`pytest tests/`）
- [x] `grep -rn "from backend\|import backend" --include="*.py" .` 在仓库内（排除 `.venv/`、`.scratch/`、`spike/`、`build/`、`dist/`）零命中
- [x] `grep -rn "backend\.main:app\|backend\.auth\|backend\.storage\|backend\.api\|backend\.core" scripts/ pixiv-tool.spec docs/` 零命中
- [x] `./scripts/dev.ps1 restart` → `curl http://127.0.0.1:9962/api/health` 200
- [x] 浏览器打开 `http://localhost:9961/` 前端渲染完整（菜单/抓取表单/路由）
- [x] `.venv/Scripts/python.exe scripts/build.py` 打包成功
- [x] `dist/pixiv-tool/pixiv-tool.exe --no-window` → `/api/health` 200 + `/api/auth/diag-version` 返回 `{"frozen":true,...}`
- [x] frozen exe 双击启动（用 `DETACHED_PROCESS | CREATE_NO_WINDOW` 模拟）→ SPA 200 + pywebview 主窗加载前端
- [x] `docs/SPEC.md §3.4` + `docs/PACKAGING.md` 目录图已同步更新
- [x] 一次原子 commit（或拆成 move + edit 两个 commit 也行，但必须连续）—— 拆成 refactor + docs 两个连续 commit

## 实施建议

1. **先物理移动目录**：`git mv backend src/pixiv_tool`（注意要先 `mkdir src && git mv backend src/pixiv_tool`，保留 git history）
2. **机械替换 import**：用 `sed` 或 ripgrep 批量改，但务必人工 review（避免改到 `.venv/` 或字符串里的 "backend"）
3. **改 7 处配置**：按上表逐个改
4. **`uv sync --extra win --extra dev`**：重建 editable install
5. **跑测试 + dev.ps1 + build.py 三重验证**：按 acceptance criteria 逐项过
6. **更新 docs**：SPEC §3.4 + PACKAGING.md

**预计工作量**：1-2 小时（机械改动 + 三重验证）

## 风险

- **PyInstaller datas 路径不匹配**：spec 的 `datas` 第二个参数（bundle 目标）必须和 main.py 里 `_MEIPASS` 拼接路径一致。改错一个就 frozen SPA 404（见前一份 session 的 `fix(packaging): prod 模式下 frozen exe 兼容` commit）
- **dev.ps1 改完忘重启**：`--reload` 模式下 uvicorn 不会自动重新加载 dev.ps1 里的模块路径改动；必须 `./scripts/dev.ps1 restart`
- **editable install 缓存**：`uv sync` 后如果测试还报 `ModuleNotFoundError: No module named 'pixiv_tool'`，删 `.venv` 重建
