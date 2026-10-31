# ADR 0046：翻译等待确认的持久化与系统通知

- 状态：已接受
- 日期：2026-10-31
- 关联 ADR：[ADR 0035](0035-whole-novel-agent-translation.md)（整篇小说 Agent 翻译）、[ADR 0036](0036-translation-runtime-guard-and-continuation.md)（任务续跑）、[ADR 0044](0044-structured-command-error-payload.md)（设置保存错误载荷）
- 规格：[SPEC](../SPEC.md) §3（小说翻译 Job）/ §5.2（设置键）/ §7（命令表）

## 背景

1. **等待确认状态在重启后不可操作**：`translation_confirm_before_start` 开启时，任务在冻结约束后进入 `awaiting_confirmation` 并落盘到 manifest。应用重启后底栏仍能列出该任务（状态来自磁盘），但 `novel_translation_confirm` 只查进程内 `JobHandle`，直接返回「翻译任务不存在」。用户只能取消重来，等待确认的功能价值被重启抹掉。
2. **等待确认容易被错过**：术语与风格抽取可能需要数分钟，期间窗口常被最小化。进入等待后没有系统级提醒，用户回到应用才会发现任务停在待确认。
3. **通知需要可控**：用户可能不希望被系统通知打扰，或系统通知被误触发时无法关闭；默认应开启（功能价值的默认体现），但要提供明确的开关。

## 决策

### 1. 确认命令支持从磁盘重建任务上下文

`TranslationService::confirm` 在进程内没有句柄时先 `rehydrate(job_id)`，与 `resume` / `skip_chunks` 同口径：

- 校验缓存世代、内置资源版本、任务未处于终态；
- 重新打开 `JobStore`、读回原文记录与 journal、从 `settings_snapshot.json` 还原模型参数，凭据仍从系统凭据存储读取；
- 预算用量、手动追加额度、约束版本、阶段与块计划全部来自 manifest 与产物；
- 重建后的 `confirm` 与进程内行为一致：写回约束（版本 +1，用户值优先）、`mark_confirmed`、清空等待标志、转 `queued` 并拉起 supervisor。

`confirm_before_start` 在重建时按 `translation_confirm_before_start && manifest.constraint_version == 0` 计算；即使用户在重启前关闭了开关，`mark_confirmed` 仍保证确认后不会再进入等待。

### 2. 等待确认发送系统通知

- 接入 `tauri-plugin-notification`，在 `EventSink` 上新增 `notify` 默认方法（`SystemNotification { title, body }`），由 `TauriEventSink` 实现为系统通知；测试出口 `RecordingSink` 记录通知，离线宿主（`NoopSink`）丢弃。通知不进入 `translation://job` 事件流，不占用 `seq`。
- 触发点只有一处：`drive` 中状态迁移到 `AwaitingConfirmation` 之后发送一次。重启后的等待由界面与底栏提示，不重复通知；暂停 / 恢复不会重新触发（`constraint_version` 已非 0）。
- 文案跟随 `settings.language`（zh-CN / en-US / ja-JP），正文包含小说标题，标题读取失败或为空时回落 `小说 #<novel_id>`；标题截断到 80 字符。`input.json` 含整篇原文，标题解析放 `spawn_blocking`，不占用 async worker。
- 发送失败只记录 `event=notification_failed` 日志（不含正文），不改变任务状态：界面、底栏与事件流仍是真相源。

### 3. 通知开关

- 新增设置键 `notifications_enabled: bool`，默认 `true`，加入 `settings_save` 白名单（`WRITABLE_KEYS` 第 50 个键）并校验布尔类型（`settings_bool_invalid`）。
- 设置弹窗「通用」分组新增「通知管理」小节，复用 `settings-fieldset` + `md-checkbox`，整表保存 / 取消语义不变；不加新 token，不改视觉方向。
- 开关实时读取全局设置：关闭时通知路径连小说标题都不读。

### 4. 插件版本锁定

`tauri-plugin-notification` 锁 `=2.4.0`：2.5.0 起要求 `tauri ^2.12`，会把 tauri 从 2.11 升到 2.12 并连带把 `webview2-com` 从 0.38.2 升到 0.39.1，而 `src-tauri/src/menu_bar.rs` 显式依赖 `webview2-com = "=0.38.2"` 与 `windows = "0.61"` 的 WebView2 COM 类型，升级即出现 trait 不匹配的编译错误。升级该插件时必须同时验证 tauri / webview2-com / windows 三项并回归 Windows 菜单栏。

插件在 Windows 上的限制：只有安装版以应用身份显示通知；开发运行（未注册 AppUserModelID）显示 PowerShell 名称与图标。这是插件与 Windows Toast 的平台行为，不是本项目的缺陷。

## 后果

- 等待确认成为跨重启的稳定状态：关掉软件再打开，底栏任务仍可确认并继续。
- 通知与设置开关可通过 `RecordingSink` 在离线 e2e 中验证；真实系统通知的最终呈现由安装包验证。
- 通知目前只覆盖「等待确认」；后续新增通知类型时复用同一 `EventSink::notify` 出口并统一受 `notifications_enabled` 控制。
- 新增依赖带来 `notify-rust` / `tauri-winrt-notification` 与第二份 `windows`（0.62）依赖树；`Cargo.lock` 变化限定在通知相关条目与依赖列表排序。
