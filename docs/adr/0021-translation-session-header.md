# ADR 0021：翻译请求对 opencode 网关携带会话标识头

- 日期：2026-10-04
- 状态：已接受

## 背景

实测 opencode zen 的 Go 档网关（`https://opencode.ai/zen/go/v1`）对三个生成端点
（`/chat/completions`、`/responses`、`/messages`）一律要求 `x-opencode-session`
请求头，缺失即返回 `400 {"type":"error","error":{"type":"MissingSessionID",...}}`；
而 `GET /v1/models` 不要求。用户侧表现为「获取模型正常、检测可用报 400」，且界面
按 ADR 0017 不回显服务正文，只给通用 4xx 文案，无法自诊断。应用此前也无法发送该
头——高级 JSON 的 `headers` 是保留字段（防请求头/凭据注入）。

## 决策

- 翻译客户端在**生成请求与 `/models` 请求**上附加 `x-opencode-session`，但**仅当
  URL 主机名包含 `opencode`**（`translation::needs_session_header`）。其它供应商
  （OpenAI、Anthropic、本机模型等）请求头不变。
- 取值是稳定 UUID 形态：`SHA-256(seed)` 前 16 字节转 `8-4-4-4-12` 十六进制，跨进程、
  跨重启恒定（网关的 prompt 缓存与路由依赖稳定性）。
  - 小说翻译：seed = `pixiv-tool-novel-<novel_id>`，**以小说为会话单位**——同一小说的
    各页共用会话，与共享设定集的语义一致。
  - 探测（获取模型 / 检测可用）：固定 seed `pixiv-tool-probe`。
- 值不含凭据、模型 ID 或用户内容，不写日志；高级 JSON 的 `headers` 保持保留字段，
  用户仍不能注入任意请求头。
- 该头是 vendor 专属契约：域名判定与取值算法集中在 `translation.rs`，改动需同步本 ADR。

## 影响

无新增 IPC、设置键或 schema 变化；前端无改动。opencode 系网关（含 Go 档）可直接用于
小说翻译的三种协议；其它供应商的请求头逐字不变，无回归面。会话按小说隔离：同一小说
跨页、重译沿用同一会话，不同小说互不影响。若 opencode 更换域名或改用真实会话语义，
只需调整 `needs_session_header` 与 seed 规则。离线测试用本机模拟 HTTP 断言「非 opencode
主机不带该头」，并单测覆盖取值稳定形态与域名判定。
