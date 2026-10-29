# ADR 0043：子任务内容拒绝快速失败与被拒块跳过

- 状态：已接受（细化 [ADR 0036](0036-translation-runtime-guard-and-continuation.md)
  的恢复分类与派发隔离；不推翻其 fatal 判据）
- 日期：2026-10-29
- 关联 ADR：[ADR 0035](0035-whole-novel-agent-translation.md)、
  [ADR 0036](0036-translation-runtime-guard-and-continuation.md)
- 规格：[SPEC](../SPEC.md) §4.3 / §7

## 背景

真实任务 `20261028235331-6d4fa1`（作品 19162266，14 块）中，c0012 / c0013 原文包含
未成年人露骨性内容，模型按内容政策正确拒绝翻译且不提交草稿。工程链路把这次拒绝
处理成了最坏的形态：

1. **拒绝原因丢失**。翻译子 Agent 按 `translator.md` 的失败上报格式输出了结构化
   摘要（`status=failed reason=prohibited_content detail=…`），但运行层不解析最终
   文本，只检查「有没有提交草稿」，统一归为 `RunError::Internal` →
   `FailureClass::Permanent` → 整批 `fatal`，界面上是误导性的
   「internal：需要修正设置后重试」——而内容拒绝没有任何设置可修。
2. **无限重派循环**。主 Agent 连续三轮重派同一对块（每轮烧约 6 次模型调用），结果
   完全相同；`resume` 后还会再来一遍，任务永远出不了 `needs_attention`，其余 12 块
   的成果无法交付。

产品原则：我们无法控制用户用翻译功能处理什么内容，也**不做任何绕过内容审查的
尝试**（提示词层 1.3.1 已有的降触发设计——极简 system 提示词、逐块隔离派发——保持
不变）；工程上要做到的是：被拒后快速失败、不无限重试、把其余块的成果交到用户手里。

## 决策

### 1. 内容拒绝是一等失败分类

- 运行层新增 `RunError::Refused { reason, message }`（错误码 `content_refused`）与
  `FailureClass::ContentRefused`；与 provider 层的 `ContentFiltered`（HTTP 内容过滤，
  含采样随机性、可退避重试）明确分层——本分类表示**子 Agent 业务层显式拒绝**。
- `run_translator` / `run_reviewer` 在「没有产物」路径先解析最终文本的失败摘要
  （`status=failed reason=<码> detail=<说明>`，`detail` 脱敏截断 200 字符）；reason
  命中拒绝词表（`polic` / `prohibited` / `refus` / `safety` /「违规」/「政策」）时
  返回 `Refused`，否则维持原 `Internal` 路径（`constraint_conflict` 等仍走
  返工额度机制）。

### 2. 恰好重试一次，之后快速失败

- `dispatch_one` 对 `ContentRefused` **允许原样重试 1 次**（不做退避——内容拒绝与
  负载无关；采样有随机性，首次拒绝可能偶发）；第 1 次重试仍被拒即视为确定性拒绝：
  记入被拒清单并返回 `content_refused` 块结果，不再有任何自动重试。
- **不升级整批 fatal**：fatal 判据维持只看 `permanent` / `budget_exhausted`
  （ADR 0036）。内容拒绝是块级属性，同批其他块照常返回与推进。
- 被拒块记入任务记忆（manifest `refused_chunks: 块ID→原因码` + 进程内镜像），后续
  派发规划直接短路为 `content_refused`，不请求模型、不消耗预算；与 `unknown_outcome`
  的「resume 即清空」不同，**resume 不清被拒记忆**——同一内容重试结果确定相同，
  清空只会再烧一次调用。只有用户显式跳过或重新发起任务才解除。

### 3. 被拒块用户跳过，任务用其余部分交付

- 新增 IPC 命令 `novel_translation_skip_chunks(jobId, chunkIds)`：
  - 仅 `needs_attention` / `paused` 状态可调用（避免与在途派发竞争）；每个块必须在
    分块计划中且在 `refused_chunks` 清单里——跳过是对「模型拒绝」的人工兜底，不是
    绕过翻译的通用手段；
  - 通过后块从被拒清单移入 manifest `skipped_chunks`，自动 `resume` 让任务继续；
- 跳过块的语义：
  - 派发规划短路为 `skipped`（不请求模型）；待办、完成判据、收尾计数、进度快照全部
    排除 skipped 块；
  - 合并与发布按双口径处理：**校验用活跃块子集**（跳过块无草稿，不参与
    `validate_merge`），**产物用全集**——跳过块的页行以「片段 ID 不变的直通草稿」
    回退原文，保证合并稿可读；
  - 结果状态为 `partial`（既有 `publish_status` 对缺草稿块已按 Partial 处理），
    前端沿用 partial 提示并显示「已跳过 N 块」。
- 完成态收敛：全部活跃块 `draft_valid && review_pass` 且整体复核、合并通过即
  `completed`；仅剩被拒块未处理时，任务停在 `needs_attention`，错误码
  `content_refused`，消息列出块 ID 并给出跳过指引。

### 4. 快照与界面

- `TranslationJobSnapshot` 新增 `refused_chunks: string[]`、`skipped_chunks: string[]`
  （升序，serde 默认值兼容旧载荷）；`JobCounts` 新增 `skipped`。
- 任务菜单在 `error_code=content_refused` 时给出中性说明与「跳过被拒块并继续」主
  按钮；文案不渲染成产品故障（用户不应为模型的内容限制感到是应用坏了）。

## 取代关系

不推翻既有决策。ADR 0036 的「只有全局性错误才 fatal」「unknown_outcome 短路与
resume 清空」不变；本 ADR 新增一个块级、持久的拒绝分类，并给「内容确实无法翻译」
的场景一条用户可控的交付出路。

## 后果

- **快速失败**：内容拒绝从「internal + 三轮重派 + 反复 resume」变成「重试 1 次 →
  标记 → 停在明确的 `content_refused`」，每次拒绝最多多花 1 次调用。
- **成果可交付**：部分内容被拒不再拖死整篇任务；跳过后其余块正常走完审查、复核与
  发布，阅读器得到 partial 结果（跳过块回退原文）。
- **状态诚实**：拒绝原因码、被拒块清单与跳过清单都进 manifest 与快照，跨重启保留，
  界面与日志不再出现误导性的「需要修正设置」。
- **边界明确**：跳过仅限被拒块，系统不提供绕过内容审查的任何路径；提示词层的降
  触发设计维持原样。
- **残余风险**：失败摘要解析依赖模型遵守上报格式，格式漂移时回退 `Internal`（表现
  退回旧行为，不更糟）；`refused_chunks` 只增不减（除非跳过或重建任务），极端情况
  下大量块被拒时任务价值趋零，用户应直接重建任务。
