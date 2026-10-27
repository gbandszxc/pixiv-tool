# ADR 0039：ugoira 应用内播放与多 P 按页下载

- 状态：已接受（细化 [ADR 0012](0012-browse-mode-own-ui.md) 的浏览边界与
  [ADR 0007](0007-illustration-crawling.md) 的抓取粒度；取代 ADR 0012 中「ugoira 只显示
  封面帧」的 V1 限制，并为多页作品引入按页下载）
- 日期：2026-10-27
- 关联 ADR：[ADR 0007](0007-illustration-crawling.md)、
  [ADR 0012](0012-browse-mode-own-ui.md)、
  [ADR 0016](0016-author-follow.md)、
  [ADR 0028](0028-image-cache-limit-and-partitioned-eviction.md)
- 规格：[SPEC](../SPEC.md) §3.4 / §5.1 / §6.4 / §7；接口契约见
  [PIXIV-API](../PIXIV-API.md) §4.3 / §8.1

## 背景

浏览模式落地后留下三处明确的体验缺口：

1. **动图只能跳转 Pixiv**。`ugoira_meta` 早已被详情命令读取，但前端只把封面帧当普通
   插画展示，说明行让用户「去 Pixiv 查看动画」。ugoira 的全部信息（帧文件表与逐帧
   `delay`）本来就在同一个响应里，缺的是把 zip 变成可播放的帧序列。
2. **多 P 作品只能整包下载**。抓取侧以作品为单位：命中 `illustrations` 记录就整条 skip，
   否则下载全部页。用户只想补第 3 页或只存其中两页时，只能重下整包并覆盖已有文件。
3. **设置页的输出目录选择器从未生效**。`@tauri-apps/plugin-dialog` 在 Cargo 与
   capability 里都声明过，但 Builder 上漏了 `.plugin(tauri_plugin_dialog::init())`，
   于是「浏览…」必然 reject，前端只能弹「无法打开目录选择器」。

## 决策

### 1. 帧管线：`src-tauri/src/ugoira.rs`

- `/ajax/illust/{id}/ugoira_meta` 返回 `src`（600×600 zip）、`originalSrc`（1920×1080 zip）
  与 `frames: [{file, delay}]`（delay 毫秒）。播放侧取 **`src` 优先**（体积远小于
  originalSrc），缺失回退 `originalSrc`，再回退 `zip_urls.original`；抓取侧仍用
  `originalSrc`，两条路径互不影响。
- 缓存目录 `<data>/cache/img/ugoira/<id>/`：`manifest.json` 记录 zip URL 与帧表。
  命中条件 = zip URL 一致且帧文件齐全，命中即直接复用，不再下载。
- 解包只写 frames 白名单里的条目，且条目名必须是**单段文件名**：拒绝 `..`、`a/b`、
  盘符、控制字符与 Windows 非法字符，杜绝路径穿越写出缓存目录外。新 zip 解包前清理
  旧帧文件，避免上一版残留混入本轮序列。
- 帧目录最多保留 **12 个作品**（按 mtime 保留最新），维护只在新解包后触发——刚写入的
  目录 mtime 最新，不会被自己清掉；维护失败只 `log::warn`，不影响本次播放。
- 模块不依赖 `AppState`：缓存根目录由调用方传入，解析 / 帧名净化 / URL 构造 / 解包
  全部可离线单测。

### 2. 自定义协议 `pixiv-ugoira`

- `lib.rs` 注册协议：Windows 为 `http://pixiv-ugoira.localhost/<id>/<file>`，其它平台为
  `pixiv-ugoira://localhost/<id>/<file>`（与 `pixiv-img` / `pixiv-avatar` 同形态）。
- 读 `<data>/cache/img/ugoira/<id>/<file>`；路径第 1 段必须是正整数 id，文件名走与写入侧
  **同一个** `sanitize_frame_name`，两侧规则不允许漂移。违规 400、缺失 404，一律
  `Cache-Control: no-store`（帧文件随 zip 版本变化，不能沿用图片代理的 immutable 长缓存）。
- CSP `img-src` 增加 `pixiv-ugoira:` 与 `http://pixiv-ugoira.localhost`。
- 新命令 `browse_ugoira_frames(artworkId)` 返回
  `{frames:[{src,delay}], width:null, height:null}`：`src` 即协议 URL，尺寸由前端按
  `naturalWidth` 读取，后端不解码图片。登录守卫与其它浏览命令一致。

### 3. 播放：前端帧轮播

- `ImageViewer` 用单张 `<img>` 换 `src`，逐帧预载，按当前帧 `delay` 用 `setTimeout`
  递归调度下一帧（下限 20ms，后端给 0 时不会变成死循环），跳过尚未就绪的帧。
- 舞台右下角 32px 胶囊：播放/暂停 + 「第 N / M 帧」。`prefers-reduced-motion` 初始暂停；
  KeepAlive 停用清计时器；R-18 遮罩期间停播。
- 无帧 / 加载中 / 失败退回封面帧 + 说明行：加载中给旋转图标，失败给重试并保留
  「去 Pixiv 查看动画」兜底文案——降级路径始终可读，不出现空舞台。

### 4. 按页下载：schema、参数与差集语义

- **schema**：`illustrations.saved_pages TEXT NOT NULL DEFAULT '[]'`（0 起页码，与
  `saved_paths` 同序一一对应）、`tasks.pages TEXT NOT NULL DEFAULT '[]'`（任务选中的
  页码集合，重试复制）。两者都由 `Db::open` 走 `PRAGMA table_info` + `ALTER TABLE`
  补齐，旧库升级不重建表。
- **参数**：`task_create` 新增可选 `pages: number[]`，仅在 `category=illustration` 且
  `sourceType=single` 生效；空数组 / 含负数 → `{"error":"页码必须是非负整数"}`；入库前
  去重升序；其它组合忽略该参数。
- **差集语义**：抓取侧 `IllustCrawlArgs.pages` 为 `None` 时保持旧行为（已有记录仍
  skip，向后兼容）；为 `Some` 时只下缺的页，并与既有 `saved_paths` / `saved_pages`
  合并（按页码升序、一一对应）。选中页码全部不可用（越界 / 空 URL）报错而不写空记录。
  ugoira 恒 `[0]`，不参与按页语义。
- **查询**：`illust_saved_pages(artworkId)` → `{pages:[0,2], ugoira:false}`；无记录 →
  `{"pages":[],"ugoira":false}`；ugoira 有记录 → `{"pages":[0],"ugoira":true}`。
- **前端**：`PageDownloadPicker`（`components/download/`）在**下载侧栏**渲染，作品详情页
  不再出现——出现条件为面板「插画」页签 + 来源「单张」+ 解析出的多页且非 ugoira 作品。
  组件只做受控选择（方形缩略图网格 + 已下载标记 + 选中态 2px primary 描边 + 实心圆勾，
  不只靠颜色），选中集合经 `v-model` 交给面板，由面板的下载主按钮统一下发
  `task_create` 带 `pages`；自身不建任务、不查已下载。页面数据两条来源：详情页「返填
  表单」预置已加载的 `pages`（零额外请求），或面板按 350ms 防抖自取 `browse_work_detail`
  （失败静默、不渲染选择器）。新作品默认全选，任务侧按页差集只下缺的页、已下载页跳过，
  手动清空后不再自动选上；插画页签按钮文案统一「下载」措辞：部分选择「下载所选 N 项」，
  全选 / 清空及无选择器形态「下载全部」（清空后禁用；同日文案统一调整，初版为
  「开始抓取」/「下载所选 N 页」）。布局随侧栏落位：面板 `container-type: inline-size`，
  右侧 480px 落位单列（选择器在表单下方），底部整宽落位在 `@container (min-width: 640px)`
  且含选择器时改两栏。
  ugoira 整块不渲染。选择器位置：下载侧栏（2026-10-27 调整，初版计划曾放详情页右栏）。
- **详情页 pid 复制**：`CopyWorkId` 在右栏信息区显示 `pid` + 数字，点击复制并 snackbar
  确认——补上「拿到作品 id 去别处搜索 / 分享」这条最常见的动作。

### 5. 插件注册

`lib.rs` 补 `.plugin(tauri_plugin_dialog::init())`。目录选择仍不走 IPC 命令（前端直接调
`open({directory: true})`），本次只是让早已声明的能力真正可用。

## 后果

- **缓存有上限**：帧目录最多保留 12 个作品，超出按 mtime 淘汰；重看更早的动图会重新
  下载 zip。这是刻意的取舍——帧文件数量多且体积不参与 `image_cache_max_mib` 的三档
  分区淘汰，若不单独设上限会无限增长。
- **ugoira 没有按页语义**：动图是单文件（zip），`saved_pages` 恒 `[0]`，
  `PageDownloadPicker` 对 ugoira 整块不渲染（下载侧栏也只在非 ugoira 的多页作品上出现
  选择器），`task_create` 的 `pages` 对 ugoira 无效。
- **旧行靠推断**：`saved_pages` 列新增前写入的行按 `saved_paths` 长度推断连续页码
  （ugoira 推断为 `[0]`），只在读取出口归一、**不写回**；推断结果对历史上「部分页下载」
  不存在的场景是准确的，但无法还原真实页序（当时也没记录）。
- **重试复制 `pages`**：失败项重试沿用原任务的页码集合，不会退化成整包下载。
- **文件名仍按全局页序**：部分页下载的文件名模板照旧以作品内真实页序渲染 `{page}`，
  因此只下第 3 页会得到 `..._p2.jpg` 而非 `_p0`——页码与 pixiv 页序保持一致，便于与
  网页端对照，也避免同一作品不同批次产生重名。
- **协议不进通用图片缓存**：`pixiv-ugoira` 响应 `no-store`，帧文件的生命周期完全由
  `ugoira.rs` 的目录维护决定，不被 `pixiv-img` 的淘汰策略波及。
- **残余风险**：`ugoira_meta` 的 `src` 缺失时回退 `originalSrc` 会让单次下载体积显著变大；
  解包安全性依赖 frames 白名单与文件名净化的双重约束，两者改动必须同步。

## 替代方案

- **内嵌 webview 播放 pixiv 的 gif**：需要重新引入内嵌浏览器（ADR 0013 已移除），且
  pixiv 并不直接提供 gif，仍要解 zip；还会把 Cookie 暴露给第三方页面。
- **前端直接解 zip**：需要引入 JS 解压库并把整包 zip 读进内存，帧名安全校验散落在
  前端、无法与后端写入侧共用同一份净化规则，且大 zip 的解析会阻塞主线程。
- **全量重下覆盖**：多 P 补页时重下整包最简单，但会重复下载已有页、覆盖用户可能已经
  移动/重命名过的文件，也与「只补缺的页」的诉求相反。
