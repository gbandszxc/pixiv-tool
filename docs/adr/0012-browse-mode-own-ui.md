# ADR 0012 — 浏览模式：左侧栏自有 UI 代理 pixiv 接口（内嵌浏览器保留）

日期：2026-10-01 · 状态：已接受（V1 只读已实施）

> 状态补充（2026-10-01）：决策「内嵌浏览器保留（`/pixiv`）」已被
> [ADR 0013](0013-remove-embedded-browser.md) 推翻——`/pixiv` 内嵌浏览器已整体移除，
> 自有浏览 UI 为唯一入口；本文其余决策（接口代理 / 图片代理 `pixiv-img` /
> 限速继承 / 测试策略）仍有效。

## 背景

V1.1 起应用提供 `/pixiv` 内嵌浏览器（子 WebView，ADR 0008/0009 体系）直接访问 pixiv
主站。实测资源消耗较重（完整站点前端 + 轮询），且无法与本地工具（历史、任务）联动。
用户计划「先保留内嵌浏览器，左侧栏追加自有 UI，借助登录态调用 pixiv 接口，逐步转向
第三方客户端」。

## 决策

### 1. 自有浏览 UI（只读 V1）

主 WebView 内新增「浏览」导航组：首页 / 插画 / 漫画 / 小说 / 发现 / 动态 / 搜索 /
排行榜 8 个列表页 + 作品查看器（illust/manga/ugoira 封面）/ 小说阅读器 / 系列目录 /
作者页。数据一律经 Rust 侧既有 `PixivClient`（wreq Chrome147 指纹 + 2 并发 + 0.4s
间隔 + 429 闸门，见 SPEC §4.3）请求 `www.pixiv.net/ajax/*` 同源 GET 接口，复用
Keychain 登录态。**V1 不做写操作**（点赞/收藏/关注/评论），接口清单以实测调研为准：
`docs/research/pixiv-browse-api.md`（2026-10-01 抓包；含失效端点勘误，如
`/ajax/ranking/illust` 不存在、`stacc.php` 已下线；现行契约事实源为
`docs/PIXIV-API.md`）。

### 2. 图片代理协议 `pixiv-img`

i.pximg.net 校验 `Referer: https://www.pixiv.net/`（缺省 403），WebView 内 `<img>`
无法直连。新增自定义 URI scheme：

- 前端 `convertFileSrc(pximgUrl, "pixiv-img")` —— **传原始 URL，不要预编码**：
  `convertFileSrc` 在 Windows 侧会做一次 `encodeURIComponent`，后端按「单次编码」
  解码；预编码会造成双重编码、白名单解析失败（pixiv-img 403，修复见 commit 9063b46）；
- 后端 `register_asynchronous_uri_scheme_protocol`：白名单（https + `*.pximg.net`）
  → 磁盘缓存命中回读（`<data>/cache/img/<sha256>.<ext>`，上限 1GB 按 mtime 淘汰）
  → 未命中经进程级共享的无 cookie `PixivClient` + `download_bytes_ungated`
  （自带 Referer、不占 ajax 限速与 400ms 间隔）下载落盘回传；同一 URL 的并发
  冷启动合并为一次在途下载；
- CDN 下载**不走 ajax 限速器**（图片非接口请求），独立 `Semaphore(10)` 闸门；
- 单个逻辑下载（含全部重试与退避）受 15s 总预算约束（`DOWNLOAD_TIMEOUT_SECS`）：
  首次失败后按 200/500ms 短退避，最多 3 次尝试；404 / 401 / 429 为终止态不重试，
  CDN 侧 429 无跨请求退避、立即回落 502；
- 响应带 `Content-Type` 与 `Cache-Control: public, max-age=31536000, immutable`
  （pximg 路径内容寻址、同 URL 内容不变，命中与回源共用同一响应头）。

### 3. 首页 street 的 csrf token 策略

登录后首页混合流 `POST /ajax/street/v2/main` 需 `x-csrf-token`，token 无法从静态
HTML 常规位置提取，藏在 `__NEXT_DATA__` 的
`props.pageProps.serverSerializedPreloadedState`。实测（2026-10-01）该节点是
**JSON 字符串**，需再 parse 一次才拿到 `api.token`（历史/部分场景直接下发对象，
两种都兼容）。实现为：GET 主站 HTML 解析该字段
（`pixiv/csrf.rs::fetch_web_csrf_token`），进程内 TTL 缓存 30 分钟位于
`pixiv/browse_api.rs::web_csrf_token`（`WEB_CSRF_TTL`），Auth/Client 失败时自动
失效自愈。其余浏览接口全部为 GET，无此依赖。

### 4. 分页范式映射

pixiv 五种分页范式统一收敛为契约 `next_page`/`is_last_page`/`next_last_order` 语义
（对照表见 `docs/research/pixiv-browse-api.md` §落地建议）：follow_latest 用
`p`+`isLastPage`；搜索用 `total`+`lastPage`；系列目录用 `last_order` 游标；发现/首页
street 无翻页参数（前端重复调用 + 按 id 去重）；相关推荐为一次性池（`page` 参数无效）。

### 5. 前端 mock 层

`frontend/src/api/browse.ts` 在非 Tauri 环境（`!isTauri()`）返回确定性样例数据，
使浏览 UI 可在纯浏览器中开发与视觉验收；生产 Tauri 环境不受影响。

## 后果

- 内嵌浏览器保留原样（`/pixiv`）；浏览页的打开动作走系统默认浏览器（`tauri-plugin-opener`），
  不依赖 `/pixiv`；浏览菜单为内嵌浏览器（计划后续版本移除）的替代方向。
  （2026-10-01 追加：过渡已完成——内嵌浏览器已随 [ADR 0013](0013-remove-embedded-browser.md) 整体移除。）
- 限速继承既有保守参数（R2 风险不放大）；图片 CDN 并发独立放宽至 10 并对同一 URL
  在途合并，缓存落盘 `data/cache/img/`（该目录已在 gitignore 的 data/cache/ 规则内）。
- V1 限制：ugoira 只显示封面帧；小说内嵌图（`[pixivimage:]`）显示占位块；
  写操作、评论浏览、收藏夹浏览留待 V2。
- 测试：全部接口解析为离线单测（内嵌样例 JSON），命令层离线冒烟
  （`tests/pixiv_api/offline_guard.rs`），真实链路由人工/实机冒烟覆盖。
  （2026-10-01 追加：真实链路已有 `test-live` 在线用例覆盖——`./dev.ps1 test-live`
  / `bash ./dev.sh test-live`，需本机登录态、串行执行、真实访问 pixiv。）
