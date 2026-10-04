# ADR 0025：翻译生成请求改用流式，避免长请求被链路按空闲切断

- 日期：2026-10-04
- 状态：已接受

## 背景

用户报告小说第 1 页翻译报「无法连接翻译服务，请检查 URL 和网络」。复现与插桩（同一台机器、
同一 Key、同一网关 `https://opencode.ai/zen/go/v1` + `deepseek-v4.1-flash`）后拿到原始错误：

```
wreq::Error { kind: Request, source: SendRequest →
  wreq_proto::Error(Io, BrokenPipe, "stream closed because of a broken pipe"),
  connect_info: Connected { alpn: H2, remote_addr: 198.18.0.250:443 } }   // Clash TUN fake-ip 链路
```

同一份 318 行渲染请求实测：

| 方式 | 结果 |
|---|---|
| `stream:false` | 96.0s / 143.6s / 183.0s 三次里两次被切断（BrokenPipe） |
| `stream:true` | **首字节 2.4s**，持续流动，303.4s 跑满拿到完整译文 |

差别只在 `stream`：ADR 0017 把 `stream` 固定为 `false`，而非流式下网关**在整段生成期间一个
字节都不回传**（首字节即末尾，实测 140 秒级），链路中间任一环节按「空闲无数据」超时就会掐断
连接——用户浏览器里的流式客户端（含 opencode 自己的会话）从不受影响，正是这个原因。
小请求（几十 KB 级、几秒返回）不受影响，所以 pixiv 抓取与设置页「检测可用」一直是好的。

## 决策

- 生成请求（Pass 1 / Pass 2 / 检测可用）固定 `stream: true`；`stream` 仍属应用保留字段，
  高级 JSON 不可覆盖。
- 按协议解析 SSE 增量并累积模型文本，再走原有 JSON 解析与校验：
  - Chat Completions：`choices[0].delta.content`（`reasoning_content` 等推理增量不算译文）；
  - Responses：`response.output_text.delta` 的 `delta`；
  - Anthropic Messages：`content_block_delta` 且 `delta.type == "text_delta"`（`thinking_delta` 必须排除）。
  其余事件（`event:`/`id:`/空行/保活文本）忽略，`[DONE]` 终止。
- 截断与失败信号改为读流内事件：Chat `finish_reason`、Responses `response.incomplete|failed|cancelled`、
  Anthropic `delta.stop_reason`；文案沿用 ADR 0017 的既有措辞。
- 上限：译文文本 4MB（与非流式时代的响应上限等价），原始 SSE 16MB（推理增量同样计入，
  防止无界内存）。超限报「翻译响应过大」。
- `/models` 列表仍是普通 JSON 端点，单独读取，不走 SSE。
- 不自动重试的约束不变（ADR 0017 / 0020 / 0023）；超时仍是可配置的单请求总超时。

## 影响

长请求不再因为「静默 100 秒以上」被中间链路掐断；代价是解析要按协议分辨事件类型，且极少数
只支持非流式的兼容端点会因拿不到文本增量而报错（报错文案仍指向高级 JSON 与模型 ID）。
流式只改变传输形态：两轮流程、共享设定集、串行、超时、凭据与落盘布局都不变。
离线验收用本机模拟服务端下发三种协议的 SSE（含逐字节切分、跨块断行、Anthropic 推理增量
必须排除、截断事件必须报错）；真实链路验收见 `docs/SPEC.md` §10。
