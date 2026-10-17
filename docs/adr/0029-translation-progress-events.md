# ADR 0029：小说翻译实时进度（progress Channel JSON 事件流 + 后台续跑）

- 日期：2026-10-17
- 状态：已接受
- 关联：ADR 0017、0025

## 背景

`novel_translate_page` 自 ADR 0017 起就带 `progress: Channel<String>` 参数，但实现是空壳：命令只发过裸字符串阶段标签、前端从未监听，界面在整页两轮翻译（Pass 1 整理设定集 + Pass 2 翻译正文，单请求可跑数分钟）期间只回显统一的「翻译中…」。具体盲区：

1. **阶段不可见**：排队、整理设定集、翻译正文三段的耗时与等待原因无法区分；长请求（ADR 0023 默认 600s 超时）下用户无从判断是卡住还是正常生成。
2. **退出页面即失联**：翻译由调用页的组件生命周期持有，离开小说页（翻页、返回、切作品）后没有任何入口知道任务仍在跑。
3. **失败被吞**：翻译途中退出页面，Promise 的 rejection 随组件卸载丢失；回到页面只看到旧的「翻译中」态或空白，得不到失败原因，也无法直接重试。
4. **串行锁不可见**：后端 `translation_lock` 全局串行（ADR 0017），第二页翻译要排队等待，界面没有任何排队提示。

ADR 0025 已把生成请求改为流式，译文增量本来就逐块到达后端——把其中一部分节流后外供界面，成本极低。

## 决策

**后端：progress Channel 升级为节流 JSON 事件流**（`translation.rs` + `translation_summary.rs`，命令签名不变）：

| 事件 | 载荷 | 时机 |
|---|---|---|
| 阶段 | `{"type":"phase","phase":"queued"\|"prepare"\|"translate"}` | 命令入口、Pass 1 开始、Pass 2 开始；立即发，不节流 |
| 进度 | `{"type":"progress","phase":"prepare"\|"translate","received":N,"summary":"…"}` | 节流放行时发 |

- `received`：只统计译文增量（ADR 0025 的 `stream_event_text` 产出，推理/思考增量不计），单条流内累计字节数。
- `summary`：`translation_summary.rs` 的跨增量轻量扫描器从译文 JSON 流（`{"lines":[{"line":0,"text":"…"},…]}`）提取最后一个完整 `text` 值，`StreamSummary::display()` 只输出该摘要文本、按 60 字符（char 边界）截断、不追加省略号；摘要只需纯文本，`line` 字段不再消费，行号解析与配对逻辑随 `L{n}: ` 前缀一并删除（死重量 clean cutover）。增量可任意切分（逐字节、跨转义、跨键名均可续上）。提取不到完整值（排队阶段、Pass 1 的设定集 JSON 无译文行、流被截断）一律空串，由前端降级显示已接收字节数——摘要尽力而为，不阻塞、不补造。
- 节流：距上次进度事件 ≥300ms，或本次累计新增 ≥2048B，任一满足即发（时间条件独立成立，字节不足也发）；以流起始时间锚定首次判定，避免每个增量都触发一条。阶段事件不节流。
- 替代原裸字符串阶段标签（前端从未监听、单测不断言内容，属 clean cutover）；`translation_test` / `translation_models` 无小说上下文，传 `None` 完全静默。

**前端：全局 store 持有任务，进度事件驱动界面**（`stores/translation.ts` + `api/translation.ts`）：

- store 自身持有 invoke promise 与 1s 间隔计时器（elapsed = now − startedAt），调用页组件卸载不取消任务：浮窗进度区与底部状态栏的后台指示持续刷新，任务结束自行清理 active 与计时器。
- Channel 每行解析为一个事件对象（`TranslationPhase` / `TranslationProgressEvent`）；非法 JSON 或形状不符的行忽略，不影响其他行。phase 事件切阶段，progress 事件更新 received/summary。
- 失败写入跨页保留的 error 记录（novelId + page + message），Promise 原样 rethrow（页内 catch 照旧展示）；重进目标页时 `consumeError(novelId, page)` 读取并清除，页不匹配则保留现场。翻译中进行中重复调用 join 同一 promise，不起第二个 invoke。
- 前端不做队列：串行锁在后端，界面的「排队等待中」只是如实回显后端状态。

**界面：浮窗内嵌进度区 + 底部状态栏后台指示**（规则同步 DESIGN.md 与 `.impeccable/design.json`）：

- 进度区内嵌于「小说翻译」浮窗（`NovelTranslationMenu.vue`）：状态行下方、模式组上方，`active: TranslationActive | null` 仅在当前小说有在途任务时渲染（`store.active.novelId === 当前小说`），别的小说页浮窗不显示他人进度。构成：`md-linear-progress` indeterminate + meta 行（阶段 · 已用 · 已接收，12px ink-muted）+ 一行流式摘要；无自有容器边框/背景/内外边距（浮窗自身即 surface-container + 合法轻阴影），面板宽度维持 256px 不加宽；摘要语义（60 字符截断、无行号前缀、空串降级后台安抚文案）不变。
- 摘要 marquee：`<code>` 单行 nowrap，JS 判定首份文本宽度超过容器宽度（实现为首份 `offsetWidth` 与 clip 盒 `clientWidth` 比较，避免克隆份介入后的反馈环）才启用双份内容无缝滚动（`animation: mq-scroll 12s linear infinite`），不溢出时静态全显；`@media (prefers-reduced-motion: reduce)` 下 `animation:none` + 静态 ellipsis，克隆份退场。这是本项目唯一的自定义循环动效，在此明示备案。
- aria 纪律：浮窗状态行 `role="status" aria-live="polite"` 是全组件唯一播报区（挂页面状态文案）；进度区整体不进播报区，秒级 elapsed 与字节数不播报（避免朗读刷屏）；`md-linear-progress` 与摘要行 `aria-hidden`，摘要全文经 title 悬停查看；等宽字体沿用 CopyPathButton 的 `<code>` 先例。
- 底部状态栏左侧在 active 时显示文字按钮「翻译中：《title》第 N 页」，点击回到该小说页（`/browse/work/novel/{id}`，已有重复导航 guard）；aria-label 拼接「回到小说页：…」满足 WCAG 2.5.3 可访问名称；≤640px 只显示短文案「翻译中…」；空闲不占位。

## 影响

- **IPC 契约变化**：`novel_translate_page` 的 progress Channel 载荷从裸字符串阶段标签变为 JSON 事件流；命令签名、返回体、串行/超时/持久化语义都不变，§3.4 命令计数不变（无新命令）。拿不到增量的兼容端点（ADR 0025 的退化场景）只损失摘要行，阶段事件照常。
- **失败不再因卸载丢失**：跨页保留的 error 记录把「退出页面后失败被吞」变为重进页面即可看到原因并重试；store 持有 invoke 也让离开页面不再中断翻译。
- **功耗/带宽可忽略**：进度事件最密 300ms 一条、百字节量级，且只在翻译期间存在；摘要扫描是单次字符遍历，无额外请求。
- 后续演进：progress 事件增加思考尾窗字段、摘要放宽为任意字符串值（Pass 1 亦可显），见 ADR 0030。
- 离线验收：`translation_summary.rs` 单测覆盖逐字节/跨块切分、转义与代理对解码、完整值到达前恒空、60 字符 char 边界截断、未知键与非 JSON 流忽略、尾部残值不丢已提取结果、游离括号不丢提取（行号配对相关单测随该逻辑删除一并移除）；`translation.rs` 单测覆盖两种事件的 JSON 形状、节流放行条件（时间/字节各自独立成立）与两轮流程中的实际上报。浏览器验收经 api/translation.ts 的非 Tauri 确定性 mock 事件流（与后端协议同构）驱动浮窗进度区（含摘要 marquee 溢出滚动与 reduced-motion 静态降级）与底部状态栏。
