# ADR 0034：Rig 通用 Agent 内核与 Rust 1.99.1 工具链

- 状态：已接受
- 日期：2026-10-25
- 关联 ADR：[ADR 0008](0008-tauri-rewrite.md)、[ADR 0019](0019-security-patched-toolchain.md)、[ADR 0020](0020-translation-api-formats.md)、[ADR 0021](0021-translation-session-header.md)、[ADR 0023](0023-translation-timeout-setting.md)、[ADR 0025](0025-translation-streaming.md)、[ADR 0027](0027-storage-maintenance-diagnostics.md)、[ADR 0029](0029-translation-progress-events.md)、[ADR 0030](0030-translation-stream-preview-and-usage.md)、[ADR 0032](0032-translation-thinking-params-per-format.md)、[ADR 0033](0033-translation-dynamic-output-cap-and-default-thinking.md)、[ADR 0035](0035-whole-novel-agent-translation.md)
- 规格：[SPEC](../SPEC.md) §2 / §4.3 / §8；[PACKAGING](../PACKAGING.md) §2 / §3
- 实施计划：[Rig 通用 Agent 内核与整篇小说翻译实施计划](../plans/agent-kernel-and-smart-translation.md)

## 背景

整篇小说翻译需要多轮模型调用、原生工具调用、子 Agent、预算控制、上下文压缩、有限恢复与检查点。自研完整的工具循环与三套模型协议编解码，会把长期维护成本压在翻译业务之外，也容易在流式事件、工具参数增量、推理字段与结束原因上产生协议偏差。

项目已经落地三协议（Chat Completions / Responses / Anthropic Messages）、URL 与端点推导、凭据存储、系统代理、单请求超时、禁重定向、opencode 会话头、错误脱敏等外部契约（ADR 0020 / 0021 / 0023 / 0027）。接入 Agent 框架时不能把这些行为换成框架默认值。

同时，Rig 0.45 的依赖下限高于项目原先声明的 Rust 1.88；继续贴着旧下限会迫使依赖选择与安全补丁分叉。需要一个明确、可复现的工具链基线，而不是跟随 `stable` 漂移。

## 决策

### 1. 采用 Rig 0.45.0，精确锁版本、最小 feature

- `rig-core` / `rig-agent` / `rig-memory` 均以 `=0.45.0` 精确锁定，且 `default-features = false`。
- 不启用 Rig 的 server / MCP / vector / embeddings / image / audio / pdf 能力，也不启用其自带的 reqwest / rustls 传输路径。
- 模型 HTTP 仍统一走项目既有的 wreq 客户端；Rig 只负责协议编解码与工具循环。
- `rig-memory` 仅作为依赖图中的锁定项存在，不作为会话、摘要或产物的持久化真相源。

### 2. 内置 provider wire + 自定义 wreq `HttpClientExt`

- 使用 Rig 自带的 Chat Completions、Responses、Anthropic Messages provider wire，不自研三套协议编解码。
- 通过 `WreqTransport` 实现 `HttpClientExt`，把 Rig 生成的请求交给 wreq；每个请求的 URI 由宿主按 `ProviderConfig::endpoint` 整体重写，只保留 scheme / host / port / path，丢弃 Rig 按自身习惯追加的 `/v1/...` 后缀。
- 保留既有外部行为：系统代理、单请求超时、禁重定向、opencode `x-opencode-session` 头、错误脱敏与有界错误正文。三协议端点推导、凭据头与请求体语义与旧行为逐字对齐，包括没有 `/v1` 的 Anthropic 网关。
- 模型请求并发由运行层调度器按全局 / 单小说 / 活跃小说三级控制；传输层同时支持注入全局请求闸门（非流式请求覆盖到响应体读完，流式请求由返回的 stream 持有到流结束或调用方 drop，取消时许可随 future drop 释放）。

### 3. 工具调用循环由 `rig-agent` 提供，运行层自建

- `rig-agent` 负责模型轮次、工具调用解析、工具结果回填与正常结束判定。
- 运行层自行实现调度、任务与子任务、预算、上下文压缩、有限恢复、检查点、事件与存储；这些能力不依赖框架默认的 memory / session 目录。
- 主 Agent 与子 Agent 都通过同一套运行层进入模型请求；子 Agent 的派生由业务工具显式启动，运行层不内置递归派生能力。
- 事件、日志与持久化绑定 Job 拥有者，不能落到 config、用户主目录或另一个 Agent 数据根。

### 4. 上下文压缩在完整工具批次之间执行

- 压缩通过 `CompletionCallAction::Patch(RequestPatch::history)` 修改后续请求的历史投影，不覆盖原始记录。
- 压缩只在完整请求 / 工具批次之间进行，保留 `assistant tool_call + 对应 tool result` 的配对边界。
- 摘要请求使用固定结构化提示，并经过同一全局模型请求闸门、计入任务总预算与总用量。
- 摘要失败时保留原记录并停止运行，由上层进入暂停 / 需要处理；不做无限「压缩—重试」。
- 全量原文、译文、术语与审查记录继续从 Job 产物读取，不依赖有损摘要还原。

### 5. Rust 1.99.1 作为全平台构建基线

- `rust-toolchain.toml` 固定 `channel = "1.99.1"`；`src-tauri/Cargo.toml` 的 `rust-version` 同步为 `1.99.1`。
- Windows 统一使用 MSVC：`dev.ps1` 显式选择 `1.99.1-x86_64-pc-windows-msvc`；Bash 入口固定 `1.99.1`，由 rustup 选择宿主 triple。
- 发版 CI 固定 `dtolnay/rust-toolchain@1.99.1`，不使用 `stable` 标签。
- 本决策取代 [ADR 0019](0019-security-patched-toolchain.md) 中「最低 Rust 1.88」的工具链承诺；ADR 0019 关于 Vite / Rust 传递依赖安全补丁的理由仍然有效。
- 不采用 nightly / beta，也不在实施日之后静默漂移版本；升级需重新验证并同批更新工具链文件、脚本、CI 与文档。

### 6. 明确不包含

- MCP 服务或客户端、自由聊天页面、外部插件、任意 shell 执行。
- 为 Agent 引入额外的向量库、嵌入模型、图片 / 音频 / PDF 处理能力。
- 本地 HTTP 服务、固定端口或额外后端进程。

## 备选方案

- **自研三套协议 Wire 与工具循环**：可控但重复实现流式解析、工具参数增量、推理字段与结束原因，长期维护成本高；且会与既有三协议契约产生第二套真相源。未采用。
- **使用 Rig 自带的 reqwest / rustls 传输**：会引入新的 TLS 栈与代理语义，丢失 wreq 的浏览器指纹、系统代理与既有请求头行为。未采用。
- **直接使用 Rig 默认端点拼接**：Rig 会追加 `/v1` 等后缀，无法覆盖用户粘贴的完整端点与无 `/v1` 的 Anthropic 网关。未采用。
- **把 `rig-memory` 当作持久化真相源**：其目录与生命周期不受 Job 拥有者控制，清理、恢复与防复活语义无法保证。未采用。
- **停留在 Rust 1.88 或跟随 `stable`**：会限制依赖选择并造成版本漂移；固定 1.99.1 更可复现。未采用。
- **引入 MCP / 通用聊天 / 第三方编排框架**：超出当前产品边界，且会扩大权限与数据面。未采用。

## 后果

### 正面

- 三协议与工具循环复用成熟实现，翻译业务只需提供工具、资源、调度与存储。
- 外部契约（端点、代理、超时、凭据、opencode 会话头、错误脱敏）保持单一实现。
- 运行层拥有预算、压缩、恢复、检查点与事件，整篇翻译可以暂停、恢复和清理。
- 工具链版本在本地脚本、Cargo manifest 与 CI 三处一致，构建可复现。

### 风险与代价

- **Rig 0.45 很新**：后续升级预期包含 breaking change；必须继续精确锁版本，升级时重新验证三协议、工具调用、流式事件与取消。
- **`serde_json/preserve_order` 全局生效**：`rig-core` 无条件开启该 feature，JSON Map 迭代从字典序变为插入序。依赖 Map 迭代序的位置必须显式排序；`src-tauri/src/pixiv/browse_api.rs` 的作者作品对象形响应已按 key 显式排序修复。新增代码不得再依赖 `serde_json::Map` 的默认迭代顺序。
- **端点重写是载荷路径**：Rig 默认 Anthropic 路径为 `/v1/messages`，由传输层整体替换为宿主端点；任何端点推导或传输层改动都必须覆盖无 `/v1` 网关、完整端点与基址三种输入。
- **编译与体积**：Rig 增加编译时间与二进制体积；不启用无关 feature 以控制增量。
- **工具链门槛提高**：本地低于 1.99.1 的环境需要安装固定工具链；CI 与脚本不会回退到 `stable`。
- **协议能力边界**：Rig wire 不覆盖的供应商私有字段仍通过既有高级 JSON 归一化与 `additional_params` 处理，不能借 Agent 接入绕过保留字段校验。

## 实现位置

- 依赖与 feature：[`src-tauri/Cargo.toml`](../../src-tauri/Cargo.toml)
- 传输层与端点重写：[`agent/model/transport.rs`](../../src-tauri/src/agent/model/transport.rs)
- 三协议 provider 与请求参数：[`agent/model/providers.rs`](../../src-tauri/src/agent/model/providers.rs)
- 工具循环、压缩与预算 hook：[`agent/runtime.rs`](../../src-tauri/src/agent/runtime.rs)
- 调度、预算、压缩、恢复、事件、存储：[`agent/`](../../src-tauri/src/agent/)
- 工具链：[`rust-toolchain.toml`](../../rust-toolchain.toml)、[`dev.ps1`](../../dev.ps1)、[`dev.sh`](../../dev.sh)、[`.github/workflows/release.yml`](../../.github/workflows/release.yml)
