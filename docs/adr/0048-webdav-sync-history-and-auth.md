# ADR 0048：WebDAV 同步浏览历史与登录态

- 状态：已接受（新增外部契约：第三方 WebDAV 目录；扩展 [ADR 0010](0010-multi-account-login.md)
  的多账号凭据落点与 [ADR 0014](0014-browse-history-persistence.md) 的浏览历史合并语义）
- 日期：2026-11-03
- 关联 ADR：[ADR 0008](0008-tauri-rewrite.md)（wreq 传输）、
  [ADR 0010](0010-multi-account-login.md)、[ADR 0014](0014-browse-history-persistence.md)、
  [ADR 0044](0044-structured-command-error-payload.md)（结构化错误载荷）
- 规格：[SPEC](../SPEC.md) §3.4 / §5.2 / §5.3 / §6.1 / §7 / §10 / §11

## 背景

浏览访问历史（ADR 0014）与多账号登录态（ADR 0010）此前只存在于本机：换设备后
历史为空，所有账号都要重新登录。用户自建 / 托管的 WebDAV 是最低门槛的跨设备落点
（Nextcloud、坚果云、群晖等），不需要应用自建服务端；本项目不引入本地 HTTP 服务与
后端进程的边界（ADR 0008）保持不变。

同步包同时包含 Cookie 等登录凭据，因此远端文件必须是**密文**，且解密口令不能与
WebDAV 密码混为一谈：WebDAV 服务方（包括第三方托管）能看到文件内容与元数据。
同步是**手动触发**的一次性动作，不做后台轮询或自动合并——两端同时改动时的自动
冲突解决会放大凭据覆盖风险。

## 决策

### 1. 外部契约：WebDAV 目录 + 固定文件

- 用户配置的是**目录 URL**（`webdav_url`）。`settings_save` 只按白名单落盘；同步
  动作解析时只允许 `http` / `https`，要求有 host，禁止 query、fragment 与 URL 内嵌
  凭据，并 trim、去掉尾部 `/`（不合法 → `sync_url_invalid`）。
- 远端文件固定为 `<目录>/pixiv-tool-sync.bin`（`REMOTE_FILE_NAME`），不按设备或
  账号分文件——同一目录只保存一份“最新快照”，最后上传者覆盖。
- 写入前先 `MKCOL` 目录（已存在 / 不支持 MKCOL 均继续，由 PUT 定性），再 `PUT`
  整个包；读取直接 `GET`。实现为独立 wreq client（UA `pixiv-tool-sync/1`），
  **不使用 Chrome 指纹伪装**：第三方 WebDAV 不校验浏览器指纹，伪装无收益且增加
  兼容面。
- Basic Auth 仅在 `webdav_username` 非空时发送；用户名非空而密码缺失视为配置
  不完整（`sync_config_incomplete`），用户名空 = 匿名访问。
- MKCOL 必须显式空 body + `Content-Length: 0`，否则 wreq 发 chunked 编码，部分
  WebDAV 服务（含部分 Nextcloud 版本）拒绝。

### 2. 应用层加密与格式

包格式（全部小端）：

```text
magic "PTSYNC01" (8)
iterations u32 LE (4)
salt (16)
nonce (12)
ciphertext + tag (AES-256-GCM)
```

- 密钥：`PBKDF2-HMAC-SHA256(sync_passphrase, salt, iterations, 32)`；上传固定
  `iterations = 600_000`，每次上传重新生成随机 salt 与 nonce。
- AAD = `magic || iterations LE || salt`（28 字节），把头部参数绑进认证范围；篡改
  头部任一字节都会让 GCM 认证失败。解密接受包头声明的迭代次数但设 10,000,000
  上限，防御构造的超大迭代拖死进程。
- 解密侧任何异常（魔数错误 / 截断 / 参数异常 / 认证失败）统一为
  `sync_decrypt_failed`，不回显底层错误细节或密文片段。
- 明文载荷是 JSON：`{version: 1, exported_at: <UTC RFC3339>, history: [...] | null,
  accounts: {active, items} | null}`；未选范围对应字段为 `null`。版本不识别
  → `sync_payload_invalid`。

### 3. 凭据落点

| 数据 | 落点 |
|---|---|
| WebDAV 目录地址 / 用户名 | `settings.json`（非敏感） |
| 同步范围开关 | `settings.json`（`sync_scope_history` / `sync_scope_auth`，默认 `true`） |
| WebDAV 密码 | 系统凭据库 service `pixiv-tool.sync`、account `webdav-password` |
| 同步口令（加密口令） | 系统凭据库 service `pixiv-tool.sync`、account `sync-passphrase` |

- 口令长度 8~256 **字符**（按 `chars().count()`，中文按字符计）；解析时先整体
  trim，trim 后为空 = 未配置。WebDAV 密码不 trim（空格可能是密码的一部分）。
- `settings_save` 的 `webdav_password` / `sync_passphrase` 是独立写入参数：省略 =
  保持既有条目，空串 = 删除条目。`settings_get` 只额外返回
  `webdav_password_configured` / `sync_passphrase_configured` 布尔，原文永不出后端。
- settings.json、数据库、日志、错误文案都不得出现两个凭据原文；凭据库条目的
  读写错误也只给安全检查文案。

### 4. 手动双向与合并语义

命令只有三条：`sync_test_connection`（只 MKCOL，不读写文件，不要求同步口令）、
`sync_upload`（导出 → 加密 → MKCOL + PUT）、`sync_download`（GET → 解密 → 解析 →
按所选范围合并）。均接受可选 `config` 草稿覆盖（地址 / 用户名 / 密码 / 口令），
使设置弹窗可以先测试、先上传再保存；`None` 表示全部用已保存值与凭据库。

**浏览历史合并**：`INSERT ... ON CONFLICT(kind, work_id) DO UPDATE SET ...`，
仅当 `excluded.visited_at > browse_history.visited_at` 时更新整行（标题、作者、
封面、页数、R-18、访问时间）。这一“仅更晚覆盖”规则让下载恢复既可补齐本机缺失
的作品，也不会把本机更晚的访问记录倒退。`visited_at` 导入前统一归一为 UTC 微秒
精度 + `Z`（固定 27 字符，可直接字符串比较）；解析失败、`kind` 不在
`illust|manga|novel`、`work_id <= 0` 的条目跳过，不视为整体失败。

**账户合并**：同 `user_id` 覆盖（逐条 `store_cookies` 写 `u-<id>` 条目 +
`upsert` 索引元信息）。导出只包含“已登记且 Cookie 含非空 PHPSESSID”的账号；
导入逐条校验 `user_id` 词法、`info.user_id` 与被条目一致、PHPSESSID 非空，
非法条目跳过。本地额外账户（远端包中没有的）不删除。

**active（当前账号）规则**：

- 本地已有 active：保持本地选择（upsert 会移动 active 指针，因此写完显式还原）；
  若云端 active == 本地 active，用包内 Cookie 刷新 `default` 镜像。
- 本地无 active 且云端 active 在导入集合内：写入 `default` 并切换到该账号，
  `active_changed = true`。
- 本地无 active 且云端 active 不在导入集合：保持无 active，不擅自挑一个账号。
- 任何账户写入或 `default` 刷新后调用
  `pixiv::browse_api::invalidate_session_caches()`，清空进程级自 uid 与主站
  csrf 缓存（否则切号后仍按旧身份拼路径 / 带旧 token）。

### 5. 同步范围

- 命令参数 `scopes: Vec<String>` 的合法值只有 `history` / `auth`；去重后为空或
  含未知值 → `sync_no_scope`。
- 设置键 `sync_scope_history` / `sync_scope_auth` 默认 `true`，设置弹窗范围开关
  默认全选（老配置 / mock 缺键按 `true` 回显）；两项都关时动作按钮禁用并提示
  至少选一项。
- 下载只合并所选范围；包内未包含的范围字段为 `null`，对应导入计数为 0，不报错。

### 6. 上限、超时与代理

- 下载响应体上限 **8 MiB**：先看 `Content-Length`，再边流式读边累计长度，任一
  超限立即 `sync_oversize` 并中止（服务端谎报或省略 `Content-Length` 也安全）。
- 单次请求总超时 **30s**，重定向策略 `none`（不跟随 3xx 到未知主机）。
- 默认跟随系统代理；连接 / DNS / 超时类网络错误用 `.no_proxy()` **直连重试一次**
  （每次操作最多一次，避免重复写入）。
- 状态码语义：MKCOL `200/201/204/301/302/405/409` 继续，认证 `sync_auth_failed`，
  其余 `sync_http_error`；PUT 仅 `200/201/204` 成功；GET 2xx 成功、`404`
  `sync_not_found`、`401/403` `sync_auth_failed`。
- 错误统一为 `{code, field, message}`（ADR 0044 形状），前端按 `code` 取
  `sync.errors.<code>` 文案，字段错误停靠到对应输入框。

### 7. 界面与安全边界

- 设置左栏新增一级分组「高级」，内含「同步」二级页（同一内容区切换，不叠模态）；
  配置字段、凭据“已保存”胶囊与清除、范围 checkbox、测试 / 上传 / 恢复动作行、
  会话级回执，均复用既有设置配方，不新增颜色 / 圆角 / 阴影 / 动效 token。
- 「从云端恢复」是破坏性动作，点击后先弹二次确认（原生 `dialog`，文案写明合并
  与覆盖范围）；恢复后 `auth_changed` 为真时前端刷新登录态（auth store / 账号
  列表），`history_imported > 0` 时广播 `pixiv-tool:history-restored`，浏览历史页
  回到第 1 页重拉；刷新失败不覆盖同步成功回执。
- 安全边界：同步客户端只访问用户显式配置的 WebDAV 主机；不自动上传、不在后台
  运行；日志不记录 URL query、Cookie、WebDAV 密码或同步口令；错误文案不含这些
  原文，也不回显解密细节。

## 后果

- 用户可用任意标准 WebDAV 目录在设备间搬运历史与登录态，应用不承担服务端运维；
  没有账号体系，也没有厂商可见的明文。
- 快照语义是“最后一次上传覆盖”：两台设备各自上传会互相覆盖远端，必须手动先
  下载再上传；应用不提供自动合并或版本历史。
- 同步口令丢失 = 远端包不可恢复（无口令提示、无后门）。WebDAV 密码与同步口令
  都依赖系统凭据库可用；Linux 无 Secret Service 时相关动作直接失败并给安全文案。
- 合并规则只保证“历史不倒退、本地账号不丢”；同名账号的 Cookie 以包内为准，
  若远端快照较旧，可能回退该账号的登录态（可在本地重新登录修复）。
- 同步客户端使用独立、无指纹伪装的 wreq client 与 30s 超时，不参与 pixiv 的
  限速与 429 退避体系；第三方 WebDAV 的兼容性（MKCOL 行为、自签证书、非标准
  端口）只在单元测试与本地模拟服务器验证，真实服务端尚未全量实测。
- 远端文件是固定名，多账号 / 多设备共用同一目录时无法并存多份快照。

## 替代方案

- **应用自建云同步服务**：需要账号体系、服务端运维与隐私合规，违背“无本地 HTTP
  服务、无后端进程”的轻量定位，放弃。
- **明文 JSON 直接上传**：实现最简单，但 Cookie 落在第三方 WebDAV 上等于把登录态
  交给托管方，放弃。
- **仅用 WebDAV 密码派生密钥**：少一个凭据，但改 WebDAV 密码即无法解密历史包，
  且托管方知道密码即可解密；独立同步口令把“存储访问”和“内容解密”分开。
- **系统 keyring 直接加密包（无口令）**：换设备无法解密（keyring 不跨设备），
  与跨设备目标冲突。
- **自动后台同步 / 双向实时合并**：需要冲突解决、版本向量与后台调度，凭据覆盖面
  大且不可预期，本期只做手动快照。
- **按设备 / 账号分文件**：便于并行，但目录语义与清理变复杂；固定单文件换取
  可预测的“最新快照”行为。
