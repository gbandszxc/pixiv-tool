# ADR 0053：SauceNAO API Key 纳入 WebDAV 同步

- 状态：已接受（扩展 [ADR 0048](0048-webdav-sync-history-and-auth.md) 决策 2 的载荷
  形状与决策 5 的同步范围；包格式、凭据落点与合并语义不变）
- 日期：2026-11-15
- 关联 ADR：[ADR 0048](0048-webdav-sync-history-and-auth.md)（WebDAV 同步基座）、
  [ADR 0049](0049-sync-passphrase-optional-and-connection-gate.md)（口令可选与
  测试门控）、[ADR 0044](0044-structured-command-error-payload.md)（结构化错误载荷）
- 规格：[SPEC](../SPEC.md) §5.2 / §6.1 / §7

## 背景

`settings.saucenao_api_key`（以图识图必需，见 ADR 0048 之前的产品设定）此前只存在
本机：换设备后要在设置页重新粘贴。同步（ADR 0048）已能把浏览历史与登录态搬到新
设备，把这一个键也带上即可「上传一次、到处可用」，不需要新增任何外部契约。

Key 与 Cookie 同属用户凭据：同步包里已经有完整登录 Cookie，Key 的敏感级别不高于
既有内容；真正的约束是**传输与展示路径不得泄漏**——包必须是密文（既有），日志与
错误文案、测试连接回显都不得出现 Key 原文。

## 决策

### 1. 同步范围扩为三个：`history` / `auth` / `saucenao`

- 命令参数 `scopes` 白名单从 `{history, auth}` 扩为 `{history, auth, saucenao}`；
  去重、trim、空集或含未知值 → `sync_no_scope` 的语义不变。
- 新设置键 `sync_scope_saucenao`（默认 `true`，进 `settings_save` 白名单与
  「高级 → 同步」范围勾选），与 `sync_scope_history` / `sync_scope_auth` 同模式。
- 「设置弹窗范围开关默认全选」沿用；三个都关时动作按钮禁用。顶部范围摘要从
  穷举组合文案改为**按所选范围拼接短名**（`scopeName*` 键），新范围只加名字键、
  不新增组合文案。

### 2. 载荷追加字段，`PAYLOAD_VERSION` 保持 1

- 明文 JSON 载荷追加 `saucenao_api_key: string | null`（`#[serde(default)]`
  `Option<String>`）：scope 含 saucenao 且本机 Key trim 后非空才携带，否则为
  `null`。旧客户端（serde 默认忽略未知字段）可读新载荷，新客户端读旧载荷得
  `None`——双向兼容，**版本号不 bump**，`sync_payload_invalid` 的版本校验不动。
- 解析侧归一：Key trim 后非空才保留（空白值与缺失字段同样视为「云端未携带」）。
  下载侧据此保持本机不变，不产生覆盖。

### 3. 上传 / 恢复行为

- **上传**：`build_payload` 扩一位参数（本机 Key 原文引用）；scope 与非空判断在
  archive 层完成。`sync_upload` 返回体追加 `saucenao_included: bool`——只回布尔，
  不回显 Key 原文。
- **恢复**：scope 含 saucenao 且包内 Key 非空时，覆盖本机 `settings.saucenao_api_key`
  并持久化；写入顺序与 `settings_save` 一致——**先落盘再更新内存**，磁盘失败时
  内存保持旧值，命令报 `sync_internal`（安全文案，不含 Key）。合并语义与 Cookie
  的「包内为准、同名覆盖」一致：云端 Key 直接覆盖本机（Key 是单一标量值，无
  「仅更晚覆盖」的时间可比维度；两设备各自改 Key 时，最后恢复者生效，与快照
  语义一致）。云端未携带或未选范围 → 本机保持不变。
  `sync_download` 返回体追加 `saucenao_restored: bool`（只回布尔）。
- 恢复写入走 `AppState.settings`（`Arc<Mutex<Settings>>`）短锁，复用
  `Settings::save(config_dir)`；不另建设置文件写入路径，内存与磁盘不会分叉。

### 4. 测试连接不回显「是否含 Key」

ADR 0049 的备份探测（HEAD / GET 只读响应头）**不下载、不解密包内容**——要判断
云端包是否携带 Key 必须下载整个包并持有正确口令解密，这与「测试连接不要求同步
口令、只读元数据」的设计冲突。故 `sync_test_connection` 的返回体维持
`{status, http_status, has_backup, backup_time}` 不变；「包含 SauceNAO Key」的
指示落在**上传 / 恢复的动作回执**上（前端把 `saucenao_included` /
`saucenao_restored` 拼进会话级成功文案）：刚上传的包内容本机可知，回执准确；
测试探测阶段则本就无法可靠得知，不伪造指示。

### 5. 安全考量（与 Cookie 同档）

- Key 只随 `PTSYNC01` 加密档案传输（AES-256-GCM + PBKDF2，可选内置默认口令的
  保护强度与披露口径沿用 ADR 0049）：WebDAV 托管方只见密文。
- 日志、错误文案、i18n、测试连接回显一律只有「是 / 否」布尔，不出现 Key 原文；
  `sync_internal` 文案固定措辞，不拼接动态值。
- 前端不把 Key 写进任何持久化回执（回执本就是会话级）。
- 远端文件是「最后上传者覆盖」的单文件快照（ADR 0048），Key 随包被覆盖 / 回退
  与 Cookie 的既定取舍一致；设备 A 换 Key 后上传前，设备 B 恢复到的是旧 Key，
  用户重新上传即可收敛。

## 后果

- 换设备只需配置 WebDAV + 恢复一次，SauceNAO Key 即可用；不再手工搬运。
- `SyncPayload` / `ParsedPayload` / 两个 Summary 多一个字段；旧版本客户端与新
  版本客户端的互操作性由 serde 的字段默认值保证，无需迁移脚本。
- 测试连接回显卡不显示「是否含 Key」，用户在上传 / 恢复回执中确认——这是
  「不解密就能测连通」与「内容指示必须准确」之间的取舍。
- `sync_scope_saucenao` 使设置键总数 +1（`settings_save` 白名单 57 键）。

## 替代方案

- **bump `PAYLOAD_VERSION` 到 2**：为加一个可选字段强升版本，会让旧客户端直接
  拒读新包（`sync_payload_invalid`），破坏「新客户端上传后旧客户端仍能恢复」的
  既有互操作；serde 默认行为已提供同等安全性，放弃。
- **测试连接时下载并解密探测**：能回显「云端包含 Key」，但测试从此要求同步口令、
  每次测试多传 8 MiB 上限的流量与一次 PBKDF2 派生，且与 ADR 0049「探测只读
  元数据」的决策冲突，放弃。
- **Key 独立文件 / 独立条目上传**：多文件破坏固定单文件快照语义与「一次测试
  管住一切」的简单性；Key 的敏感级别不高于包内 Cookie，单独加强隔离无收益，
  放弃。
- **恢复时提示用户确认再覆盖**：多一步交互；Key 覆盖的风险与既有「账号 Cookie
  同名覆盖」同级，后者从未要求确认，保持一致，放弃。
