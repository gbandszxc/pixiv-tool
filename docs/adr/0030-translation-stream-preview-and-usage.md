# ADR 0030：翻译浮窗流式展示思考过程与 token 用量

- 日期：2026-10-17
- 状态：已接受
- 关联：ADR 0017、0025、0029

## 背景

ADR 0029 把翻译进度做成了浮窗内的实时进度区，但两处仍然黑盒：

1. **思考过程被丢弃**。推理模型（DeepSeek R1 系、Claude thinking、OpenAI reasoning 系）在
   正文 JSON 生成前有数分钟量级的思考阶段，此期间 `received` 恒 0、摘要恒空，浮窗只有
   不定进度条在动，用户无从判断是卡住还是正常生成；ADR 0025 明确「推理增量不算译文」，
   但解析层直接丢弃，没想过外供。
2. **Pass 1（整理设定集）恒无摘要**。ADR 0029 的摘要扫描器只认 `text` 键，而 Pass 1 的
   设定集 JSON（`style` / `terms`）没有 `text` 键，「整理设定集…」阶段只有字节数在涨。
3. **用量不可见**。翻译是真实计费调用，跑完一页用户不知道花了多少 token；三协议的流式
   usage 事件本来就逐帧到达（Chat Completions 需要 `stream_options.include_usage` 才下发）。

## 决策

**流解析：增量二分类 + 用量捕获**（`translation.rs`）：

- `stream_event_text` 升级为 `stream_event_delta`，返回 `Text | Reasoning` 二分类：
  - Chat Completions：`delta.content` 为正文；`delta.reasoning_content`（DeepSeek 系）与
    `delta.reasoning`（OpenRouter 系）为思考；
  - Responses：`response.output_text.delta` 为正文；`response.reasoning_summary_text.delta`
    与 `response.reasoning_text.delta` 为思考；
  - Anthropic：`text_delta`（`delta.text`）为正文；`thinking_delta`（`delta.thinking`）为思考。
- 思考增量不进正文文本（ADR 0025 的译文语义不变），只喂给进度上报的思考尾窗；
  原始 SSE 16MB 上限继续同时约束两类增量。
- token 用量按协议落点解析并做流内字段级 max 合并（Anthropic 输入随 `message_start`、
  输出随 `message_delta` 分帧到达，输出计数逐帧递增；Anthropic 的 prompt 含缓存读/写
  token，即账单口径的总输入）：Chat `usage.prompt_tokens/completion_tokens`（末帧）、
  Responses `response.usage.input_tokens/output_tokens`（随 `response.completed`）、
  Anthropic `/message/usage` 与 `/usage`。`total` 统一按 prompt + completion 收尾计算，
  不取服务的 total 字段。单条流拿不到 usage 即缺省，不虚构。
- Chat Completions 请求体默认注入 `stream_options: {"include_usage": true}`（不加这个
  OpenAI 不下发 usage）；`stream_options` 移出高级 JSON 保留字段——个别不兼容网关可在
  高级 JSON 显式覆盖关闭（如 `"stream_options": false`），Responses / Anthropic 不注入。

**进度事件：progress 增加 reasoning 字段**（载荷见 §3.2）：

- 正文摘要（`summary`）从「只认 `text` 键」放宽为「任意完整的字符串值」：Pass 2 的译文
  JSON 里字符串值只有 `text`（行为不变），Pass 1 的设定集 `style` / 词条字段值因此随
  流式生成滚动可见，两轮阶段浮窗都有内容可显。
- 思考尾窗（`reasoning`）：纯文本流无需解析，滚动保留最近约 240 字符、取尾部 60 字符
  （与摘要同限，char 边界）输出；无思考输出为空串。
- 思考增量同时驱动节流（占节流窗口、不占 `received`）：思考期间零正文也持续放行进度
  事件，浮窗始终有内容在动——这正是本决策要解决的「在没在跑」问题。

**返回体：`PageTranslation.usage`**：

- 本次两轮请求的用量合计（跨轮相加；拿到几轮算几轮，全拿不到则整体缺省）；缓存命中与
  「原文已是目标语言」未调用模型时缺省。`translation_test` 把探测用量写入日志（info 级）。

**界面：浮窗进度区单行流式预览 + 完成态用量回显**（规则同步 DESIGN.md 与 `.impeccable/design.json`）：

- 思考与摘要**共用同一行预览位置**：无正文摘要而有思考尾窗时显示思考行——`<code>` 等宽、
  12px、`--ink-subtle`（比摘要行的 `--ink-muted` 弱一档），600 字重「思考」前缀 + 尾窗
  文本；正文摘要一旦到达即取代思考的显示位置，思考的最后一句不驻留（同日修订：初版为
  思考/摘要双行并存，思考结束后末句冻结在原地、正文挂在下一行，不符合「预览行始终反映
  最新进展」的意图）。
- 思考态做静态 ellipsis、**不做 marquee**：思考逐帧刷新，循环滚动动画会被不断重置反而
  抖动；摘要在低频刷新下保留既有 marquee。两态整行 `aria-hidden`，全文经 title 悬停查看，
  不进播报区（沿用 ADR 0029 的 aria 纪律）。
- 完成态：状态行在「本页译文已保存」后追加「· 用量 N tokens」（千分位，`lastDone` 匹配
  当前小说与页码且有 usage 才显示；缓存恢复不冒充）。状态行 `role=status aria-live=polite`
  播报一次完成与用量，可接受。

## 修订（2026-10-17）：空 `content` 曾整段吞掉思考增量

DeepSeek 系网关（实测 `opencode-go/deepseek-v4.1-flash`）的思考帧形如
`{"delta":{"role":"assistant","content":"","reasoning_content":"我们需要"}}`——空字符串
也是 `content` 键。原解析先取 `content` 再回落 `reasoning_content`，`""` 也命中，于是
整段思考被当成「空正文」丢弃：浮窗一直空白，直到正文开始才突然出现，表现为「有时很快、
有时快翻译完才出来」。

修正为两路并行取值：**空字符串不算增量**（`non_empty_str`），同一帧同时带正文与思考时
两路都收（`StreamDeltaView`）。单测 `empty_content_does_not_shadow_reasoning` 锁死该形态。

## 影响

- **IPC 契约变化**：progress 事件增加 `reasoning` 字段（旧事件流无此字段，前后端同批升级，
  无跨版本兼容问题）；`novel_translate_page` 返回体增加可选 `usage`。命令签名与计数不变。
- **外部请求契约变化**：Chat Completions 生成请求默认多带 `stream_options`；不兼容网关
  经高级 JSON 覆盖关闭后行为与升级前一致（只损失用量与思考显示）。
- 拿不到思考增量的端点（ADR 0025 退化场景）只损失思考行，摘要与进度照常；拿不到 usage
  的服务完成态不显示用量，不虚构数值。
- 离线验收：`translation.rs` 新增三协议思考增量捕获与 usage 解析合并单测（含 Anthropic
  分帧合并与缓存 token 口径）、`stream_options` 注入与高级 JSON 覆盖断言；`translation_summary.rs`
  摘要扫描器「任意字符串值」语义与思考尾窗滚动裁剪单测；浏览器验收（`/tests/translation.html`）
  分步驱动事件流，覆盖思考期显示、正文到达即取代（末句不驻留）、完成态用量回显与进度区退场。

## 修订（2026-10-25）：整篇 Job 的预览归属

ADR 0035 的整篇 Job 不再复用本文的单页预览实现：运行层预览的归属（主会话 / 翻译 / 审查）、
240 字符尾窗、并发子任务的稳定展示与 Job 阶段隔离见
[ADR 0037](0037-translation-preview-ownership-and-stability.md)。本文描述的三协议增量分类、
usage 解析与 v1 单页行为继续有效。
