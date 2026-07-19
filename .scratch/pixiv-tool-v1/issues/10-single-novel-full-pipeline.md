# 10 — Source 抽象 + 单篇任务完整链路

**What to build:**

从用户视角：在抓取页面输入一个 novel id（或 pixiv 小说页 URL），选择输出格式（txt/markdown 复选框），点"开始抓取"。UI 出现一个任务卡片显示进度，几秒后任务变成"完成"状态，文件落到 `downloads/` 目录，历史记录里能看到这条小说。这是整个工具最核心的端到端链路——从用户输入到文件落盘的全流程打通。

**Blocked by:** 07（登录态展示）、08（PixivClient）、09（Exporter）

**Status:** ready-for-agent

**Acceptance criteria:**

- [ ] `backend/core/source.py` 定义 `NovelSource` 抽象基类（ABC），方法 `resolve(client) -> AsyncIterator[tuple[novel_id, series_order]]`
- [ ] `SingleNovelSource` 实现：构造时传 novel_id，resolve yield `(novel_id, None)`
- [ ] `backend/core/crawler.py` 实现 `Crawler.run(source, task)`：消费 source 的 id 流，按限速+并发抓取每篇
- [ ] `backend/core/task.py` 实现 Task 状态机：`pending → running ⇄ paused → done/failed/canceled`（省略 pausing 过渡态）
- [ ] Task 序列化到 SQLite `tasks` 表（配合 ticket 02）
- [ ] `POST /api/tasks` 创建任务，body: `{source_type: "single", source_id: "<novel_id>", formats: ["txt","markdown"]}`
- [ ] `GET /api/tasks/{id}/events` SSE 推进度，事件类型见 SPEC §7.6（progress/item/failed/done）
- [ ] Vue 抓取页（路由 `/`）：
  - [ ] 来源类型单选（默认"单篇"）
  - [ ] 输入框（支持 novel id 或 URL，URL 自动提取 id）
  - [ ] 输出格式复选框（默认 txt + markdown 都勾）
  - [ ] "开始抓取"按钮
- [ ] Vue 任务页（路由 `/tasks`）显示任务列表 + 进度条 + 状态
- [ ] SSE 事件实时更新任务卡片
- [ ] 抓取完成后调用 Exporter 写文件（配合 ticket 09）
- [ ] 写入 novels 表去重（配合 ticket 02 的 is_downloaded，已抓的跳过）
- [ ] 任务结束 UI 显示统计：done/total/skipped/failed
- [ ] 错误处理：cookie 失效提示重新登录；网络错误显示在任务卡片
- [ ] 集成测试：抓一篇真实小说，验证文件落盘 + 数据库记录 + UI 进度
