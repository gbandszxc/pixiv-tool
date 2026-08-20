# ADR 0003 · 任务模型：Source + Crawler + Task

**状态**：已接受 · **日期**：2026-07-19 · **关联 SPEC**：§4.2

> 状态补充（2026-08-20）：任务模型由 Rust 版 core/task_manager.rs 承接，语义保持；取消终态不再被 done 覆写、retry 计数累计两处旧版 bug 已修正（见 [ADR 0008](0008-tauri-rewrite.md)）。

## 背景

需支持三类抓取来源（单篇 / 系列 / 用户全集），共享单篇抓取逻辑但编排不同。

候选：
- A. 三个独立函数
- B. Crawler 基类 + 继承
- C. Source + Target 分离
- D. 统一 Task 模型

## 决策

**C + D 结合**。

- `NovelSource`（ABC）：解析来源 → `AsyncIterator[tuple[novel_id, series_order]]`
  - `SingleNovelSource` / `SeriesSource` / `UserNovelsSource`
- `Crawler`：消费 id 流，统一执行并发、限速、重试、写库、推 SSE
- `Task`：状态机 `pending → running ⇄ paused → done/failed/canceled`，序列化到 SQLite `tasks` 表

## 理由

1. 抓住了正确的"变与不变"：来源解析是变的，单篇抓取是不变的。
2. 加新来源（V2 搜索/收藏）只需新增 Source 子类，Crawler 与 Task 零改动。
3. Task 序列化支持断点续传（SPEC §6.1）。
4. 避免 OOP 继承的脆弱基类问题（B 的方案）。

## 后果

**正面**
- 扩展性强，符合开闭原则。
- 状态机明确，可序列化。
- 单元测试易：Source 与 Crawler 可独立 mock。

**负面**
- 抽象层数多，V1 学习曲线略陡。
- `AsyncIterator` 在调试时栈跟踪较深。
