# ADR 0051：Pass 1 扫描改并行子 Agent 派发（dispatch_scan）

- 状态：已接受（演进 [ADR 0035](0035-whole-novel-agent-translation.md) 的 Pass 1
  执行主体：从协调者逐块直扫改为按块派发扫描子 Agent；冻结与冲突合并机制不变）
- 日期：2026-11-05
- 关联 ADR：[ADR 0034](0034-rig-agent-kernel-and-rust-toolchain.md)（Agent 内核）、
  [ADR 0035](0035-whole-novel-agent-translation.md)（整篇翻译 Job）、
  [ADR 0038](0038-translation-review-convergence.md)（审查分级与返工）、
  [ADR 0047](0047-language-policy-convergence.md)（语言策略与校验口径）、
  [ADR 0050](0050-aimd-concurrency-and-stream-idle-watchdog.md)（自适应并发）
- 规格：[SPEC](../SPEC.md) §4.3 / §7.1

## 背景

Pass 1（术语与风格扫描）原本由协调者逐块直读：提示词限制每轮最多 2 个
`read_chunk`，一本 27 块的小说意味着 14+ 轮串行的协调者推理，且全部扫描内容
流经协调者主上下文（压缩压力大）。扫描产物（块级术语、风格基调）本质是
**逐块独立产出、冻结时聚合**的，与翻译 / 审查一样天然按块并行——串行只是
历史实现的产物。

## 决策

### 1. 新增扫描子 Agent 角色与 `dispatch_scan`

- `Role::Scanner`（`scanner`，默认阶段 `prepare_constraints`），角色契约
  `resources/agent/agents/scanner.md`，资源 ID `role:scanner`。
- 工具集：`load_resource`、`read_chunk`、`submit_scan`（`chunk_id` 锁定为本次
  派发的块，仿 `submit_translation` 的 out_of_scope 校验）。
- 单块视角 + 保守记录：扫描者只对自己的块负责，不确定项如实标注（`confidence`），
  跨块冲突由协调者在冻结时合并——扫描是聚合的输入，不是裁定。
- 子 Agent 与翻译 / 审查一致地降低思考档位（`apply_review_thinking` 同款）：
  扫描是结构化抽取，长思考挤占输出封顶。
- 完成判定 = scan 产物存在且带当前 `prepare_constraints` 版本戳（与
  `freeze_constraints` 的 missing 校验、`pending_scan` 口径同源）。

### 2. 协调者 Pass 1 改派发

- `coordinator.md` Pass 1 主路径：`job_status().pending_scan` →
  `dispatch_scan(chunk_ids)`（批次规则同 Pass 2）→ `freeze_constraints()`。
  协调者不再逐块直读正文，扫描内容不再流经主上下文。
- `submit_scan` / `read_chunk` 直用**保留**，仅限冻结冲突的定点修正流
  （`constraint_policy_conflict` 时读对应块、改扫描、重新冻结，ADR 0047）；
  约束已冻结后再派发 `dispatch_scan` 会被拒绝（`constraints_already_frozen`），
  重扫必须走 `reset_phase(prepare_constraints)` 递增版本。
- `plan_dispatch` 的 Scan 分支：scan 产物存在且 stamp 有效 → 复用短路；
  否则派发。无返工额度（返工是翻译专属概念），跳过 / 拒绝短路与其他派发一致。
- 检查点、事件流、`subtask_scan_done` 日志与预览归属（scanner 角色，ADR 0037
  的归属模型扩展一档）与其他子任务同构。
- SKILL.md / coordinator.md 的角色表、工具签名表、Pass 1 章节同步更新。

### 3. 资源版本与存量任务

`scanner.md` 加入 `agents/` 组 → `ResourceVersions::agents` 哈希变化 →
进行中的旧任务恢复时资源版本比对失败，按既有语义保留原状态（用户 force 重启）。
这是资源版本校验的既定行为，不是迁移问题。

## 后果

- 正向：Pass 1 从 O(块数) 轮串行协调者推理变为与 Pass 2 同构的按块并行
  （27 块在默认并发下约 5~7 批子 Agent，每批两轮请求）；协调者主上下文不再
  装载全文扫描内容，压缩压力显著下降。
- 代价：每块多一层子 Agent 会话（派发请求 + read_chunk + submit_scan ≈ 3 次
  请求，原直扫约 2 轮但串行）；调用预算公式 `64 + 32×chunks` 余量充足，
  不需要调整。
- 旧缓存：v1 单页缓存与本机制无关；v2 任务的 scan 产物带版本戳，重启后
  `pending_scan` 口径不变，续跑语义不破坏。
