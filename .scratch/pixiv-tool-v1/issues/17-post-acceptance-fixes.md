# 17 — V1 验收后修复（Post-Acceptance Fixes）

**What to build:**

V1 实施完成后（commit ab357e0），调度 agent 验收发现多个严重问题——前端编译不过、登录 cookie 提取 bug 复发、pywebview 主窗缺失、i18n 没接入、测试质量有水分。本 ticket 把这些问题清单化为三波修复，每波由独立子 agent 完成，主 agent 严格验收。

**Blocked by:** None — 直接基于 ab357e0 修复

**Status:** ready-for-agent

## 问题清单（按严重度）

### P0 阻塞性

- [ ] F1.1 · `frontend/src/views/HistoryView.vue:21` Vue 模板里写了 TS 类型注解 `(row: { novel_id: number }) => row.novel_id`，vue-tsc 编译报错 TS1005。整个前端 `pnpm build` 跑不通
- [ ] F1.2 · `backend/auth/login_window.py:79` `morsel.get("value", "")` 永远返回空——这是 ADR 0005 第 4 条警告过的 bug，agent 没复用 spike 经验，复发
- [ ] F1.3 · `backend/main.py` 只起 uvicorn，没起 pywebview 主窗（SPEC §3.1 要求）
- [ ] F1.4 · `/api/ping` 和 `/api/test/events` 端点缺失（ticket 05 acceptance criteria 明确要求）

### P1 重要

- [ ] F2.1 · `backend/core/pixiv_client.py:148` `await asyncio.sleep(REQUEST_INTERVAL)` 缩进在 `async with self._semaphore:` 块外，永远不执行——限速失效，会触发 pixiv 风控
- [ ] F2.2 · `backend/core/pixiv_client.py:126` `asyncio.get_event_loop().call_later()` 在 Python 3.12+ 弃用，改 `asyncio.create_task` + `asyncio.sleep`
- [ ] F2.3 · `pyproject.toml` 在根目录而非 `backend/pyproject.toml`（SPEC §3.4）
- [ ] F2.4 · `tests/` 在根目录而非 `backend/tests/`（SPEC §3.4）
- [ ] F2.5 · `config/settings.example.json` 缺失（ticket 04 acceptance criteria）
- [ ] F2.6 · `__pycache__/` 被 commit（违反 .gitignore）
- [ ] F2.7 · `backend/main.py` CORS 配置反了——dev 走 proxy 同源不需要，prod 走 pywebview 同源也不需要

### P2 i18n + 测试质量

- [ ] F3.1 · `frontend/package.json` 缺 `vue-i18n` 依赖（ticket 15）
- [ ] F3.2 · `frontend/src/main.ts` 缺 `app.use(i18n)`
- [ ] F3.3 · 所有 Vue 组件用硬编码中文文案，应改 `t('...')`（ticket 15）
- [ ] F3.4 · `frontend/src/locales/*.ts` 是裸 dict，没有任何代码引用它们
- [ ] F3.5 · `tests/test_login.py` 有水分测试：`test_extract_csrf_js_is_valid_js_string` 只测 JS 字符串括号配对，不测提取逻辑
- [ ] F3.6 · `tests/test_login.py::test_morsel_to_dict_real_morsel_value_buggy` 名字直接暴露 bug 没修，需删并替换真实提取测试
- [ ] F3.7 · 缺少 csrf 真实提取逻辑的测试（用 spike result.json 做 fixture）

## 修复波次

### 波 1（F1.*）：阻塞性修复——目标"能跑起来"

派 fix sub-agent #1 完成 F1.1~F1.4。修完后：
- 前端 `pnpm build` 必须通过
- 后端 `/api/ping` + `/api/test/events` 必须返回正确响应
- 后端必须能起 pywebview 主窗（prod 模式）
- 登录链路 PHPSESSID value 必须能正确提取（用 spike 验证过的同款代码）

### 波 2（F2.*）：结构对齐 + 核心逻辑——目标"符合 SPEC"

派 fix sub-agent #2 完成 F2.1~F2.7。修完后：
- 限速逻辑正确（每请求间隔 0.4s 实际生效）
- 项目结构对齐 SPEC §3.4
- `__pycache__` 清理干净
- 所有 60 个测试仍全过

### 波 3（F3.*）：i18n + 测试质量——目标"质量达标"

派 fix sub-agent #3 完成 F3.1~F3.7。修完后：
- 切换语言后整个 UI 翻译生效
- 测试覆盖真实功能而非字符串本身
- 所有测试仍全过

## 防错措施（所有子 agent 必须遵守）

### 1. 提供正确代码模板（不留判断空间）

每个 fix 的正确实现已在 spike/ADR 中验证过，子 agent 必须**直接复用**，不得重新发明。具体见各 fix sub-prompt。

### 2. 强制贴验证输出

子 agent 返回时**必须**贴：

- `pnpm build` 完整输出（证明前端编译通过）
- `pytest tests/ -v` 末尾 30 行（证明测试全过）
- 受影响端点的 `curl` 输出（证明功能正常）
- `git status` 输出（证明工作区干净）

未贴输出视为未完成，主 agent 直接打回。

### 3. 明示边界（禁止改的文件）

**绝不能改**：

- `docs/SPEC.md`
- `docs/adr/*.md`（含 ADR 0005）
- `.scratch/pixiv-tool-v1/` 下任何文件
- `spike/cookie_probe/probe.py`（参考源，不改）
- 不属于当前 fix 范围的其他源文件

### 4. 禁止改测试讨好

子 agent **不能为了让测试通过而修改测试断言**——除非测试本身是错的（这种情况下必须在 commit message 里说明"测试 X 是错的因为 Y，正确断言是 Z"）。

特别针对 `test_morsel_to_dict_real_morsel_value_buggy`：这个测试名字暴露了 bug 没修，**子 agent 不能改测试名字或断言让 bug 测试"通过"**，必须修 `_morsel_to_dict` 让测试自然通过（或者删除该测试并加新的真实提取测试）。
