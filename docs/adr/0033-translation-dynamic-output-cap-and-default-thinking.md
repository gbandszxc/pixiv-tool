# ADR 0033：翻译输出上限按输入分档、默认开启思考

- 日期：2026-10-18
- 状态：已接受（演进 ADR 0032）

## 背景

ADR 0032 之后思考参数能正确落到各协议，但两个新问题暴露：

1. **输出上限不可靠**。Chat Completions 之前不发送 `max_completion_tokens`，
   由服务端默认值兜底，长页开思考后频繁以 `finish_reason: length` 截断
   （截图报「模型未完整输出」即此）；Anthropic 的固定默认（不开思考 8192 /
   开思考 16384）对长页同样不够。思考 token 与译文 JSON 共享输出预算，
   现代推理模型「长思考」进一步放大缺口。
2. **思考默认关闭**。思考只在用户高级 JSON 显式配置时开启，空配置的用户
   （大多数场景）拿不到思考带来的译文质量提升。

同时收束高级 JSON 的语义表述：用户 JSON 与后台装配参数是**逐字段合并**
（无则新增、同名覆盖后台默认值），`model` / `messages` / `stream` / `store` /
`system(instructions)` 等核心契约字段由应用写入、不允许覆盖——实现自始如此，
文档与提示按此口径统一。

## 决策

- **动态输出上限（三协议一致）**：按本次请求输入字符数（提示词 + 用户 JSON，
  含原文行）分档——
  | 输入字符数 | 输出上限 |
  |---|---|
  | ≤ 6,000 | 16,384 |
  | 6,001 ~ 24,000 | 32,768 |
  | > 24,000 | 65,536 |
  档位向主流模型支持的输出上限取足余量（实测 opencode 网关三协议对 65,536
  均接受；上限只是天花板，按实际输出计费）。字段名随协议：Chat
  `max_completion_tokens`、Responses `max_output_tokens`、Anthropic
  `max_tokens`；用户高级 JSON 显式给出的上限（含跨协议写法，装配时统一改名）
  永远优先。模型不支持该上限返回 400 时，按既有「检查高级 JSON」文案处理。
- **默认开启思考（三协议一致）**：高级 JSON 没有给出任何思考字段时按
  `medium` 注入——Chat `reasoning_effort: "medium"`、Responses
  `reasoning: {effort: "medium", summary: "auto"}`、Anthropic
  `thinking: {type: enabled, budget_tokens: 4096}`（受 `max_tokens` 夹取）。
  用户显式给出的任一形状思考字段优先；显式 `none` / `disabled` / `minimal`
  即关闭。Anthropic 的 budget 档沿用 ADR 0032（low 2k / medium 4k / 其余 8k）。
- 上一版 ADR 0032 的「Anthropic 固定默认 8192 / 16384」由分档取代，
  `ANTHROPIC_DEFAULT_MAX_TOKENS` / `ANTHROPIC_THINKING_MAX_TOKENS` 常量删除。

## 影响

- 空配置用户三协议都能拿到思考与足够的输出上限；长页截断从「频繁失败」
  变为「极端页才可能触顶」，触顶时报错带实际上限值（ADR 0032 修订）。
- 成本结构变化：思考 token 按输出计费，默认 medium 会增加每页 token 用量；
  浮窗完成态的用量回显（ADR 0030）可直观看到。想省可在高级 JSON 写
  `reasoning_effort`（Chat）/ `reasoning.effort`（Responses）/ thinking
  budget（Anthropic），或 `none` 关闭。
- 高级 JSON 的「逐字段合并」语义写入设置页帮助与 SPEC，实现不变。
- 单测：`scales_output_cap_with_input_size`（三协议分档 + Chat 默认思考 +
  用户上限优先）、`builds_request_body_per_api_format` 与
  `normalizes_extra_fields_per_api_format` 同批更新。
