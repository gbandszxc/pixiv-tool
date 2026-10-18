# ADR 0032：思考参数按协议归一化，不再透传 Chat 习惯字段

- 日期：2026-10-17
- 状态：已接受

## 背景

ADR 0030 让浮窗展示思考过程，前提是服务端真的在思考。默认配置来自 Chat
Completions：高级 JSON 写 `{"reasoning_effort":"high"}`。切到 Responses 或
Anthropic 后，这段配置被原样塞进请求体，而这两个协议都不认这个字段：

- **Responses**：思考开关是嵌套的 `reasoning.effort`。顶层 `reasoning_effort`
  被 OpenAI 以 `unsupported_parameter` 拒绝；兼容网关则多半静默忽略。`reasoning`
  对象里不给 `summary` 时，多数模型只回 `reasoning_text`（或什么都不回），
  浮窗没有思考摘要可显示。
- **Anthropic Messages**：思考开关是顶层 `thinking`。`reasoning_effort` 不在
  schema 内，实测（opencode 网关 + `deepseek-v4.1-flash`）被静默丢弃，请求照常
  200，但首个 `content_block` 直接是 `text`，表现为「点翻译立刻开始出译文、没有
  思考」。

同类的跨协议错配还有 `max_completion_tokens`（Responses 用 `max_output_tokens`，
Anthropic 用 `max_tokens`）与 `stream_options`（只有 Chat Completions 有）。

## 决策

在请求装配前加一层按协议归一化（`normalize_extra`），只处理「Chat 写法进错协议」，
用户显式给出的目标协议字段永远优先：

- **Chat Completions**：不改写，高级 JSON 原样透传。
- **Responses**：剔除 `stream_options`；`max_completion_tokens` / `max_tokens`
  改名为 `max_output_tokens`；顶层 `reasoning_effort` 移入 `reasoning.effort`
  （已有 `reasoning` 对象时只补缺）；`effort` 非 `none` 时补
  `reasoning.summary: "auto"`，让服务下发思考摘要。`effort` 为 `none` 时只写
  effort、不补 summary。
- **Anthropic**：剔除 `stream_options`；`max_completion_tokens` /
  `max_output_tokens` 改名为 `max_tokens`；OpenAI 形状的 `reasoning` 对象整块
  剔除，只取其 `effort`。`effort` 非 `none`/`disabled`/`minimal` 且用户没显式写
  `thinking` 时，注入 manual thinking：
  `{"type":"enabled","budget_tokens":N}`，N 按等级取 low 2048 / medium 4096 /
  其余 8192，并按 `max_tokens - 1024` 夹取（该协议要求 budget 严格小于
  `max_tokens`）；`max_tokens ≤ 1024` 放不下最小预算时不开思考。
- **Anthropic 默认输出上限**：开思考时从 8192 抬到 16384。思考 token 与正文
  共享 `max_tokens`，沿用 8192 会让长页在思考阶段耗尽预算、正文被截断。用户
  显式给了 `max_tokens` 就尊重原值。长页仍截断时在高级 JSON 调大 `max_tokens`
  （如 32768，需模型支持）；三协议的「输出达到上限」截断单独报可执行文案
  （附实际上限值），不再与「被服务拒绝」混在一条笼统提示里。
- **Anthropic 采样参数**：开启思考（自动注入或用户显式写了 `thinking`）后，
  `temperature` 固定为 1、`top_p` 低于 0.95 时直接剔除。这是该协议对 thinking
  的硬约束（「`temperature` may only be set to 1 when thinking is enabled」），
  否则手上带 `temperature: 0.7` 的配置一开思考就 400。未开思考时不动这两个字段。

选 manual（`type: enabled` + `budget_tokens`）而不是 adaptive：后者只在
Claude 4.6+ 可用，4.5 及更早会直接报错；manual 在 Claude 4 全系可用，网关侧也
普遍实现。

## 影响

- 用户在高级 JSON 里维持一份 Chat 写法的配置，切协议后仍能生效，不必逐协议改写。
- Anthropic 切到思考后输出上限默认翻倍，单页 token 成本相应上升；想压低成本可在
  高级 JSON 写 `max_tokens`，或在 `thinking` 里给自定义 `budget_tokens`（显式
  `thinking` 会完全覆盖自动注入）。
- 显式写 `thinking` 的用户行为完全不变；`reasoning_effort: "none"` 可关闭自动开启。
- 无新增 IPC 或设置键。单测：
  `builds_request_body_per_api_format`（三协议装配 + 思考开/关 + 上限夹取）、
  `normalizes_extra_fields_per_api_format`（跨协议字段改名与剔除）、
  `http_roundtrip_for_responses_and_anthropic_formats`（切协议后上限沿用）。
- 离线验证：`bash ./dev.sh test`。实测对照（本机 opencode 网关）：Anthropic 带
  `reasoning_effort` 首帧即 `text_delta`，带 `thinking` 首帧为 `thinking_delta`；
  Responses 顶层 `reasoning_effort` 在 `response.created` 里回显为
  `reasoning.effort: null`，嵌套写法才生效。
