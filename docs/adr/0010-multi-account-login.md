# ADR 0010 · 多账号登录态存储与切换

**状态**：已接受 · **日期**：2026-08-29 · **关联**：SPEC §4.1、§5.3、ADR 0006、ADR 0009

> 状态补充（2026-10-01）：决策 4 与 5 中「同步内嵌 webview」的机制（含
> `browse_sync_login` 路径）已随 [ADR 0013](0013-remove-embedded-browser.md) 移除——
> `/pixiv` 内嵌浏览器整体删除后，账号切换 / 退出只更新凭据存储与账号索引，
> 不再有 webview 同步步骤；多账号存储与切换的其余决策（镜像语义 / 每账号条目 /
> 索引 / 退出语义 / 实例缓存）仍有效。

## 背景

V1 登录态是单账号模型：keyring 单条目（service `pixiv-tool.cookies`、
account `default`）存当前账号 cookie，登录即覆盖。用户需要在多个 Pixiv
账号间切换（如个人/工作号分离），重复登录-登出成本高且旧账号凭据直接丢失。

约束与既有事实：

- 登录态是安全边界：只允许进系统凭据存储（Keychain / Credential Manager /
  Secret Service），绝不入 SQLite、绝不落 `config/`（AGENTS.md）。
- `default` 条目已被广泛依赖：抓取客户端、auth_status、内嵌 webview
  自动注入（`browse::auto_inject_on_first_load`）、同步登录回退
  （`browse_sync_login` / `inject_saved_and_reload`）都从它读取。
- 账号的**元信息**（user_id / pixiv_id / 昵称 / 头像缓存名）不是秘密，
  但索引需要持久化且随凭据一致变化。

## 决策

1. **`default` 条目语义不变，恒为「当前激活账号」的镜像**。所有既有读取方
   零改动，切换账号 = 原子地换掉镜像内容。
2. **每账号独立凭据条目** `u-<user_id>`（同 service，复用 CookieStore 的
   Windows 分片机制）。显式登录与网页同步时同时更新镜像和账号条目；切换时
   直接把目标条目读出写入镜像，避免无变化的重复归档触发 Keychain 授权。
3. **账号索引** `config/accounts.json`：`{active, accounts:[元信息]}`，
   **只存元信息，任何 cookie 都不进 config**。登记时机：登录成功
   （浏览器 / 内嵌 webview / 手动 Session）、`browse_sync_login` 成功、
   `auth_status` 校验成功只刷新元信息；旧单账号 default 数据首次校验时补建
   账号凭据条目完成迁移。
4. **切换后内嵌 webview 自动同步**：`auth_account_switch` 在 webview 已创建
   时复用 `inject_saved_and_reload`（先清全部旧 PHPSESSID，再注入新账号
   cookie → 导航回首页）；
   未创建时无需处理（首次加载的 auto_inject 读镜像即新账号）。既有
   「webview 已有有效会话则跳过注入」探测只影响 auto_inject 路径，
   不拦截显式切换。
5. **退出登录语义收窄为「退出当前账号」**：清镜像 + 删该账号条目 + 移除
   索引项；若仍有账号则自动激活列表首项并同步 webview，若没有则同步清除
   webview 的 PHPSESSID。`auth_status` 探测确定失效（401/403）同样
   移除该账号，避免「死账号」反复出现在切换列表里。
6. 账号条目实例按 user_id 缓存（Arc 共享其 load 内存缓存）：macOS Keychain
   对 dev 重编译的二进制首读会弹授权框，实例缓存把弹框压到每账号一次。

## 后果

**正面**

- 多账号切换零重复登录；切换对抓取、webview、状态探测全部无感生效。
- 旧版本单账号数据无缝迁移（首次 auth_status 成功自动建档）。
- 凭据仍在系统凭据存储，索引文件不含敏感数据，安全边界不放宽。

**负面**

- cookie 在「镜像 + 账号条目」双写，约 2× keyring 空间（每条几 KB，可忽略）；
  一致性由显式登录/网页同步双写与切换时替换镜像保证，无并发切换场景。
- macOS 每个账号凭据条目首次访问仍可能弹一次 Keychain 授权框；稳定签名后
  「始终允许」可持续生效，ad-hoc 重打包仍会使授权失效（见 PACKAGING §7）。
- 索引与凭据分开存储：索引损坏只丢列表不丢登录态（重新校验自动重建）；
  凭据条目被用户手动删除的账号在切换时报「登录态不存在或已失效」。
