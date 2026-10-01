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
