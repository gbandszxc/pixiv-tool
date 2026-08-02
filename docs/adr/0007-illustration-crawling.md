# ADR 0007 · 插画抓取：IllustSource + IllustCrawler + 目录布局

**状态**：已接受 · **日期**：2026-08-02 · **关联 SPEC**：§4.2、§4.4、§5.1

## 背景

V1 聚焦小说抓取。用户要求新增插画抓取（单作品 / 多页作品 / 用户全集），
任务状态复用任务页并加小说/插画筛选，输出目录改为 novel/ 与 pic/ 分域。

## 浏览器实测结论（2026-08-02，登录态 Chrome）

1. **单图作品**：`GET /ajax/illust/{id}` → `body.urls.original` 即原图直链
   （`i.pximg.net/img-original/.../{id}_p0.{ext}`）；部分作品 URL 带 hash 段
   （如 `147635069-499627f2..._p0.png`），必须原样使用，不可字符串拼接。
2. **多图作品**：pixiv 已改版，**登录态响应也不再返回 `meta.pages`**——
   各页 URL 由客户端从 p0 推导（`_p0.{ext}` → `_p{N}.{ext}`）。实测 p1..p5
   推导直链全部 200。实现保留 meta 优先、推导兜底，两者兼容。
3. **用户全集**：`GET /ajax/user/{id}/profile/all` → `body.illusts` + `body.manga`
   （`{id: null}` 映射）。需登录态，匿名被 noLoginData 掩码。
4. **ugoira 动图**：`GET /ajax/illust/{id}/ugoira_meta` →
   `body.originalSrc`（原始尺寸 zip，如 `_ugoira1920x1080.zip`）；`src` 是
   展示用缩放 zip。原图下载取 `originalSrc`。
5. **图片下载**：`i.pximg.net` 防盗链要求 `Referer: https://www.pixiv.net/`，
   实测匿名加 Referer 即可 200；R-18 等仍需要登录态。

## 决策

**平行于小说模型，不复用**（变与不变不同）：

- `IllustSource`（ABC）：`SingleIllustSource` / `UserIllustsSource`
- `IllustCrawler`：消费 artwork id 流，与 `Crawler` 相同的并发/限速/重试/
  429 暂停/SSE 进度机制（共用 `PixivClient._request`），按页下载原图。
- `tasks` 表新增 `category` 列（`novel`/`illustration`，旧库 ALTER 迁移），
  任务页按分类筛选；`illustrations` 表独立于 `novels`（ID 空间不同）。

## 目录布局（用户要求）

```
{output_dir}/
├── novel/                        # 小说（原为输出目录根，现迁入 novel/）
└── pic/                          # 插画
    ├── {title}_{id}_p{N}.{ext}   # 单作品（多页 p0..pN-1，扁平）
    └── users/
        └── {作者}_{userId}/      # 用户全集
```

## 后果

**正面**：与小说抓取完全平行，限速/重试/SSE 复用；新增来源只需加 Source 子类。
**负面**：两套 Crawler 有少量相似编排代码（可接受，避免强行抽象）。
**风险**：pixiv 若恢复 meta.pages 或改 ugoira 字段，`illust_crawler` 的
兜底链（meta → 推导 / originalSrc → zip_urls.original）可平滑兼容。
