# ADR 0038：翻译审查收敛与按块返工

- 状态：已接受（细化 [ADR 0035](0035-whole-novel-agent-translation.md) /
  [ADR 0036](0036-translation-runtime-guard-and-continuation.md) 的审查与返工判定；
  取代其中「任何已记录问题都使 `verdict=needs_fix`」与「全局返工额度」的口径）
- 日期：2026-10-24
- 关联 ADR：[ADR 0024](0024-translation-bible-conflict.md)、
  [ADR 0035](0035-whole-novel-agent-translation.md)、
  [ADR 0036](0036-translation-runtime-guard-and-continuation.md)、
  [ADR 0037](0037-translation-preview-ownership-and-stability.md)
- 规格：[SPEC](../SPEC.md) §4.3 / §5.2 / §7 / §7.1

## 背景

真实任务 `29453477`（9,430 字、4 块）连续三轮审查不收敛，最终永久无法交付：

1. **`minor` 与事实性错误同等阻塞**。审查规则要求「任何已记录问题都使
   `verdict=needs_fix`」，润色建议、轻微格式偏差与漏翻、术语错误一样把块判为不通过。
2. **返工额度是任务级全局计数**。任意一块返工都消耗同一个 2 轮额度，几块各自的小问题
   很快把全局额度用尽，剩余块连一次修复机会都没有。
3. **返工只能整块重译**。审查指出一两个片段的问题，也必须丢弃整块草稿重新派发翻译
   子 Agent；重译又引入新的问题，下一轮审查再次失败，形成「审查 → 整块重译 → 新问题」
   的循环。
4. **收尾判定与计数失真**。收尾把「块级审查未通过」误报为
   `incomplete_global_review`，用户看到的卡点不在真实阶段；计数在收尾时还被覆盖成 0，
   日志与界面无法还原究竟有多少块待修、返工了几轮。

结论：当前设计把「润色建议」和「必须修复的阻塞问题」混为一谈，且用整块重写代替定点
修订，导致局部问题无法收敛。审查分级与返工粒度必须一起改。

## 决策

### 1. 审查分级：只有 `major` / `critical` 阻塞

- 问题严重度保持 `critical` / `major` / `minor` 三档，语义不变：
  - `critical`：影响事实、法律或技术结论、关键情节、人物关系或用户决策；
  - `major`：明显漏翻、错翻、术语错误、语义不通、目标语言残留导致不可读、标记损坏；
  - `minor`：不改变原意的表达、格式或润色项。
- `verdict=pass` 允许携带 `minor` 问题。`minor` 仍须具体到 `segment` 与证据，供用户查看
  和后续打磨，但不阻塞交付。
- `verdict=needs_fix` 必须至少包含一条 `major` 或 `critical`。只有 `minor` 时不得判
  `needs_fix`；提交校验按此收紧：
  - `pass` 携带 `minor`：接受；
  - `needs_fix` 且至少一条 `major` / `critical`：接受；
  - `pass` 但含 `major` / `critical`：以 `blocking_issues` 拒绝；
  - `needs_fix` 但全部是 `minor`（或 issues 为空）：以 `minor_only` 拒绝，
    提示改交 `pass`；
  - 未知严重度：拒绝。拒绝均不落盘。
- 阻塞判定只看**当前草稿版本**的**当前审查版本**结论。`minor` 不进入待修清单、不消耗
  返工额度、不阻止整体复核与发布；`major` / `critical` 才构成阻塞。

### 2. 局部修订工具 `revise_draft`

- 新增协调者工具
  `revise_draft(chunk_id, patches:[{segment_id, text}])`：
  - 只替换点名的 `segment`，未被点名的片段保持原样；
  - `patches` 不得为空，`segment_id` 必须属于该块；未知 / 重复 / 跨块片段整批拒绝，
    不产生部分写入；
  - 提交前对替换后的完整草稿重新做覆盖、顺序、空值、目标语言与标记保真校验，校验失败
    不写盘；
  - 成功后草稿 `version + 1`，旧审查因 `draft_version` 不匹配自动失效，返回
    `{chunk_id, version, patched, pending_review:true}`。
- 参数级错误码：`duplicate_segment`（同一片段重复）、`unknown_segment`（片段不属于
  该块）、`empty_translation`（替换文本为空）、`draft_invalid`（修订后结构校验失败）、
  `draft_missing`（本块尚无草稿）。以上都在写盘前返回，不产生部分修订。
- `revise_draft` 不派发翻译子 Agent、不产生模型调用、不消耗返工额度；它要求约束已冻结，
  是协调者工具，不是 IPC 命令，前端不直接调用。
- 适用边界：问题集中在少数片段时优先定点修订；整块结构损坏、大面积漏翻或目标语言
  整体错误时仍用 `dispatch_translation` 重译整块。两类动作都走同一套块状态与完成判据。

### 3. 返工额度按块独立

- `translation_agent_rework_rounds`（默认 2，0~5）改为**每块**额度，按
  `rework_by_chunk: Record<chunk_id, number>` 记录：
  - 只有实际重新派发重译的块各自 `+1`；首次翻译与 `revise_draft` 不计数；
  - 同一轮重派多块时，各受影响块分别计数，不合并成一个全局轮次；
  - 某块达到额度后，后续针对该块的返工派发以 `rework_limit` 单独失败，其余块照常
    派发与完成；不再存在全局返工轮次上限。
- 用户手动 `resume` 时清空全部块的返工计数（与既有「恢复时清空 `unknown_outcome`
  记忆」同批处理）。暂停、取消、限流自动重试与自动续跑都不清空计数。
- 总成本仍受累计调用预算与并发闸门约束；按块额度只决定「单块还能修几次」，不放开
  无上限重试。

### 4. 统一块状态 `chunk_gate`

- 新增一个从磁盘产物计算的块状态判定 `chunk_gate`，由三个可同时观察的布尔标记组成，
  不作为互斥枚举：
  - `draft_valid`：草稿带当前 `translate` 阶段版本戳、结构校验通过且未标 `needs_repair`；
  - `review_pass`：审查带当前 `review` 阶段版本戳、`draft_version` 等于当前草稿版本且
    `verdict=pass`（可带 `minor`）；
  - `needs_rework`：当前版本草稿标了 `needs_repair`，或当前版本审查为 `needs_fix`
    （此时必含 `major` / `critical`）。
- `chunk_gate` 是以下判定的唯一来源，不再各处各写一套：
  - `job_status` 与续跑指令的待办按优先级归属：`pending_translate` = `!draft_valid`；
    `pending_rework` = `draft_valid && needs_rework`；`pending_review` =
    `draft_valid && !needs_rework && !review_pass`；
  - 进度计数：`translated` = `draft_valid`，`reviewed` = `review_pass`，
    `needs_repair` = `needs_rework`；
  - 返工目标集合（`needs_rework`）；
  - 完成判据（`draft_valid && review_pass`）与收尾错误码定位。

### 5. 完成判据、收尾计数与错误码

- 完成判据：全部块 `draft_valid && review_pass`，整体复核通过，合并校验通过，正式结果
  为 `complete`。`minor` 不参与该判定。
- 收尾时从磁盘按 `chunk_gate` 重算计数并写回快照 / manifest，不再沿用在途内存里的
  陈旧计数；收尾日志与最终快照一致。
- 审查 / 返工相关错误码：
  - `incomplete_review`：运行结束仍有块没有有效审查（缺失、版本不匹配或审查未通过）；
  - `rework_exhausted`：仍有 `major` / `critical` 阻塞，且相关块的返工额度已用尽，
    在额度内无法收敛；
  - `rework_limit`：工具级拒绝，表示单块返工额度用尽，其他块仍可继续；
  - `incomplete_global_review`：只表示整体复核缺失或未通过，不再用于块级审查失败。

### 6. 可观测性

- 收尾与 `needs_attention` 日志记录：各块已用返工轮次、返工预算 `used` / `limit`、
  受影响块 ID、`verdict` 分布（`pass` / `needs_fix`）与严重度计数（`critical` /
  `major` / `minor`）。
- 日志仍不记录原文、译文、提示词或审查正文，只记录计数、块 ID 与安全摘要。

### 7. 快照与兼容

- Job 快照 / manifest 新增：
  - `pending_rework: string[]`：因阻塞审查待修的块 ID；
  - `rework_by_chunk: Record<string, number>`：每块已用返工轮次；
  - 保留旧字段 `rework_rounds`，语义改为「所有块已用轮次的最大值」，仅供旧消费者
    展示，不参与门禁。
- 新字段使用 serde 默认值；旧 manifest / 旧快照缺少这些字段时按空列表 / 0 读取，
  不影响恢复。旧前端只读 `rework_rounds` 时仍能显示一个单调不减的最大值。

## 取代关系

- 本 ADR 取代 ADR 0036 §3.3 中沿用的「`verdict=pass` 不得携带问题、任何已记录问题都使
  `verdict=needs_fix`」与「任务级 / 全局返工额度」两部分。该口径原先落在
  `submit_review` 校验与内置 reviewer / coordinator 资源中，不在 0036 的运行层防护
  代码里；本 ADR 只改审查判定与返工粒度。
- ADR 0036 的窗口防护、紧急截断、任务级续跑、可重试失败重试与块级派发隔离
  （§1、§2、§3.1、§3.2 及 §3.3 的派发 / 预算部分）不变。
- ADR 0035 的整体流程不变：一篇作品一个 Job、Pass 0～3、独立子 Agent、整体复核、
  发布门禁与 Job 持久化继续有效；其中「质检问题只返工受影响块」与「审查结论与草稿
  版本匹配且通过」由本 ADR 细化为分级放行、局部修订与按块额度。

## 后果

- **`minor` 可放行**：润色建议不再阻塞交付，审查仍完整记录问题，用户可以查看但不被迫
  为每一条建议整篇返工。
- **阻塞问题优先局部修订**：少量片段的问题用 `revise_draft` 定点替换，避免整块重译
  引入新问题；整块损坏仍有重译路径。
- **预算与轮次仍受控**：额度从「全局 2 轮」变成「每块 2 轮」，单块反复不收敛只消耗
  自己的额度并以 `rework_exhausted` 收敛，不拖累其他块；总调用量仍受累计预算约束。
- **不静默发布阻塞问题**：只有 `review_pass` 的块可进入完成判定，`major` / `critical`
  不会被 `minor` 放行规则吞掉；收尾计数与错误码直接对应用户看到的阶段。
- **判定口径统一**：`chunk_gate` 收敛待办、计数、返工目标与完成判据，减少「界面说待修、
  收尾说已审、错误码说整体复核」这类互相矛盾的输出。
- **残余风险**：严重度仍由审查模型判断，可能把 `major` 误报为 `minor`；缓解手段是
  严重度定义、审查复检与收尾的 `verdict` / 严重度分布日志。`revise_draft` 只保证结构
  与覆盖率，替换文本的语义质量仍需再审。
