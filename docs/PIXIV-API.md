# pixiv 接口契约（PIXIV-API）

> 本文是本项目**所有 pixiv 接口调用的事实源**：端点、参数、登录要求、响应消费字段、分页语义、实现位置与测试映射。
> 抓包证据与字段细节档案在 `docs/research/pixiv-browse-api.md`（下称 research）；两者冲突时**以本文为准**，并把勘误回填 research。
> 相关：`docs/SPEC.md` §7（IPC 命令表）、ADR 0012（浏览模式）、`AGENTS.md` 硬约束 1。
> 行号口径：截至 2026-10-01 的 `src-tauri/src` 工作树；行号漂移时以符号名为准。

## 1. 定位与维护规则

### 1.1 铁律

改任何 pixiv 端点调用 / 参数 / 解析（`src-tauri/src/pixiv/**`、`src-tauri/src/commands/browse_api_cmds.rs`、`src-tauri/src/image_proxy.rs`），必须同批完成：

1. 更新本文对应端点小节 + §8 维护矩阵；
2. 更新 `src-tauri/tests/pixiv_api/` 对应用例（在线 `live_read.rs` / `live_write.rs`，离线 `offline_guard.rs`）；
3. 端点或分页语义变化 → 回填 `docs/research/pixiv-browse-api.md`；命令名 / 参数契约变化 → 同步 `docs/SPEC.md` §7（命令表 + §3.4 计数）。

只改代码不改文档（或反之）视为未完成。

### 1.2 如何验证

| 场景 | 命令 | 说明 |
|---|---|---|
| 离线（默认） | `./dev.ps1 test` / `bash ./dev.sh test` | `cargo test --locked`，全离线、不发网络 |
| 在线只读实测 | `./dev.ps1 test-live` / `bash ./dev.sh test-live` | `cargo test --locked --test pixiv_api -- --ignored --test-threads=1`；真实访问 pixiv，串行防限速（dev.ps1:362 / dev.sh:271） |
| 在线写端点 | 在线 + `PIXIV_LIVE_WRITE=1` | `live_write.rs` 的 add/remove 往返默认跳过 |

登录态前置（`tests/pixiv_api/common.rs:65`，三级获取）：

1. 环境变量 `PIXIV_TOOL_TEST_COOKIES`（JSON 对象 `{"PHPSESSID":"..."}` 或 Cookie 头式 `k=v; k2=v2`，仅测试进程环境变量，不落盘）；
2. 应用凭据存储的 `default` 条目（先在应用里登录一次）；
3. 仓库 `config/accounts.json` 的 active 账号条目。

三级都取不到（或没有 PHPSESSID）→ `common.rs:87` panic 中止，不会静默通过。Cookie / token 值**不得**写进本文、日志或提交物（`AGENTS.md` 硬约束 2）。在线用例以 `#[ignore]` 标注，由 `--ignored` 显式启用；写用例需再显式 `PIXIV_LIVE_WRITE=1`（`live_write.rs:18`）。

> 跑之前先停掉正在运行的 dev 应用（`./dev.ps1 dev stop`）：Windows 上 cargo 无法覆盖被占用的 `target/debug/pixiv-tool.exe`，会报 `failed to remove file ... (os error 5)`。不想停应用时可用 release 产物跑等价命令（`cargo test --release --test pixiv_api -- --ignored --test-threads=1`）。

## 2. 通用约定

### 2.1 域名

| 域 | 用途 | 代码 |
|---|---|---|
| `www.pixiv.net` | 全部 ajax 与页面接口（同源） | `BASE_URL` client.rs:41 |
| `i.pximg.net` | 图片原图 / 缩略 / ugoira zip（防盗链） | §5 |
| `s.pximg.net` | 表情贴图、默认占位图（同样走 pixiv-img 代理） | comment stamp `browse_api.rs:1421` |
| `embed.pixiv.net` | collection 卡片嵌图（V1 不消费） | — |

### 2.2 响应信封与 `error` 真值语义

- ajax 统一 `{error, message, body}`；`error` 真值判定用 Python truthiness：`null` / `false` / `0` / `""` / `[]` / `{}` 均为假（`json_truthy` client.rs:86）。
- `error` 为真值 → `PixivError::Client("API error: {message}")`；否则取 `body`，缺 `body` 键返回整个对象（`extract_ajax_body` client.rs:99，`get_json` client.rs:309）。
- **例外**：`/ajax/user/self` 是扁平结构（顶层 `userData`/`token`，无信封），body 兜底恰好原样返回（csrf.rs:97）。
- 业务失败时 HTTP 可能仍是 200，必须判 `error`（research §0）。

### 2.3 登录守卫

- 应用侧：18 个浏览命令统一经 `build_browse_api`（browse_api_cmds.rs:33）——`cookies.load()` 为 None 或 `PHPSESSID` 缺失/空 → `Err(NOT_LOGGED_IN)`（`"未登录或登录态已失效，请先登录"`，browse_api_cmds.rs:30）；前端按「登录」关键字弹登录窗，文案逐字保留。
- pixiv 侧匿名可用性见各端点小节；「需登录」只描述 pixiv 行为，应用侧浏览命令一律要 PHPSESSID。

### 2.4 请求头

| 头 | 值 / 规则 | 代码 |
|---|---|---|
| `user-agent` | Chrome147 Windows UA（与 wreq `Emulation::Chrome147` 配套） | client.rs:44,146 |
| `referer` | `https://www.pixiv.net/`（ajax 与图片下载都带） | client.rs:46,292 |
| `accept-language` | `zh-CN,zh;q=0.9,en;q=0.8` | client.rs:47 |
| `x-csrf-token` | Cookie 里的 `x-csrf-token` 拆出放 ajax 默认头；写操作另取主站 token 覆盖；下载禁用默认头，绝不携带此 token | client.rs `new` / `download_request` |
| `Cookie` | 其余 cookie 拼 `k=v; k2=v2`（排序稳定），**只在 ajax 请求**携带；图片下载不带 | client.rs:150-165,270-274 |

- 项目**不发** `x-user-id` 头（research §11.10 建议贴近官方；实测 GET 不带也正确）。
- 安全纪律：只记录头名与规则，任何 cookie / token 值不入库、不进日志、不进文档。

### 2.5 限速 / 重试 / 超时（`client.rs` 真实常量）

| 参数 | 值 | 常量 / 位置 |
|---|---|---|
| 并发 | 2（ajax 信号量） | `CONCURRENCY` client.rs:27 |
| 请求间隔 | 400ms，持有信号量期间 sleep，成功与失败路径都执行 | `REQUEST_INTERVAL_MS` client.rs:30 |
| 单请求超时 | 15s | `REQUEST_TIMEOUT_SECS` client.rs:32 |
| 最大尝试次数 | 3（首次 + 退避重试 2 次；退避表 `[1,2,4]` 秒，第 3 位不可达） | `MAX_RETRIES` client.rs:34、`RETRY_BACKOFF_SECS` client.rs:39 |
| 429 | 当次不重试；全队列暂停闸门置 60s 后自动恢复 | `PAUSE_ON_429_SECS` client.rs:36、`trigger_global_pause` client.rs:193 |
| 立即终止（不重试） | 401/403 Auth、404 NotFound、429 RateLimit | `run_gated` client.rs:225-253 |

`run_gated` 管线（client.rs:225）：取信号量许可 → 等 429 暂停闸门 → 最多 `MAX_RETRIES` 次尝试 → 无论成败在信号量持有期间 sleep 400ms。图片 CDN 走独立通道（`download_bytes_ungated` client.rs:440，不经该闸门），代理层有自己的闸门与重试（§5.3）。

### 2.6 错误分类映射（`classify_status` client.rs:74）

| HTTP 状态 | `PixivError` | 中文文案 |
|---|---|---|
| 200 | —（成功） | — |
| 401 / 403 | `Auth` | 认证失败：登录态无效或已过期 |
| 404 | `NotFound` | 资源不存在 |
| 429 | `RateLimit` | 请求过于频繁（429），已触发全队列暂停 |
| 500–599 | `Server` | Pixiv 服务端错误 |
| 其余（含其他 2xx） | `Client(msg)` | `客户端错误: HTTP {code}` / `API error: {message}` |
| 网络 / 超时 | `Network(msg)` | 仅输出请求超时或请求/响应读取失败及去查询串、锚点的地址；不格式化可能内嵌原始 URL 的 wreq 错误 |

## 3. 端点总览

登录列：匿名可读 / 需登录 / 未验证匿名（实测为登录态）/ 写操作需登录。命令列为 `#[tauri::command]` 名（无命令者为抓取管线消费）。

| 端点 | 方法 | 用途 | 登录 | 对应命令 | 代码位置 |
|---|---|---|---|---|---|
| `/ajax/street/v2/main` | POST | 首页混合推荐流（需 csrf） | 需登录 | `browse_home_feed` | browse_api.rs:1742 |
| `/ajax/top/illust / manga / novel` | GET | 频道仪表盘快照 | 未验证匿名 | `browse_channel` | browse_api.rs:1762 |
| `/ajax/watch_list/manga / novel` | GET | 追更系列列表 | 需登录 | `browse_watchlist` | browse_api.rs:1780 |
| `/ajax/discovery/artworks` | GET | 发现推荐 | 未验证匿名 | `browse_discover` | browse_api.rs:1816 |
| `/ajax/follow_latest/illust / novel` | GET | 关注的新作品 | 未验证匿名 | `browse_follow_latest` | browse_api.rs:1826 |
| `/ajax/search/artworks / novels/{word}` | GET | 插画 / 小说搜索（小说自带收藏数；已有计数透传） | 匿名基本可用 | `browse_search` | browse_api.rs:2067 |
| `/ajax/illust/{id}` · `/ajax/novel/{id}` | GET | 作品三项计数批量（搜索页本页排序补取） | 匿名可读 | `browse_work_counts` | browse_api.rs:2086 |
| `/ranking.php?format=json` | GET | 插画 / 漫画 / 动图排行 | 匿名基本可用 | `browse_ranking` | browse_api.rs:1870 |
| `/ajax/ranking/novel` | GET | 小说排行 | 匿名基本可用 | `browse_ranking` | browse_api.rs:1870 |
| `/ajax/illust/{id}` | GET | 作品主信息 | 匿名可读 | `browse_work_detail`；抓取 | browse_api.rs:1904 / api.rs:282 |
| `/ajax/illust/{id}/pages` | GET | 多页图片 URL | 匿名可读 | `browse_work_detail` | browse_api.rs:1908 |
| `/ajax/illust/{id}/ugoira_meta` | GET | 动图 zip + 帧序列 | 匿名可读 | `browse_work_detail`；抓取 | browse_api.rs:1912 / api.rs:301 |
| `/ajax/novel/{id}` | GET | 小说全文 | 匿名可读 | `browse_work_detail`；抓取 | browse_api.rs:1936 / api.rs:248 |
| `/ajax/illust / novel/{id}/recommend/init` | GET | 相关推荐（一次性池） | 匿名可读 | `browse_related` | browse_api.rs:1955 |
| `/ajax/user/{id}?full=1` | GET | 作者资料 | 匿名可读 | `browse_user_profile` | browse_api.rs:1977 |
| `/ajax/user/{id}/profile/all` | GET | 作者作品 id 全集 | 匿名可读 | `browse_user_works`；抓取 | browse_api.rs:2507 / api.rs:271 |
| `/ajax/user/{id}/illusts / novels?ids[]=` | GET | 作者作品批量缩略 | 匿名可读 | `browse_user_works` | browse_api.rs:2507（批量步 :2555；`parse_user_works_batch` :1493） |
| `/ajax/novel/series/{id}` | GET | 小说系列元数据 | 匿名可读 | `browse_novel_series` | browse_api.rs:2057 |
| `/ajax/novel/series_content/{id}` | GET | 系列目录（游标） | 匿名可读 | `browse_novel_series`；抓取 | browse_api.rs:2062 / api.rs:259 |
| `/ajax/illusts / novels/comments/roots` | GET | 评论根列表（关闭评论区恒 400 → `disabled` 信封） | 匿名可读 | `browse_work_comments` | browse_api.rs:2246 |
| `/ajax/illusts / novels/comments/replies` | GET | 评论回复列表 | 匿名可读 | `browse_comment_replies` | browse_api.rs:2539 |
| `/rpc/post_comment.php` | POST | 发表评论 / 回复 / 官方表情贴图（插画·漫画，需 csrf） | 写操作需登录 | `browse_comment_add` | `post_comment` · `post_stamp_comment` |
| `/novel/rpc/post_comment.php` | POST | 发表评论 / 回复 / 官方表情贴图（小说，需 csrf） | 写操作需登录 | `browse_comment_add` | `post_comment` · `post_stamp_comment` |
| `/rpc_delete_comment.php` · `/novel/rpc_delete_comment.php` | POST | 删除评论（**仅在线写用例清理**，无 IPC 命令） | 写操作需登录 | — | browse_api.rs:2612 |
| `/ajax/user/{uid}/illusts / novels/bookmarks` | GET | 收藏列表（自己 / 他人公开收藏） | 需登录 | `browse_bookmark_list` | browse_api.rs:2742 |
| `/ajax/user/{uid}/illusts / novels/bookmark/tags` | GET | 收藏标签 | 需登录 | `browse_bookmark_tags` | browse_api.rs:2168 |
| `/ajax/illusts / novels/bookmarks/add` | POST | 添加收藏（需 csrf） | 写操作需登录 | `browse_bookmark_add` | browse_api.rs:2188 |
| `/ajax/illusts/bookmarks/delete` | POST | 取消插画收藏（需 csrf） | 写操作需登录 | `browse_bookmark_remove` | browse_api.rs:2247 |
| `/novel/bookmark_setting.php` | POST | 取消小说收藏（旧式表单） | 写操作需登录 | `browse_bookmark_remove` | browse_api.rs:2247 |
| `/ajax/user/self` | GET | 登录态探测 / 自 uid / token | 需登录 | `auth_status` 等；浏览自 uid | csrf.rs:169 / browse_api.rs:427 / api.rs:312 |
| `https://www.pixiv.net/`（`__NEXT_DATA__`） | GET | 主站会话 csrf token | 需登录 | street / 收藏写操作前置 | csrf.rs:200 |
| `i.pximg.net` 图片 | GET | 图片 CDN（需 Referer） | 无需 Cookie | `pixiv-img` 协议（不走 invoke） | image_proxy.rs:539 / client.rs:422 |

## 4. 逐端点契约

「消费字段」只列代码真正读取的字段（字段全表见 research 对应章节）。行号格式 `文件:行`，文件均在 `src-tauri/src/`；在线用例省略目录前缀，实际路径为 `src-tauri/tests/pixiv_api/`。

### 4.1 首页 / 频道 / 发现

**POST `/ajax/street/v2/main?lang=zh`** · 实现 `pixiv/browse_api.rs:1742`（`get_home_street`）· 在线 `live_read.rs::live_home_street` · 离线 `parse_street_flattens_kinds`
- 请求体固定 `{"k":null,"vhi":null,"vhm":null,"vhn":null,"vhc":null}`；头 `x-csrf-token`（主站 `__NEXT_DATA__` 取，30min 缓存 browse_api.rs:386，Auth/Client 失败自动清缓存自愈 browse_api.rs:1753）。
- 登录：pixiv 侧明确要求会话（无 token → 400「请重新登录」）；应用侧要 PHPSESSID。
- 消费字段：`contents[].kind`（illust / manga / novel；collection 等跳过）、`contents[].thumbnails[]` 逐项走缩略解析（见 4.8 字段清单）。
- 分页：无；前端「换一批」重复调用并按 id 去重。

**GET `/ajax/top/illust / manga / novel?lang=zh[&mode=r18]`** · 实现 `browse_api.rs:1993`（`get_channel`）/ `:852`（`channel_path`）· 在线 `live_read.rs::live_channel_illust_manga_novel` · 离线 `parse_channel_assembles_sections`、`parse_channel_missing_sections_is_empty_not_error`
- 参数：kind 白名单 `illust | illustration | manga | novel`（`illustration` 归一到 `illust`）；可选 mode 白名单 `all | r18`。省略/all 不附 mode 查询参数；r18 附 `mode=r18`，等价官方 `/cate_r18.php` 的频道请求。`safe` 在 Pixiv 侧报业务错误，因此一般向使用普通快照并本地过滤。无翻页，一次性快照。
- **2026-10-03 实测**：同一 Chrome 会话普通插画快照推荐 18 条/排行 100 条均无 R-18，r18 快照分别含 18/100 条受限作品，并返回独立 `recommendByTag` 分区；标签名与候选会随请求变化，不硬编码特定标签。漫画与小说亦支持 r18。不能仅从普通快照过滤取得 R-18 推荐/排行/标签。候选偶有一般向或未知条目（小说推荐在线测试亦观察到混入），前端仍按 `x_restrict` 过滤；在线回归断言每分区含可显示的受限作品，不假定候选全为 R-18。
- 消费字段：`page.follow[]`、`page.recommend.ids[]`、`page.ranking.items[]`（**对象数组 `{id,rank}`**，非标量——2026-10-01 实测；`items_from_ids` 兼容两种形态并带回 rank）、`page.ranking.date`（见下方归一说明）、`page.newPost[]`、`page.recommendByTag[].{tag,ids[]}`（实测仅 illust 频道）、`page.trendingTags[]`（实测条目键 `{tag,ids,trendingRate}`，**无** `translatedName`/`illustCount`；译名由同响应 `tagTranslation[tag].zh`（回退 `zh_tw`）映射，`count` 无源字段恒缺席；且实测仅 illust 频道有该板块）；索引表 `thumbnails.illust / novel`（`parse_index` browse_api.rs:608）、`users{}`（`name` / `imageBig` / `image`，`users_index` browse_api.rs:624）。
- 日期归一：`page.ranking.date` 三频道两种形态（illust/manga `20260930`、novel `2026-09-30`，2026-10-01 实测）统一经 `normalize_ymd` 归一为 `yyyymmdd`——前端 `formatYmd` 只认 8 位数字，否则整段日期不显示（novel 频道曾因此丢日期）。
- 分页：无。

**GET `/ajax/discovery/artworks?mode=all&limit=60&lang=zh`** · 实现 `browse_api.rs:1816`（`get_discover`）· 在线 `live_read.rs::live_discover` · 离线 `parse_discover_maps_ids_in_order`
- 参数写死：`mode=all`（其余 400）、`limit=60`（过大 400）。
- 消费字段：`recommendedIllusts[].illustId`（顺序）、`thumbnails.illust`、`users{}`。
- 分页：无服务端翻页；前端重复调用按 id 去重追加。

### 4.2 关注动态 / 搜索 / 排行榜

**GET `/ajax/follow_latest/illust / novel?p={p}&mode={mode}&lang=zh`** · 实现 `browse_api.rs:1826`（`get_follow_latest`）· 在线 `live_read.rs::live_follow_latest_illust_novel` · 离线 `parse_follow_latest_pagination_semantics`、`parse_follow_latest_novel_uses_novel_index`
- 参数：`p` ≥ 1（`max(1)` 兜底）；`mode` 白名单 `all | safe | r18`。
- 消费字段：`page.ids[]`、`page.isLastPage`、`thumbnails.illust / novel`、`users{}`。
- 分页：`next_page = isLastPage ? null : p+1`；`isLastPage` 缺失时按每页 60 条估算（browse_api.rs:975）。

**GET `/ajax/search/artworks / novels/{word}?order=&mode=&s_mode=&type=&p=&lang=zh`** · 实现 `browse_api.rs:2067`（`get_search`）· 在线 `live_read.rs::live_search_artworks_novels` · 离线 `search_path_builds_artworks_and_novels`、`sanitize_search_word_rules`、`parse_search_artworks_and_novels`
- word 在路径段：先清理 `?` `#` 与控制字符、trim（`sanitize_search_word` browse_api.rs:1809），空 → 报错，长度 > 200 字符 → 报错；随后**必须 percent-encode**（`search_path` browse_api.rs:1825，`percent_encode` browse_api.rs:1903）——wreq 不会对路径里的非 ASCII 自动编码，直拼日文/中文标签实测 **HTTP 400**（2026-10-01 修复），编码后 200。
- 参数白名单（`search_path` browse_api.rs:1825）：`order` = `date_d`(默认) / `date` / `date_asc` / `popular_d`；`mode` = `all`(默认) / `safe` / `r18`；`s_mode` = `s_tag_full` / `s_tag` / `s_tc`（可选）；`type` = `illust` / `manga` / `ugoira`（仅 artworks，默认取 kind）；`p` ≥ 1；artworks 固定带 `ai_type=0`（排除 AI）；`lang=zh`。**每页固定 60 条**（`limit` / `per_page` 等参数实测无效，2026-10-02）。
- 消费字段：`illustManga.data[]` 或 `novel.data[]`（缩略字段见 4.8）、`total`、`lastPage`。
- **已有计数优先复用（2026-10-03 复核）**：artworks 项无 `likeCount` / `bookmarkCount` / `viewCount`；novels 项自带 `bookmarkCount`，无另外两项。`parse_work_thumb` 保留实际存在的三项字段，输出可选 `like_count` / `bookmark_count` / `view_count`；零值保留，缺失或 null 不冒充零。前端「本页排序」先使用详情计数缓存，缺失字段回退到列表；仅所选排序维度缺失的作品经 `browse_work_counts` 请求详情，切换维度复用已取三项计数。小说列表收藏数完整时无需额外详情请求。逐项请求仍走既有全局限速，单项失败跳过，未知计数垫底。
- 分页：`next_page = page < lastPage ? page+1 : null`；`last_page` 原样回传（前端页码分页换算总显示页数：接口每页 60 条切 3 个显示页 × 20 条）。

**GET `/ranking.php?format=json&mode={mode}&content={kind}&p={p}&lang=zh[&date=]`**（illust / manga / ugoira） · 实现 `browse_api.rs:1870`（`get_ranking`）· 在线 `live_read.rs::live_ranking_illust_and_novel` · 离线 `parse_ranking_illust_next_semantics`、`parse_ranking_illust_reads_r18_flag`、`validate_ranking_modes_table`
- mode 白名单（browse_api.rs:1580）：`daily / weekly / monthly / rookie / daily_r18 / weekly_r18`（Premium 限定榜不收录）；`date` 可选、必须 8 位数字 `yyyymmdd`（browse_api.rs:1676）。
- 消费字段：`contents[].{illust_id,illust_type,rank,title,user_id,user_name,profile_img,url,illust_page_count,tags[],date}`、`illust_content_type.sexual`（R-18 判据，回退顶层 `x_restrict`；`is_masked` 不用）、`next`、`date / prev_date / next_date`（均经 `normalize_ymd` 归一为 `yyyymmdd` 后出契约）。
- 分页：每页 50；`next` 为数字 → `next_page`，`false` → 到底，键缺失按 50 条估算。

**GET `/ajax/ranking/novel?mode={mode}&p={p}&format=json&lang=zh[&date=]`** · 实现 `browse_api.rs:1870` · 同上在线用例 · 离线 `parse_ranking_novel_shape_and_last_page`
- mode 白名单（browse_api.rs:1588）：`daily / weekly / monthly / male / female / daily_r18`。
- 消费字段：`display_a.rank_a[].{id,rank,title,user_id,user_name,profile_img,url,x_restrict,tag_a[],create_date,character_count,series_id,series_title,bookmark_count}`、`date`。
- 日期形态勘误：该端点 `date` 实测是**日文展示串**（`2026年9月30日`，与 ranking.php 的 `yyyymmdd` 不同形态），`prev_date`/`next_date` 恒 `null`（无日期翻页）。已由 `normalize_ymd` 统一归一为 `yyyymmdd` 出契约（2026-10-01）。
- 分页：每页 50，无 next 字段 → 满 50 条则估算有下一页。

### 4.3 作品详情（插画 / 漫画 / 动图 / 小说）

**GET `/ajax/illust/{id}`** · 实现 `browse_api.rs:1904`（`get_work_detail_illust`）；抓取 `api.rs:282`（`get_illust`）· 在线 `live_read.rs::live_illust_detail_pages_ugoira` · 离线 `parse_detail_metadata_nested_tags_and_counts`、`parse_illust_detail_fields`、`parse_illust_full_shape`、`parse_illust_anonymous_masked_meta`
- 参数：路径 id（正整数，命令层 `validate_id` browse_api_cmds.rs:59）。pixiv 匿名可读，`bookmarkData` 匿名恒 null。
- 浏览消费字段：`illustId, illustType, title, userId, userName, urls.regular, pageCount, xRestrict, tags.tags[].tag（详情为嵌套对象，列表仍兼容直接数组）, createDate, illustComment, width, height, viewCount, likeCount, bookmarkCount, bookmarkData, seriesNavData.{seriesId,title,orderNumber}`。`seriesNavData` 解析带 `seriesId > 0` 守卫：缺失 / 非数字 / 0 视为无系列，不再产出 `id = 0` 的假系列入口（离线 `parse_illust_detail_series_requires_positive_id`）。
- 抓取消费字段：`urls.original`（p0 原图）、`meta.pages[].image_urls.original`（回退条目 `original`；匿名掩码时为空，由 `core/illust_crawler.rs:22` 的 `collect_page_urls` 从 p0 直链推导 `_p0 → _pN`）、`illustType, pageCount, userId, userName, title`。
- 分页：无。

**GET `/ajax/illust/{id}/pages`** · 实现 `browse_api.rs:1908` · 在线同上用例 · 离线 `parse_illust_pages_urls_and_fallback`
- 消费字段：数组每项 `urls.small / urls.regular（→ medium）/ urls.original`（original 缺失回退 regular / small，三档皆空跳过该页）、`width, height`。
- 分页：无。

**GET `/ajax/illust/{id}/ugoira_meta`** · 实现 `browse_api.rs:1912`（仅 illustType=2 调用，失败降级 None）；抓取 `api.rs:301` · 在线同上用例 · 离线 `parse_ugoira_frames_and_missing_src`、`parse_ugoira_meta_priority_and_error`
- 浏览消费字段：`src`、`frames[].{file,delay}`；抓取消费字段：`originalSrc`（优先）、`zip_urls.original`（兜底），两者皆空 → Client 错误。
- 分页：无。注意路径是 `ugoira_meta`，不是 `/ugoira`（见 §6）。

**GET `/ajax/novel/{id}`** · 实现 `browse_api.rs:1936`（`get_work_detail_novel`）；抓取 `api.rs:248` · 在线 `live_read.rs::live_novel_detail`、`live_read.rs::live_novel_embedded_images` · 离线 `parse_detail_metadata_nested_tags_and_counts`、`parse_novel_detail_fields`、`parse_novel_full_shape`、`parse_novel_missing_series_and_defaults`、`parse_novel_embedded_images_prefers_display_tier`
- 浏览消费字段：`id, title, userId, userName, coverUrl, pageCount, xRestrict, tags.tags[].tag（原文）, createDate, description, characterCount, likeCount, bookmarkCount, viewCount, readingTime, content, bookmarkData, seriesNavData.{seriesId,title,orderNumber,next.id}`。`seriesNavData` 解析同样带 `seriesId > 0` 守卫（离线 `parse_novel_detail_series_requires_positive_id`）。
- 抓取消费字段：`title, userId, userName, pageCount, updateDate, content, seriesNavData.{seriesId,title}`。
- 分页：无；全文一次返回，多页由 `content` 内 `[newpage]` 标记，前端切分。
- **正文内嵌图**：`textEmbeddedImages` 为 `图片 id → {novelImageId, sl, urls}` 索引表（键即正文 `[uploadedimage:id]` 的 id）；`urls` 有 `1200x1200 / 128x128 / 240mw / 480mw / original` 五档，浏览取 `1200x1200`（缺失按 `original → 480mw → 240mw → 128x128` 回退），组装为契约 `embedded_images`（`id → URL`），前端经 `pixiv-img` 代理显示。实测形状与样本见 research §7.2。现行编辑器只产出 `[uploadedimage:]`；`[pixivimage:illustId]`（含 `-N` 页号）需另查 `/ajax/illust/{id}` 或 `/pages`，V1 不解析、前端走占位块。

### 4.4 相关推荐 / 作者页

**GET `/ajax/{illust / novel}/{id}/recommend/init?limit={limit}&lang=zh`** · 实现 `browse_api.rs:1955`（`get_related`）· 在线 `live_read.rs::live_related_illust_novel` · 离线 `parse_related_accepts_illusts_and_novels_keys`
- kind：`novel` → novel 段；`illust / manga / ugoira` → illust 段。`limit` 默认 30、clamp 1..30（browse_api.rs:1966）。
- 消费字段：`illusts[]` 或 `novels[]`（缩略字段见 4.8）。
- 分页：无（一次性池；`nextIds` 未接入，见 §7）。

**GET `/ajax/user/{id}?full=1&lang=zh`** · 实现 `browse_api.rs:1977`（`get_user_profile`）· 在线 `live_read.rs::live_user_profile` · 离线 `parse_user_profile_fields`
- 消费字段：`userId, name, imageBig（回退 image）, commentHtml, background, following, mypixivCount`。
- **`pixiv_id` 恒为空串**：实测（2026-10-01，他人/自己 × full=1/0 四种组合）响应**没有** `account` 键（research §8 字段表过时）——不是解析 bug，也没有替代端点可取他人 handle（`/ajax/user/self` 只给登录者自己的 `pixivId`；作品详情的 `userAccount` 可作旁路但需额外请求）。前端 `BrowseAuthorView` 在为空时隐藏 @handle 行。
- 分页：无。

**GET `/ajax/user/{id}/profile/all?sensitiveFilterMode=userSetting&lang=zh`** · 实现 `browse_api.rs:2507`（`get_user_works` 第一步）；抓取 `api.rs:271` · 在线 `live_read.rs::live_user_works_illust_novel` · 离线 `parse_profile_all_mixed_shapes`、`parse_profile_all_empty_body`
- 消费字段：`illusts{}` / `manga{}` / `novels{}`（键即 id，非法键跳过）、`novelSeries[].{id,title}`（抓取用）。
- 分页：无（一次拿全 id）。**本地排序**：`browse_user_works` 的 `order` 参数（asc/desc，缺省/非法回落 desc）只影响本端点所得 id 全集的排序与切片（先排序后切片，`slice_user_works_ids` browse_api.rs:1526），端点请求串不变。

**GET `/ajax/user/{id}/illusts / novels?ids[]={id}&ids[]={id}...&lang=zh`** · 实现 `browse_api.rs:2555`（`get_user_works` 第二步）· 在线同上用例 · 离线 `parse_user_works_batch_object_and_array_shapes`、`user_works_batch_reordered_to_requested_ids`、`user_works_batch_object_shape_keeps_request_order`
- 参数：kind `illust` / `manga` / `novel`；每批固定 60 个 id（`BATCH=60` browse_api.rs:1527），id 全集按 `order` 排序后切片；`ids[]` 重复参数。
- 消费字段：响应对象形 / 数组形条目（缩略字段见 4.8；此端点**不**回退 users 索引）。**批内条目由客户端按请求的 id 顺序重排**（`order_user_works_batch` browse_api.rs:1541）：对象形响应经 serde_json BTreeMap 按字符串键升序迭代，直接渲染会得到「正序」观感，线序不可依赖。
- 分页：`page` 即批序号（≥1）；`total` = id 全集数；`next_page = 还有剩余批 ? page+1 : null`。

### 4.5 小说系列 / 评论

**GET `/ajax/novel/series/{id}?lang=zh`** · 实现 `browse_api.rs:2057`（`get_novel_series` 第一步）· 在线 `live_read.rs::live_novel_series_detail_and_content` · 离线 `parse_novel_series_detail_cursor_semantics`
- 消费字段：`id, title, userId, userName, caption, cover, total, isConcluded`。

**GET `/ajax/novel/series_content/{id}?limit=30&last_order={n}&order_by=asc&lang=zh`** · 实现 `browse_api.rs:2062`；抓取 `api.rs:259` · 在线同上用例 · 离线 `parse_series_content_real_shape`、`parse_series_content_empty_or_missing`
- 参数：`limit=30`（写死 browse_api.rs:2053）、`last_order` 缺省 0（`max(0)`）、`order_by=asc`。
- 消费字段：`page.seriesContents[].{id,title,series.contentOrder,textLength,uploadTimestamp,xRestrict}`。
- 分页：游标 `next_last_order` = 末条 `contentOrder`；空批或条数 < limit 或已覆盖 total → null（browse_api.rs:1357）。

**GET `/ajax/illusts / novels/comments/roots?{illust_id|novel_id}={id}&offset={n}&limit=10&lang=zh`** · 实现 `browse_api.rs`（`get_work_comments`）· 在线 `live_read.rs::live_comments_roots_and_replies`、`live_comments_closed_work` · 离线 `parse_comments_next_cursor_semantics`、`parse_comment_maps_fields_and_completes_img_protocol`、`parse_comment_stamp_and_has_replies_forms`、`comments_closed_envelope_and_bad_request_gate`
- kind：`illust / manga` → illusts + `illust_id`；`novel` → novels + `novel_id`。`offset` ≥ 0，`limit=10` 写死。
- 消费字段：`comments[].{id,userId,userName,img,comment,stampId,commentDate,hasReplies,replyToUserName}`、`hasNext`。
- 分页：`next = hasNext ? offset + len : null`（无 total）。
- **关闭评论区**：作者关闭评论的作品该端点恒 **400**，body `{"error":true,"message":"不正确的请求。","body":[]}`——泛化文案、无专属标志，且详情 `/ajax/illust/{id}` 无任何评论关闭标志字段（2026-10-02 在线探针实测，research §7.1）。实现捕获 400（`PixivError::is_bad_request`，client.rs）返回 `{"comments":[],"disabled":true}` 空信封而非报错；能打开详情页的作品不存在其他已知 400 来源。

**GET `/ajax/illusts / novels/comments/replies?comment_id={id}&page={n}&lang=zh`** · 实现 `browse_api.rs:2096`（`get_comment_replies`）· 在线同上用例 · 离线 `parse_comments_next_cursor_semantics`
- 参数：`comment_id` 非空；`page` ≥ 1；无 `limit`（同官方 web）。
- 消费字段：同 roots；`hasReplies` 兼容 bool 与 `"true"/"false"` 字符串（`as_bool_loose` browse_api.rs:1390）。
- 分页：`next = hasNext ? page+1 : null`。

**POST `/rpc/post_comment.php`（插画·漫画）· POST `/novel/rpc/post_comment.php`（小说）** · 实现 `browse_api.rs`（文本 `post_comment` / 贴图 `post_stamp_comment`，公共尾部 `post_comment_form`；form 构造 `comment_add_request` · `comment_stamp_request`，响应解析 `parse_published_comment`）· 命令 `browse_comment_add` · 在线 `live_write.rs::live_comment_add_and_delete_roundtrip`（`PIXIV_LIVE_WRITE=1`，含贴图步骤）· 离线 `comment_add_request_shapes_and_encoding`、`comment_stamp_request_shapes_and_reply`、`parse_published_comment_reads_body_and_requires_id`、`offline_guard::comment_add_validates_before_login_and_requires_login` · 前端 `browse.ts` `browseCommentAdd` / `CommentEmojiPicker.vue` / `/tests/comments.html`
- 参数（form，`application/x-www-form-urlencoded`；文本评论 2026-10-04 Chrome 抓包实测、贴图 2026-10-04 由 pixiv web bundle 反查，research §14）：
  - 插画 / 漫画（manga 同走 illust 段）：`type=comment&illust_id={作品 id}&author_user_id={作品作者 userId}&comment={正文}`；
  - 小说：同形状但路径带 `/novel` 前缀、键为 `novel_id`；
  - **表情贴图**（官方表情，与文本评论同端点同鉴权）：`type=stamp&{illust_id|novel_id}&author_user_id={作品作者}&stamp_id={贴图 id}`，**不带 `comment` 字段**；`stamp_id` 为官方数字 id（网页面板可见 40 个，目录见 research §14.6）；
  - 回复他人评论：追加 `&parent_id={被回复评论 id}`（**根评论不带该字段**，实测根评论请求无 parent_id）；
  - 正文 percent-encode（`&`/`=`/空格/非 ASCII 必须编码，`percent_encode`）；`author_user_id` 是**作品作者**的用户 id（前端由详情 `user_id` 传入），不是评论者自身 id（服务端从 Cookie 取会话身份）。
- 头：`x-csrf-token` = 主站 `__NEXT_DATA__` 的 `api.token`（复用 `web_csrf_token` 进程内缓存，与 street / 收藏写同源；实测请求头值与该 token 逐字相等）。失败自愈策略同收藏写（Auth/Client → `invalidate_web_csrf`）。
- 消费字段：`body.{comment_id,user_id,user_name,stamp_id,parent_id}`；契约 `PublishedComment` 输出 `comment_id`（字符串，数字形态兼容）、`user_id`、`user_name`、`parent_id`（根评论响应为 null → 省略）、`stamp_id`（文本评论为 null → 省略）。缺 `comment_id` → `Err`（不因 HTTP 200 回显成功）。
- 入参二选一：`comment`（文本）与 `stamp_id`（贴图 id）**有且仅有一个**——都给 / 都不给 / 贴图 id 非纯数字一律中文报错，命令层与 api 层同规则。
- 长度：官方评论框 `maxlength=140`（实测）；后端 `COMMENT_MAX_CHARS` 按**字符**校验，命令层同规则前置。
- 分页：无。
- 集合评论（`/ajax/comments/collection/post`）官方 web 亦使用，本项目不接入（无合集页面）。

**正文里的官方文本表情（渲染约定，非端点）**：评论正文用 `(code)` 或 `:code:` 写官方表情（38 个，如 `(normal)`/`(happy3)`/`(heart)`），pixiv 网页把 token 渲染为 24px 行内图 `https://s.pximg.net/common/images/emoji/{id}.png`；未知 code 原样显示。目录与图片地址由 web bundle 反查（research §14.6），本约定只做客户端渲染，不新增端点。

**POST `/rpc_delete_comment.php`（插画·漫画）· POST `/novel/rpc_delete_comment.php`（小说）** · 实现 `browse_api.rs:2612`（`comment_delete`，**未暴露 IPC**）· 在线同上用例（发完即删的清理步骤）· 离线 `comment_delete_request_shapes`
- form：`i_id={作品 id}&del_id={评论 id}`，同 csrf 头；响应 `{"error":false,"message":"ok","body":[]}`。
- 用途：在线写用例在本人作品发评论后立刻删除，避免留下测试垃圾；产品侧暂无删除入口（2026-10-04 实测抓包）。

### 4.6 收藏 / 追更

**GET `/ajax/user/{uid}/illusts / novels/bookmarks?tag=&offset=&limit=&rest=&order=desc&mode=all&lang=zh`** · 实现 `browse_api.rs:2742`（`bookmark_list`）· 在线 `live_read.rs::live_bookmark_list_and_tags` · 离线 `parse_bookmark_list_total_and_next_semantics`、`bookmark_request_encoding_and_forms`
- uid：缺省由 `/ajax/user/self` 探测并 30min 缓存（`self_user_id` browse_api.rs:427，`SELF_UID_TTL` browse_api.rs:422）；IPC 可显式传 `userId` = 该作者的他人公开收藏（§11.8：路径直接用传入 uid、仅支持 rest=show，命令层拒绝 hide 与非正整数；作者页收藏 tab 用）。
- 参数：`rest` = `show`(公开) / `hide`(非公开)；`tag` 可选（percent-encode，缺省空串）；`offset` ≥ 0；`limit` 默认 illust 48 / novel 30（browse_api.rs:2062）、clamp 1..100；`order=desc`、`mode=all` 写死（asc 静默空列表，其余 mode 报错）。
- 消费字段：`works[]`（缩略字段见 4.8 + `bookmarkData.{id,private}`）、`total`。
- 分页：`next = offset + works.len()`（`works.len() < limit` 或 `offset+len >= total` 时 null）。

**GET `/ajax/user/{uid}/illusts / novels/bookmark/tags?lang=zh`** · 实现 `browse_api.rs:2168`（`bookmark_tags`）· 在线同上用例 · 离线 `parse_bookmark_tags_groups_and_empty_name_kept`
- 路径是 `bookmark/tags` 单数子路径，无 `rest` 参数；一次返回两组。
- 消费字段：`public[].{tag,cnt}`、`private[].{tag,cnt}`（空 tag 名 = 「未分类」聚合标签，前端 i18n）。
- 分页：无。

**POST `/ajax/illusts / novels/bookmarks/add`** · 实现 `browse_api.rs:2188`（`bookmark_add`）· 在线 `live_write.rs::live_bookmark_add_remove_illust_roundtrip` / `..._novel_roundtrip`（`PIXIV_LIVE_WRITE=1`）· 离线 `parse_bookmark_add_id_both_response_shapes`、`bookmark_wire_field_names_camel_case`
- 头：`x-csrf-token`（主站 token）；体 JSON：`{illust_id|novel_id: "字符串 id", restrict: 0|1, comment: "", tags: []}`。
- 消费字段：插画 `body.last_bookmark_id`；小说 `body` 即 id 字符串（`parse_bookmark_add_id` browse_api.rs:1557）；返回 `{bookmarkId}`。
- 分页：无。

**POST `/ajax/illusts/bookmarks/delete`**（form：`bookmark_id={id}`，带 `x-csrf-token`） · 实现 `browse_api.rs:2247` · 在线 `live_write.rs::live_bookmark_add_remove_illust_roundtrip` · 离线 `bookmark_request_encoding_and_forms`
- 消费字段：响应信封 `{error,body}`（`error` 真值报错），成功即忽略 body。

**POST `/novel/bookmark_setting.php`**（form：`tt={csrf}&p=1&untagged=0&rest=show&book_id[]={bookmarkId}&del=1`，**不加** csrf 头） · 实现 `browse_api.rs:2247` · 在线 `live_write.rs::live_bookmark_add_remove_novel_roundtrip` · 离线 `novel_delete_form` 相关用例
- 成功 = 302 跳转（wreq 自动跟随，`post_form` 对 2xx/3xx 放行 client.rs:378）；新式 `/ajax/novels/bookmarks/delete` 参数形状未破解，不用（§6）。

**GET `/ajax/watch_list/manga / novel?p={p}&lang=zh`** · 实现 `browse_api.rs:1780`（`get_watchlist`）· 在线 `live_read.rs::live_watchlist_manga_novel` · 离线 `parse_watchlist_manga_maps_thumbs_and_order`、`parse_watchlist_novel_uses_series_fields`、`parse_watchlist_missing_or_empty_is_not_error`
- 参数：kind `manga / novel`（无 illust 变体）；`p` 从 1；后端按 `page.maxPage` 聚合至多 20 页（`WATCHLIST_MAX_PAGES` browse_api.rs:835），空页提前收尾，前端无需翻页。
- 消费字段：`page.total`（字符串数字）、`page.maxPage`、`page.watchedSeriesIds[]`、`illustSeries[] / novelSeries[].{id,userId,userName,profileImageUrl,title,total,publishedContentCount,updateDate,latestIllustId|latestNovelId,xRestrict,cover.urls.240mw}`、`thumbnails.illust`（漫画最新话封面 `urls.240mw` 回退 `url`、`xRestrict`）、`users[]`（数组形状，`userId` → `name` / `imageBig` / `image`）。
- 分页：服务端 `p` + `maxPage`，后端一次聚合。

### 4.7 自身信息 / 会话

**GET `/ajax/user/self?lang=zh`** · 实现 `csrf.rs:169`（`fetch_session_probe`）、`browse_api.rs:427`（`self_user_id`）、`api.rs:312`（`get_user_self`）· 在线 `live_read.rs::live_self_and_csrf` · 离线 `parse_self_success`、`parse_self_anonymous_is_invalid`、`parse_self_status_classification`、`parse_self_bad_json_and_missing_token`、`self_uid_cache_roundtrip`
- 扁平结构：`userData.id` 才是登录有效的证明（匿名 self 同样 200 + token）；`userData` 缺失/非对象 → Invalid；缺 `token` → Csrf。
- 消费字段：`userData.{id,pixivId,name,profileImg,profileImgBig}`、`token`；抓取侧只用 `userData` + `token`。
- 分页：无。

**GET `https://www.pixiv.net/` → `__NEXT_DATA__`** · 实现 `csrf.rs:200`（`parse_next_data_token`）、`csrf.rs:224`（`fetch_web_csrf_token`）· 在线 `live_read.rs::live_home_street`（street 用例覆盖 token 获取；写用例亦覆盖）· 离线 `parse_next_data_token_success`、`parse_next_data_token_string_wrapped_state`、`parse_next_data_token_missing_or_broken`
- 定位 `id="__NEXT_DATA__"` script → `props.pageProps.serverSerializedPreloadedState`（**JSON 字符串需二次 parse**，对象形态兼容）→ `api.token`；失败 → `Client("无法获取会话令牌，请重新登录")`。token 值不写日志。

### 4.8 缩略条目消费字段（`parse_work_thumb` browse_api.rs:536，列表类端点共用）

`id`（缺失整条跳过）、`illustType`（0/1/2 → illust/manga/ugoira；`type` 字符串兜底；再兜底调用方 kind）、`title`（空回退 id 字符串）、`userId, userName, profileImageUrl`、封面兜底链 `url` → `urls.square` → `urls.medium` → `pages[0].urls["540x540"|"360x360"|"1200x1200_standard"|任一]`、`pageCount`、`xRestrict`、`tags[].name | .tag`、`createDate`（回退 `updateDate`）、novel 字数 `textCount | characterCount | wordCount`、`seriesId`（0 视为无）、`seriesTitle`、`bookmarkData.{id,private}`、`rank`、可选 `likeCount / bookmarkCount / viewCount`（分别透传为 snake_case，缺失省略、零值保留）。

### 4.9 离线单测索引（内嵌 `mod tests`，`./dev.ps1 test` 默认执行）

- `pixiv/client.rs`：`classify_status_branch_table`、`json_truthy_python_semantics`、`extract_ajax_body_rules`、`new_splits_csrf_and_cookie_header`、`new_without_cookies_has_no_cookie_header`、`rate_limit_sets_pause_gate`、`run_gated_retries_then_succeeds`、`run_gated_auth_aborts_immediately`、`run_gated_exhausts_retries_with_last_error`、`run_gated_rate_limit_does_not_retry`、`wait_not_paused_returns_when_resumed`
- `pixiv/csrf.rs`：`normalize_accepts_plain_value`、`normalize_accepts_prefix_and_cookie_header_forms`、`normalize_rejects_empty`、`normalize_rejects_too_long`、`normalize_rejects_control_and_separators`、`parse_self_success`、`parse_self_anonymous_is_invalid`、`parse_self_status_classification`、`parse_self_bad_json_and_missing_token`、`parse_next_data_token_success`、`parse_next_data_token_string_wrapped_state`、`parse_next_data_token_missing_or_broken`
- `pixiv/api.rs`：`parse_novel_full_shape`、`parse_novel_missing_series_and_defaults`、`parse_series_content_real_shape`、`parse_series_content_empty_or_missing`、`parse_profile_all_mixed_shapes`、`parse_profile_all_empty_body`、`parse_illust_full_shape`、`parse_illust_anonymous_masked_meta`、`parse_ugoira_meta_priority_and_error`
- `pixiv/browse_api.rs`：`parse_work_thumb_illust_full`、`parse_work_thumb_novel_shape`、`parse_work_thumb_tolerates_missing`、`parse_work_thumb_street_pages_urls`、`items_from_ids_maps_order_and_skips_missing`、`parse_street_flattens_kinds`、`parse_street_missing_contents_is_empty`、`parse_channel_assembles_sections`、`parse_channel_normalizes_novel_ranking_date`、`parse_channel_missing_sections_is_empty_not_error`、`normalize_ymd_forms`、`parse_watchlist_manga_maps_thumbs_and_order`、`parse_watchlist_novel_uses_series_fields`、`parse_watchlist_missing_or_empty_is_not_error`、`parse_discover_maps_ids_in_order`、`parse_follow_latest_pagination_semantics`、`parse_follow_latest_novel_uses_novel_index`、`parse_search_artworks_and_novels`、`parse_search_missing_container_is_empty`、`parse_search_preserves_available_counts`、`parse_ranking_illust_next_semantics`、`parse_ranking_illust_reads_r18_flag`、`parse_ranking_novel_shape_and_last_page`、`parse_ranking_dates_normalized_to_yyyymmdd`、`parse_ranking_empty_or_missing_contents`、`parse_illust_detail_fields`、`parse_illust_pages_urls_and_fallback`、`parse_ugoira_frames_and_missing_src`、`parse_novel_detail_fields`、`parse_related_accepts_illusts_and_novels_keys`、`parse_user_profile_fields`、`parse_user_works_batch_object_and_array_shapes`、`user_works_order_asc_and_desc_slice`、`user_works_order_invalid_falls_back_to_desc`、`user_works_pages_are_sorted_before_slicing`、`user_works_batch_reordered_to_requested_ids`、`user_works_batch_object_shape_keeps_request_order`、`parse_illust_detail_series_requires_positive_id`、`parse_novel_detail_series_requires_positive_id`、`parse_novel_series_detail_cursor_semantics`、`parse_comment_maps_fields_and_completes_img_protocol`、`parse_comment_stamp_and_has_replies_forms`、`parse_comment_tolerates_missing_fields`、`parse_comments_next_cursor_semantics`、`sanitize_search_word_rules`、`search_path_builds_artworks_and_novels`、`search_path_percent_encodes_non_ascii_word`、`validate_ranking_modes_table`、`web_csrf_cache_roundtrip`、`kind_of_priority_order`、`parse_tags_mixed_shapes`、`parse_bookmark_data_three_states`、`parse_bookmark_list_total_and_next_semantics`、`parse_bookmark_tags_groups_and_empty_name_kept`、`parse_bookmark_add_id_both_response_shapes`、`bookmark_request_encoding_and_forms`、`bookmark_wire_field_names_camel_case`、`parse_novel_detail_bookmark_state_numeric_id`、`self_uid_cache_roundtrip`、`comment_add_request_shapes_and_encoding`、`comment_stamp_request_shapes_and_reply`、`comment_delete_request_shapes`、`parse_published_comment_reads_body_and_requires_id`
- `image_proxy.rs`：`sha256_matches_official_vectors`、`percent_decode_rules`、`whitelist_accepts_pximg_subdomains`、`whitelist_rejects_everything_else`、`path_to_pximg_url_handles_encoded_and_raw`、`url_extension_rules`、`cache_key_is_hex_plus_extension`、`mime_table_covers_common_types`、`trim_cache_deletes_oldest_first_and_skips_keep`、`trim_cache_noop_under_limit`、`handle_rejects_non_whitelist_with_403`、`handle_serves_from_cache_without_network`、`image_response_sets_immutable_long_cache`、`should_retry_skips_terminal_errors`、`failure_mapping_splits_not_found_from_other`、`coalesce_shares_one_fetch_among_concurrent_callers`、`coalesce_propagates_failure_to_waiters`、`coalesce_different_urls_are_independent`、`coalesce_refetches_after_completion`、`coalesce_cleans_up_when_caller_cancelled`、`coalesce_keeps_entry_when_initializer_cancelled_with_waiters`
- 命令层离线冒烟：`tests/pixiv_api/offline_guard.rs`（登录守卫、参数白名单，不发网络）。


### 4.10 作者关注 / 取消关注（2026-10-03 接入）

作者资料 `/ajax/user/{id}?full=1` 消费布尔 `isFollowed` → 可选 `is_followed`；缺失保留未知，前端禁用写操作。新增 IPC `browse_user_follow(id, followed)`，正整数 ID 和登录守卫前置；公开关注，返回 `{is_followed}`。复用 `web_csrf_token` 与 `PixivClient::post_form` 的 Cookie、并发、限速与重试，不增凭据落点。

- 关注：POST `/bookmark_add.php`，form `mode=add&type=user&user_id={id}&tag=&restrict=0&format=json`，`x-csrf-token`；成功原始 JSON 为 `[]`。内部 API 的 restrict=1 仅供获准在线往返用例私密关注取样。
- 取消：POST `/rpc_group_setting.php`，form `mode=del&type=bookuser&id={id}`，同 CSRF 头；成功响应 `type=bookuser`。
- `error` 信标复用既有解析；非 JSON、意外对象、非空错误数组均失败，不因 HTTP 200 就回显成功。Auth 错误失效 CSRF 缓存。
- 来源：[Pixiv Previewer 原始客户端实现](https://greasyfork.org/en/scripts/30766-pixiv-previewer/code) 的作者关注回调（bookmark_add.php / rpc_group_setting.php）。本次未执行真实账号写操作；在线契约复核用例为 live_user_follow_roundtrip（ignore + PIXIV_LIVE_WRITE=1，取未关注非本人作者，私密关注后先尝试取消恢复再断言）。离线 user_follow_form_and_success_signals、user_follow_validates_before_login_and_requires_login；前端 /tests/author-follow.html。

## 5. 图片 CDN 与 Referer

### 5.1 URL 模式（只读接口给出的形态，**绝不自行构造** `{datePath}` 与文件名）

| 类型 | 模式 |
|---|---|
| 方形缩略 | `https://i.pximg.net/c/{尺寸段}/img-master/img/{datePath}/{id}_p{n}_square1200.jpg` |
| 大图 | `https://i.pximg.net/img-master/img/{datePath}/{id}_p{n}_master1200.jpg`（`pages` 的 regular） |
| 原图 | `https://i.pximg.net/img-original/img/{datePath}/{id}_p{n}.{jpg / png / gif}` |
| 动图 zip | `https://i.pximg.net/img-zip-ugoira/img/{datePath}/{id}_ugoira600x600.zip` / `_ugoira1920x1080.zip` |
| 小说封面 | `https://i.pximg.net/c/{尺寸段}/novel-cover-master/img/{datePath}/{specifier}_master1200.jpg` |
| 用户头像 | `https://i.pximg.net/user-profile/img/{datePath}/{userId}_{hash}_{size}.{jpg / png}` |
| 表情 / 占位 | `https://s.pximg.net/common/images/stamp/generated-stamps/{stampId}_s.jpg`、`.../no_profile_s.png` |

尺寸段改写由前端 `frontend/src/utils/thumb.ts` 负责（只替换 / 插入 `/c/` 段；`/img-original/`、`/img-zip-ugoira/`、`/user-profile/` 与 `original` 档原样），细节见 SPEC §6.4。

### 5.2 防盗链规则

- 不带 `Referer` 或非 pixiv Referer → **403**；`Referer: https://www.pixiv.net/` 固定值即可（来源路径不参与校验）。
- 不需要 Cookie；`download_bytes` / `send_download` 只接受 pximg HTTPS 白名单，下载请求禁用客户端默认头，只显式携带 UA、Referer、Accept-Language，Cookie 与 x-csrf-token 均不携带。
- CDN 响应自带长缓存（`cache-control: max-age=31536000`），代理层据此做磁盘缓存。

### 5.3 代理层（`src-tauri/src/image_proxy.rs`）

- 入口 `handle_image_request` image_proxy.rs；前端 `convertFileSrc(encodeURIComponent(url), "pixiv-img")`。
- 白名单 `is_allowed_pximg_url`（image_proxy.rs）：仅 `https` 且 host 以 `.pximg.net` 结尾，拒绝 userinfo、authority 的 percent 编码与非 443 端口（挡凭据地址、后缀欺骗与解析分歧）；路径兼容 percent-encoded 与未编码（`path_to_pximg_url`）。白名单外 → 403；抓取下载同样复用此校验。
- 缓存：键 = `SHA-256(完整 URL)` + 原扩展名（`cache_key` image_proxy.rs）；目录 `<data>/cache/img/`；上限由设置 `image_cache_max_mib` 决定（默认 512 MiB，合法区间 256~2048，`max_cache_bytes` 每次写入后实时读取，ADR 0028）。淘汰（`trim_cache`）为三档分区：按字节数划 小图 ≤1MiB（预算 60%）/ 大图 ≤8MiB（25%）/ 原图（15%），全局未超上限不动，超限只削超预算档、档内按 mtime 从旧到新删除，本次刚写入的文件受保护。
- 回源：进程级共享的无 cookie client（`cdn_client`，`send_download` 自带 Referer）；全局并发 10（`CDN_MAX_CONCURRENT_DOWNLOADS`）；同一 URL 并发冷启动单飞合并（`coalesce_download_with`）。下载完成即回传，后台串行缓存写入临时文件后原子重命名，目录扫描与淘汰用 `spawn_blocking`；最多 10 份响应在后台等待/执行缓存维护，满载时只跳过该次缓存，仍完整返回图片，避免慢磁盘下无界保留字节。写盘维护结束前在途条目保留共享字节，避免重复回源。缓存失败不影响图片响应。离线回归：`returns_before_cache_write_and_shares_until_persisted` / `saturated_cache_queue_returns_image_without_retaining_inflight_bytes`。
- 重试：`download_with_retry` image_proxy.rs——首次 + `[200,500]ms` 两次重试，总预算 15s（`DOWNLOAD_TIMEOUT_SECS`）；404 / 401 / 403 / 429 为终止态不重试（`should_retry`），CDN 的 429 立即 502、无跨请求退避。
- 响应：成功 200 + 按扩展名 Content-Type + `Cache-Control: public, max-age=31536000, immutable`（image_response）；CDN 404 → 404，其余失败 → 502（error_response）。
- 内存所有权：`download_bytes` / `download_bytes_ungated` 返回 `bytes::Bytes`，单飞等待者共享同一字节缓冲；只在 Tauri 成功响应的独占 `Cow<[u8]>` 边界转为 `Vec<u8>`。不改变 CDN 端点、请求头、响应体、错误分类或缓存策略；仍整包接收，响应收集期间的峰值未因这次减少复制而消失。

## 6. 已失效端点与勘误（截至 2026-10-01 实测）

| 旧端点 / 旧形态 | 现状 | 现行替代 | 依据 |
|---|---|---|---|
| `stacc.php`（动态页） | 404，新版导航无入口 | `/ajax/follow_latest/illust` + `/bookmark_new_illust.php`（页面） | research §0/§4 |
| `/ajax/illust/{id}/ugoira` | 404 | `/ajax/illust/{id}/ugoira_meta` | research §7.1 |
| `/ajax/illust/{id}/comments` | 404 | `/ajax/illusts/comments/roots`（`novels` 同理） | research §7.1/§10 |
| `/ajax/ranking/illust` | 404 | `/ranking.php?format=json&mode=&content=` | research §6/§10 |
| `/ajax/user/{id}/profile/illusts` / `profile/novels` | 400「不正确的请求」 | `/ajax/user/{id}/illusts / novels?ids[]=` | research §8/§10 |
| `/ajax/illust/{id}/bookmark/add`、`/ajax/novel/{id}/bookmark/add` | 不存在 | 全局端点 `/ajax/illusts / novels/bookmarks/add`（JSON 体，非 form） | research §11.4 |
| `/ajax/illust/{id}/bookmark/delete` | 404 | `/ajax/illusts/bookmarks/delete`（form `bookmark_id=`） | research §11.5 |
| `/ajax/user/{uid}/illust-bookmark-tags` | 404 | `/ajax/user/{uid}/illusts / novels/bookmark/tags` | research §11.2 |
| `/ajax/novels/bookmarks/delete` | 端点存在但 5 种参数形状全失败 | `/novel/bookmark_setting.php` 旧式表单（`tt` + `book_id[]` + `del=1`） | research §11.6 |
| 首页静态 HTML 的 `pixiv.context.token` / `global-data` meta | 不存在 | `__NEXT_DATA__.props.pageProps.serverSerializedPreloadedState` → `api.token` | research §10（勘误） |
| 小说 `totalPages` / `pageNo` 字段 | 不存在 | `pageCount` + `content` 内 `[newpage]` 切分 | research §7.2 |
| `profile/all` 的 `works` / `works_count` | 不存在 | `Object.keys(illusts / manga / novels).length` | research §8 |
| 收藏 add 旧 form 形态（`application/x-www-form-urlencoded`） | 过时 | JSON 体 + `x-csrf-token` | research §11.4 |
| `page.trendingTags[].translatedName` / `.illustCount` | 不存在（实测键仅 `{tag,ids,trendingRate}`） | 译名取自同响应 `tagTranslation[tag].zh`；`count` 无源字段，契约保留但恒缺席 | 2026-10-01 实机（`live_read.rs::live_channel_illust_manga_novel`） |
| `/ajax/user/{id}?full=1` 的 `account` | 不存在（四种组合均无） | 无替代端点取他人 handle；`pixiv_id` 恒空串，前端隐藏 @handle 行 | 2026-10-01 实机（`live_read.rs::live_user_profile`） |
| `/ajax/ranking/novel` 的 `date` | 日文展示串（`2026年9月30日`），`prev_date`/`next_date` 恒 null | 后端 `normalize_ymd` 归一为 `yyyymmdd`（`2026-09-30` 形态同理） | 2026-10-01 实机（`live_read.rs::live_ranking_illust_and_novel`） |
| 搜索 `{word}` 直拼原文 | 非 ASCII → **HTTP 400**（wreq 不自动编码路径） | `search_path` 先 `percent_encode` 再拼路径 | 2026-10-01 实机（`live_read.rs::live_search_artworks_novels`） |
| `page.ranking.items[]` 为标量 id 数组 | 实为 `{id,rank}` 对象数组，按标量解析会整块丢弃 | `items_from_ids` 兼容两种形态并带回 `rank` | 2026-10-01 实机（`live_read.rs::live_channel_illust_manga_novel`） |

## 7. 已调研但未接入的端点

以代码 grep 为准（`src-tauri/src/**` 无调用）。原因均为 V1 只读范围不需要；接入时补本文小节 + 用例。

| 端点 | 用途 | 未接入原因（V1） |
|---|---|---|
| `/ajax/user/{id}/profile/top` | 作者主页置顶 / 代表作（illusts / manga / novels / collections） | 作者页只做资料 + 作品 tab，无置顶板块 |
| `/ajax/novel/series/{id}/content_titles` | 系列全部章节轻量目录 `[{id,title,available}]` | 系列目录用 `series_content` 游标（自带标题 / 字数 / 时间），无需再调 |
| `/ajax/user/extra` | 登录用户菜单（following / followers / mypixivCount / background） | 自身信息走 `/ajax/user/self`（含 uid）；这些计数 V1 不展示 |
| `/ajax/user/{id}/novels/tags` | 作者的小说标签列表 | 作者页无标签筛选 |
| `/ajax/user/{uid}/bookmarks/sync_status` | 收藏同步状态（官方收藏页加载时调用） | 收藏列表直接读接口，无本地同步概念 |
| `/ajax/illusts / novels/bookmarks/rename_tag_progress` | 批量改收藏标签进度轮询 | V1 无改标签写操作 |
| `/ajax/street/access` | street 曝光上报 | 第三方客户端不做埋点 |
| `/rpc/index.php?mode=message_thread_unread_count`、`/rpc/notify_count.php?op=count_unread` | 通知未读角标 | V1 无通知 UI |
| `bookmark_new_illust.php`（页面） | 官方「已关注用户的新作品」 | 等价数据走 `/ajax/follow_latest/*` |
| `recommend/init` 的 `nextIds`（字段非端点） | 推荐后续候选池（实测 162 个） | 相关推荐只取首批，不续取 |

## 8. 维护矩阵与排查流程

### 8.1 维护矩阵

「代码函数」为端点发起与解析的入口；「在线用例」在 `src-tauri/tests/pixiv_api/` 下；「离线单测」为内嵌 `mod tests` 函数（§4.9 全量索引）；「前端消费点」为 `frontend/src/api/browse.ts` 封装（抓取管线消费点另注）。

| 端点 | 代码函数（文件:行） | 在线用例 | 离线单测 | 前端消费点 / 其它消费 |
|---|---|---|---|---|
| `/ajax/street/v2/main` | `get_home_street` browse_api.rs:1742；`web_csrf_token` browse_api.rs:389 | `live_read.rs::live_home_street` | `parse_street_flattens_kinds` | browse.ts:354 |
| `/ajax/top/*`（含 mode=r18） | `get_channel` / `channel_path` / `parse_channel`（browse_api.rs） | `live_read.rs::live_channel_illust_manga_novel` / `live_channel_r18_recommend_ranking_and_tags` | `channel_path_selects_server_r18_mode` / `parse_channel_assembles_sections` / `parse_channel_normalizes_novel_ranking_date` / `offline_guard::invalid_params_rejected_before_login_guard` | `browseChannel`（browse.ts）/ `BrowseChannelView`；前端回归 `/tests/channel.html` |
| `/ajax/watch_list/*` | `get_watchlist` browse_api.rs:1780；`parse_watchlist` :842 | `live_read.rs::live_watchlist_manga_novel` | `parse_watchlist_manga_maps_thumbs_and_order` | browse.ts:524 |
| `/ajax/discovery/artworks` | `get_discover` browse_api.rs:1816；`parse_discover` :943 | `live_read.rs::live_discover` | `parse_discover_maps_ids_in_order` | browse.ts:366 |
| `/ajax/follow_latest/*` | `get_follow_latest` browse_api.rs:1826；`parse_follow_latest` :961 | `live_read.rs::live_follow_latest_illust_novel` | `parse_follow_latest_pagination_semantics` | browse.ts:372 |
| `/ajax/search/artworks / novels/{word}` | `get_search`；`search_path`；`parse_search` / `parse_work_thumb` | `live_read.rs::live_search_artworks_novels` / `live_search_count_fields` | `search_path_builds_artworks_and_novels` / `search_path_percent_encodes_non_ascii_word` / `parse_search_artworks_and_novels` / `parse_search_preserves_available_counts` | browse.ts；`frontend/tests/search-counts.html`（计数复用、零值、切维度、缓存、缺失项补取） |
| `/ajax/illust/{id}` · `/ajax/novel/{id}`（计数批量） | `get_work_counts` browse_api.rs:2086；`parse_work_counts` :693；命令 `browse_work_counts_impl` browse_api_cmds.rs:368 | —（沿用详情在线用例 `live_illust_detail_pages_ugoira` / `live_novel_detail`） | `parse_work_counts_reads_three_fields` / `offline_guard::not_logged_in_blocks_all_commands_with_login_error` / `invalid_params_rejected_before_login_guard` | browse.ts:471（`browseWorkCounts`） |
| `/ranking.php?format=json` | `get_ranking` browse_api.rs:1870；`parse_ranking_illust` :1027 | `live_read.rs::live_ranking_illust_and_novel` | `parse_ranking_illust_next_semantics` / `parse_ranking_dates_normalized_to_yyyymmdd` | browse.ts:401 |
| `/ajax/ranking/novel` | `get_ranking` browse_api.rs:1870；`parse_ranking_novel` :1088 | `live_read.rs::live_ranking_illust_and_novel` | `parse_ranking_novel_shape_and_last_page` / `parse_ranking_dates_normalized_to_yyyymmdd` | browse.ts:401 |
| `/ajax/illust/{id}` | `get_work_detail_illust` browse_api.rs:1904；`get_illust` api.rs:282 | `live_read.rs::live_illust_detail_pages_ugoira` / `live_detail_metadata_illust_manga_novel` | `parse_detail_metadata_nested_tags_and_counts` / `parse_illust_detail_fields` / `parse_illust_full_shape` | browse.ts:412；core/sources.rs:106、core/crawler.rs:475,615 |
| `/ajax/illust/{id}/pages` | browse_api.rs:1908；`parse_illust_pages` :1186 | `live_read.rs::live_illust_detail_pages_ugoira` | `parse_illust_pages_urls_and_fallback` | browse.ts:412 |
| `/ajax/illust/{id}/ugoira_meta` | browse_api.rs:1912；`parse_ugoira` :1215；`get_ugoira_meta` api.rs:301 | `live_read.rs::live_illust_detail_pages_ugoira` | `parse_ugoira_frames_and_missing_src` / `parse_ugoira_meta_priority_and_error` | core/crawler.rs:633 |
| `/ajax/novel/{id}` | `get_work_detail_novel` browse_api.rs:1936；`get_novel` api.rs:248 | `live_read.rs::live_novel_detail` / `live_detail_metadata_illust_manga_novel` | `parse_detail_metadata_nested_tags_and_counts` / `parse_novel_detail_fields` / `parse_novel_full_shape` | browse.ts:412；core/crawler.rs:370 |
| `/ajax/illust / novel/{id}/recommend/init` | `get_related` browse_api.rs:1955；`parse_related` :1275 | `live_read.rs::live_related_illust_novel` | `parse_related_accepts_illusts_and_novels_keys` | browse.ts:421 |
| `/ajax/user/{id}?full=1` | `get_user_profile` browse_api.rs:1977；`parse_user_profile` :1291 | `live_read.rs::live_user_profile` | `parse_user_profile_fields` | browse.ts:431 |
| `/ajax/user/{id}/profile/all` | browse_api.rs:2507（`get_user_works`）；`get_user_profile_all` api.rs:271 | `live_read.rs::live_user_works_illust_novel` | `parse_profile_all_mixed_shapes` | browse.ts:595（`browseUserWorks`）；core/sources.rs:55,86,99 |
| `/ajax/user/{id}/illusts / novels?ids[]=` | browse_api.rs:2555（批量步）；`parse_user_works_batch` :1493 | `live_read.rs::live_user_works_illust_novel` | `parse_user_works_batch_object_and_array_shapes` / `user_works_batch_reordered_to_requested_ids` / `user_works_batch_object_shape_keeps_request_order` | browse.ts:595（`browseUserWorks`）；本地 `order` 参数只影响排序/切片（端点请求串不变），批内条目由客户端按请求 id 顺序重排（对象形响应按键迭代会得到升序，不可依赖线序） |
| `/ajax/novel/series/{id}` | browse_api.rs:2057；`parse_novel_series_detail` :1349 | `live_read.rs::live_novel_series_detail_and_content` | `parse_novel_series_detail_cursor_semantics` | browse.ts:447 |
| `/ajax/novel/series_content/{id}` | browse_api.rs:2062；`parse_series_contents` :1321；`get_series_content` api.rs:259 | `live_read.rs::live_novel_series_detail_and_content` | `parse_series_content_real_shape` | browse.ts:447；core/sources.rs:48,60 |
| `/ajax/illusts / novels/comments/roots` | `get_work_comments` browse_api.rs:2246；`parse_comments_roots` :1607 | `live_read.rs::live_comments_roots_and_replies`、`live_comments_closed_work` | `parse_comments_next_cursor_semantics`、`comments_closed_envelope_and_bad_request_gate` | browse.ts:453 |
| `/ajax/illusts / novels/comments/replies` | `get_comment_replies` browse_api.rs:2539；`parse_comments_replies` | `live_read.rs::live_comments_roots_and_replies` | `parse_comments_next_cursor_semantics` | browse.ts（`browseCommentReplies`） |
| `/rpc/post_comment.php` · `/novel/rpc/post_comment.php` | `post_comment` / `post_stamp_comment` browse_api.rs；`comment_add_request` · `comment_stamp_request`；`parse_published_comment` | `live_write.rs::live_comment_add_and_delete_roundtrip`（`PIXIV_LIVE_WRITE=1`） | `comment_add_request_shapes_and_encoding` / `comment_stamp_request_shapes_and_reply` / `parse_published_comment_reads_body_and_requires_id` / `offline_guard::comment_add_validates_before_login_and_requires_login` | `browseCommentAdd`（browse.ts）/ `CommentsSection.vue` / `CommentEmojiPicker.vue`；前端回归 `/tests/comments.html` |
| `/rpc_delete_comment.php` · `/novel/rpc_delete_comment.php` | `comment_delete` browse_api.rs:2612；`comment_delete_request` :1760 | `live_write.rs::live_comment_add_and_delete_roundtrip`（清理步骤） | `comment_delete_request_shapes` | —（无 IPC，仅供在线用例清理） |
| `/ajax/user/{uid}/illusts / novels/bookmarks` | `bookmark_list` browse_api.rs:2742；`bookmark_list_path` :2080 | `live_read.rs::live_bookmark_list_and_tags` | `parse_bookmark_list_total_and_next_semantics` / `offline_guard::invalid_params_rejected_before_login_guard` | browse.ts:675（`browseBookmarkList`）/ `BrowseAuthorView.vue` |
| `/ajax/user/{uid}/illusts / novels/bookmark/tags` | `bookmark_tags` browse_api.rs:2168；`parse_bookmark_tags` :1545 | `live_read.rs::live_bookmark_list_and_tags` | `parse_bookmark_tags_groups_and_empty_name_kept` | browse.ts:495 |
| `/ajax/illusts / novels/bookmarks/add` | `bookmark_add` browse_api.rs:2188；`parse_bookmark_add_id` :1557 | `live_write.rs::live_bookmark_add_remove_illust_roundtrip` / `..._novel_roundtrip` | `parse_bookmark_add_id_both_response_shapes` | browse.ts:501 |
| `/ajax/illusts/bookmarks/delete` | `bookmark_remove` browse_api.rs:2247；`illust_delete_form` :1719 | `live_write.rs::live_bookmark_add_remove_illust_roundtrip` | `bookmark_request_encoding_and_forms` | browse.ts:512 |
| `/novel/bookmark_setting.php` | `bookmark_remove` browse_api.rs:2247；`novel_delete_form` :1726 | `live_write.rs::live_bookmark_add_remove_novel_roundtrip` | `bookmark_request_encoding_and_forms` | browse.ts:512 |
| `/ajax/user/self` | `fetch_session_probe` csrf.rs:169；`self_user_id` browse_api.rs:427；`get_user_self` api.rs:312 | `live_read.rs::live_self_and_csrf` | `parse_self_success` / `self_uid_cache_roundtrip` | 登录流程；browse_api.rs:2155,2175 |
| `https://www.pixiv.net/`（`__NEXT_DATA__`） | `fetch_web_csrf_token` csrf.rs:224；`parse_next_data_token` :200 | `live_read.rs::live_home_street`（+ `live_write.rs` 两用例） | `parse_next_data_token_string_wrapped_state` | street / bookmark 写操作前置 |
| `i.pximg.net` 图片 | `handle_image_request`（image_proxy.rs）；`download_bytes` / `download_request`（client.rs） | `live_read.rs::live_image_download_with_referer` | `whitelist_accepts_pximg_subdomains` / `handle_serves_from_cache_without_network` / `download_request_does_not_forward_credentials` / `network_error_does_not_expose_query_or_fragment` / `saturated_cache_queue_returns_image_without_retaining_inflight_bytes` / `offline_guard::image_download_rejects_untrusted_hosts_without_network` | `pixiv-img` 协议（全前端 `<img>`） |
| `/bookmark_add.php` / `/rpc_group_setting.php` | `set_user_follow` / `user_follow_request` / `validate_user_follow_response` | `live_write.rs::live_user_follow_roundtrip`（双重开关） | `user_follow_form_and_success_signals` / `offline_guard::user_follow_validates_before_login_and_requires_login` | `BrowseAuthorView.vue` / `browseUserFollow` |

### 8.2 pixiv 侧变了怎么定位（5 步）

1. `./dev.ps1 test-live` 跑在线套件，看哪个端点的用例红（串行执行，避免误伤限速）。
2. 按 §8.1 矩阵跳到代码函数（发起 + 解析），确认失败发生在请求、状态码还是解析。
3. 用 research 文档对应章节核对响应字段与分页语义；必要时在浏览器 DevTools 复调一次端点（保持低频率）。
4. 修 `src-tauri/src/pixiv/**` 的调用或解析（保持字段级容错：能兜底就兜底，不能兜底才报 Client 错误）。
5. 同批更新：本文对应小节 + §8.1 矩阵 + `tests/pixiv_api/` 用例；端点失效 / 参数语义变化回填 research，命令契约变化同步 SPEC §7。

## 变更记录

| 日期 | 变更 | 依据 |
|---|---|---|
| 2026-10-01 | 初版：定位与维护规则、通用约定、28 行端点总览、逐端点契约、图片 CDN、勘误表、未接入清单、维护矩阵与 5 步排查流程 | 代码实测（`src-tauri/src/pixiv/**`、`commands/browse_api_cmds.rs`、`image_proxy.rs`）+ `docs/research/pixiv-browse-api.md`（2026-10-01 抓包） |
| 2026-10-01 晚 | 在线套件（`tests/pixiv_api/`，17 只读用例）实机复核后勘误 + 修复 4 处：①搜索词 percent-encode（非 ASCII 原为 400）②频道 `ranking.items` 对象形态（原整块为空）③热门标签译名改取 `tagTranslation`（`translatedName`/`illustCount` 不存在）④排行榜日期归一 `normalize_ymd`（novel 原为日文串/`2026-09-30`）；`/ajax/user/{id}` 无 `account` 记为契约事实（前端隐藏空 @handle）；全部行号按修复后工作树重校 | `./dev.ps1 test-live` 全绿（20 用例：17 只读 + 2 写跳过 + 探针已删）；离线 `parse_channel_*` / `search_path_*` / `normalize_ymd_*` 用例 |
| 2026-10-02 | 关闭评论区语义：关闭评论的作品 roots 恒 400（body 仅泛化「不正确的请求。」、无专属标志；详情亦无关闭标志字段）；`get_work_comments` 捕获 400（新增 `PixivError::is_bad_request`）映射为 `{"comments":[],"disabled":true}` 空信封而非报错，前端 `CommentsSection` 显示「作者已关闭评论区」终态；评论端点两行行号按当前工作树重校 | 在线探针实测（illust 150326647）；离线 `comments_closed_envelope_and_bad_request_gate` + 在线 `live_read.rs::live_comments_closed_work` |
| 2026-10-02 | 搜索列表契约补充：**列表项不含点赞/收藏/浏览计数**（artworks 全无、novels 仅 `bookmarkCount`）、**每页固定 60 条**（`limit` 等参数无效）、`last_page` 原样回传；新增 `browse_work_counts`（详情端点逐项补取三项计数，ids ≤ 60、单项失败跳过、全局限速排队）供搜索页「本页排序」；搜索相关行号按当前工作树重校 | 浏览器实测（pixiv 登录态：搜索响应字段、`limit`/`per_page` 无效、官方搜索页卡片收藏数逐项请求详情证实）+ 离线 `parse_work_counts_reads_three_fields`、`offline_guard` 计数批量用例 |
| 2026-10-03 | 频道 R-18 使用独立服务端 mode=r18；修复普通快照本地过滤导致推荐/排行/标签为空，频道网格覆盖全局档并拒收迟到响应；榜单与标签导航继承内容档 | Chrome 同账号接口对照；在线 live_channel_r18_recommend_ranking_and_tags；离线路径/参数守卫；前端 /tests/channel.html |
| 2026-10-03 | 三类详情标签修正为 tags.tags[].tag（兼容列表数组）；小说补 likeCount/viewCount、简介与三项计数展示；描述转纯文本保留换行，标签原生链接按类型精确搜索 | Chrome 三类详情实测；离线 parse_detail_metadata_nested_tags_and_counts；在线 live_detail_metadata_illust_manga_novel；前端 /tests/detail.html |
| 2026-10-03 | 搜索已有计数透传，小说收藏排序零额外详情请求；按排序维度补取缺失项，零值有效、维度切换复用缓存 | Chrome 与 Rust 原始搜索字段覆盖实测；parse_search_preserves_available_counts；live_search_count_fields；frontend/tests/search-counts.html |
| 2026-10-04 | 评论写路径接入：发根评论 / 回复他人评论 / **官方表情贴图**（插画·漫画 `POST /rpc/post_comment.php`、小说 `POST /novel/rpc/post_comment.php`；文本 `type=comment&…&comment=`，贴图 `type=stamp&…&stamp_id=` 不带 comment，两者二选一；csrf 复用主站 token，正文 ≤140 字）；官方文本表情 38 个 `(code)` 与贴图 40 个目录内置前端（pixiv 同为 bundle 内置，无接口），正文 `(code)`/`:code:` 渲染为 24px 行内图；另有 `comment_delete`（`i_id&del_id`）仅供在线写用例发完即删，**不暴露 IPC** | Chrome 同账号抓包实测（文本评论提交与删除的请求/响应原样记录）+ pixiv web bundle 反查（贴图 `type=stamp` 参数、`hidden` 分组不渲染、emoji 目录与图片地址；research §14.4 §14.6）+ 离线 `comment_add_request_shapes_and_encoding` / `comment_stamp_request_shapes_and_reply` / `parse_published_comment_reads_body_and_requires_id` + 在线 `live_write.rs::live_comment_add_and_delete_roundtrip`（含贴图步骤）；前端回归 `frontend/tests/comments.html` |
| 2026-10-06 | `browse_user_works` 新增本地 `order` 参数（asc/desc，缺省/非法回落 desc）与批内重排：先排序后切片（`slice_user_works_ids`），批内条目按请求 id 顺序重排（`order_user_works_batch`，对象形响应 BTreeMap 键升序迭代、线序不可依赖）——两条端点请求串逐字不变，仅本地排序语义 | 离线 `user_works_order_asc_and_desc_slice` / `user_works_order_invalid_falls_back_to_desc` / `user_works_pages_are_sorted_before_slicing` / `user_works_batch_reordered_to_requested_ids` / `user_works_batch_object_shape_keeps_request_order`；在线 `live_user_works_illust_novel` 追加 asc/desc 对照断言（test-live） |
| 2026-10-06 | `parse_illust_detail` / `parse_novel_detail` 新增 `seriesId > 0` 守卫：seriesNavData 缺失 / 非数字 / id 为 0 视为无系列，不再产出 `id = 0` 的假系列入口（解析行为变化，非端点变更） | 离线 `parse_illust_detail_series_requires_positive_id` / `parse_novel_detail_series_requires_positive_id` |
