# 09 — Exporter（txt + markdown 双输出）

**What to build:**

从用户视角：抓到的小说数据能保存成本地文件，用户可以选择 txt 或 markdown 格式（或两者）。文件名清晰可读，内容正确处理 pixiv 小说的特殊标记（章节、换页、跳转、ruby 注音等）。markdown 格式渲染后能用 VS Code / Obsidian 直接阅读，章节结构清晰。

**Blocked by:** 08（PixivClient，提供 novel 数据结构）

**Status:** ready-for-agent

**Acceptance criteria:**

- [ ] `backend/core/exporter.py` 定义 `Exporter` 抽象基类，方法 `export(novel, target_dir, series_order) -> list[Path]`
- [ ] `TxtExporter` 实现：纯文本输出，多页用空行分隔
- [ ] `MarkdownExporter` 实现：渲染章节标记为 `## <chapter>`、`[newpage]` 为 `---`、`[jump:xxx]` 移除、`[pixivimage:xxx]`/`[uploadedimage:xxx]` 替换为占位符、`[rb:base>ruby]` 渲染为 `base(ruby)` 或 HTML ruby 标签
- [ ] 文件命名规则（SPEC §4.4）：
  - [ ] 单篇：`<title>_<novelId>.txt/.md`
  - [ ] 系列内：`[NN]_<episode>_<title>.txt/.md`（padded 2-3 位）
  - [ ] 文件名 sanitize：移除非法字符（参考旧脚本 Get-SafeFileName）
- [ ] 目录结构：
  - [ ] 单篇直接落到 output_dir
  - [ ] 系列落到 `<seriesTitle>_<seriesId>/` 子目录 + 轻量 `series.json`（含 seriesId/title/章节数/抓取时间）
- [ ] Exporter 工厂：根据配置 output_formats 列表选择启用哪些 exporter
- [ ] 返回值是生成的文件路径列表，供写入数据库 novels 表
- [ ] 覆盖策略：文件已存在时默认跳过（去重，配合 ticket 02 的 is_downloaded），可配置强制覆盖
- [ ] 注册 `EpubExporter` stub（抛 NotImplementedError），为 V2 预留接口
- [ ] 单元测试：标记处理（每种标记至少一个测试 case）、文件名 sanitize、多页合并
- [ ] 集成测试：抓一篇真实多页小说，导出后人工检查文件内容正确
