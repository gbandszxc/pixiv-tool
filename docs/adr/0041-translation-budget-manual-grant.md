# ADR 0041：翻译调用预算手动追加

- 状态：已接受（细化 [ADR 0036](0036-translation-runtime-guard-and-continuation.md)
  的预算语义；不改变「预算防失控」的设计意图）
- 日期：2026-10-29
- 关联 ADR：[ADR 0034](0034-rig-agent-kernel-and-rust-toolchain.md)、
  [ADR 0036](0036-translation-runtime-guard-and-continuation.md)
- 规格：[SPEC](../SPEC.md) §4.3 / §7

## 背景

预算耗尽（`budget_exhausted`）的任务在界面上点「继续」会立即再失败：`resume` 只重置
取消位与按块返工额度，预算用量从 manifest 原样还原，主 Agent `run_start` 后第一次
`try_consume_call` 即失败（日志表现为 turn=1 直接 `run_failed`），用户得到一次无效
点击。`BudgetTracker` 早已实现 `add_manual(amount, reason)` 且 `manual_extra` 随
manifest 持久化、重启可还原，但全仓库没有任何 IPC 命令或界面入口调用它——手动追加
能力存在于运行层却从未暴露。

同时，预算的存在意义是防失控烧钱，恢复时「自动补一笔预算」会让护栏形同虚设。

## 决策

### 1. 手动追加，且只手动

- 新增 IPC 命令 `novel_translation_grant_budget(jobId, amount)`：
  - `amount` 校验 1 ~ 10 000，越界拒绝；
  - 进程内有句柄时：状态不得为 `completed` / `cancelled` / `deleted` /
    `awaiting_confirmation`（运行中、暂停、需要处理、中断均可追加），调用
    `budget.add_manual(amount, "user_grant")` 并把 `manual_extra` 写回 manifest；
  - 应用重启后无句柄时：读改 manifest 的 `attempts.manual_extra`，返回手算快照
    （`resume` 重建句柄时既有逻辑会还原 `manual_extra`）；
  - 返回追加后的 `BudgetSnapshot { used, limit, remaining, manual_extra }`。
- **不做自动追加**：`resume` 遇 `budget_exhausted` 不补预算，任务停在
  `needs_attention` 等用户显式决定。追加是用户对成本的显式授权，与「预算随检查点
  持久化、恢复不重置」（ADR 0036）共同构成「只增不减、只认手动」的口径。

### 2. 界面：耗尽态用「追加预算并继续」替换「继续」

- 任务菜单在 `needs_attention` / `interrupted` 且 `error_code=budget_exhausted` 时：
  - 不再渲染裸「继续」按钮（避免无效点击）；
  - 展示预算用量（`used / limit`，`manual_extra > 0` 时注明「含手动追加」）与追加
    档位（+64 / +128 / +256，默认 64）；
  - 主按钮「追加预算并继续」串行调用 grant → resume，失败走既有 actionError 展示。
- 走 M3 组件与既有 token，不新增弹窗。

## 后果

- 耗尽后的「继续」从必然失败的无效点击变成一次显式的成本决策；`manual_extra`
  随 manifest 持久化，跨重启保留（重开应用后追加额不丢）。
- 自动预算公式（`64 + 32 × 分块数`）继续作为基线；频繁需要追加说明分块或预算
  公式需要再校准，追加记录（`ManualGrant` 的 amount / reason / 时间）保留在
  `BudgetTracker` 中供诊断。
- 残余风险：追加只解决「预算被正常工作耗尽」；若耗尽源于死循环（如 ADR 0040 的
  注释策略死锁），追加会继续烧钱——因此追加入口必须与根因修复同批交付。
