# ADR 0020：小说翻译支持三种模型接口协议

- 日期：2026-10-04
- 状态：已接受

## 背景

ADR 0017 把小说翻译锁定在 OpenAI Chat Completions 兼容端点：URL 固定推导
`/chat/completions`，凭据固定 Bearer，响应固定解析 `choices[].message.content`。
但用户手上的服务并不总是这套：OpenAI 新版 Responses 接口（`/responses`，请求用
`instructions` + `input`，响应用 `output[].content[].output_text`）与 Anthropic
Messages（`/messages`，`x-api-key` + `anthropic-version` 头，`system` 顶层字段，
`max_tokens` 必填，响应 `content[].text` + `stop_reason`）都无法按现有链路调用。

## 决策

- 新增设置键 `translation_api_format`，取值 `chat_completions`（默认）/ `responses` /
  `anthropic`；白名单外的值（含手改 settings.json）在加载期回落默认，保存时拒绝。
  设置页「小说翻译 · 服务连接」以 md-outlined-select 选择，切换协议不自动改写 URL。
- URL 仍接受 API 基址或当前协议的完整端点：按协议推导后缀（`/chat/completions`、
  `/responses`、`/messages`）；粘贴的是其它协议的完整端点时，先剥掉已知后缀再按当前
  协议拼接。`/models` 从同一基址推导（三种协议都有该端点）。HTTPS / 本机 HTTP、
  禁重定向、禁 URL 凭据/query/fragment 不变。
- 凭据落点不变（keyring service `pixiv-tool` / account `novel-translation-api-key`）：
  OpenAI 系走 `Authorization: Bearer`，Anthropic 走 `x-api-key` + 固定
  `anthropic-version: 2023-06-01`。Key 依旧不入 settings.json、不回显、不写日志。
- 请求装配：chat = `messages`（system + user）+ `store: false`；responses =
  `instructions` + `input`（用户文本）+ `store: false`；anthropic = `system` +
  `messages` + `max_tokens`（默认 8192，可用高级 JSON 的 `max_tokens` 覆盖），
  不发送该协议不认识的 `store`。保留字段按协议追加（responses 禁 `input` /
  `instructions`，anthropic 禁 `system`），其余高级 JSON 扩展原样传递。
- 响应解析：chat = `choices[0].finish_reason` + `message.content`；responses =
  `status`（completed / incomplete / failed）+ `output[].content[].output_text`；
  anthropic = `stop_reason`（end_turn / stop_sequence 以外视为未完成）+ `content[].text`。
  三者都先取出模型文本，再走既有的去代码围栏 + JSON 解析与两轮校验。
- 两轮流程、共享设定集、本机持久化、全局串行、180s 超时、4MB 响应上限与不自动重试
  全部不变。

## 影响

无新增 IPC、无 SQLite 或本机译文目录结构变化；设置白名单 19 → 20 键，前端探测草稿
（`translation_models` / `translation_test`）多带一个 `format` 字段。Anthropic 的
输出上限默认 8192 token，超长页可能截断，报错文案提示调整高级 JSON（该协议没有
Chat Completions 那种「不设上限」的默认）。三种协议的行为随服务实现差异，协议切换
后需用户自行确认 URL 与模型 ID 匹配；离线测试只用本机模拟 HTTP 服务验证路径、凭据头、
请求体与响应解析，不访问真实付费端点。
