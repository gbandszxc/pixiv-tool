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

## 验证矩阵

| # | 验证点 | 期望 | 实测 |
|---|---|---|---|
| 1 | `get_cookies()` 返回类型 | `list[SimpleCookie]` | |
| 2 | 能否拿到 `PHPSESSID` | ✅ 能（HttpOnly 也读得到） | |
| 3 | 能否拿到 `device_token` / `privacy_policy_agreement` | ✅ 能 | |
| 4 | `private_mode=False` 后跨会话持久化 | ✅ 能 | |
| 5 | 重定向到 www.pixiv.net 后能读全 cookie | ✅ 能 | |
| 6 | 提取 `x-csrf-token`（从页面 JS） | ✅ 能 | |

## 备用方案

若验证失败 → 退 SPEC §4.1 的 C 兜底（手动粘 PHPSESSID）。
