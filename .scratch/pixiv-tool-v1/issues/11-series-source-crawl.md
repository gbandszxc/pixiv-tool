# 11 — SeriesSource + 系列抓取

**What to build:**

从用户视角：在抓取页面切换来源类型为"系列"，输入 series_id（或 pixiv 系列页 URL），点开始。工具按系列顺序抓取所有篇章，每篇独立保存（不合并），文件名带 `[01]_`、`[02]_` padded 序号，能一眼看出系列内顺序。所有文件落到以系列名命名的子目录，目录内附一个轻量 `series.json` 记录系列元数据。

**Blocked by:** 10（单篇任务完整链路）

**Status:** done

**Acceptance criteria:**

- [x] `SeriesSource(NovelSource)` 实现：构造传 series_id
- [x] resolve 调用 `client.get_series_content(series_id)` 拿到系列内容列表
- [x] yield `(novel_id, order)`，order 从 1 开始递增（按系列定义顺序）
- [x] PixivClient 新增 `get_series_content(series_id)` 方法（调 `/ajax/novel/series/{series_id}` 或类似接口，需 spike 确认真实路径）
- [x] Crawler 把 series_order 传给 Exporter（配合 ticket 09 的命名规则）
- [x] Exporter 输出目录：`<output_dir>/<seriesTitle>_<seriesId>/`
- [x] 系列内文件名：`[NN]_<episode>_<title>.txt/.md`，padded 到位数（≤99 篇 2 位，>99 篇 3 位）
- [x] 目录内生成 `series.json`：`{series_id, title, total, captured_at}`
- [x] 任务卡片显示"N 篇 / 共 M 篇"进度
- [x] 系列内已抓的篇跳过（去重）
- [x] 抓取页"来源类型"支持"系列"选项
- [x] URL 输入框支持识别 series URL（自动提取 series_id）
- [x] 集成测试：抓一个真实小系列（3-5 篇），验证顺序、命名、series.json
