# ADR 0054：小说 EPUB 导出与系列整包下载

- 状态：已接受
- 日期：2026-11-15
- 关联 ADR：[ADR 0015](0015-navigation-download-workspace.md)（核心导航与非模态下载
  工作区）、[ADR 0028](0028-image-cache-limit-and-partitioned-eviction.md)（图片缓存
  上限与下载通道）、[ADR 0035](0035-whole-novel-agent-translation.md)（整篇小说
  Agent 翻译 Job）、[ADR 0044](0044-structured-command-error-payload.md)（结构化命令
  错误载荷）、[ADR 0045](0045-download-grid-view-and-local-image-protocol.md)
  （下载历史宫格与书本封面色板来源）
- 规格：[SPEC](../SPEC.md) §4.3 / §4.4 / §5.1 / §7

## 背景

1. **EPUB 长期停留在「预留」**：SPEC §1.2 自 V1 起把「EPUB 输出」列为明确不做
   （仅预留格式接口），§13 开放问题挂着「EPUB exporter 实现」；`task_create` 与
   `novel_translation_export` 的格式白名单之外的 `epub` 值被静默忽略。阅读器、整篇
   翻译与下载历史宫格相继落地后，「只有 txt / md」成为产物层最明显的短板——主流
   阅读设备直接吃 EPUB，而 txt / md 缺少封面、目录与内嵌插图。
2. **小说内嵌图片的「落盘下载」方案被取代**：`[pixivimage:...]` /
   `[uploadedimage:...]` 标记的图片此前作为开放问题计划按独立图文件落盘；但散图
   与正文位置的对应关系要靠用户自行维护。把图片直接内嵌进 EPUB，产物自包含，
   落盘散图方案失去必要性。
3. **系列逐话产物不满足整书阅读**：V1 的 series 来源按「各话独立文件 + 序号编号」
   产出（SPEC §1.1 明确「不合并为单文件」）；系列越长产物越碎，导入阅读器后无法
   作为一本书连续阅读，md / epub 也没有目录。系列目录获取还隐含「≤30 话」的
   单次响应假设，长系列取不全。

## 决策

### 1. EPUB 用 zip crate 手写组装，不引入 epub-builder

- `task_create` 的 formats 白名单从 `{txt, markdown}` 扩为 `{txt, markdown, epub}`，
  single / series / user 三来源全部放行（user 来源 = 每篇一个 epub）。
- EPUB 3.0 容器结构：`mimetype`（Stored 无压缩、必须为首文件）→
  `META-INF/container.xml` → `OEBPS/` 下 `content.opf` + `nav.xhtml` + `toc.ncx` +
  `style.css` + `cover.xhtml` + `images/*` + 章节 XHTML。单篇 = 1 个章节文件；
  系列整包 = 每话 1 个（按 contentOrder 顺序）。
- 组装实现在 `core/epub.rs`，用已在树的 zip crate（`ugoira.rs` 已有 Stored 写入
  先例）手写；不引入 `epub-builder`（容器结构固定后手写成本低、依赖面更小）与
  `image` crate（封面 PNG 用 `png` crate 直出，见决策 3）。

### 2. 标记 → XHTML 转换口径与阅读器一致

`[chapter:X]` → `<h2>`；整行 `[newpage]` → `<div class="page-break">`（CSS
`page-break-before: always`）；`[rb:A>B]`（容忍 `A/B` 旧分隔）→
`<ruby>A<rt>B</rt></ruby>`；`[[jumpuri:text > url]]` → 外链 `<a>`；`[jump:*]` /
`[jumpurl:*]` 删除；其余逐行 `<p>`、空行折叠、全部 HTML 转义。与阅读器 / markdown
导出的既有渲染口径同源，不发明第二套转换规则。

### 3. 内嵌图片与封面：尽力下载、失败降级

- **内嵌图**：`[uploadedimage:id]` → 同响应 `textEmbeddedImages` 的 URL（1200x1200
  优先，回退 original → 480mw → 240mw → 128x128）；`[pixivimage:id(-N)]` →
  `/ajax/illust/{id}`（第 0 页 `urls.original`）或 `/ajax/illust/{id}/pages`
  （第 N 项）。经既有 `PixivClient::download_bytes` 下载内嵌；单图失败降级为
  XHTML 文字占位块「〔图片占位〕」，不中断任务。
- **封面**：pixiv 封面优先——单篇取 `coverUrl`，系列取 `/ajax/novel/series/{id}`
  的 `cover`（兼容字符串与 `{urls:{...}}` 对象两种形态，取 1200x1200 档），下载
  成功直接用；缺失或失败时**后端生成占位封面 PNG**（1000×1414，1:1.414 书籍比例）：
  色板移植自前端下载历史宫格的 `BOOK_THEMES` 五色（藏青 / 黛绿 / 绯木 / 玄黑 /
  琥珀，按 novel_id 取模，ADR 0045 同源），内容为标题（自动缩字号、最多 3 行）+
  作者名 + 顶部「小说」细框印章字样；文字用 `ab_glyph` 光栅化系统字体（候选：
  Windows msyh.ttc / simhei.ttf / simsun.ttc，macOS PingFang.ttc /
  Arial Unicode.ttf，Linux NotoSansCJK / wqy-microhei；全缺失 → 纯色无字封面）。
  新增依赖仅 `png` + `ab_glyph`。

### 4. 系列来源改为整包（推翻 V1 逐话独立编号）

- series 来源的 txt / md / epub 全部改为**每格式一个文件**的整包产物：txt 每话以
  标题分隔行 + 原文原样；md 含目录 + 每话 `## NN. 标题`（标记按既有
  render_markdown 口径转换）；epub 含 nav 目录、每话一个章节 XHTML。user 来源仍
  逐篇落盘、命名不变。
- 整包文件名沿用 `novel_series_filename_template` 渲染（`{order}` 在 series 来源
  无值，按既有空值 token 吸收规则删除）；渲染为空回落
  `sanitize(系列标题)_{sid}`。
- 系列目录获取改造为游标循环聚合（废除「≤30 话」假设），逐话 `get_novel` 全文，
  复用既有 client 限速管线（并发 2 / 400ms 间隔 / 429 暂停 60s），不自建并发。
  任务事件沿用 `task://progress` / `task://done`，整包 total = 话数、
  done = 已完成话数、current_title = 当前话标题。

### 5. 系列译文导出：新命令 + 全系列校验 + 结构化错误

- 单篇 `novel_translation_export` 的 formats 白名单扩 `epub`（渲染复用下载管线的
  EPUB 结构，正文替换为译文）。
- 新命令 `novel_translation_export_series(seriesId, formats)`：成功返回
  `Vec<String>` 路径数组；失败 `200 + {error:{code, field, message}}`（对齐 ADR 0044
  结构化方向），code = `translation_incomplete`。校验：联网取系列目录（browse
  `get_novel_series` 游标取全目录），以 contentOrder 最小话的最新可用译文 Job 的
  目标语言为基准，逐话要求存在该语言 `Completed` + `Complete` 的 Job
  （`read_exportable_job` 口径）；任一话缺失或语言不一致即失败。校验通过后正文 /
  译文全部读本地 Job 产物、不逐话联网。产物命名 `{系列词干}_{语言后缀}.{ext}`
  （词干 = 系列模板渲染同决策 4）。

### 6. novels.epub_path 轻量迁移

`novels` 表加 `epub_path TEXT` 可空列，`Db::open` 沿用 `PRAGMA table_info` 探测 +
`ALTER TABLE` 补齐的轻量迁移路径（与 `tasks.pages` / `illustrations.saved_pages`
同模式），旧库不重建；旧行该列为空，仅在产出 EPUB 后写入。`open_novel_file` 的
kind 白名单加 `epub`（epub 不插队 txt / md，仅显式指定时使用）。

## 后果

- **产物布局变化**：series 来源不再产出逐话独立文件；已下载的旧系列逐话文件不
  迁移、不合并，重新创建 series 任务才得到整包。
- **新增依赖**：`png` + `ab_glyph`（占位封面 PNG 光栅化）；不引入 `image` /
  `epub-builder`。
- **内嵌图与封面是尽力而为**：单图失败降级文字占位块、封面缺失降级占位封面 PNG，
  EPUB 产物始终可交付；失败计入任务失败计数，供任务页与下载历史回显（SPEC R16）。
- **设置页**：`outputFormats` 增加 EPUB 选项（默认值不变 `["txt", "markdown"]`）；
  `novel_series_filename_template` 键保留，但其 `{order}` 占位符在 series 来源
  不再有值（整包无话序），模板校验规则不变。
- README 删除「路线图」整章；SPEC §1.2 删除「EPUB 输出（架构预留）」并改述
  「小说内嵌图片落盘下载」、§13 删除「EPUB exporter 实现」「小说内嵌图片下载」
  与已落地的「自适应限速」条目。
