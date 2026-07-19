# ADR 0004 · Cookie 存储：Windows DPAPI + 跨平台接口

**状态**：已接受 · **日期**：2026-07-19 · **关联 SPEC**：§5.3

## 背景

PHPSESSID 等于账号登录态，存储需平衡安全与便利。候选：

- A. 明文 JSON（任何程序可读，风险高）
- B. Windows DPAPI（仅当前 Windows 用户可解）
- C. Fernet 对称加密 + 主密码（UX 差）

## 决策

**B（Windows DPAPI）**，并通过 `CookieStore` 抽象 + 工厂模式按 `sys.platform` 切换实现。

```python
class CookieStore(ABC):
    def save(self, cookies: dict) -> None: ...
    def load(self) -> dict | None: ...

# 工厂
def create_cookie_store() -> CookieStore:
    if sys.platform == 'win32':    return DpapiCookieStore()
    if sys.platform == 'darwin':   return KeychainCookieStore()   # V2
    if sys.platform == 'linux':    return SecretStorageCookieStore()  # V2
```

V1 仅实现 `DpapiCookieStore`（纯 ctypes 调 `crypt32.dll`，无 pywin32 依赖）；macOS / Linux stub 抛 `NotImplementedError`。

## 理由

1. A 风险真实：恶意程序读到 cookie 文件即可登录用户账号。
2. C 违背"客户端工具"体验，每次输主密码。
3. B 是 Windows 客户端工具行业默认（Chrome/Edge 同做法）。
4. 接口抽象保证跨平台扩展不返工。

## 后果

**正面**
- Windows 用户开箱即用且安全。
- Mac/Linux 实现可在 V2 平滑加入。

**负面**
- V1 Mac/Linux 构建能启动但登录抛 `NotImplementedError`。
- DPAPI 绑定 Windows 用户账号，换机需重新登录（可接受）。
