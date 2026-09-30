# pixiv 网页版浏览类 API 调研（Web Ajax/RPC 接口）

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
| GET | `/ajax/illusts/comments/roots?illust_id={id}&offset=0&limit=3&lang=zh` | `offset/limit` | 评论根列表（`hasNext` 翻页） |
| GET | `/ajax/user/{uid}/illusts?ids[]=...` | `ids[]` 可重复 | 同作者其他作品批量缩略信息 |

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
示例（original）：`https://i.pximg.net/img-original/img/2026/09/28/15/06/28/150209107_p0.jpg`，页码后缀 `_p{n}` 从 0 开始。

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
| `content` | string | **全文纯文本**。多页小说用 `[newpage]` 标记分页（`[newpage]` 数量 = pageCount - 1），章节标题用 `[chapter:标题]` 标记（渲染为节标题）；正文内嵌插画用 `[pixivimage:illustId]`、外链跳转用 `[jumpurl:文字:URL]`、嵌入图片 `textEmbeddedImages` |
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

### 对 pixiv-tool 的落地建议（与 SPEC 对齐）

1. 浏览类数据全部走同源 `www.pixiv.net/ajax/*` GET 接口 + Cookie（来自系统凭据存储），响应解析只需 `error/message/body` 包裹判断。
2. 图片一律走本地代理：后端请求 i.pximg.net 时固定加 `Referer: https://www.pixiv.net/`，落盘缓存（CDN 本身允许长期缓存）。
3. 列表页统一用「id 列表 + 索引表（thumbnails/users）」模型渲染；详情页用 `/ajax/illust/{id}` + `/pages`（动图加 `ugoira_meta`）、`/ajax/novel/{id}`。
4. 分页统一模型：`p`+`isLastPage`（follow_latest）、`lastPage`+`total`（search）、`last_order`（novel series）、重复调用去重（discovery/street）。
