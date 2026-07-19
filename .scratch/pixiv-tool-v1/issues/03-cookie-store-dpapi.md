# 03 — CookieStore 接口 + DPAPI 实现

**What to build:**

从用户视角：登录成功后，PHPSESSID 和 x-csrf-token 等敏感 cookie 被安全地保存到本地（`config/cookies.dat`），文件内容是 Windows DPAPI 加密的二进制，只有当前 Windows 用户能解密。下次启动 App 时，自动从该文件读回 cookie 用于后续请求。模块对外提供清晰的 `CookieStore` 接口，方便 macOS/Linux 未来扩展。

**Blocked by:** None — 可立即开始

**Status:** done

**Acceptance criteria:**

- [x] `backend/storage/cookies.py` 定义 `CookieStore` 抽象基类（ABC），含 `save(cookies: dict) -> None` 和 `load() -> dict | None` 方法
- [x] 工厂函数 `create_cookie_store() -> CookieStore` 按 `sys.platform` 选择实现
- [x] `backend/storage/cookie_dpapi.py` 实现 `DpapiCookieStore`，使用纯 ctypes 调 `crypt32.dll` 的 `CryptProtectData` / `CryptUnprotectData`（不依赖 pywin32）
- [x] 加密数据序列化：dict → JSON → bytes → DPAPI encrypt → 写文件；读取逆过程
- [x] 文件位置：`config/cookies.dat`
- [x] macOS/Linux 平台返回 stub 实现，调用 save/load 抛 `NotImplementedError("V2 实现 keychain/secretstorage")`
- [x] load 时如果文件不存在返回 None（不抛异常）
- [x] load 时如果解密失败（如换 Windows 用户）抛清晰异常并提示用户重新登录
- [x] 单元测试：save → load 往返一致；不存在的文件 load 返回 None；Windows 平台验证 DPAPI 加密结果确实无法在另一用户下解密（可手动验证）
- [x] 日志中绝不打印 cookie 完整值，只打前 8 位 + `(masked)`
- [x] `config/cookies.dat` 已加入 `.gitignore`（前序工作已完成）
- [x] 参考实现：spike/cookie_probe/probe.py 的 `cookies_to_dicts()` 和 `_morsel_to_dict()`（注意 SimpleCookie.Morsel 继承自 dict 的坑，见 ADR 0005）
