# 12 — UserNovelsSource + 用户全集

**What to build:**

从用户视角：在抓取页面切换来源类型为"用户"，输入 user_id（或 pixiv 用户主页 URL），点开始。工具抓取该用户名下所有小说，按系列分子目录 + 散篇散文件，顶层目录以作者名命名。这是规模最大的抓取场景（可能几百篇），任务卡片显示总进度，长任务支持暂停后继续。

**Blocked by:** 11（系列抓取，复用 Source 抽象）

**Status:** ready-for-agent

**Acceptance criteria:**

- [ ] `UserNovelsSource(NovelSource)` 实现：构造传 user_id
- [ ] resolve 调用 `client.get_user_novels(user_id)` 拿到全部 novel id（一次返回，pixiv 该接口不分页）
- [ ] PixivClient 新增 `get_user_novels(user_id)` 方法（调 `/ajax/user/{user_id}/profile/all`）
- [ ] 处理用户作品的结构：先抓 profile/all 拿 novel id 列表 + novelSeries 列表，对每个 series 走 SeriesSource，剩余散篇走 SingleNovelSource
- [ ] Exporter 顶层目录：`<output_dir>/<authorName>_<userId>/`
- [ ] 系列作品落到 `<authorName>_<userId>/<seriesTitle>_<seriesId>/`
- [ ] 散篇直接落到 `<authorName>_<userId>/`
- [ ] 任务卡片显示"作者：xxx，共 N 篇（M 个系列 + K 散篇）"
- [ ] 大规模抓取（>50 篇）验证限速和 429 暂停机制有效
- [ ] 抓取页"来源类型"支持"用户"选项
- [ ] URL 输入框支持识别 user URL（自动提取 user_id）
- [ ] 集成测试：抓一个小作者（10 篇以内），验证目录结构
