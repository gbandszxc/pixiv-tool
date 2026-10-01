# ADR 0005 · Spike 结果：pywebview Cookie 探测可行性

**状态**：已归档（仅「R1 已解决」结论仍有效） · **日期**：2026-07-19 · **关联**：SPEC §4.1、ADR 0002、风险登记 R1

> 状态补充（2026-10-01）：本文是 pywebview 旧栈的 spike 存档，**只有「R1 已解决」这一结论仍然有效**（现行 R1 见 SPEC §11）。
> 正文引用的 `spike/cookie_probe/probe.py`、`result.json`、`backend/auth/`、`backend/storage/` 以及末尾「spike → 主代码复用清单」
> 随 Python 栈一并移除（ADR 0008），**不要照清单复用代码**；现行的 csrf 与登录实现见
> `src-tauri/src/pixiv/csrf.rs`、`src-tauri/src/auth/`。

## 背景

R1（最高风险）：SPEC §4.1 的 A 方案依赖 pywebview `window.get_cookies()` 读到 HttpOnly 的 `PHPSESSID`。读不到就要退 C 兜底（手动粘 PHPSESSID）。

通过 `spike/cookie_probe/probe.py` 实测验证（2026-07-19），登录真实 pixiv 账号 `gbandszxc`（user_id `19509348`），跑完 6 个验证点。

## 结论

**✅ A 方案完全成立**。pywebview 5.4 + Windows WebView2 + Python 3.13 可：

- 读取 HttpOnly 的 `PHPSESSID`（含完整 value）
- 读取 `device_token` / `privacy_policy_agreement` 等其他登录 cookie
- 持久化 cookie（`private_mode=False` + `http_server=True` 跨会话复用）
- 提取 `x-csrf-token`
- 通过 `/ajax/user/self` 接口验证登录态（返回 `userData.id/pixivId/name`）

实测 result.json 摘要：
```
phpsessid_found: true
phpsessid_is_httponly: true
phpsessid_is_secure: true
phpsessid_value_preview: 19509348...
persisted_across_read: true
csrf_token_found: true (2fbbbdae8ffd...)
is_logged_in: true
user_id: 19509348
user_pixiv_id: gbandszxc
```

## 调查中发现并修复的 5 个 bug

这些 bug 都是 SPEC 或 spike 初版的错误假设，全部修正并写入代码：

| # | bug | 真相 | 修正 |
|---|---|---|---|
| 1 | `/ajax/user/self/status` 接口 | **404 不存在** | 改用 `/ajax/user/self`，返回 `userData.{id, pixivId, name}` |
| 2 | csrf token 在 `pageProps.token` | **空** | 在 `dehydratedState.queries[*].meta.apiClient.token`（pixiv apiClient 注入 react-query meta） |
| 3 | `cookie_to_dict()` 把 Morsel 当 Morsel 读 | pywebview 返回 `list[SimpleCookie]`，每个是 dict-like 容器 | 必须遍历 `.items()` 取 `(name, Morsel)` 对 |
| 4 | `isinstance(morsel, dict)` 判断 dict 路径 | **Morsel 继承自 dict**，永远 True，导致永远走 dict 路径 | 显式排除：`isinstance(morsel, dict) and not isinstance(morsel, Morsel)` |
| 5 | `evaluate_js(async_function)` | pywebview 同步模式**不 await Promise**，返回 None | 必须用 callback 模式：`evaluate_js(script, callback=fn)`，且 JS 写成 `new Promise(...)` 而非 `async () => {...}` |

## 对 SPEC / ADR 0002 的影响

| 文档 | 原文 | 修正 |
|---|---|---|
| SPEC §4.1 | 登录检查用 `/ajax/user/self/status` | 改 `/ajax/user/self` |
| SPEC §4.1 | csrf 从 `window.__NEXT_DATA__.token` 提取 | 改从 `dehydratedState.queries[*].meta.apiClient.token` |
| SPEC §7.1 | `GET /api/auth/status` 内部调用接口 | 实际调 `/ajax/user/self` |
| ADR 0002 | 流程图提到 `window.__NEXT_DATA__.token` | 同上修正 |
| 风险登记 R1 | 等级"高" | **降级为"已解决"** |
| 备选触发条件 | "spike 失败退 C" | 不触发，A 方案保留 |

## 后果

**正面**
- 整个项目最大风险消除，A 方案可放心铺开。
- 5 个隐藏 bug 提前发现，避免到登录模块开发时再返工。
- 形成可复用的"pywebview cookie 提取 + csrf + 登录验证"完整方案，直接复用到 `backend/auth/login_window.py`。

**负面**
- `evaluate_js` 必须用 callback 模式——这给后续主代码带来约束：所有要 await Promise 的 JS 调用都得用 `threading.Event` 同步包装。
- csrf token 路径依赖 pixiv 内部 react-query meta 结构——pixiv 改版时要重新探测。已在 `EXTRACT_AND_VERIFY_JS` 写多路径兜底（A/B/C/D）。

## 复用清单（spike → 主代码）

以下代码可直接搬到 `backend/auth/`：

- `cookies_to_dicts()` → `backend/storage/cookies.py` 的 cookie 序列化
- `_morsel_to_dict()` → 同上
- `EXTRACT_AND_VERIFY_JS` → `backend/auth/login_window.py` 的 csrf + login 提取
- callback 模式的 `evaluate_js` 包装 → `backend/auth/pywebview_eval.py` 工具函数
