# pixiv-tool V1 · Ticket 索引

> 来源：`docs/SPEC.md`（13 章）+ `docs/adr/0001~0005`
> 拆分方式：tracer-bullet（端到端垂直切片），每个 ticket 完成后可独立演示
> 共 16 个 ticket，分 7 个阶段

## 阶段 1 · 地基（4 个并行可启动，无 blocker）

- [01 — 项目骨架 + dev.ps1 + 端口管理](issues/01-project-scaffold-dev-script.md)
- [02 — SQLite + schema 初始化 + 基础模型](issues/02-sqlite-schema-models.md)
- [03 — CookieStore 接口 + DPAPI 实现](issues/03-cookie-store-dpapi.md)
- [04 — Settings JSON 配置 + 日志](issues/04-settings-logging.md)

## 阶段 2 · FastAPI+Vue 通信

- [05 — FastAPI 路由 + Vue axios + SSE 通道](issues/05-fastapi-vue-sse-channels.md) ← 01

## 阶段 3 · 登录

- [06 — pywebview 登录窗（复用 spike 成果）](issues/06-pywebview-login-window.md) ← 03, 05
- [07 — 登录态展示 + 失效检测](issues/07-login-status-display.md) ← 06

## 阶段 4 · 抓取核心

- [08 — PixivClient（httpx + 限速 + 重试）](issues/08-pixiv-client-throttle-retry.md) ← 02, 06
- [09 — Exporter（txt + markdown 双输出）](issues/09-exporter-txt-markdown.md) ← 08
- [10 — Source 抽象 + 单篇任务完整链路](issues/10-single-novel-full-pipeline.md) ← 07, 08, 09

## 阶段 5 · 系列与用户全集

- [11 — SeriesSource + 系列抓取](issues/11-series-source-crawl.md) ← 10
- [12 — UserNovelsSource + 用户全集](issues/12-user-novels-source.md) ← 11
- [13 — 任务控制（暂停/继续/取消/重试）](issues/13-task-controls-pause-cancel.md) ← 10

## 阶段 6 · UI 完善

- [14 — 历史页（筛选/搜索/打开文件）](issues/14-history-page-filter-search.md) ← 10
- [15 — 设置页 + i18n + 主题切换](issues/15-settings-i18n-theme.md) ← 04, 10

## 阶段 7 · 打包发布

- [16 — PyInstaller 打包 + GitHub Actions 三平台 CI](issues/16-pyinstaller-github-actions-ci.md) ← 所有功能 ticket

## Frontier（当前可立即开始的 ticket）

阶段 1 全部 4 个 ticket 无 blocker，可立即并行开工：

- 01 项目骨架
- 02 SQLite
- 03 CookieStore
- 04 Settings + 日志

## 依赖图

```
01 ──┬──> 05 ──┬──> 06 ──> 07 ──┐
02 ──┤         │                │
03 ──┘         │                │
              02 ──> 08 ──> 09 ─┤
                              10┴──> 11 ──> 12
                               │
                               ├──> 13
                               ├──> 14
04 ──> 15 <── 10
16 (全部完成后)
```

## 工作流

按 `/implement` skill，每次只领 frontier 里的一个 ticket，做完后清理 context，再领下一个：

1. 选 frontier 里编号最小的 ticket
2. 新开 context（或 /handoff 当前会话）
3. 按 ticket 的 acceptance criteria 逐条实现
4. 跑测试
5. commit
6. 回到 step 1
