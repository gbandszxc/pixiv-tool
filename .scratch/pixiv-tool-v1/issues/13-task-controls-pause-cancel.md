# 13 — 任务控制（暂停/继续/取消/重试）

**What to build:**

从用户视角：任务卡片上有四个控制按钮——暂停、继续、取消、重试失败。点暂停任务立即停下来（不丢失进度），点继续从断点接着抓，点取消彻底终止任务并清理，点重试只重抓失败的篇。所有操作响应迅速，状态正确切换。

**Blocked by:** 10（单篇任务完整链路，需要 Task 状态机）

**Status:** ready-for-agent

**Acceptance criteria:**

- [ ] Task 状态机正确处理所有状态转换（SPEC §4.2）：pending/running/paused/done/failed/canceled
- [ ] 暂停：通过共享 asyncio.Event 通知 Crawler 停止取下一个 id，当前正在抓的等完成
- [ ] 继续：清除 asyncio.Event，Crawler 从未完成的 id 接着抓
- [ ] 取消：彻底终止 asyncio 任务，已抓的保留，未抓的标记放弃，Task 状态设为 canceled
- [ ] 重试失败：从 failed_ids（SQLite tasks 表字段）创建新任务，只抓这些 id
- [ ] `POST /api/tasks/{id}/pause` / `resume` / `cancel` / `retry-failed` 端点
- [ ] Vue 任务卡片四个按钮根据状态启用/禁用：
  - [ ] running：暂停 + 取消可用，继续 + 重试禁用
  - [ ] paused：继续 + 取消可用，暂停 + 重试禁用
  - [ ] done：重试（仅 failed > 0 时）可用，其他禁用
  - [ ] failed：重试 + 取消可用，其他禁用
  - [ ] canceled：无可用按钮
- [ ] 状态变更通过 SSE 推送给前端
- [ ] 持久化：暂停/取消时立即更新 SQLite tasks 表（防 App 崩溃丢状态）
- [ ] 单元测试：状态机所有转换路径
- [ ] 集成测试：抓 5 篇小说的任务，中途暂停/继续/取消各一次
