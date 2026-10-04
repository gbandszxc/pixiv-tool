# pixiv 网页版浏览类 API 调研（Web Ajax/RPC 接口）

> **定位说明**：本文是 2026-10-01 的调研证据档案（含字段细节与当日实测）；
> **现行 pixiv 接口契约事实源是 `docs/PIXIV-API.md`**，两者冲突以后者为准，
> 勘误请回填 `docs/PIXIV-API.md`，不要只改本文。

> **勘误速查（2026-10-01 晚，`tests/pixiv_api/` 在线套件实机复核；细节与依据见 `docs/PIXIV-API.md` §6 与各端点小节）**
>
> | 本文原记 | 实测 | 影响 |
> |---|---|---|
> | §2 `page.trendingTags[].{tag,translatedName,illustCount}` | 条目键实为 `{tag,ids,trendingRate}`；且仅 illust 频道有该板块 | 译名改取同响应 `tagTranslation[tag].zh`；`count` 无源字段 |
> | §2 频道三接口同构 | `page.ranking.date` 形态不一（illust/manga `20260930`、novel `2026-09-30`）；`page.ranking.items[]` 是 `{id,rank}` 对象 | 日期经 `normalize_ymd` 归一；对象条目需专门解析（原按标量解析会整块丢弃） |
> | §5 `{word}` 放路径 | 必须 percent-encode，直拼非 ASCII → HTTP 400 | `search_path` 先 `percent_encode` |
> | §6 `/ajax/ranking/novel` 的 `date` 为 yyyymmdd | 实为日文展示串（`2026年9月30日`），`prev_date`/`next_date` 恒 null | 后端归一为 yyyymmdd |
> | §8 `/ajax/user/{id}?full=1` 含 `account` | 响应无 `account` 键（四种组合实测） | `pixiv_id` 恒空串，前端隐藏 @handle 行 |

> **2026-10-03 频道模式勘误**：Chrome `/cate_r18.php` 发起 `/ajax/top/illust?mode=r18&lang=zh`。同会话普通快照推荐 18/排行 100 均为一般向，r18 快照推荐 18/排行 100 均为受限；两种模式各有独立标签推荐。`mode=all` 等同普通快照，`mode=safe` 返回业务错误。漫画与小说 r18 请求亦成功。当前契约与测试映射见 `docs/PIXIV-API.md` §4.1 / §8.1。

> **2026-10-03 详情标签勘误**：插画、漫画、小说详情的 `tags` 实为 `{authorId,isLocked,tags:[{tag,...}],writable}` 对象，标签数组在 `tags.tags`，不能按顶层数组解析。三类均带 `likeCount` / `bookmarkCount` / `viewCount`；小说此前仅消费 bookmarkCount，已补全。另外描述 HTML 的换行须在转文本时保留。现行契约与回归映射见 `docs/PIXIV-API.md` §4.3 / §8.1。

> **2026-10-03 搜索计数复核**：真实 Chrome 登录会话与 Rust `live_search_count_fields` 均核对 `original` 首屏：小说 30 条全部自带 `bookmarkCount`（Chrome 样本中 27 条为零），不带 `likeCount` / `viewCount`；插画 59 条有效作品均无这三项。列表收藏数可直接复用，不能把零视为缺失。已通过 `parse_work_thumb` 透传可用计数，本页排序仅补取所选维度缺失项。证据只输出字段覆盖条数，不保存响应或凭据；当前契约与回归映射见 `docs/PIXIV-API.md` §4.2 / §8.1。

> 调研方式：2026-10-01 对已登录 pixiv 的真实 Chrome 会话（中文界面，pixiv-web-next 前端）做只读抓包，逐页面用浏览器内 `fetch()` 复调确认响应结构。
> 基础域名：`https://www.pixiv.net`（接口同源）；图片 `https://i.pximg.net`；静态资源 `https://s.pximg.net`；嵌入图 `https://embed.pixiv.net`。
> 本文只记录浏览（读）类接口；点赞/收藏/关注等写操作接口未调研。
> 通用返回包裹：`{ "error": false, "message": "", "body": ... }`；失败时 `error: true`，HTTP 状态可能仍是 200，业务失败需判断 `error` 字段。

## 0. 概览表

| 功能 | 接口 | 方法 | 备注 |
|---|---|---|---|
| 首页混合推荐流（street） | `/ajax/street/v2/main` | POST | 需 `x-csrf-token` 头；无分页，每次请求一批混合内容 |
| 频道页（插画/漫画/小说仪表盘） | `/ajax/top/illust` / `/ajax/top/manga` / `/ajax/top/novel` | GET | 一站式：关注新作+推荐+排行+新投稿+热门标签 |
| 发现页 | `/ajax/discovery/artworks` | GET | 仅 `mode=all`；无翻页参数，无限滚动=重复调用 |
| 已关注用户的新作品（插画/漫画） | `/ajax/follow_latest/illust` | GET | `p` 翻页 + `isLastPage` |
| 已关注用户的新作品（小说） | `/ajax/follow_latest/novel` | GET | 同上 |
| 动态页 stacc.php | 已下线（404） | - | 由 bookmark_new_illust.php + follow_latest 替代 |
| 搜索插画/漫画（合并） | `/ajax/search/artworks/{word}` | GET | `illustManga.data/total/lastPage` |
| 搜索小说 | `/ajax/search/novels/{word}` | GET | `novel.data/total/lastPage` |
| 排行榜（插画/漫画/动图） | `/ranking.php?format=json&mode=...&content=...` | GET | 每页 50 条；插画的 `/ajax/ranking/illust` 不存在（404） |
| 排行榜（小说） | `/ajax/ranking/novel` | GET | `display_a.rank_a[]`，每页 50 条 |
| 插画/漫画详情 | `/ajax/illust/{id}` | GET | 匿名可访问 |
| 多页图片列表 | `/ajax/illust/{id}/pages` | GET | 每页原图/大图 URL |
| 动图元数据 | `/ajax/illust/{id}/ugoira_meta` | GET | zip URL + 帧序列（注意不是 `/ugoira`） |
| 插画相关推荐 | `/ajax/illust/{id}/recommend/init` | GET | 一次性推荐池 + `nextIds`，`page` 参数无效 |
| 插画评论 | `/ajax/illusts/comments/roots` | GET | `illust_id` + `offset/limit` |
| 小说详情 | `/ajax/novel/{id}` | GET | 全文在 `content`，`[newpage]` 分页标记 |
| 小说相关推荐 | `/ajax/novel/{id}/recommend/init` | GET | 同插画推荐 |
| 小说评论 | `/ajax/novels/comments/roots` | GET | `novel_id` + `offset/limit` |
| 发表评论 / 回复（插画·漫画） | `/rpc/post_comment.php` | POST | form `type=comment&illust_id&author_user_id&comment[&parent_id]`；需 `x-csrf-token`（§14） |
| 发表评论 / 回复（小说） | `/novel/rpc/post_comment.php` | POST | 同上，路径带 `/novel` 前缀、键为 `novel_id`（§14） |
| 删除评论（插画·漫画 / 小说） | `/rpc_delete_comment.php` · `/novel/rpc_delete_comment.php` | POST | form `i_id&del_id`；本轮仅供在线写用例发完即删（§14） |
| 小说系列详情 | `/ajax/novel/series/{id}` | GET | 系列元数据 |
| 小说系列内容列表 | `/ajax/novel/series_content/{id}` | GET | `last_order` + `limit` 翻页 |
| 小说系列章节标题 | `/ajax/novel/series/{id}/content_titles` | GET | `[{id,title,available}]` |
| 作者基础信息 | `/ajax/user/{id}` | GET | `full=1` 完整 |
| 作者全部作品 ID 索引 | `/ajax/user/{id}/profile/all` | GET | illusts/manga/novels 的 `{id:...}` 映射 |
| 作者作品批量取详情（插画） | `/ajax/user/{id}/illusts?ids[]=...` | GET | 60 个 id 一批实测可行 |
| 作者作品批量取详情（小说） | `/ajax/user/{id}/novels?ids[]=...` | GET | 同上 |
| 作者主页置顶 | `/ajax/user/{id}/profile/top` | GET | illusts/manga/novels/pickup |
| 通知/未读数（辅助） | `/rpc/index.php?mode=message_thread_unread_count`、`/rpc/notify_count.php?op=count_unread` | GET | UI 角标 |
| 登录用户菜单（辅助） | `/ajax/user/extra` | GET | 当前登录用户信息 |
| CSRF token | 见 §8 | - | `POST` 类接口需要 `x-csrf-token` 头 |

---

## 1. 首页推荐流（street）

### 接口清单

| 方法 | URL | 关键参数 | 说明 |
|---|---|---|---|
| POST | `/ajax/street/v2/main?lang=zh` | 请求体 `{"k":null,"vhi":null,"vhm":null,"vhn":null,"vhc":null}` | 登录后首页混合流。**必须带 `x-csrf-token` 请求头**（与 `Content-Type: application/json`），否则 400「请重新登录」。无分页参数，每次返回一批 |

首页另有埋点/辅助请求：`POST /ajax/street/access`（曝光上报，客户端无需复刻）、`lc-event.pixiv.net/log/pixiv/street`（埋点，可忽略）。

### 响应关键字段（body）

| 字段 | 类型 | 含义 |
|---|---|---|
| `contents` | Array | 流内容项数组，每项为一个"卡片组" |
| `contents[].kind` | string | `"illust"` / `"manga"` / `"novel"` / `"collection"` |
| `contents[].thumbnails` | Array | 该卡片组的作品缩略信息（1 到多项） |
| `contents[].pickup` | object（可选） | 热评卡片：`{type:"comment", userName, profileImageUrl, comment, commentCount}` |
| `contents[].access` | object | pixiv 内部推荐跟踪信息（`typ`/`cid`/`rms.mtd` 推荐方法名等），第三方客户端可忽略 |

`thumbnails[]` 内的作品对象（illust/manga 类）：`type, id, title, description, tags[{name, translatedName, isEmphasized}], userId, userName, profileImageUrl, createDate, updateDate, pageCount, pages[{width,height,urls{...}}], xRestrict, restrict, sl, aiType, bookmarkable, isSensitiveForMasking, width/height, episodeCount(manga), bookStyle(manga), customThumbnail`。novel/collection 项结构类似（collection 的图片在 `embed.pixiv.net`）。

**分页机制**：没有翻页参数。前端在滚动/刷新时重复 POST 同一 URL 获取新一批，由前端去重。第三方客户端轮换推荐流即重复调用即可。

## 2. 频道页（/illustration、/manga、/novel）

三个频道页是"仪表盘"式页面（已关注用户的作品 / 推荐作品 / 每日排行榜 / pixivision / 比赛 / 热门标签……），页面 HTML 里不含数据（不含 __NEXT_DATA__ 作品数据），全部通过以下接口在客户端组装：

### 接口清单

| 方法 | URL 模式 | 关键参数 | 说明 |
|---|---|---|---|
| GET | `/ajax/top/illust?mode=all&lang=zh` | `mode`（实测 `all`） | 插画频道页全部数据 |
| GET | `/ajax/top/manga?lang=zh` | - | 漫画频道页 |
| GET | `/ajax/top/novel?lang=zh` | - | 小说频道页 |

### 响应关键字段（body，三类接口同构）

| 字段 | 类型 | 含义 |
|---|---|---|
| `page.follow` | Array[id] | 已关注用户的最新作品 id 列表（实测 48 个） |
| `page.recommend.ids` | Array[id] | 推荐作品 id 列表（18 个） |
| `page.recommend.details` | Object[id] | 每个推荐的 `{methods, score, seedIllustIds}` |
| `page.ranking.items` / `page.ranking.date` | Array / string | 每日排行 id 列表（100 个）与榜单日期 `yyyymmdd` |
| `page.newPost` | Array[id] | 本站最新投稿 id（36 个） |
| `page.trendingTags` | Array | 热门标签（含 `tag`/`translatedName`/`illustCount`） |
| `page.tags` | Array | 潮流标签（导航上的话题标签） |
| `page.recommendUser` | Array | 推荐用户 |
| `page.recommendByTag` | Array | 按标签的推荐板块配置 |
| `page.pixivision` / `page.contestOngoing` / `page.contestResult` / `page.editorRecommend` / `page.userEventIds` / `page.completeRequestIds` | Array | 各辅助板块 id/配置 |
| `thumbnails.illust` | Array | **作品索引表**：`id/title/illustType/xRestrict/restrict/sl/url/description/tags/userId/userName/width/height/pageCount/isBookmarkable/bookmarkData/alt/titleCaptionTranslation/createDate/updateDate/isUnlisted/isMasked/aiType/visibilityScope/urls/profileImageUrl`（700+ 条，覆盖上面所有板块） |
| `thumbnails.novel` | Array | 小说索引表：多 `genre/textCount/wordCount/readingTime/useWordCount/isOriginal/bookmarkCount/language` |
| `users` | Object[id] | 用户索引表（`userId/name/image/imageBig/...`） |
| `tagTranslation` | Object | 标签翻译表 `{tag: {zh-cn: ...}}` |
| `illustSeries` / `novelSeries` / `requests` / `boothItems` | Array | 系列/企划/约稿/BOOTH 板块 |

**渲染逻辑**：`page.*` 里的 id → 到 `thumbnails.illust|novel` 与 `users` 索引表查详情。第三方客户端按同样方式组装即可，一个接口拿全频道页。
**分页**：无。频道页是一次性快照。

### recommendByTag（#标签推荐板块，2026-10-01 实测）

- **仅 `/ajax/top/illust` 返回**（实测 manga/novel 响应无此字段；官方 /manga、/novel 页面也没有对应板块）。
- 形状：`page.recommendByTag` 为板块数组（实测 10 个），每项：
  - `tag`：标签名（官方板块标题即「#tag的推荐插画作品」）；
  - `ids`：24 个**字符串**作品 id（展示顺序）；
  - `details`：`{[id]: {methods: ["by_tag"], score: 0, seedIllustIds: []}}`，推荐跟踪信息，客户端可忽略。
- 作品本体在 `thumbnails.illust` 索引表，按 ids 顺序映射即可。
- **官方页面渲染**（/illustration 实测）：板块位于「推荐用户」之后、「本站的最新作品」之前；每板块 6 列 × 2 行 = **12 件**（接口给 24 个 id 但只渲染 12），卡片滚动进入视口才挂载；板块标题无「查看更多」链接。

## 3. 发现页（/discovery）

### 接口清单

| 方法 | URL 模式 | 关键参数 | 说明 |
|---|---|---|---|
| GET | `/ajax/discovery/artworks?mode=all&limit=60&lang=zh` | `mode`：仅 `all`（`illust`/`manga`/`novel` 均 400）；`limit`：实测 60 为常规值，过大（200）返回 400 | 每次返回 60 个推荐作品 |

**无限滚动翻页**：没有 lastId/cursor/page 参数。前端滚到底后**重复调用同一 URL**（实测一次会话调用了 6 次，URL 完全相同），前端自行去重追加。

### 响应关键字段（body）

| 字段 | 类型 | 含义 |
|---|---|---|
| `recommendedIllusts` | Array[60] | 每项 `{illustId, recommendMethods, recommendScore, recommendSeedIllustIds}` |
| `thumbnails.illust` | Array[60] | 作品索引表（结构与 §2 `thumbnails.illust` 相同） |
| `thumbnails.novel/novelSeries/novelDraft/collection` | Array | 本接口主要返回插画/漫画，这些通常为空 |
| `users` / `tagTranslation` / `illustSeries` / `requests` | - | 同 §2 索引表 |

## 4. 动态 / 已关注用户的新作品

**注意：旧版「动态」页 stacc.php 已下线（HTTP 404），新版导航没有动态入口，功能由「已关注用户的最新作品」（bookmark_new_illust.php）承接。**

### 接口清单

| 方法 | URL 模式 | 关键参数 | 说明 |
|---|---|---|---|
| GET | `/ajax/follow_latest/illust?p=1&mode=all&lang=zh` | `p`：页码从 1 开始；`mode`：`all` / `safe` / `r18` | 插画+漫画+动图混合新作品流 |
| GET | `/ajax/follow_latest/novel?p=1&mode=all&lang=zh` | 同上 | 小说新作品流 |

### 响应关键字段（body，两个接口同构）

| 字段 | 类型 | 含义 |
|---|---|---|
| `page.ids` | Array[60] | 本页作品 id（顺序即展示顺序） |
| `page.isLastPage` | bool | 是否最后一页（**翻页终止条件**） |
| `page.tags` | Array | （一般为空） |
| `thumbnails.illust` | Array | 作品索引表（同 §2；illust 接口里 illustType 0/1/2 混合） |
| `thumbnails.novel` | Array | 小说索引表（novel 接口） |
| `users` / `tagTranslation` / `illustSeries` / `requests` | - | 索引表 |

**分页机制**：`p` 递增直到 `isLastPage=true`。没有 `total`。每页固定 60 条（以 `page.ids` 实际长度为准）。

## 5. 搜索

### 接口清单

| 方法 | URL 模式 | 关键参数 | 说明 |
|---|---|---|---|
| GET | `/ajax/search/artworks/{word}?...` | 见下 | 插画+漫画合并搜索（页面 `/tags/{word}/artworks`） |
| GET | `/ajax/search/novels/{word}?...` | 同上 | 小说搜索（页面 `/tags/{word}/novels`），主体在 `body.novel` |

`{word}` 为 URL 编码后的关键词。

### 关键 query 参数

| 参数 | 合法值 | 说明 |
|---|---|---|
| `word` | - | 也放在路径里，query 可不传 |
| `order` | `date_d`（新着，默认）/ `date`（旧着）/ `date_asc` / `popular_d`（人气，需 Premium 才真生效） | 排序 |
| `mode` | `all` / `safe` / `r18` | 年龄限制过滤 |
| `p` | 1..`lastPage` | 页码 |
| `s_mode` | `s_tag_full`（完全一致标签）/ `s_tag`（标签部分一致）/ `s_tc`（标题·说明文） | 检索方式 |
| `type` / `illust_type` | `illust` / `manga` / `ugoira` | 作品类型过滤（artworks 接口） |
| `ai_type` | `0`（排除 AI 生成，实测默认带 0）/ `1` | AI 作品过滤 |
| `x_format` | 数值 | 尺寸比例过滤相关（实测任意值均 200，UI 的"横/竖/方形"过滤） |
| `ratio` | 如 `0.5` | 横竖比过滤 |
| `csw` | `0` | UI 状态参数 |
| `lang` | `zh` 等 | 翻译语言 |

### 响应关键字段（artworks：body；novels：body.novel）

| 字段 | 类型 | 含义 |
|---|---|---|
| `illustManga`（或 `novel`）.data | Array[≤60] | 作品列表，字段与 §2 `thumbnails.illust|novel` 一致（artworks 多 `profileImageUrl/isOriginal/is_howto`；novels 多 `seriesId/seriesTitle/seriesContentOrder/language`） |
| `.total` | number | 搜索结果总数（UI 显示"投稿超过 1 万件"即由此而来） |
| `.lastPage` | number | 最后一页页码（**分页终止条件**；也用于 UI 限制最大可跳转页） |
| `.bookmarkRanges` | Array | 收藏数区间过滤（`30000users入り` 等 chip 的依据） |
| `popular.recent` / `popular.permanent` | Array[6] | "人气"板块样例作品（Premium 完整版预览） |
| `relatedTags` | Array | 相关标签（`{tag, translatedName, count}`） |
| `tagTranslation` / `suggestChips` / `extraData.meta` | - | 翻译 / 搜索建议 chips / 页面 meta（canonical 等） |

> **列表项不含三项计数（2026-10-02 实测）**：`illustManga.data[]` 项**没有**
> `likeCount` / `bookmarkCount` / `viewCount`（novels 项仅自带 `bookmarkCount`，
> 无 like/view）。三项计数只在详情端点 `/ajax/illust/{id}` 与 `/ajax/novel/{id}`
> （字段名一致）。官方搜索页卡片上的收藏数（❤️N）为**逐项请求详情端点**所得
> （登录态抓包：一次搜索页加载触发 60 个 `/ajax/illust/{id}` 请求）；应用侧对应
> `browse_work_counts`（详见 `docs/PIXIV-API.md` §搜索）。
> **每页固定 60 条**：`limit` / `per_page` / `rows` / `count` 参数实测均无效
> （返回条数不变）；`lastPage` 上限 1000。

### 按 ID 直达的 URL 形态（客户端路由）

| 内容 | URL |
|---|---|
| 插画/漫画/动图详情 | `https://www.pixiv.net/artworks/{illustId}` |
| 作者主页 | `https://www.pixiv.net/users/{userId}` |
| 小说详情 | `https://www.pixiv.net/novel/show.php?id={novelId}` |
| 小说系列 | `https://www.pixiv.net/novel/series/{seriesId}` |
| 插画系列（漫画 series） | `https://www.pixiv.net/user/{userId}/series/{seriesId}` |
| 标签页 | `https://www.pixiv.net/tags/{word}/artworks`、`/novels` |

## 6. 排行榜

### 接口清单

| 方法 | URL 模式 | 关键参数 | 说明 |
|---|---|---|---|
| GET | `/ranking.php?format=json&mode={mode}&content={content}&p={p}&date={yyyymmdd}&lang=zh` | 见下 | **插画/漫画/动图**排行（页面 ranking.php）。注意：`/ajax/ranking/illust` 不存在（404），不要用 |
| GET | `/ajax/ranking/novel?mode={mode}&p={p}&date={yyyymmdd}&format=json&lang=zh` | 见下 | **小说**排行（页面 novel/ranking.php 实际也调它） |

`date` 缺省时返回最新一期（响应里的 `date` 字段告知实际榜单日期）。**pixiv 排行榜有约 2 天延迟**（今天 10-01 的最新榜是 `20260929`）。

### mode / content 合法值（实测）

| 接口 | 参数 | 合法值 | 说明 |
|---|---|---|---|
| ranking.php | `mode` | `daily` / `weekly` / `monthly` / `rookie` / `daily_r18` / `weekly_r18` | `male` / `female` / `original` / `daily_ai` 在未登录 Premium 的会话下 format=json 返回 404（Premium 限定榜单） |
| ranking.php | `content` | `all` / `illust` / `manga` / `ugoira` | 小说不在此接口 |
| /ajax/ranking/novel | `mode` | `daily` / `weekly` / `monthly` / `male` / `female` / `daily_r18` | 全部实测 200 |

### 响应关键字段

**ranking.php（插画/漫画/动图）**

| 字段 | 类型 | 含义 |
|---|---|---|
| `contents` | Array[50] | 每条：`rank, illust_id, title, date(发布时间), tags[], url(缩略图), illust_type, illust_page_count, illust_book_style, illust_content_type, illust_series, user_id, user_name, profile_img, width, height, yes_rank(昨日名次), rating_count, view_count, illust_upload_timestamp, attr, is_masked, is_bookmarked, bookmarkable` |
| `rank_total` | number | 该榜总条数（daily 500、rookie 300、daily_r18/weekly_r18 100、ugoira 69） |
| `date` / `prev_date` / `next_date` | string | 榜单日期（`yyyymmdd`）与前一日/次日 |
| `mode` / `content` / `page` / `prev` / `next` | - | 当前页参数；`next` 为下一页页码，`false` 表示无 |

**/ajax/ranking/novel（小说）**

| 字段 | 类型 | 含义 |
|---|---|---|
| `display_a.rank_a` | Array[50] | 每条：`rank, id, title, create_date, user_id, user_name, profile_img, comment(简介), restrict, x_restrict, is_original, language, character_count, word_count, ai_type, tag_a[](字符串数组), url(封面图), series_id, series_title, genre, bookmark_count, reading_time, is_bookmarked, bookmarkable, marker` |
| `h_title` | string | 榜单标题（"[pixiv] 小说 今日排行榜"） |
| `start` / `end` | string | 周/月榜的日期范围文本（日榜为 null） |
| `date` | string | 榜单日期 |

**分页**：`p` 递增（novel 榜 rank 1-50 → 51-100 实测正确）；插画榜响应 `next` 字段指示下一页。每页均 50 条。

> **R-18 判据（2026-10-01 实测）**：插画/漫画/动图榜的 `contents[]` 条目**没有顶层
> `x_restrict`**，R-18 标记在 `illust_content_type.sexual`（`0` 一般 / `1` R-18 /
> `2` R-18G）；`is_masked` 语义不明，不作判据。小说榜的
> `display_a.rank_a[]` 条目自带顶层 `x_restrict`。详见 §9。

## 7. 作品详情

### 7.1 插画 / 漫画 / 动图

页面 `https://www.pixiv.net/artworks/{id}`（单页、多页、动图同一 URL；`#2` 锚点定位页码）。

#### 接口清单

| 方法 | URL 模式 | 关键参数 | 说明 |
|---|---|---|---|
| GET | `/ajax/illust/{id}?lang=zh` | - | 作品主信息。匿名可访问 |
| GET | `/ajax/illust/{id}/pages?lang=zh` | - | 多页作品的每页 URL（单页作品也适用） |
| GET | `/ajax/illust/{id}/ugoira_meta?lang=zh` | - | **仅动图（illustType=2）**。注意路径是 `ugoira_meta`，`/ugoira` 返回 404 |
| GET | `/ajax/illust/{id}/recommend/init?limit=18&lang=zh` | `limit` | 相关推荐 |
| GET | `/ajax/illusts/comments/roots?illust_id={id}&offset=0&limit=3&lang=zh` | `offset/limit` | 评论根列表（`hasNext` 翻页；作者关闭评论区恒 400，见下注） |
| GET | `/ajax/user/{uid}/illusts?ids[]=...` | `ids[]` 可重复 | 同作者其他作品批量缩略信息 |

> **关闭评论区（2026-10-02 在线探针实测）**：作者关闭评论的作品，`comments/roots`
> 恒返回 **HTTP 400** + `{"error":true,"message":"不正确的请求。","body":[]}`
> ——泛化文案、无专属标志（novels 同族端点未单测，语义同族）；而
> `/ajax/illust/{id}` 详情响应**不含任何评论关闭标志字段**，前端无法提前感知。
> 案例：illust 150326647。应用侧映射见 `docs/PIXIV-API.md` §评论。

#### `/ajax/illust/{id}` 响应关键字段（body）

| 字段 | 类型 | 含义/示例 |
|---|---|---|
| `illustId` / `illustTitle` / `illustComment` | string | id / 标题 / 描述（HTML） |
| `illustType` | number | `0`=插画 `1`=漫画 `2`=动图 |
| `pageCount` | number | 页数 |
| `createDate` / `uploadDate` / `reuploadDate` | string | ISO 时间 |
| `userId` / `userName` / `userAccount` | string | 作者 |
| `userIllusts` | Object | 作者其他作品锚点（`{id: null}` 映射，配合批量接口） |
| `urls` | Object | `mini/thumb/small/regular/original` 五档代表图 URL |
| `tags` | Array | `{tag, locked, deletable, userId, userName, translation:{en,...}}` |
| `width` / `height` | number | 原图尺寸 |
| `bookmarkCount` / `likeCount` / `commentCount` / `responseCount` / `viewCount` | number | 计数 |
| `seriesNavData` | Object/null | 所属漫画系列 `{seriesType, seriesId, title, orderNumber, isConcluded, next, isReplaceable}` |
| `xRestrict` / `restrict` / `sl` | number | 限制级别（xRestrict: 1=R-18, 2=R-18G）；`sl` 为登录态相关敏感级 |
| `bookmarkData` / `likeData` | Object/null | 当前用户是否已收藏/点赞 |
| `isBookmarkable` / `isUnlisted` / `isHowto` / `isOriginal` / `aiType` / `bookStyle` / `commentOff` / `locationMask` | - | 各种状态位 |
| `descriptionBoothId` / `descriptionYoutubeId` / `comicPromotion` / `fanboxPromotion` / `contestBanners` / `zoneConfig` / `extraData` | - | 附加推广/配置（可忽略） |

#### `/ajax/illust/{id}/pages` 响应

Array，每项：`{ urls: { thumb_mini, small, regular, original }, width, height }`。
示例（original，示意数据）：`https://i.pximg.net/img-original/img/2021/01/01/00/00/00/9000012_p0.jpg`，页码后缀 `_p{n}` 从 0 开始。

#### `/ajax/illust/{id}/ugoira_meta` 响应（body 直接平铺）

| 字段 | 类型 | 含义 |
|---|---|---|
| `src` | string | 合成预览 zip：`https://i.pximg.net/img-zip-ugoira/img/{datePath}/{id}_ugoira600x600.zip` |
| `originalSrc` | string | 原始帧 zip：`.../{id}_ugoira1920x1080.zip` |
| `mime_type` | string | 帧图片类型（`image/jpeg`） |
| `frames` | Array | `[{file:"000000.jpg", delay:65}, ...]`，按序播放即动图（播放/导出 GIF 需自行拼合） |

#### `/ajax/illust/{id}/recommend/init` 响应

| 字段 | 类型 | 含义 |
|---|---|---|
| `illusts` | Array[limit] | 首屏推荐作品缩略信息（字段同 §2 索引表） |
| `nextIds` | Array | 后续候选作品 id（实测 162 个）。**`page` 参数无效**（page=2 返回与 page=1 完全相同），继续加载需用 `nextIds` 配合批量接口取详情 |
| `details` | Object[id] | `{methods, score, seedIllustIds, banditInfo, recommendListId}` 推荐元数据 |

### 7.2 小说

页面 `https://www.pixiv.net/novel/show.php?id={id}`。

#### 接口清单

| 方法 | URL 模式 | 关键参数 | 说明 |
|---|---|---|---|
| GET | `/ajax/novel/{id}?lang=zh` | - | 小说主信息+**全文**。匿名可访问 |
| GET | `/ajax/novel/{id}/recommend/init?limit=9&lang=zh` | `limit` | 相关小说推荐（`page` 参数同样无效，用 `nextIds`） |
| GET | `/ajax/novels/comments/roots?novel_id={id}&offset=0&limit=3&lang=zh` | `offset/limit` | 评论 |
| GET | `/ajax/user/{uid}/novels?ids[]=...` | `ids[]` | 同作者小说批量 |
| GET | `/ajax/user/{uid}/novels/tags?lang=zh` | - | 该作者的 novel 标签列表 |

#### `/ajax/novel/{id}` 响应关键字段（body）

| 字段 | 类型 | 含义/示例 |
|---|---|---|
| `id` / `title` / `description` | string | 基本信息（description 为 HTML） |
| `content` | string | **全文纯文本**。多页小说用 `[newpage]` 标记分页（`[newpage]` 数量 = pageCount - 1），章节标题用 `[chapter:标题]` 标记（渲染为节标题）；正文内嵌插画用 `[uploadedimage:novelImageId]`（见下「正文内嵌图」）、站内链接用 `[[jumpuri:文字 > URL]]`、旧式跳转 `[jump:...]` / `[jumpurl:文字:URL]` |
| `textEmbeddedImages` | Object | **正文内嵌图索引**：键 = `novelImageId`（即 `[uploadedimage:]` 的 id），值 `{novelImageId, sl, urls}`；`urls` 五档见下 |
| `pageCount` | number | 页数（按 `[newpage]` 切分；一次请求返回全部页内容，没有按页拉取的接口） |
| `tags` | Array | 同插画 tags 结构 |
| `seriesNavData` | Object/null | `{seriesType:"novel", seriesId, title, orderNumber, isConcluded, next}` |
| `userId` / `userName` / `coverUrl` | string | 作者与封面 |
| `characterCount` / `wordCount` / `useWordCount` / `readingTime` | - | 字数/词数/阅读时长（分钟） |
| `genre` | string | 流派 id（`0`=无） |
| `bookmarkCount` / `commentCount` / `markerCount` / `likeCount` / `viewCount` | number | 计数 |
| `xRestrict` / `restrict` / `isOriginal` / `isBungei` / `aiType` / `language` / `isUnlisted` / `commentOff` | - | 状态位 |
| `userNovels` | Object | 作者其他小说锚点 |

> 注：旧资料中的 `totalPages` / `pageNo` 字段在当前响应中不存在；多页即 `pageCount` + `content` 内 `[newpage]`，由客户端切分。

**正文内嵌图（2026-10-02 实测 novel 28669064）**

正文里的内嵌插画是**整行**的 `[uploadedimage:novelImageId]`；同一响应的 `textEmbeddedImages` 就是它的 URL 索引表（键 = id，值按档位给 URL）。该样本 24 张图，`content` 内的 24 个 `[uploadedimage:]` 标记与 `textEmbeddedImages` 的 24 个键**一一对应**：

```json
"textEmbeddedImages": {
  "25163259": {
    "novelImageId": "25163259",
    "sl": "6",
    "urls": {
      "1200x1200": "https://i.pximg.net/c/1200x1200/novel-cover-master/img/2026/07/22/19/21/12/tei…_master1200.jpg",
      "128x128":   "https://i.pximg.net/c/128x128/novel-cover-master/img/…_square1200.jpg",
      "240mw":     "https://i.pximg.net/c/240x480_80/novel-cover-master/img/…_master1200.jpg",
      "480mw":     "https://i.pximg.net/c/480x960/novel-cover-master/img/…_master1200.jpg",
      "original":  "https://i.pximg.net/novel-cover-original/img/…/tei…jpg"
    }
  }
}
```

- 路径段为 `novel-cover-master`（缩略档）与 `novel-cover-original`（`original`），均在 `i.pximg.net`，与作品封面同源同防盗链（需 Referer）。
- 正文渲染取 `1200x1200`（正文列宽度足够，且明显小于 `original`）。
- `sl` 语义未确认（样本恒 `"6"`），不消费。
- 另有一处站内链接标记形如 `[[jumpuri:文字 > https://www.pixiv.net/novel/show.php?id=…]]`（**双层方括号**）——旧的单层 `[jumpuri:文字>URL]` 记法在本样本中未出现。
- **`[pixivimage:illustId]` 在本样本与同期小说日榜前 30 条中均未出现**（0 例）：现行编辑器把插画以 `[uploadedimage:]` 内嵌，`[pixivimage:]`（连同 `-N` 页号后缀）属历史写法。若要支持，需对每个 id 另查 `/ajax/illust/{id}`（第 0 页）或 `/ajax/illust/{id}/pages`（第 N 页），成本为每图一次 ajax 请求，V1 不做，前端渲染占位块。

#### 小说系列

| 方法 | URL 模式 | 说明 |
|---|---|---|
| GET | `/ajax/novel/series/{id}?lang=zh` | 系列元数据 |
| GET | `/ajax/novel/series_content/{id}?limit=30&last_order=0&order_by=asc&lang=zh` | 系列内容列表（**翻页接口**） |
| GET | `/ajax/novel/series/{id}/content_titles?lang=zh` | 全部章节 `[{id, title, available}]`（轻量目录） |

`/ajax/novel/series/{id}` 关键字段：`id, userId, userName, profileImageUrl, title, caption, tags[], isConcluded, genreId, language, publishedContentCount, publishedTotalCharacterCount, publishedReadingTime, total(总章节数), firstNovelId, latestNovelId, displaySeriesContentCount, watchCount, cover, createDate/updatedTimestamp, isWatched, maxXRestrict, aiType`。

`/ajax/novel/series_content/{id}`：
- `page.seriesContents`：Array[limit]，每项 `{id, userId, series:{id, viewableType, contentOrder}, title, commentHtml, tags[], restrict, xRestrict, isOriginal, textLength, characterCount, wordCount, readingTime, bookmarkCount, url(封面), uploadTimestamp, reuploadTimestamp, isBookmarkable, bookmarkData, aiType}`
- `thumbnails.novel`：对应索引表
- **翻页**：`last_order` = 上一批最后一条的 `series.contentOrder`（如首批 limit=30 传回 contentOrder=30，则下一批 `last_order=30`）。响应没有 `isLastPage`，以返回条数 < limit 或覆盖到 `total` 为终止条件。`order_by` 实测仅 `asc` 生效。

## 8. 作者页（/users/{id}）

### 接口清单

| 方法 | URL 模式 | 关键参数 | 说明 |
|---|---|---|---|
| GET | `/ajax/user/{id}?full=1&lang=zh` | `full=1` 完整 / `full=0` 简版（作品详情页、列表页用简版） | 基础信息 |
| GET | `/ajax/user/{id}/profile/all?sensitiveFilterMode=userSetting&lang=zh` | - | **全部作品 id 索引**（一次拿全） |
| GET | `/ajax/user/{id}/illusts?ids[]={id1}&ids[]={id2}...&lang=zh` | `ids[]` 重复参数，实测 60 个/批 OK | 插画/漫画批量缩略信息 |
| GET | `/ajax/user/{id}/novels?ids[]=...&lang=zh` | 同上 | 小说批量缩略信息 |
| GET | `/ajax/user/{id}/profile/top?sensitiveFilterMode=userSetting&lang=zh` | - | 主页置顶/代表作（`illusts/manga/novels/collections/requestPostWorks`） |

> 旧文档中的 `/ajax/user/{id}/profile/illusts?ids=` 与 `profile/novels?ids=` 已失效（实测 400「不正确的请求」），批量取详情一律走 `/ajax/user/{id}/illusts|novels?ids[]=`。

### 响应关键字段

**`/ajax/user/{id}?full=1`**：`userId, name, account, image, imageBig, premium, isFollowed, isMypixiv, isBlocking, background, comment/commentHtml, webpage, social[], canSendMessage, region, age, birthDay, gender, job, workspace, official, following, mypixivCount, followedBack, commission, publisher, group`。

**`/ajax/user/{id}/profile/all`**：

| 字段 | 类型 | 含义 |
|---|---|---|
| `illusts` | Object（`{illustId: value}`） | 插画全集（实测 1340 项）。value 通常为 `null`；作品受限时为 `[xRestrict, restrict, sl]` 数组。**key 即全部插画 id，value 无信息量** |
| `manga` | Object | 漫画全集（同一 id 空间） |
| `novels` | Object | 小说全集 |
| `mangaSeries` / `novelSeries` | Array | 系列列表 |
| `collections` / `collectionIds` | - | 珍藏册 |
| `pickup` | Array | 置顶作品（带完整缩略信息） |
| `bookmarkCount` | number | 收藏数 |

**分页机制**：`profile/all` 本身无分页（一次返回全部 id）；展示时客户端把 id 列表**切片成 60 个/批**，逐批调 `/ajax/user/{id}/illusts?ids[]=`（或 novels）拿缩略信息。旧字段 `works`/`works_count` 已不存在——数量用 `Object.keys(illusts).length` 计算。

## 9. 图片 URL 与 Referer

### URL 模式（i.pximg.net）

| 类型 | 模式 | 说明 |
|---|---|---|
| 方形缩略图 | `https://i.pximg.net/c/250x250_80_a2/img-master/img/{datePath}/{id}_p{n}_square1200.jpg` | 列表小图（250x250） |
| 方形缩略图 | `https://i.pximg.net/c/540x540_70/img-master/img/{datePath}/{id}_p0_square1200.jpg` | 常用卡片图 |
| 方形缩略图 | `https://i.pximg.net/c/360x360_70/img-master/img/{datePath}/{id}_p0_square1200.jpg` | 频道页 |
| 方形缩略图 | `https://i.pximg.net/c/128x128/img-master/img/{datePath}/{id}_p0_square1200.jpg` | pages 接口的 thumb_mini |
| 自定义裁切 | `https://i.pximg.net/c/540x540_70/custom-thumb/img/{datePath}/{id}_p0_custom1200.jpg` | 作者自定义封面裁切 |
| 大图 | `https://i.pximg.net/img-master/img/{datePath}/{id}_p{n}_master1200.jpg` | 最长边 1200（pages 的 `regular` 不带 `/c/` 前缀同此） |
| **原图** | `https://i.pximg.net/img-original/img/{datePath}/{id}_p{n}.{jpg|png|gif}` | 扩展名不定（jpg 常见、透明 png、动图 gif）；pages 接口的 `original` 字段已给出确切 URL，无需猜测 |
| 动图 zip | `https://i.pximg.net/img-zip-ugoira/img/{datePath}/{id}_ugoira600x600.zip` / `_ugoira1920x1080.zip` | 帧序列打包，配合 ugoira_meta 的 frames |
| 小说封面 | `https://i.pximg.net/c/150x150_80/novel-cover-master/img/{datePath}/{specifier}_master1200.jpg`；排行接口给 `c/100x100/novel-cover-master/...` | specifier 形如 `ci{novelId}_{hash}` 或 `sci{seriesId}_{hash}` |
| 用户头像 | `https://i.pximg.net/user-profile/img/{datePath}/{userId}_{hash}_{size}.{jpg|png}` | `_50`/`_170` 等尺寸后缀 |

其中 `{datePath}` = `YYYY/MM/DD/HH/mm/ss`（投稿时间），只能从接口响应中读取，**不能自行构造**。
`{n}` 页码从 0 开始。
安全占位图（无头像等）在 `https://s.pximg.net/common/images/no_profile_s.png`。

### Referer 约束（关键）

- pixiv 图片 CDN **校验 Referer**：不带 Referer 或非 pixiv Referer 的请求返回 **403 Forbidden**。
- 实测页面图片请求头：`referer: https://www.pixiv.net/`（固定值，来源页路径不参与校验）。
- 因此**后端代理图片必须设置请求头** `Referer: https://www.pixiv.net/`；不需要 Cookie。wreq/后端代理时记得同时带上常规浏览器 UA。
- 响应带长缓存（`cache-control: max-age=31536000`），代理层可放心做磁盘缓存。

### 本项目采用的缩略图三档与改写规则（2026-10-01）

接口给的封面 URL 已经带尺寸段：列表卡片多为 `/c/540x540_70/img-master/...`，
详情页 `pages` 的 `regular` 则是 `/img-master/img/..._master1200.jpg`（不带 `/c/`）。
本项目不改写图片内容，只按设置档位在 URL 上替换 / 插入尺寸段
（前端 `frontend/src/utils/thumb.ts`）：

| 档位 | 尺寸段 | 改写方式 |
|---|---|---|
| small | `250x250_80_a2` | 路径以 `/c/<段>/` 开头 → 替换该尺寸段；以 `/img-master/img/` 开头 → 在其前插入 `/c/<段>/` |
| medium | `540x540_70` | 同上 |
| large | `600x1200_90` | 同上 |
| original | 不改写 | 原样返回 |

- 非 `/c/` 且非 `/img-master/img/` 的路径一律原样：`/img-original/`、
  `/img-zip-ugoira/`、`/user-profile/`、`/custom-thumb/` 等；头像保留接口给的
  `_50` / `_170` 尺寸后缀。
- 只在原 URL 上动 `/c/` 尺寸段，查询串与锚点不动；`{datePath}` 与文件名
  **绝不构造**（自行拼路径即 404，见 §10）。
- 详情页档位 `medium` 的语义是「接口 `regular` 原样」（不插 `/c/`），
  `540x540_70` 只作「先低清后高清」的占位层。
- 本项目默认档位：列表/网格 `medium`、详情页 `medium`、全屏 `large`；R-18 全局
  开关 `show_r18` 默认 `true`。
- 代理层成功响应统一 `Cache-Control: public, max-age=31536000, immutable`
  （CDN 长缓存的下游延伸；同 URL 内容不变，缓存命中与回源共用同一响应头）。
- **待实测**：`novel-cover` 路径使用 `540x540_70` / `600x1200_90`，以及
  `img-master` 使用 `600x1200_90`，均属本项目新增用法（官方页面未见过这两组
  组合），需实机确认返回 200 且尺寸/裁切符合预期。

### ranking.php 的 R-18 判据（2026-10-01 实测）

- `ranking.php?format=json`（插画/漫画/动图）的 `contents[]` 条目**没有顶层
  `x_restrict`**；R-18 标记在 `illust_content_type.sexual`：`0` = 一般向、
  `1` = R-18、`2` = R-18G。
- 本项目据此刻画排行榜条目的 `x_restrict`（`parse_ranking_illust`）：
  `illust_content_type.sexual` 优先，缺失时回退顶层 `x_restrict`；
  **不采用 `is_masked`**——其语义不明（是否与 R-18 等价未验证）。
  由此排行榜网格才能与其他列表一样参与全局 `show_r18` 过滤。
- 小说排行 `/ajax/ranking/novel` 的 `display_a.rank_a[]` 条目自带顶层
  `x_restrict`（§6 字段表），无需补字段。
- ugoira 榜存在 `illust_content_type` 为空数组的条目（`sexual` 无从取得；2026-10-01
  工作包实测 ugoira 榜 50 条中 3 条），这类条目 `x_restrict` 为空。项目侧按
  fail-closed 处理：关闭全局 `show_r18` 时隐藏（仅在「全部」档显示），不当作一般向放行。
- **待实测**：`daily_r18` / `weekly_r18` 榜单条目是否恒有 `sexual >= 1`
  （过滤逻辑不依赖该假设，仅影响 R-18 榜单独查看时的直观一致性）。

## 10. 风险与备注

### 登录要求

| 接口 | 匿名可用 | 备注 |
|---|---|---|
| `/ajax/illust/{id}`、`/pages`、`/ugoira_meta` | 是 | `bookmarkData` 等个人态字段为 null |
| `/ajax/novel/{id}`、系列系列接口 | 是 | 同上 |
| `/ajax/user/{id}`、`profile/all`、`illusts?ids[]`、`novels?ids[]` | 是 | `isFollowed` 等为 false |
| `/ajax/search/*`、`/ranking.php?format=json`、`/ajax/ranking/novel` | 是（基本） | `popular` 搜索板块、`male/female/original/daily_ai` 榜为 Premium 限定 |
| `/ajax/follow_latest/*`、`/ajax/discovery/artworks`、`/ajax/street/v2/main`、`/ajax/top/*` | 未验证匿名（本次会话全程登录） | 需要登录的可能性高；`street` 明确要求会话 |
| 图片 i.pximg.net | 无需 Cookie，但必须带 `Referer: https://www.pixiv.net/` | 否则 403 |

### 会话与请求头

- 请求需携带的 Cookie 名：`PHPSESSID`（登录态核心）、`device_token`、`cf_clearance` / `__cf_bm`（Cloudflare 防护）、`p_ab_id` / `p_ab_id_2` / `p_ab_d_id`（A/B 分流）、`yuid_b`、`privacy_policy_agreement`。**任何 Cookie 值不得落盘入库**（项目安全边界：登录态仅存系统凭据存储）。
- `POST` 类接口（如 street）额外要求请求头 `x-csrf-token`（32 位 hex）。实测该 token 无法从首页静态 HTML 提取（旧版 `pixiv.context.token` / `global-data` meta 均已不存在），它是 pixiv-web-next 前端在 SSR 序列化状态（`__NEXT_DATA__.props.pageProps.serverSerializedPreloadedState` 的 `api.token` 字段）里下发的。第三方客户端若必须调 POST 接口，可 GET 一次任意页面解析该 JSON 取 token；**但更稳妥的路线是全部使用本文的 GET 接口**（均无此要求）。
- Accept 头建议 `application/json`；`lang` 参数控制标签翻译语言（`zh`/`ja`/`en`...），`tagTranslation` 与 `tags[].translation` 都受其影响。

### 限速与稳定性

- pixiv 对未登录/高频访问有 Cloudflare 挑战与限流；**建议客户端对 `/ajax` 请求做串行或低并发（≤2-3）+ 间隔**，列表批量接口（`ids[]`）按 60 个/批节流。
- 推荐类接口（street/discovery/recommend）每次返回不同内容，重复调用不会触发去重惩罚，但也不必高频刷新。
- 排行榜日期有 ~2 天延迟；`date` 参数需用响应回传的 `date/prev_date` 链条回溯，不要凭"今天"猜。
- 图片扩展名（jpg/png/gif）不确定时一律用接口给的 `original` URL；自行拼 `.jpg` 会 404。
- 本调研基于 2026-10 的 pixiv-web-next；旧资料中以下端点**已失效**：`stacc.php`（404）、`/ajax/illust/{id}/ugoira`（404，改 `/ugoira_meta`）、`/ajax/illust/{id}/comments`（404，改 `/ajax/illusts/comments/roots`）、`/ajax/ranking/illust`（404，改 `ranking.php?format=json`）、`/ajax/user/{id}/profile/illusts|novels`（400，改 `/ajax/user/{id}/illusts|novels?ids[]=`）。写代码时以本文端点为准。

### 请求头与状态形态勘误（2026-10-01 晚间实机补充）

- **csrf token 的实际形态**：登录态下 `__NEXT_DATA__` 的
  `props.pageProps.serverSerializedPreloadedState` 是 **JSON 字符串**（需再
  `JSON.parse` 一次），token 在解析后的 `api.token`；匿名/部分场景可能直接是
  对象。解析须兼容两种形态（`pixiv/csrf.rs::parse_next_data_token` 已实现）。
- **street 缩略对象的封面**：插画/漫画卡片缩略**没有**顶层 `url`/`urls`，
  封面在 `pages[0].urls`，键名是尺寸字符串（`"1200x1200_standard"` /
  `"540x540"` / `"360x360"`，取 540 优先）；小说卡片缩略有顶层 `url`
  （novel-cover-master）。解析见 `browse_api.rs::parse_work_thumb` 兜底链。

### 对 pixiv-tool 的落地建议（与 SPEC 对齐）

1. 浏览类数据全部走同源 `www.pixiv.net/ajax/*` GET 接口 + Cookie（来自系统凭据存储），响应解析只需 `error/message/body` 包裹判断。
2. 图片一律走本地代理：后端请求 i.pximg.net 时固定加 `Referer: https://www.pixiv.net/`，落盘缓存（CDN 本身允许长期缓存）。
3. 列表页统一用「id 列表 + 索引表（thumbnails/users）」模型渲染；详情页用 `/ajax/illust/{id}` + `/pages`（动图加 `ugoira_meta`）、`/ajax/novel/{id}`。
4. 分页统一模型：`p`+`isLastPage`（follow_latest）、`lastPage`+`total`（search）、`last_order`（novel series）、重复调用去重（discovery/street）。

---

## 11. 收藏（Bookmark）接口（v1 实测）

> **2026-10-03 频道模式勘误**：Chrome `/cate_r18.php` 发起 `/ajax/top/illust?mode=r18&lang=zh`。同会话普通快照推荐 18/排行 100 均为一般向，r18 快照推荐 18/排行 100 均为受限；两种模式各有独立标签推荐。`mode=all` 等同普通快照，`mode=safe` 返回业务错误。漫画与小说 r18 请求亦成功。当前契约与测试映射见 `docs/PIXIV-API.md` §4.1 / §8.1。

> **2026-10-03 详情标签勘误**：插画、漫画、小说详情的 `tags` 实为 `{authorId,isLocked,tags:[{tag,...}],writable}` 对象，标签数组在 `tags.tags`，不能按顶层数组解析。三类均带 `likeCount` / `bookmarkCount` / `viewCount`；小说此前仅消费 bookmarkCount，已补全。另外描述 HTML 的换行须在转文本时保留。现行契约与回归映射见 `docs/PIXIV-API.md` §4.3 / §8.1。

> 调研方式：2026-10-01 晚对已登录会话（uid <uid>，非 Premium）先在官方收藏页/作品页**真实点击抓包**，再用页内 `fetch()` 复现验证参数边界。
> 写操作严格按「add→delete 配对」执行：插画、小说各一次私密收藏（restrict=1）+ 立即删除，实测后已确认还原（详情 `bookmarkData` 回到 null、计数复原）。
> 本节 csrf token 记录为 `ed5…f6b`（打码）；任何 Cookie 值不落盘。

### 11.0 概览

| 功能 | 接口 | 方法 | 关键说明 |
|---|---|---|---|
| 插画/漫画收藏列表 | `/ajax/user/{uid}/illusts/bookmarks` | GET | `tag/offset/limit/rest/order/mode`；有 `total`；works[] 带 `bookmarkData.id` |
| 小说收藏列表 | `/ajax/user/{uid}/novels/bookmarks` | GET | 参数同上；官方页每页 30 |
| 收藏标签（插画） | `/ajax/user/{uid}/illusts/bookmark/tags` | GET | 一次返回 public+private；**路径是 `illusts/bookmark/tags`** |
| 收藏标签（小说） | `/ajax/user/{uid}/novels/bookmark/tags` | GET | 同上 |
| 登录用户菜单 | `/ajax/user/extra` | GET | 只有 following/followers/mypixivCount/background，**无 uid、无收藏计数** |
| 添加插画收藏 | `/ajax/illusts/bookmarks/add` | POST | **JSON 体**（非 form）；需 `x-csrf-token` |
| 取消插画收藏 | `/ajax/illusts/bookmarks/delete` | POST | **form 体** `bookmark_id=`（与 add 不对称） |
| 添加小说收藏 | `/ajax/novels/bookmarks/add` | POST | JSON 体；响应 body 直接是 bookmarkId 字符串 |
| 取消小说收藏 | `/novel/bookmark_setting.php` | POST | 旧式表单（`tt`+`book_id[]`+`del=1`）；新式 ajax 端点参数形状未破解 |

### 11.1 收藏列表（自己：插画/漫画）

| 方法 | URL 模式 | 关键参数 | 实测说明 |
|---|---|---|---|
| GET | `/ajax/user/{uid}/illusts/bookmarks?tag=&offset=0&limit=48&rest=show&order=desc&mode=all&lang=zh` | 见下 | 官方插画·漫画收藏页真实请求（每页 48） |
| GET | `/ajax/user/{uid}/novels/bookmarks?tag=&offset=0&limit=30&rest=show&order=desc&mode=all&lang=zh` | 同上 | 官方小说收藏页真实请求（每页 30） |

query 参数语义（均实测）：

| 参数 | 合法值 | 说明 |
|---|---|---|
| `tag` | 标签名（URL 编码）或空 | 按收藏标签过滤；不存在的标签 → 正常响应 `total:0, works:[]` |
| `offset` | 0..total | 超界（如 1000 > total）→ `works:[]`，`total` 照常返回，无错误 |
| `limit` | 实测 10/48/100 均可 | 官方页 artworks=48、novels=30；自定义值服务端接受 |
| `rest` | `show`（公开）/ `hide`（非公开） | **自己**：缺省等价 `show`；`hide` 返回私密收藏（条数与该模式下的 `total` 一致）。**他人**：必须显式 `rest=show`（缺省报「不正确的请求。」），`rest=hide` 报「没有权限。」 |
| `order` | `desc` | **只支持 desc**（最新收藏在前）。`asc` 返回 `works:[]`（静默空列表，不报错） |
| `mode` | `all` | **只支持 all**。`illust`/`manga`/`ugoira` 均报 error「不正确的请求。」——即插画/漫画/动图混排一个列表，类型过滤靠前端 |

响应 `body` 顶层键：`works, total, zoneConfig, extraData, lastMonthAllBookmarkCount, bookmarkTags`。

| 字段 | 类型 | 说明 |
|---|---|---|
| `total` | number | **有 total**（公开与私密模式各返回各自总数）——分页总数直接可用 |
| `works` | Array[≤limit] | 作品索引表项（字段同 §2 缩略结构），**另带 `bookmarkData`** |
| `works[].bookmarkData` | Object/null | 当前查看者的收藏态：`{id:"31000000001", private:false}`——**id 即 bookmarkId**，取消收藏直接用它，无需再查详情 |
| `works[].illustType` | number | 0/1/2 混排（前 300 条实测见 0 与 2；ugoira 不拆分列表） |
| `bookmarkTags` | Array | 内联标签列表（账号未打收藏标签时为空数组；显式打了标签的收藏才会出现在这里） |
| `lastMonthAllBookmarkCount` | number | 近 30 天收藏总数（装饰性统计） |

**分页语义**：`offset` 按 works 条数推进（`next = offset + works.length`），终止条件 `offset >= total` 或 `works.length < limit`。没有 cursor/isLastPage 字段。

### 11.2 收藏标签

| 方法 | URL 模式 | 说明 |
|---|---|---|
| GET | `/ajax/user/{uid}/illusts/bookmark/tags?lang=zh` | 插画/漫画收藏标签 |
| GET | `/ajax/user/{uid}/novels/bookmark/tags?lang=zh` | 小说收藏标签 |

- **路径勘误**：旧资料流传的 `/ajax/user/{uid}/illust-bookmark-tags` 不存在（404「无法找到您所请求的页面」）；正确路径是 `illusts/bookmark/tags`（`illusts` 复数 + `/bookmark/tags` 子路径）。无 `rest` 参数。
- 响应 `body`：`{public: [{tag, cnt}], private: [{tag, cnt}], tooManyBookmark: bool, tooManyBookmarkTags: bool}`——一次返回公开与非公开两组，无需分别请求。
- 未打标签的收藏聚合在系统标签「未分類」下（实测 public「未分類」cnt 与列表 total 一致）。`cnt` 可直接做标签下拉的计数徽标。

### 11.3 `/ajax/user/extra`（登录用户菜单）

`GET /ajax/user/extra?is_smartphone=0&lang=zh` → `body: {following, followers, mypixivCount, background}`。

**修正预期**：该接口**没有** uid、头像、收藏计数（旧资料如此描述）。获取自己 uid 的可靠途径：

1. 任意 pixiv-web-next 页面 `__NEXT_DATA__` → `props.pageProps.serverSerializedPreloadedState`（JSON 字符串二次 parse）→ `userData.self.id`（同处还有 `name/pixivId/premium/xRestrict/adult` 等登录态画像）；
2. 访问 `bookmark.php`，跟随 302 后的 URL `/users/{uid}/bookmarks/artworks` 提取（见 §11.8）。

### 11.4 添加收藏（插画/漫画/动图）

| 方法 | URL | 请求头 | 请求体 |
|---|---|---|---|
| POST | `/ajax/illusts/bookmarks/add` | `x-csrf-token: {token}` + `Content-Type: application/json; charset=utf-8` + `Accept: application/json` | **JSON**：`{"illust_id":"9000021","restrict":1,"comment":"","tags":[]}` |
| POST | `/ajax/novels/bookmarks/add` | 同上 | **JSON**：`{"novel_id":"9000012","restrict":1,"comment":"","tags":[]}` |

| 参数 | 类型 | 说明 |
|---|---|---|
| `illust_id` / `novel_id` | string | 作品 id（JSON 里是字符串） |
| `restrict` | number | `0`=公开（官方心形默认）、`1`=非公开（实测用 1） |
| `comment` | string | 收藏评论（实测空串） |
| `tags` | string[] | 收藏标签数组（实测 `[]`；带标签的具体格式未验证——写操作受配对限制） |

响应（两者形状**不同**）：

```json
// 插画：body 是对象
{"error":false,"message":"","body":{"last_bookmark_id":"31000000001","stacc_status_id":null}}
// 小说：body 直接是 bookmarkId 字符串
{"error":false,"message":"","body":"3100000001"}
```

**路径勘误**：旧资料的 `/ajax/illust/{id}/bookmark/add`、`/ajax/novel/{id}/bookmark/add`（按作品分的端点）已不存在——现行 pixiv-web-next 用**全局端点** + id 进请求体。且请求体是 **JSON**（旧资料的 `application/x-www-form-urlencoded` + `illust_id=...` form 形态已过时）。

### 11.5 取消收藏（插画/漫画）

| 方法 | URL | 请求头 | 请求体 |
|---|---|---|---|
| POST | `/ajax/illusts/bookmarks/delete` | `x-csrf-token` + **`Content-Type: application/x-www-form-urlencoded`** | form：`bookmark_id={last_bookmark_id}` |

响应：`{"error":false,"message":"","body":[]}`。实测用 add 返回的 `last_bookmark_id` 删除后，详情 `bookmarkData` 回到 `null`、计数复原。

- **注意与 add 不对称**：add 是 JSON、delete 是 form。delete 若用 JSON 体 `{bookmark_ids:[...]}` 会报「不正确的请求。」（无副作用）。
- 旧式 `/ajax/illust/{id}/bookmark/delete` → 404「无法找到您所请求的页面」。

### 11.6 取消收藏（小说）——新式端点未破解，用旧式表单

`POST /ajax/novels/bookmarks/delete` 端点存在（报错与 404 不同，是「不正确的请求。」），但实测 5 种参数形状全部失败：JSON `{novel_id}` / JSON `{bookmark_ids:[...]}` / form `bookmark_id=` / form `bookmark_ids=` / form `novel_id=`。官方小说删除 UI 实际走**旧式表单**（novel/bookmark_add.php 编辑页的「取消收藏」按钮，前端有 confirm 对话框，API 本身无需）：

| 方法 | URL | 请求头 | 请求体（form-urlencoded） |
|---|---|---|---|
| POST | `/novel/bookmark_setting.php` | 无需 x-csrf-token 头；`Content-Type: application/x-www-form-urlencoded` | `tt={csrf token}&p=1&untagged=0&rest=show&book_id%5B%5D={bookmarkId}&del=1` |

- `tt` = csrf token（与 `api.token` 同值，放表单字段而非请求头）；`book_id[]` = 要删的 bookmarkId（实测单个）；`del=1` 固定。
- 成功响应：302 → `/novel/bookmark.php?rest=show&p=1` → 302 → `/users/{uid}/bookmarks/novels`（follow redirects 即可，最终页 200）。实测删除生效。
- **客户端建议**：小说取消收藏直接用此表单端点（同源带 Cookie + tt）；不要依赖 `/ajax/novels/bookmarks/delete`。

### 11.7 详情响应中的收藏态（三态实测）

`/ajax/illust/{id}` 与 `/ajax/novel/{id}` 的 `body.bookmarkData`：

| 状态 | 形状 | 实测样例 |
|---|---|---|
| 未收藏 | `null`（**字段存在，值为 null**） | `bookmarkData: null` |
| 已收藏（公开） | `{id, private:false}` | 插画 `{id:"31000000001", private:false}` |
| 已收藏（非公开） | `{id, private:true}` | 插画 `{id:"31000000002", private:true}`；小说 `{id:3100000001, private:true}` |

- **id 类型不稳定**：插画收藏的 id 是字符串，小说收藏的 id 实测出现过 number（如 `3100000001`）——解析时统一 `String(bookmarkData.id)` 再用。
- `likeData`：未点赞时为 `false`（布尔，不是 null）。
- 匿名访问时 `bookmarkData` 恒为 null（个人态字段），登录后才反映真实收藏态。

### 11.8 他人公开收藏

| 场景 | 请求 | 结果 |
|---|---|---|
| 他人插画收藏 | `GET /ajax/user/{otherUid}/illusts/bookmarks?tag=&offset=0&limit=24&rest=show&order=desc&mode=all` | 可用，形状与自己的一致（`total`/`works`），实测样例 uid 公开收藏 total=1 |
| 他人小说收藏 | `GET /ajax/user/{otherUid}/novels/bookmarks?...&rest=show` | 同上可用 |
| 他人不带 rest | 缺省 rest | 「不正确的请求。」（**必须显式 rest=show**） |
| 他人 rest=hide | 想看私密 | 「没有权限。」 |

他人列表项的 `bookmarkData` 语义 = **当前查看者**对该作品的收藏态（null 表示「我」没收藏过它），不是列表主人的收藏行为。官方对应页面 `/users/{uid}/bookmarks/artworks` 对所有人共用同一接口。

### 11.9 官方收藏页 URL 形态

| 旧 URL | 实际行为 |
|---|---|
| `https://www.pixiv.net/bookmark.php` | 302 → `/users/{uid}/bookmarks/artworks`（插画·漫画收藏页，可从中提取自己 uid） |
| `https://www.pixiv.net/novel/bookmark.php?rest=show&p=1` | 302 → `/users/{uid}/bookmarks/novels`（小说收藏页） |
| `/bookmark_add.php?type=illust&illust_id=` | 旧版收藏编辑页（未收藏时心形即链到这里） |
| `/novel/bookmark_add.php?id=`、`/novel/bookmark_detail.php?id=` | 小说收藏编辑/详情页（旧版页面，删除按钮所在） |

官方收藏页（pixiv-web-next）的筛选 UI：排序（按最新收藏排序）、公开范围（仅限公开收藏 下拉）、年龄限制、收藏标签、作品标签（Premium）、收藏时间（Premium）。新版把标签做成了筛选下拉而非侧栏。

### 11.10 顺带发现的辅助接口与请求头

- `GET /ajax/user/{uid}/bookmarks/sync_status`：收藏同步状态（官方收藏页加载时调用）。
- `GET /ajax/illusts/bookmarks/rename_tag_progress`、`GET /ajax/novels/bookmarks/rename_tag_progress`：批量改标签进度轮询。
- **`x-user-id` 请求头**：官方前端对 `/ajax/user/{uid}/...` 请求统一带 `x-user-id: {自己uid}`（响应 `vary: X-UserId` 提示服务端缓存按其区分）。实测 GET 不带也能正确返回；客户端建议统一带上以贴近官方行为。

### 11.11 契约建议（pixiv-tool 收藏页落地）

1. **分页**：offset/limit 显式传（建议 artworks 48/页、novels 30/页，与官方一致）；`total` 在 `body.total`；`next = offset + works.length`；终止 `offset >= total || works.length < limit`；offset 超界安全（空数组不报错），可直接用「滚动加载 + total 判断到底」。
2. **列表项自带收藏态**：`works[].bookmarkData.id` 即 bookmarkId、`.private` 即公开/私密——收藏页卡片「直接取消收藏」无需先请求详情；私密收藏卡片要标「非公开」徽标。
3. **add/delete 精确参数**：
   - 插画 add：全局 JSON 端点 `/ajax/illusts/bookmarks/add`（`illust_id/restrict/comment/tags`，restrict 1=私密）；返回 `last_bookmark_id`（对象）。
   - 插画 delete：`/ajax/illusts/bookmarks/delete` **form** `bookmark_id=`；返回 `body:[]`。
   - 小说 add：`/ajax/novels/bookmarks/add` JSON（`novel_id/...`）；返回 body 为 bookmarkId **字符串**。
   - 小说 delete：旧式表单 `/novel/bookmark_setting.php`（`tt/p/untagged/rest/book_id[]/del=1`），302 跳转即成功。
4. **详情收藏态**：统一判 `bookmarkData == null`（未收藏）→ `{id, private}`（已收藏）；id 做 String 归一化。
5. **标签**：用 `/ajax/user/{uid}/(illusts|novels)/bookmark/tags` 一次拿 `{public, private}` 两组；「未分類」是聚合标签名，前端需本地化显示。
6. **参数红线**：order 只用 `desc`（asc 静默空列表）、mode 只用 `all`（其余报错）、他人列表必须显式 `rest=show`。
7. **写操作安全**：所有 POST 需登录 Cookie + token（JSON 端点走 `x-csrf-token` 头，旧式表单走 `tt` 字段）；建议客户端对 add/delete 做节流（实测间隔 >2s 无任何风控提示，但高频仍有 Cloudflare 风险）。

## 12. 追更列表（Watch List，2026-10-01 实测）

> 调研方式：已登录会话打开官方 `/following/watchlist/manga`，DevTools 网络面板抓真实请求，再用页内 `fetch()` 复现 novel 变体验证形状。
> 官方入口：「关注」区三个 tab（已关注用户的作品 / **追更列表中的作品** / 好P友的作品）的第二个。追更对象是**系列**（漫画系列 / 小说系列），非单件作品；官方页为漫画/小说两个子 tab + 双列行式卡片 + 数字分页。

### 12.0 概览

| 功能 | 接口 | 方法 | 关键说明 |
|---|---|---|---|
| 漫画追更列表 | `/ajax/watch_list/manga?p={page}&lang=zh` | GET | 系列在 `body.illustSeries`；最新话封面/R-18 经 `thumbnails.illust` 二次映射 |
| 小说追更列表 | `/ajax/watch_list/novel?p={page}&lang=zh` | GET | 系列在 `body.novelSeries`，自带 `cover.urls` 与 `xRestrict` |

- **无 illust 变体**：官方追更只有漫画 / 小说两个子 tab（系列功能不覆盖插画单件）。
- **分页**：`p` 从 1 起；`body.page.maxPage` 为总页数。单页容量未实测到边界（样本 total=2/1 → maxPage=1），以 `maxPage` 为准逐页聚合即可。
- **顺序**：`body.page.watchedSeriesIds`（字符串 id 数组）即官方列表序；`illustSeries`/`novelSeries` 数组顺序实测与之一致。

### 12.1 响应形状（body 顶层）

| 字段 | 说明 |
|---|---|
| `page.total` | **字符串**数字（如 `"2"`）——订阅系列总数 |
| `page.maxPage` | number——总页数 |
| `page.watchedSeriesIds` | string[]——系列 id 顺序表 |
| `thumbnails.illust` | manga 变体：每系列**最新话**缩略项（字段同 §2 缩略结构，另带 `seriesId/seriesTitle`）；novel 变体为 `[]` |
| `thumbnails.novel` | 实测恒 `[]`（小说封面直接在 novelSeries 条目里） |
| `illustSeries` | manga 变体的系列数组；novel 变体为 `[]` |
| `novelSeries` | novel 变体的系列数组；manga 变体为 `[]` |
| `users` | **数组**形状（注意：§2 频道页的 users 是 id→对象**映射**），条目 `{userId, name, image, imageBig, ...}` |
| `zoneConfig`/`extraData`/`tagTranslation`/`requests` | 广告位与元数据，忽略 |

### 12.2 illustSeries 条目（manga）

```json
{"id":"344074","userId":"11***16","title":"曦光之心-深层洗脑恶堕","total":14,
 "firstIllustId":"146241789","latestIllustId":"149896311",
 "updateDate":"2026-09-20T20:57:00+09:00","isWatched":true,"isNotifying":false, ...}
```

- **封面 / R-18 需二次映射**：`latestIllustId` → `thumbnails.illust` 同 id 条目；封面取 `urls.240mw`（240x480 竖版，回退顶层 `url` 方图），R-18 取该条目的 `xRestrict`（illustSeries 本体**无** xRestrict）。
- **作者名/头像**：illustSeries 不带 → `users` 数组按 `userId` 取 `name` / `imageBig`（回退 `image`）。
- 话数 `total`；更新时间 `updateDate`（ISO 含时区）。

### 12.3 novelSeries 条目（novel）

```json
{"id":"10559822","userId":"45***04","userName":"Daiakko",
 "profileImageUrl":"https://i.pximg.net/user-profile/img/...170.jpg",
 "xRestrict":1,"title":"…","total":4,"latestNovelId":"20201502",
 "updateDate":"2023-09-19T12:17:05+09:00",
 "cover":{"urls":{"240mw":"…/c/240x480_80/novel-cover-master/…","480mw":"…","original":"…"}},
 "isNotifying":false, ...}
```

- 小说条目**自带**作者（`userName`/`profileImageUrl`）、`xRestrict`、`cover.urls.240mw`——无需 thumbnails/users 映射（二者仍在响应里，可做兜底）。
- 话数 `total` 与 `publishedContentCount` 同值，取 `total`。

### 12.4 与应用的映射（browse_watchlist 契约）

- 后端按 `maxPage` 聚合（上限 20 页防异常大订阅；空页提前收尾），一次输出 `BrowseWatchlist{kind, total, max_page, items[]}`，前端无需翻页。
- 条目归一：`BrowseWatchlistItem{id, kind, title, user_id, user_name, user_avatar, cover, x_restrict, total, update_date, latest_work_id}`；`latest_work_id` 取 `latestIllustId`/`latestNovelId`，供「读最新话」直达。
- `isNotifying`（官方铃铛开关）与追更/取消追更写操作 V1 不做；卡片跳转：novel → 应用内系列目录（`/browse/series/:id`），manga → 官方系列页 `https://www.pixiv.net/user/{userId}/series/{id}`。

---

## 13. 系列分集列表（illust / manga series，2026-10-01 实测）

> 调研方式：已登录会话打开官方漫画系列页 `/user/11***16/series/344074`（14 话样本），
> DevTools 网络面板抓真实加载请求，再用页内 `fetch()` 复现并实测参数边界（全部 GET 只读）。
> 顺带复核小说系列接口（§7.2）。样本：漫画系列 `344074`（total=14，R-18）、`341166`（total=3）；
> 小说系列 `10559822`（total=4，来自追更列表）。任何 Cookie / token 值不落盘。
>
> **勘误 §7.2**：`/ajax/novel/series_content/{id}` 的 `order_by` 原记「实测仅 asc 生效」不准确——
> 实测 `asc` / `desc` 均合法，**省略时默认 desc**（官方页显式传 `order_by=asc`），其他值 → 400。
> 待回填 `docs/PIXIV-API.md` 时以本节为准。

### 13.0 概览

| 功能 | 接口 | 方法 | 关键说明 |
|---|---|---|---|
| 插画/漫画系列分集列表 | `/ajax/series/{sid}?p={p}&lang=zh` | GET | **页码制**：每页恒 12 条、恒按话数降序；分集表在 `page.series[]`，条目本体经 `thumbnails.illust` 同序映射；系列元数据在 `illustSeries[0]` |
| （复核）小说系列元数据 | `/ajax/novel/series/{id}` | GET | 与 §7.2 记载一致（见 13.4） |
| （复核）小说系列内容列表 | `/ajax/novel/series_content/{id}` | GET | `last_order` 游标制复核通过；desc 合法为勘误项（见 13.4） |

- 官方页 URL：插画与漫画系列共用 `https://www.pixiv.net/user/{userId}/series/{seriesId}`；页内数字翻页器链接形如 `?p=2#seriesContents`。
- 官方页加载序列：`/ajax/series/{sid}?p=1&lang=zh` 一次拿全本页分集（**无滚动懒加载 ajax**），辅以 `rpc/notify_count.php` 等角标接口与 `/ajax/illust/{firstIllustId}`（头部「从最初开始阅读」锚点）。
- `x-user-id` 请求头可选：页内 `fetch()` 不带该头全部正常返回（与 §11.10 一致）。

### 13.1 query 参数（实测）

| 参数 | 合法值 | 说明 |
|---|---|---|
| `{sid}` | 路径段 | 系列数字 id；不存在 → HTTP 404（`error:true`，message 空串） |
| `p` | `1` .. `ceil(total/12)` | **必传**：缺省 → 400「不正确的请求。」；`p=0` → 500「例外エラーです」；`p=-1`、`p=1.5` → 400 |
| `lang` | `zh` 等 | 翻译语言（`tagTranslation` 受其影响） |
| `limit` / `last_order` / `order` / `order_by` | - | **全部被忽略**（实测与不传响应完全一致）——本端点无游标、无排序参数，恒为话数降序 |

### 13.2 响应 body 形状

顶层键：`tagTranslation, thumbnails, illustSeries, requests, users, page, extraData, zoneConfig`（id 列表 + 索引表模型，同 §2/§12）。

**`page`**（本页翻页状态）：

| 字段 | 类型 | 说明 |
|---|---|---|
| `series` | Array | **本页分集表**，每项 `{workId, order}`：`workId` 字符串作品 id、`order` 数字话数（1..total）。恒按 order **降序**（最新话在前），与 `thumbnails.illust` 同序一一对应 |
| `total` | number | 系列总话数（样本 14）；翻页终止判断依据 |
| `seriesId` | string | 系列 id 回显 |
| `isSetCover` | bool | 是否设置了系列自定义封面（两样本均 false） |
| `otherSeriesId` | string/null | 同作者其他系列 id（对应 `illustSeries[1..]`） |
| `recentUpdatedWorkIds` | Array | 实测空数组（语义未明，忽略） |
| `isWatched` / `isNotifying` | bool | 当前登录用户的追更 / 更新通知状态 |

**`illustSeries[0]`**（系列元数据）：

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` / `userId` / `title` | string | 系列 id / 作者 uid / 标题 |
| `description` / `caption` | string | 系列简介（样本为空串） |
| `total` | number | 总话数（与 `page.total` 同值） |
| `firstIllustId` / `latestIllustId` | string | 第一话 / 最新话作品 id（头部「从最初开始阅读」→ firstIllustId） |
| `createDate` / `updateDate` | string | ISO 时间（含时区）；`updateDate` = 最近更新 |
| `content_order` / `url` / `coverImageSl` / `watchCount` | null | 实测恒 null（自定义封面未设置时无值；`isSetCover=true` 时的形状未采样） |
| `isWatched` / `isNotifying` | bool | 同 `page` |

- **无 `isConcluded` 字段**（两样本实测）——pixiv 未提供漫画/插画系列完结标记，小说系列才有（§7.2）。
- `illustSeries[1..]`：同作者其他系列（样本 [1] = `341166`「奴隶志愿」total=3），侧栏「作品・系列」数据源；`page.otherSeriesId` 给出其 id。

**`thumbnails.illust[]`**（本页分集条目；字段同 §2 索引表 + 系列平铺字段）：

| 字段 | 说明 |
|---|---|
| `id` / `title` | 作品 id（字符串）/ 标题 |
| `illustType` | `0`=插画 `1`=漫画（样本系列全部为 1；0/2 混排未采样，索引表结构与 §2/§11 一致） |
| `pageCount` | 页数（UI 角标；样本 2~83） |
| `xRestrict` | `1`=R-18（UI 徽标依据）；样本系列全部 R-18，接口正常返回 |
| `seriesId` / `seriesTitle` | 所属系列**平铺字段**（不是 seriesNavData） |
| `url` | 代表封面（`250x250_80_a2`；样本多为 `custom-thumb` 自定义裁切路径，亦有 `img-master` 方图，混见） |
| `urls` | `{250x250, 360x360, 540x540, 1200x1200}` 四档（官方卡片用 `360x360`） |
| `tags` / `alt` / `description` | 标签数组 / a11y 描述 / HTML 说明（样本 description 空） |
| `width` / `height` / `aiType` / `restrict` / `sl` | 同 §2 |
| `createDate` / `updateDate` | ISO 时间 |
| `isBookmarkable` / `bookmarkData` | 收藏锚点（未收藏 null） |
| `isUnlisted` / `isMasked` / `visibilityScope` / `titleCaptionTranslation` / `profileImageUrl` | 同 §2 |

**`users[]`**：作者条目数组（`userId/name/image/imageBig/premium/isFollowed/...`，数组形状同 §12），侧栏头像/昵称/关注态来源。

### 13.3 分页语义

- **页码制**：`p` 从 1 起；**每页恒 12 条**（14 话样本：p=1 → order 14..3 共 12 条，p=2 → order 2,1 共 2 条，切片精确）。
- 总页数 = `ceil(total/12)`；响应**无** isLastPage / hasNext 类字段。
- 终止条件：`page.series` 为空数组，或已取满 `ceil(total/12)` 页。
- 超页行为：越界页（如 p=3）→ HTTP 200，`page.series: []`、`thumbnails.illust: []`，`page.total` 照常返回——**静默空页不报错**，翻页器可直接禁用前进。
- 排序：恒 order 降序（最新在前），服务端无排序参数；升序由客户端自行反转。

### 13.4 小说系列接口复核（对 §7.2）

样本 `/ajax/novel/series/10559822`（total=4）实测：

- 元数据与 §7.2 一致：`id`（字符串）、`total == publishedContentCount == displaySeriesContentCount`、`isConcluded`、`firstNovelId/latestNovelId`、`cover.urls{240mw,480mw,1200x1200,128x128,original}`、`createDate`（ISO）+ `updatedTimestamp`（unix 秒）；`maxXRestrict` 实测为 null。
- `/ajax/novel/series_content/{id}` 补充实测：
  - `series.contentOrder` **从 1 起**（1..total）；`last_order` 语义 = 「取 contentOrder **大于**该值的下 limit 条」（排他下界）：`last_order=0` → 1..N；`limit=2&last_order=2` → 3,4（步进精确）。
  - `limit` 切片生效；省略 `limit`（按官方默认 30）与省略 `last_order`（按 0）均可正常返回。
  - 超页：`last_order` 超过最大 contentOrder（如 999）→ HTTP 200 + `seriesContents: []`，不报错。
  - 不存在的系列 id → HTTP 404「…不存在」。
  - **order_by 勘误**：`asc` / `desc` 均合法；省略时默认 **desc**；其他值（`bogus`）→ 400「不正确的请求。」。
- **页码映射结论**：`last_order = (page-1)*30` **成立**（asc 语义下每批恒 30 条、游标为排他 contentOrder 下界；个别话删除造成的 contentOrder 跳号不影响「按条数取批」，仅意味着不能反向用 `(page-1)*30+1` 推断页首话的 contentOrder）。总页数 `ceil(total/30)`；终止：返回条数 < limit 或已取满总页数；next 游标 = 本批最后一条的 `series.contentOrder`。
- 条目字段与 §7.2 记载逐项一致（`id` 字符串、`series.id` 数字、`viewableType`、`contentOrder` 等）。

### 13.5 与应用的映射建议（illust 系列分集页契约草案）

1. 后端单命令 `browse_illust_series(id, page)`：串调 `/ajax/series/{id}?p={page}&lang=zh`；归一输出系列头 + 本页分集（`page.series[i].workId` → `thumbnails.illust` 同 id 条目按位映射）。
2. 字段清单（snake_case）：
   - 系列头：`id`、`title`、`user_id`、`user_name`/`user_avatar`（users[] 按 userId 映射，imageBig 优先）、`caption`、`cover`（illustSeries[0].url，null 时前端回退最新话封面）、`total`（page.total）、`is_concluded`（**无来源恒 false**）、`is_watched`、`update_date`（illustSeries[0].updateDate）。
   - 分集条目 `contents[]`：`id`、`kind`（恒 `"illust"`，illustType 0/1/2 均入此 kind、详情页再分）、`title`、`cover`（`urls.360x360` 优先，回退 `url`）、`page_count`、`x_restrict`、`update_date`、`series_order`（page.series[].order）、`ai_type`。
   - 翻页：`page` 回显、`total_pages = ceil(total/12)`、`next_page`（page < total_pages ? page+1 : null）；**不是游标**——illust 系列端点天生页码制，与小说系列的 `last_order` 不同源，前端翻页器直接 +1。
3. R-18 过滤沿用条目级 `x_restrict`（系列头无 xRestrict；与 §12 watchlist 口径一致）。
4. 入口跳转映射：watchlist `kind="manga"` 卡 → 本页 `kind="illust"`；作品详情 `seriesNavData`（seriesType "manga"）→ 本页。官方分集直达链接 = `/artworks/{workId}`，应用内映射 `browse_work_detail(workId)`。
5. 未验证项（如实记录）：匿名访问（本会话全程登录）；`isSetCover=true` 时封面字段形状；纯插画（illustType=0）系列样本（三组关键词搜索均未命中系列导航条目；结构上与 §2/§11 的 thumbnails.illust 索引表同构，无独立端点）。

## 作者关注接入补充（2026-10-03）

资料响应 isFollowed 为布尔状态，现映射至作者页 is_followed。关注/取消为旧式表单 POST，参数与响应成功信标见 PIXIV-API §4.10；证据为 [Pixiv Previewer 的原始实现](https://greasyfork.org/en/scripts/30766-pixiv-previewer/code)，本轮未进行真实账号写实测，保留带显式开关的在线往返用例用于契约复核。作者关注为 ADR 0016 授权的浏览写操作。

## 14. 评论写路径（发评论 / 回复 / 删除，2026-10-04 实测）

### 14.1 抓包方式

Chrome 登录态打开**本人作品** `/artworks/150446397`，在官方评论区输入框（`textarea[placeholder="发表评论"]`，实测 `maxlength=140`）提交一条测试评论，再点评论行内「删除」并确认，全程用 DevTools 记录请求/响应；随后在页面内比对 csrf token 来源。**未对他人作品发表或删除任何评论**；测试评论已当场删除，未留在作品上。

### 14.2 发表评论

- 请求：`POST https://www.pixiv.net/rpc/post_comment.php`
  - 头：`content-type: application/x-www-form-urlencoded; charset=utf-8`、`x-csrf-token: <主站 __NEXT_DATA__ 的 api.token>`（实测与页面内 `props.pageProps.serverSerializedPreloadedState.api.token` **逐字相等**，长度 32）/ `accept: application/json`；`referer` 为作品页（应用侧用固定 `https://www.pixiv.net/` 即可，与既有写端点一致）。
  - 体（实测原文，正文为中文）：
    ```
    type=comment&illust_id=150446397&author_user_id=117482194&comment=%E6%8E%A5%E5%8F%A3%E6%8E%A2%E6%B5%8B...
    ```
    其中 `author_user_id` = **作品作者**的 userId（本例作者与登录用户同为 117482194）；`parent_id` 只在回复时出现。
  - 响应 200：`{"error":false,"message":"","body":{"comment_id":"235373194","comment":"...","user_id":"117482194","user_name":"wllmsb","stamp_id":null,"parent_id":null}}`（`body.user_id` 为字符串；根评论 `parent_id` 为 null）。
  - 未观察到 recaptcha token 进入请求体（页面确有 recaptcha 脚本加载，但该请求体只含上述四个字段）。

### 14.3 删除评论

- 请求：`POST https://www.pixiv.net/rpc_delete_comment.php`，同 csrf 头，体 `i_id=150446397&del_id=235373194`。
- 响应 200：`{"error":false,"message":"ok","body":[]}`；响应头含 `x-userid: <登录用户 id>`。

### 14.4 小说变体与回复参数（前端 bundle 反查）

Chrome 未登录态不可用（本轮账号无小说作品），改为在当前页面拉取 pixiv 前端 chunk 并定位评论 API 模块（`https://s.pximg.net/soy/pixiv-web-next/_next/static/chunks/40783-*.js`，模块 74004），逐字摘录（压缩后原文，`qs.stringify` = form-urlencoded）：

```js
// 插画：POST /rpc/post_comment.php
{type:"comment",illust_id:`${t}`,author_user_id:`${n}`,comment:a, ...(i!==undefined?{parent_id:`${i}`}:{})}
// 小说：POST /novel/rpc/post_comment.php
{type:"comment",novel_id:`${t}`,author_user_id:`${n}`,comment:a, ...(i!==undefined?{parent_id:`${i}`}:{})}
// 表情评论（同两端点，type=stamp，带 stamp_id，无 comment）
// 删除：POST /rpc_delete_comment.php | /novel/rpc_delete_comment.php
{ i_id: t, del_id: n }
// 集合（collection）评论另有 /ajax/comments/collection/post|delete，本轮不接入
```

即：小说与插画仅**路径前缀与 id 键名**不同；回复统一用 `parent_id`（根评论不传该字段）；删除两端点参数同形。

### 14.5 未验证项（如实记录）

- 小说评论的真实请求/响应未在线实测（账号无小说作品），仅由前端 bundle 反查确认参数形状；`PIXIV_LIVE_WRITE=1` 的在线用例覆盖插画路径的发→回复→删除往返。
- 未实测 `parent_id` 指向「某条回复（二级）」时的服务端归位语义（本轮只按根评论 id 回复），故前端回复入口只挂在根评论上。
- 未实测超长 / 敏感词 / 频率限制时的错误信封文案（`error:true` 走既有 `extract_ajax_body` 通道）。
- 未接入表情贴图评论（`type=stamp`）与集合评论端点。
