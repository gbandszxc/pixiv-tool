# ADR 0026：浏览模式发表官方表情（文本表情与贴图）

- 日期：2026-10-04
- 状态：已接受

## 背景

ADR 0022 放开浏览模式的评论发表与回复时，明确保留了「不扩展…表情贴图评论」这一条。
用户随后提出：客户端评论也要能像网页端一样**选择官方表情**。网页端评论输入框的表情面板
有两栏——「表情」（文本表情，点选把 `(code)` 插进正文）与「贴图」（stamp，点选立即发表情评论）；
两者的目录都不在任何接口里，而是**内置于 pixiv 前端 bundle**（本轮反查记录见
`docs/research/pixiv-browse-api.md` §14.6）。本次据此推翻 ADR 0022 里保留的那一条。

## 决策

- 浏览模式允许发表**官方文本表情**与**官方表情贴图（stamp）**，仍不扩展点赞、批量操作与集合评论。
- 贴图走既有写链路：`browse_comment_add` 的 `comment` 与 `stamp_id` **二选一**，
  贴图请求 form 为 `type=stamp&{illust_id|novel_id}&author_user_id&stamp_id[&parent_id]`
  （不带 `comment`），与文本评论同端点、同 CSRF、同失败自愈；契约新增 `stamp_id` 字段。
- 目录内置在前端（`frontend/src/api/browse.ts`），与 pixiv 同源、不新增端点：
  - 文本表情 38 个（`101-108 / 201-209 / 301-310 / 401-408 / 501-503`，code 如
    `normal` / `shame2` / `smile3` / `sleep4` / `heart`），图片
    `https://s.pximg.net/common/images/emoji/{id}.png`，正文写作 `(code)`（或 `:code:`）；
  - 贴图 40 个（官方面板可见的 3/4/2/1 组各 10 个：`301-310 / 401-410 / 201-210 / 101-110`；
    官方第 6/7 组标 `hidden` 面板不渲染，故不收录），图片
    `https://s.pximg.net/common/images/stamp/generated-stamps/{id}_s.jpg`。
  两类图片域 `s.pximg.net` 已在 `pixiv-img` 代理白名单内。
- 交互与网页端一致：面板两栏，文本表情点选把 `(code)` **追加**到草稿且面板不关（可连点），
  贴图点选**立即发表**并收起面板；面板两侧（根评论输入框与回复框）都能用，贴图随回复框挂到
  `parent_id` 之下。
- 渲染：评论正文里的 `(code)` / `:code:`（大小写不敏感）在客户端渲染为 24px 行内图，
  未知 code 原样显示；贴图评论按既有 `stampId → stamp_url` 渲染。均为纯前端渲染，无新端点。
- 目录是**外部事实**（pixiv 改版即可能变），改动集中在一处常量：新增/删除表情只需改
  `PIXIV_COMMENT_EMOJI` / `PIXIV_COMMENT_STAMPS`，无需触碰后端与文档契约。

## 影响

无新增 IPC（`browse_comment_add` 增两个可选参数）、无设置键、无 schema 与凭据落点变化；
`PublishedComment` 多一个可选 `stamp_id`。在线写用例在同一次往返里多测一步贴图发表与删除，
仍在 `#[ignore]` + `PIXIV_LIVE_WRITE=1` 之后。若 pixiv 调整目录分组（例如放出一批新贴图），
只需更新前端常量并回归 `/tests/comments.html`；若要支持 `(code)` 之外的新表情语法，
应扩展本 ADR。
