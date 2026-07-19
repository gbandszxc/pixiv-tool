# Spike: Cookie Probe · R1 风险验证

> **目标**：验证 pywebview 4+ 在 Windows + WebView2 下，能否通过 `window.get_cookies()`
> 读取 HttpOnly 的 `PHPSESSID` cookie。
>
> **关联**：SPEC §4.1、ADR 0002、风险登记 R1。
>
> **结论**：见 [../../docs/adr/0005-cookie-probe-result.md](../../docs/adr/0005-cookie-probe-result.md)

## 运行方式

```powershell
cd D:\ProjectSpace\gitcode\pixiv-tool\spike\cookie_probe
uv sync
uv run python probe.py
```

## 使用流程（v2 · 自动探测）

1. 启动后弹出 pywebview 窗口加载 pixiv 登录页。
2. 手动登录（输账密 / 过验证码 / 过 2FA）。
3. 登录成功页面跳转到 `www.pixiv.net`。
4. **自动**触发 cookie + csrf 提取，结果：
   - 打印到终端（PowerShell 窗口里）
   - 完整明细写入 `result.json`
5. 窗口保持打开，手动关闭即可。

> v1 用过双窗口（登录窗 + 控制面板），但 `http_server=True` 与内嵌
> HTML 字串冲突报 404，已废弃。v2 改为单窗口 + `loaded` 事件自动探测。

## 验证矩阵

| # | 验证点 | 期望 |
|---|---|---|
| 1 | `get_cookies()` 返回类型 | `list[SimpleCookie]` |
| 2 | 能否拿到 `PHPSESSID` | ✅ 能（HttpOnly 也读得到） |
| 3 | 能否拿到 `device_token` / `privacy_policy_agreement` | ✅ 能 |
| 4 | `private_mode=False` 后跨会话持久化 | ✅ 能 |
| 5 | 重定向到 www.pixiv.net 后能读全 cookie | ✅ 能 |
| 6 | 提取 `x-csrf-token`（从页面 JS） | ✅ 能 |

## 备用方案

若验证失败 → 退 SPEC §4.1 的 C 兜底（手动粘 PHPSESSID）。
