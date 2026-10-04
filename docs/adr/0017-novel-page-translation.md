# ADR 0017：小说单页两轮翻译与共享设定集

- 日期：2026-10-04
- 状态：已接受

## 背景

小说需要按 Pixiv 页翻译、段落双语对照，并借鉴外部 smart-translator 的术语准备与精翻思想，使同一小说各页的人物、场景专名、别名指代统一。

## 决策

- Rust 在现有 Tauri 进程内直连用户配置的 OpenAI Chat Completions 兼容端点，不增加本地服务、SDK 或依赖。URL 支持 API 基址或完整端点；远程 HTTPS，本机 localhost/127.0.0.1/::1 可用 HTTP；禁重定向、URL 凭据/query/fragment。
- 设置保存 translation_api_url、translation_model、translation_extra 对象。高级 JSON 可自定义 reasoning_effort、max_completion_tokens、thinking 等供应商参数，但不能覆盖 model/messages/stream/n/工具/输出格式/凭据/store。支持取决于服务和模型。参考 [OpenAI Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create)。
- Key 使用 keyring service `pixiv-tool` / account `novel-translation-api-key`，不写 settings.json，不回传原值。空白保持，单独清除操作在保存时删除；凭据写入失败回滚配置；错误不回显服务正文、请求或凭据。
- 每次只翻译一页为简体中文；标题、标签、纯文本简介、前后各最多 600 字辅助消歧，不输出邻页译文。设置提示数据发送范围及通常两次请求。
- Pass 1 维护小说共享的文风、人物/地点/组织/物品/概念及确有依据的别名，既有译名和文风不可改写，冲突拒绝；先落盘。Pass 2 按锁定设定翻译并内部对照校对，按原始行号输出纯文本 JSON。校对合入第二轮，不额外调用第三轮；验证缺段、重复、空值、截断和 JSON 结构后才保存。
- `data/translations/<novel_id>/<SHA256(原文和元信息)>.json` 保存设定集和页译文；原文或元信息变化创建新版本、保留旧版。设置变化不自动使已译页失效，用户可重译当前页，仍沿用锁定设定。损坏缓存明确报错，不覆盖。
- 请求全局串行，避免设定覆盖和重复扣费；单请求 180 秒、响应最多 4MB、不自动重试。Channel 提供 queued/prepare/translate 阶段。可翻页；路由切换后旧请求继续保存，前端忽略旧回显。
- 原分页底栏增加 SVG 翻译入口，点击展开状态、翻译/重译与仅原文/仅译文/双语（默认）浮层；字号、背景、进度控件保留。译文逐段放原文下面，primary 色（纸色模式为 primary 20% + ink 80% 混色以保持深浅主题对比度）、lang=zh-CN、纯文本；图片不重复。

## 影响

新增两个 IPC，无 SQLite schema 或 Pixiv 接口变化。强制锁定防止应用悄悄覆盖设定，但模型语义遵从受模型质量影响，不能机械保证译文绝无术语偏差。按阅读次序增量准备，没有整篇预分析、跨小说/系列共享或自动批量翻译；未来身份揭示不自动重译旧页。在途不设取消，切走仍可后台完成并落盘。离线测试不读取真实 Key，不访问 Pixiv 或付费端点。
