# ADR 0050：模型请求并发自适应收敛（AIMD）与流间空闲看门狗

- 状态：已接受（演进 [ADR 0031](0031-parallel-novel-translation-and-status-bar-picker.md)
  的静态并发闸与 [ADR 0023](0023-translation-timeout-setting.md) 的单请求总超时；
  两者的设置键与区间保留，语义扩展）
- 日期：2026-11-05
- 关联 ADR：[ADR 0031](0031-parallel-novel-translation-and-status-bar-picker.md)、
  [ADR 0023](0023-translation-timeout-setting.md)、
  [ADR 0034](0034-rig-agent-kernel-and-rust-toolchain.md)（运行层内核）、
  [ADR 0036](0036-translation-runtime-guard-and-continuation.md)（运行层防护）
- 规格：[SPEC](../SPEC.md) §4.3 / §5.2

## 背景

整篇 Agent 翻译在实测中过慢（长篇一宿未完）。排查确认三个放大器：

1. **并发是静态的**：全局与单小说并发都是固定信号量（默认各 4）。用户侧网关的
   实际承受力各不相同——限流从不反馈给并发闸：宁可空转也不多开，触发限流也不会
   收敛，只能靠用户手调设置反复试。
2. **Pass 1 是串行的**（另见 [ADR 0051](0051-dispatch-scan-subagent.md)）。
3. **挂死流占满总超时**：流式请求只有单请求总超时（默认 600 秒）。网关断流、
   长尾慢响应会让一条挂死流占住并发槽位最长 10 分钟；系统休眠唤醒后的在途请求
   同样要等满超时。槽位被占期间其他请求只能在闸外排队。

## 决策

### 1. 全局并发改为 AIMD 自适应收敛

- `translation_agent_global_concurrency`（1~16）语义从「固定并发」改为**自适应
  上限**，默认值 4 → 8。控制器放在全局闸一层（API 限额本质是 key 级的）；单小说
  并发保持静态（语义 = 用户允许单本占用的路数，默认 4 → 6）。
- **信号集**：负信号 = `FailureClass::Retryable`（408/409/425/429/5xx）、
  `UnknownOutcome`（超时 / 断流）、`NotSent`（连接失败）。鉴权 / 余额等
  `Permanent`、内容拒绝、输入超限**不算**负信号（降并发无益）。成功 = 一次完整
  的模型请求（completion 到达）；压缩摘要请求同样计入。
- **控制律**：负信号 → 容量减半（下限 1），连续成功清零；连续 2 次成功且未达上限
  → 容量倍增（slow-start 式爬升）。起步容量 = 上限的一半（至少 1），两个请求周期
  内逼近上限；冷启动即满速容易直接触发限流。
- **容量应用**：tokio `Semaphore` 扩容 `add_permits`；缩容先 `try_acquire + forget`
  可用许可，差额记账（deficit），由在途许可归还时吞掉（不再返还），最终收敛到
  目标容量。
- **开关**：新设置 `translation_agent_adaptive_concurrency`（默认开）。关闭时容量
  恒等于上限（= 0031 的静态语义）。
- 容量变化写 `log::info`（`AIMD 全局调度容量 X→Y`），不进事件流；诊断接口新增
  `Scheduler::current_global_capacity()`。
- 进程级硬顶 `MODEL_GATE_PERMITS = 32`（providers.rs 共享闸）不变。

### 2. 单 Job 的 `dispatch_gate` 移除

`dispatch_gate`（Job 内子任务并发闸）与调度器的单小说信号量同为
`translation_agent_novel_concurrency`，纯重复限制。移除后子任务并发完全由
`model_permit` 的两级闸控制（单小说静态 + 全局自适应），模型请求粒度在
permit 上排队；调单本并行只动 `translation_agent_novel_concurrency` 一处。

### 3. 流间空闲看门狗

- 新设置 `translation_stream_idle_seconds`（默认 60，区间 10~300）。传输层在单请求
  总超时（0023，语义不变）之外，按请求叠加 wreq 原生 `read_timeout`：响应体两次
  读取之间超过该时长即断开报错。它同时覆盖「响应头已到但正文迟迟不来」与「流到
  一半挂死」两种情形；keepalive 字节计为活跃，健康连接不受影响。
- 看门狗错误的消息含 `timeout`，沿用现有归类落 `UnknownOutcome`（0023 的归类
  语义不变），按「请求结果未知」走既有恢复路径；同时在 AIMD 里作为负信号收缩
  容量。
- 应用**不干预系统电源策略**：休眠与否是用户选择，客户端不做 keep-awake。

## 后果

- 正向：无限流时自动逼近上限（默认最高 8 路在途），限流时自动收敛不打爆网关；
  挂死流的槽位占用从 10 分钟级降到 60 秒级。
- 代价：并发数成为随运行状态浮动的值（日志可查）；极少数对每 key 并发极敏感的
  网关可关闭自适应回退静态行为。
- 存量影响：设置键与区间不变，旧 settings.json 无需迁移；恢复中的任务按快照里
  的运行参数继续（新任务才用新默认值）。
