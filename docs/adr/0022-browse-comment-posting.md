# ADR 0022：浏览模式发表评论与回复

- 日期：2026-10-04
- 状态：已接受

## 背景

ADR 0016 划定的浏览写操作边界是「作品收藏 + 作者关注/取消关注」，并明确写了
**不扩展点赞、评论发布**。用户反馈客户端（插画 / 漫画 / 小说）只能看评论、不能
发评论，要求补齐。评论读路径（roots / replies）自 v2.1 起已在 `docs/PIXIV-API.md`
§4.5 有契约与在线用例，写路径此前完全未调研。本次用 Chrome 登录态在本人作品上抓包，
拿到发表与删除的端点和响应形状（证据见 `docs/research/pixiv-browse-api.md` §14），
据此推翻 ADR 0016 中「不扩展评论发布」这一条。

## 决策

- 浏览模式允许**发表评论与回复评论**，仍不扩展点赞、批量操作与表情贴图评论。
- 新增 IPC `browse_comment_add(kind, id, authorId, comment, parentId?)`：
  - 插画·漫画 → `POST /rpc/post_comment.php`，form
    `type=comment&illust_id={id}&author_user_id={authorId}&comment={正文}[&parent_id={评论 id}]`；
  - 小说 → `POST /novel/rpc/post_comment.php`，同形但键为 `novel_id`；
  - CSRF 复用现有主站 `__NEXT_DATA__` token（与 street / 收藏写同一进程内缓存与
    失效自愈），凭据仍只在 Rust 侧、不入前端与数据库。
  - `authorId` 是**作品作者**的 userId（官方 web 同形参数），由作品详情 `user_id` 传入；
    服务端从 Cookie 取会话身份，客户端不传评论者自身 id。
- 正文按**字符**限制 1–140（官方评论框 `maxlength=140`），命令层粗校验 + api 层权威校验；
  成功后前端重拉评论首页 / 该评论回复首页，展示服务端权威数据，不做乐观插入。
- 回复入口只挂在根评论上（`parent_id` = 该评论 id）。指向二级回复的 `parent_id` 语义
  未实测，不实现、不猜。
- 删除评论（`/rpc_delete_comment.php`、`/novel/rpc_delete_comment.php`）只作为在线写用例
  发完即删的清理手段，**不暴露 IPC、界面无入口**——避免在线用例在用户作品上留垃圾。
- 真实的发表/删除往返仅由 `#[ignore]` + `PIXIV_LIVE_WRITE=1` 的在线用例执行，目标从
  「本人作品列表」动态取样，先发根评论再发回复，最后无条件删除还原。

## 影响

新增 1 个 IPC（浏览端点 21 → 22）与一个组件内的发表/回复 UI，无新增设置键、数据库表或
凭据落点。评论写与收藏写共用同一 CSRF 缓存：写失败（Auth/Client）会清缓存自愈，与既有
语义一致。pixiv 侧频率限制、敏感词与超长校验的真实报错信封未逐一实测，`error:true`
一律走既有 `extract_ajax_body` 通道，原样回传服务端文案，不模拟成功。若后续要支持回复
二级评论或删除自己的评论，需重新实测 `parent_id` 归位语义并扩展本 ADR。
