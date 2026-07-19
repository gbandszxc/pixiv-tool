# 06 — pywebview 登录窗（复用 spike 成果）

**What to build:**

从用户视角：在主界面（Vue）点"登录"按钮，弹出一个独立的 pywebview 窗口加载 Pixiv 登录页（`https://accounts.pixiv.net/login`）。用户在窗口里输账号密码、过验证码、过 2FA，登录成功后窗口自动关闭，主界面提示"登录成功"。后台已经拿到了 PHPSESSID（HttpOnly）和 x-csrf-token，并通过 CookieStore 加密保存到本地。下次启动 App 直接复用，无需重新登录。

**Blocked by:** 03（CookieStore）、05（FastAPI+Vue 通信）

**Status:** done

**Acceptance criteria:**

- [x] `backend/auth/login_window.py` 实现 `open_login_window() -> dict`，弹出 pywebview 窗口加载登录页
- [x] pywebview 配置：`private_mode=False`、`http_server=True`、固定 `http_port`（保证 cookie 持久化）
- [x] 登录成功检测：监听 `loaded` 事件，URL 含 `www.pixiv.net` 时触发提取
- [x] Cookie 提取：调用 `window.get_cookies()`，用 spike 验证过的 `cookies_to_dicts()` 转换（注意 Morsel-is-dict 坑）
- [x] x-csrf-token 提取：用 `evaluate_js(script, callback=fn)` 的 callback 模式（不是同步模式），JS 写成 `new Promise(...)` 而非 `async () => {...}`
- [x] csrf 提取路径正确：`__NEXT_DATA__.props.pageProps.dehydratedState.queries[*].meta.apiClient.token`（多路径兜底 A/B/C/D）
- [x] 提取到的 PHPSESSID + x-csrf-token 通过 `CookieStore.save()` 持久化
- [x] `POST /api/auth/login` 触发上述流程，返回登录结果（成功/失败/取消）
- [x] 主窗 Vue 收到登录成功后显示提示
- [x] 用户关闭登录窗（未登录）时返回明确状态，不抛异常
- [x] 登录窗大小合理（960x720），标题明确（"登录 Pixiv"）
- [x] 单元测试：mock pywebview 验证提取逻辑（用 spike 的 result.json 作 fixture）
- [x] 参考实现：`spike/cookie_probe/probe.py`（已通过 R1 验证，见 ADR 0005）
