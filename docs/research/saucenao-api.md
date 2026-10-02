# SauceNAO 搜索 API 调研（saucenao.com 以图识图）

> **定位说明**：本文是 2026-10-02 的调研证据档案（含参数、响应结构与当日匿名实测）；
> **现行「以图识图」命令契约事实源是 `docs/SPEC.md` §7**，两者冲突以后者为准，
> 勘误请回填 `docs/SPEC.md`，不要只改本文。

> **调研方式与证据口径**：2026-10-02 用真实 Chrome（saucenao.com 同源上下文）对
> `search.php` 与官方公开页（`status.html`、`tools/examples/api/index_details.txt`）做匿名实测；
> 因**当前账号体系下匿名不能调用 JSON API**（见 §1.3），成功响应的完整 JSON 结构无法当日实测，
> 该部分引用社区维护客户端库（PicImageSearch / nomnoms12 saucenao_api / pysaucenao /
> InsanusMokrassar SauceNaoAPI / ilfey SauceNao.js）及其测试套件中**记录的 2020 年真实响应**，
> 每处均标注来源。标记约定：**【实测】**= 2026-10-02 匿名实测；**【社区记录】**= 第三方库/文档记载（未实测）；
> **【未验证】**= 完全无证据，仅备注。
> 本文不含任何 API key（本次全部匿名测试，本来也没有 key）。

## 0. 关键结论速览（给实现者）

| 项 | 结论 | 证据 |
|---|---|---|
| 端点 | `https://saucenao.com/search.php`，GET（`url=`）/ POST multipart（`file` 字段） | 【实测】两通道均通 |
| 匿名调 JSON API | **不可用**：HTTP 403 + `{"header":{"status":-1,"message":"The anonymous account type does not permit API usage."}}` | 【实测】 |
| 因此 | 应用集成**必须**引导用户注册账号取 api_key，无 key 时功能禁用 | 【实测】+【社区记录】 |
| pixiv 数据库判定 | `header.index_id`：**5 = pixiv（现行）**、**6 = pixivhistorical（旧图）**；`index_name` 仅展示用（形如 `Index #5: Pixiv Images - 4933944_s.jpg`） | 【实测·官方 index_details.txt】+【社区记录】 |
| 取 pid / 作者 id | `data.pixiv_id` / `data.member_id`（number），辅以 `data.member_name`、`data.title`、`data.ext_urls`；解析回退：从 `ext_urls` 正则提 `illust_id=(\d+)` | 【社区记录】+【实测·HTML 佐证】 |
| `pixiv_type` / `illust_type` | **无证据**：GitHub 全量代码检索无 `"pixiv_type"` 字面量，各客户端库均未引用；不要依赖，作品类型到应用内经作品详情接口确认 | 【未验证】 |
| 缩略图域 | 全部是 saucenao 自有缓存域（`img1/img3.saucenao.com`）的**签名临时 URL**（`?auth=…&exp=…`），**不是 i.pximg.net**；匿名网页版直接给 `blocked.gif` | 【实测】+【社区记录】 |
| 限额 | 双窗口：每 30 秒（short）+ 每 24 小时（long），剩余量在响应 `header.short_remaining / long_remaining`；注册免费账号社区一致数字 **150 次/天、4 次/30 秒**；数字随年份变过（2020 曾为 100/天），**以响应 header 为准，不要硬编码** | 【社区记录】 |
| similarity 类型 | `results[].header.similarity` 是**字符串**（如 `"93.5"`），解析需显式转数值 | 【社区记录】（两个独立来源一致） |
| Cloudflare | 无浏览器导航铺垫的纯 `fetch()` 首请求会被 CF 挑战（403 "Just a moment..."）；过一次挑战后同源请求正常。Rust wreq（Chrome147 指纹）预期可过，**未实测** | 【实测】+【未验证】 |

---

## 1. 端点与调用方式

### 1.1 基本信息

| 项 | 值 |
|---|---|
| 端点 | `https://saucenao.com/search.php` |
| 方法 | GET（传 `url=` 图片地址）或 POST `multipart/form-data`（`file` 字段传文件本体） |
| 鉴权 | query 参数 `api_key`（GET/POST 均放 query，POST 时参数也在 URL 上、body 只放文件） |
| 输出格式 | `output_type`：`0`=HTML（默认）、`1`=XML、`2`=JSON（本项目只用 2） |
| HTTPS | 强制；无官方版本化，参数变更无兼容承诺（见 §11） |

### 1.2 参数表

| 参数 | 取值域 | 默认 | 说明 | 证据 |
|---|---|---|---|---|
| `url` | http(s) 图片直链 | - | URL 搜索通道；`url` 与 `file` 二选一 | 【实测】 |
| `file` | multipart 文件 | - | 文件上传通道，字段名 `file`；Kotlin 社区库按 gif/jpeg/png/svg 后缀发送 | 【实测】通道可用；格式清单【社区记录】 |
| `api_key` | 注册后在用户页获取 | 无 | 无效或缺省按匿名处理（返回 §8 的 -1 错误） | 【实测】 |
| `output_type` | 0 / 1 / 2 | 0 | `1` 实测返回 200 + 纯文本 `XML not yet implemented.`（XML 从未实现） | 【实测】 |
| `numres` | 1–40 | 库默认约 6（各客户端默认 5–6 不一） | 返回结果条数；上限 40 为社区文档（PicImageSearch）记载 | 【社区记录】；上限值【未验证】 |
| `db` | 0–44 / 999 | 999 | 指定单一索引；`999` = 全部（官方注明 999 组合索引不含 #15 Shutterstock） | 【实测·官方 status.html】 |
| `dbs[]` | 索引号，可重复多次 | - | 多索引搜索；首页表单即用 `dbs[]`；与 `db` 同时给时社区库以 `dbs[]` 优先 | 【实测·首页表单】；优先级【社区记录】 |
| `dbmask` | 位掩码（各索引按位或） | - | 启用指定索引；官方 `index_details.txt` 给出掩码表（§2）；官方原文：`dbmask=8191` 覆盖前 14 个索引 | 【实测·官方文件】 |
| `dbmaski` | 位掩码 | - | 反向掩码：启用**未**被置位的索引，适合「只排除某几个」 | 【实测·官方文件】 |
| `minsim` | 0–100（百分数） | 30 | 最低相似度阈值 | 【社区记录】；HTML 通道实测行为存疑（§3.3） |
| `hide` | 0 / 1 / 2 / 3 | 0 | 内容过滤：0 全显 / 1 隐藏 explicit / 2 隐藏 questionable / 3 只显安全 | 【社区记录】 |
| `testmode` | 0 / 1 | 0 | 社区文档描述为「干跑、不消耗配额」 | 【社区记录】【未验证】 |
| `dedupe` | 0 / 1 / 2 | - | 去重级别（社区库定义 0/1/2 三档） | 【社区记录】【未验证】 |

> 站内官方参数文档在 `https://saucenao.com/user.php?page=search-api`，**登录后可见**（【实测】匿名访问跳登录页）。
> 上表 `dbmask/dbmaski/dbs[]/minsim/hide` 的语义由官方 `index_details.txt`（掩码部分）与社区库共同印证，
> 逐值边界（如 numres 上限、dedupe 分档语义）注册后请在官方页核对一次。

### 1.3 匿名可用性（重要实测结论）

| 通道 | 匿名行为（2026-10-02） |
|---|---|
| `output_type=2`（JSON API），GET 或 POST | **一律 HTTP 403** + `{"header":{"status":-1,"message":"The anonymous account type does not permit API usage."}}`，参数校验不执行（`numres=0`、坏 `url`、无效 `api_key` 都先撞这一条） |
| `output_type=1`（XML） | HTTP 200 + `text/html`，正文 `XML not yet implemented.` |
| 不带 `output_type`（网页 HTML 搜索，GET `url=` 或 POST `file`） | **可用**，HTTP 200 返回完整结果页（§3.3、§4.3 为实测样例） |

含义：SauceNAO 把「API」定义为 `output_type=2`（以及未实现的 XML）；网页 HTML 搜索不受此限制。
2020 年的真实响应 fixture（nomnoms12 测试套件）里 `user_id: 0, account_type: 0` 仍能带限额
（4/30 秒、100/天）正常调 JSON API——**匿名 API 政策在 2020→2026 间收紧为完全禁止**，这是本项目必须内置 api_key 的直接原因。

### 1.4 Cloudflare 观察【实测】

- 浏览器内**未经过导航**直接 `fetch()` 首请求 `search.php`：403 + Cloudflare "Just a moment..." 挑战页（HTML）。
- 用顶层导航访问同一 URL：挑战自动通过，之后同源 `fetch()` 全部正常（清关 Cookie 生效）。
- 对 Rust 后端（wreq Chrome147 指纹）的含义：请求头完整、带常规 UA 时预期可直连；**未实测**，
  若上线后遇 403 挑战页（特征：HTML + "Just a moment"），需考虑加齐浏览器指纹头或降低频率，不能靠重试硬闯。

---

## 2. 索引（数据库）总表

pixiv 相关的两个索引（其余全表见附录 B）：

| index_id | 名称（官方 status.html） | dbmask | 更新状态 | 说明 |
|---|---|---|---|---|
| **5** | pixiv Images | `0x20`（十进制 32） | 持续更新，数小时一轮，「相当完整」 | 现行 pixiv 索引，**本项目判定的主键** |
| **6** | pixivhistorical | `0x40`（十进制 64） | - | 旧 pixiv 图历史索引；网页结果里显示为「Old pixiv ID: …」【实测】 |
| 999 | ALL（组合） | - | - | `db=999` 的默认全集；官方注明 999 **不含** #15 Shutterstock，且整体召回质量低于逐库检索（换速度） |

- 官方掩码文件原文（`tools/examples/api/index_details.txt`，2022-04-25 更新【实测】）：
  `Even though pixiv is labeled as index 5, it would be controlled with the 6th bit position, decimal 32`（0x20）。
- 判定 pixiv 结果**以 `header.index_id ∈ {5, 6}` 为准**，`index_name` 字符串措辞随年份可能变（2020 fixture 为
  `Index #5: Pixiv Images - 4933944_s.jpg`），只作展示。

---

## 3. 响应结构（output_type=2，JSON）

### 3.1 顶层

```json
{
  "header": { "...": "见下表" },
  "results": [ { "header": {...}, "data": {...} } ]
}
```

**顶层 `header` 字段**（成功形态无法匿名实测，字段清单来自 nomnoms12/pysaucenao/PicImageSearch 三家一致【社区记录】；
2020 年真实取值样例见附录 A3）：

| 字段 | 类型 | 含义 |
|---|---|---|
| `user_id` | number | SauceNAO 用户 id（匿名时为 0） |
| `account_type` | number | 账号类型（0=匿名；其余档位官方未公开，无需依赖） |
| `short_limit` | **string** | 每 30 秒窗口限额（如 `"4"`） |
| `long_limit` | **string** | 每 24 小时限额（如 `"100"`、`"150"`） |
| `short_remaining` | number | 30 秒窗口剩余次数 |
| `long_remaining` | number | 当日剩余次数 |
| `status` | number | 业务状态：`0`=成功；`-1`=鉴权/账号问题；社区库按 `429`=超限处理【未验证】 |
| `results_requested` | number | 请求的条数（回显 numres） |
| `results_returned` | number | 实际返回条数 |
| `minimum_similarity` | number | 本次实际生效的最低相似度 |
| `search_depth` | string | 搜索深度（2020 样例 `"128"`） |
| `query_image_display` | string | 查询图在 saucenao 的相对路径（如 `userdata/O3bQNenXC.gif.png`） |
| `query_image` | string | 查询图文件名 |
| `index` | object | （部分响应出现）各索引的 `{status, parent_id, id, results}` 明细【未验证】 |

**错误时 `header` 只剩两个字段**【实测】：

```json
{ "status": -1, "message": "The anonymous account type does not permit API usage." }
```

### 3.2 `results[]` 条目

每条 = `header`（匹配元数据）+ `data`（按命中库不同而不同的业务字段）。

**`results[].header`**：

| 字段 | 类型 | 含义 |
|---|---|---|
| `similarity` | **string** | 相似度百分数（如 `"93.5"`）——**注意是字符串**，`JSON.parse` 后要手动 `parseFloat`（sauce-api（Rust）/ nomnoms12 两个独立实现都按字符串解析） |
| `thumbnail` | string | 缩略图 URL，saucenao 自有域签名临时地址（§5） |
| `index_id` | number | 命中库编号（pixiv=5 / pixivhistorical=6） |
| `index_name` | string | 形如 `Index #5: Pixiv Images - 4933944_s.jpg`（库名 + 命中缓存文件名） |
| `dupes` | number | 重复计数（社区库 SauceNao.js 记载；字段名曾见 `dup` 写法，以宽容解析为宜）【社区记录】 |
| `hidden` | number | 内容分级标记，0=安全，非 0=explicit 候选（PicImageSearch 语义）【社区记录】 |

**pixiv 库（index_id 5 / 6）的 `data`**【社区记录·2020 真实 fixture + 各库一致】：

| 字段 | 类型 | 含义 |
|---|---|---|
| `pixiv_id` | number | **pixiv 作品 id（pid）**，跳应用内详情的主键 |
| `member_id` | number | **作者 uid** |
| `member_name` | string | 作者名 |
| `title` | string | 作品标题 |
| `ext_urls` | string[] | 外链数组，pixiv 条目为旧式 `https://www.pixiv.net/member_illust.php?mode=medium&illust_id={pid}`（该 URL 至今仍 302 到 `/artworks/{pid}`）【实测·HTML 通道同款链接】 |
| `pixiv_type` / `illust_type` | - | **任何实测/社区记录中均未出现**（GitHub 全量码检无 `"pixiv_type"`；五家客户端库无一引用）。不要在本项目解析中依赖；作品类型（0=插画/1=漫画/2=动图）进应用后用作品详情接口确认 |
| 其他库专属字段 | - | danbooru/gelbooru 等库各有 `danbooru_id`、`gelbooru_id`、`creator`、`material`、`eng_name`、`jp_name`、`source`、`part`、`characters` 等，本项目不消费 |

### 3.3 「无结果」与低相似度形态

- JSON：无命中时返回 `results: []`（社区库统一按空数组处理）【社区记录】。
- HTML【实测】：正常 200 结果页，无命中时结果表区域为空；有部分低相似命中时会渲染
  `Low similarity results have been hidden. Click here to display them...` 折叠提示。
- 【实测·存疑】POST HTML 通道带 `minsim=99` 仍返回了 73%–75% 的结果并附上述折叠提示——
  HTML 通道对 `minsim` 的处理与预期不符（参数可能被忽略或阈值语义不同）。**结论：`minsim` 只按 JSON API 语义使用**，客户端自己再做一次相似度过滤兜底。

---

## 4. pixiv 结果判定与字段映射（本项目核心）

### 4.1 判定

```text
is_pixiv = (result.header.index_id == 5) or (result.header.index_id == 6)
pid      = result.data.pixiv_id            // 主路径
author   = result.data.member_id
```

- index_id 5（现行库）与 6（历史库）的 `data` 字段同构，`pixiv_id` 都是可直接用的作品 id【社区记录】；
  HTML 通道实测也看到两类并存：普通条目显示 `pixiv ID: 76100412`、历史条目显示 `Old pixiv ID: 8868617`【实测】。
- 同一张图可能同时命中 pixiv、danbooru、gelbooru 等多库（`results` 按相似度降序混排），**不要只取 results[0]**，
  应遍历找出 index_id∈{5,6} 且 `pixiv_id` 存在、相似度达阈值的最高一条。

### 4.2 解析回退链（防御性）

`data.pixiv_id` 缺失时（不同年代索引行为可能有差）再从 `ext_urls` 提取：

```text
member_illust.php?mode=medium&illust_id=(\d+)   // 旧式，2026 实测仍是 ext_urls 主形态
/artworks/(\d+)                                  // 新式兜底
member.php?id=(\d+)                              // 作者 uid 兜底（对应 /users/{uid}）
```

两者都拿不到 pid 时该条目退化为「纯相似结果」展示（无跳转）。

### 4.3 HTML 通道实测佐证（匿名网页搜索，2026-10-02）

对一张 Danbooru 公网图（其源为 pixiv 作品 137850858，但因再压缩哈希偏移未高分命中原作，
第 5 条以 39.81% 命中另一 pixiv 图）抓到的 pixiv 条目原文结构：

```text
39.81%  青山君まとめ あんど出青   pixiv ID: 76100412   Member: ぶな子
  缩略链接: https://saucenao.com/search.php?db=999&url=https%3A%2F%2Fimg1.saucenao.com%2Fres%2Fpixiv%2F7610%2F76100412_p0_master1200.jpg%3Fauth%3D...%26exp%3D...
  作品外链: https://www.pixiv.net/member_illust.php?mode=medium&illust_id=76100412
  库内查询: https://saucenao.com/info.php?lookup_type=0&db=5&id=76100412      ← db=5 即 pixiv 索引
  作者外链: https://www.pixiv.net/member.php?id=17473379
```

- 印证：`db=5`=pixiv、外链为旧式 `member_illust.php` URL、saucenao 缓存域 `img1.saucenao.com/res/pixiv/{pid/1000 向下取整}/..._master1200.jpg?auth=…&exp=…`。
- 同页另一实测条目：64×64 画布 PNG 上传后命中 `pixiv ID: 77175072 Member: みずま`（61.03%）——
  低分辨率小图也能进索引匹配，本地截图类查询可行性得到佐证。
- 同页出现 `Old pixiv ID: 8868617`（index 6 历史库）——两库并存实锤。

### 4.4 与应用的映射建议

1. 后端命令（示意）：`saucenao_search(url|file_path) -> { results: [{ similarity, index_id, pixiv_id, member_id, member_name, title, hidden }], short_remaining, long_remaining }`。
2. 前端：相似度 ≥ 阈值（建议默认 60，可配置）且 `hidden==0` 的 pixiv 条目展示「跳转作品详情」（`browse_work_detail(pixiv_id)`）；非 pixiv 条目仅展示外链。
3. 多结果按 similarity 降序展示，pixiv 命中置顶标记。

---

## 5. 缩略图 URL 域名（实测汇总）

| 场景 | 实际域/形态 | 证据 |
|---|---|---|
| 匿名网页版结果图 | `https://saucenao.com/images/static/blocked.gif`（占位图，不给真缩略图） | 【实测】 |
| 登录用户 / JSON API `header.thumbnail` | `https://img1.saucenao.com/res/pixiv/{floor(pid/1000)}/{pid}_s.jpg?auth={签名}&exp={过期unix秒}`；历史库为 `/res/pixiv_historical/...` | 【社区记录·2020 fixture】+【实测·HTML 链接同域同签名结构】 |
| pixiv 大图（库内缓存） | `https://img1.saucenao.com/res/pixiv/{floor(pid/1000)}/{pid}_p0_master1200.jpg?auth=…&exp=…` | 【实测·HTML「以图为搜」链接】 |
| 非 pixiv 库 | 同为 saucenao 自有域按库分目录：`img3.saucenao.com/dA/…`（deviantArt）、`img3.saucenao.com/ehentai/…`、`img1.saucenao.com/res/mangadex/…` 等 | 【社区记录·fixture】+【实测·HTML】 |

结论：

1. **pixiv 条目缩略图不指向 i.pximg.net**，而是 saucenao 自己的缓存副本（`img1/img3.saucenao.com`），因此展示缩略图**不需要** pixiv 的 `Referer` 头。
2. URL 带 `auth`+`exp` 签名、**会过期**（pysaucenao 明确警告 "temporary signed URL; not suitable for permanent hotlinking"）——本项目图片代理对这类 URL 只能做**短期**内存/磁盘缓存，不能套用 pixiv 图的长缓存策略；过期后需重新搜索获取新签名 URL。
3. 也可以选择不展示 saucenao 缩略图，命中后直接用 `pixiv_id` 回源应用内作品封面（走既有 pixiv 图片代理），体验更一致——推荐。

---

## 6. 限额

双窗口计数：`short` = 每 30 秒，`long` = 每 24 小时（滚动窗口）。剩余量只存在于**成功响应**的 `header.short_remaining / long_remaining`，超限响应里是否回带未验证。

| 账号 | 每日（long） | 每 30 秒（short） | 证据 |
|---|---|---|---|
| 匿名（无 key） | **API 不可用**（403 -1）；网页 HTML 搜索可用且无配额字段可读 | 同左 | 【实测】 |
| 注册免费账号 | 150（2020 年为 100，数字变过） | 4 | 【社区记录】PicImageSearch（活跃维护）150/4；nomnoms12 2020 README 样例 long_remaining=99（即 100/天）；Kotlin/JS 库同记 4/30s |
| 付费（Patreon） | 更高档位存在，具体数字官方未公开可引 | 同左 | 官方靠 Patreon 运营（首页横幅 "Upgrade or Donate"【实测】）；注册后以 `header.long_limit` 实际值为准 |

工程口径：

- **不要硬编码限额数字**，每次响应读 `short_remaining/long_remaining` 做客户端限流与 UI 提示；
- 免费账号 4 次/30 秒：客户端串行调用且间隔 ≥8s 最稳；
- 429/超限的确切响应形态未实测（匿名撞不到），社区库统一按「HTTP 403/429 + header.status」分类处理（pysaucenao 区分 short / daily / invalid_requests 三类）【社区记录】。

---

## 7. API key

| 项 | 说明 |
|---|---|
| 注册入口 | `https://saucenao.com/user.php`（【实测】匿名访问为 Login 页；注册需邮箱） |
| key 查看与官方参数文档 | `https://saucenao.com/user.php?page=search-api`（登录后可见）【实测·登录墙】 |
| 放置方式 | query 参数 `api_key={key}`（GET/POST 均放 URL query，不进 body）【实测·通道】 |
| 无效 key 行为 | 【实测】与无 key 相同：403 + `status:-1` 匿名错误（无单独 "invalid key" 文案）——据此可把 -1 统一映射为「请检查 api_key」 |
| 安全 | key 属用户凭据：只进系统凭据存储/设置文件，**不入库、不写日志**（沿用项目安全边界） |

---

## 8. 错误形态（2026-10-02 实测为主）

| 场景 | HTTP | Content-Type | 响应体 |
|---|---|---|---|
| 匿名 / 无 key / 无效 key 调 JSON API（GET 或 POST，任何参数，包括 `numres=0`、坏 `url`） | **403** | `application/json` | `{"header":{"status":-1,"message":"The anonymous account type does not permit API usage."}}`【实测】（鉴权先于参数校验，故拿不到参数错误的专属形态【实测推论】） |
| `output_type=1`（XML） | 200 | `text/html` | `XML not yet implemented.`【实测】 |
| 无 Cloudflare 清关的裸 fetch | 403 | `text/html` | Cloudflare "Just a moment..." 挑战页【实测·浏览器内观察】 |
| `user.php`、`/tools/examples/api/`（目录）匿名访问 | 403 | `text/html` | nginx 403 / 登录页【实测】 |
| 超限（short/daily） | 未实测 | json（推断） | 社区按 HTTP 403/429 + `header.status` 处理【社区记录】【未验证】 |
| 查询图无效（非图片/过小） | 未实测 | - | 社区库有 InvalidImageError / ImageSizeError 分支【社区记录】 |
| 无结果 | 200 | `application/json`（推断） | `results: []`【社区记录】 |

解析红线：

1. **先判 HTTP 状态码，再判 `header.status`**——错误体也是 JSON，不能只看能否 parse。
2. `status == 0` 才是成功；`status != 0` 时读 `message` 透出给前端（文案本身不含敏感信息）。
3. body 偶发非 JSON（CF 挑战页、XML 提示），解析需容错并按 HTML 特征归类。

---

## 9. 上传限制（file 通道）

- 官方数字（大小上限、支持格式明细）在登录墙后的官方文档里，本次未能取得【未验证】。
- 社区可佐证的边界：
  - Kotlin 库（InsanusMokrassar/SauceNaoAPI）按 **gif / jpeg / png / svg** 四类 Content-Type 发送【社区记录】；
  - pysaucenao 存在 `FileSizeError`（图过大）与 `ImageSizeError`（图过小）两个错误分支，说明**上下限都存在**但未公开数字【社区记录】；
  - 【实测】64×64 PNG（约 1KB）与 32×32 PNG 均被正常接受并参与匹配——下限极低，常规截图/缩略图都能查。
- 本项目建议：客户端侧先按「png/jpg/jpeg/webp/gif、≤10MB」预检（保守值），服务端错误按 §8 分类兜底；webp 是否被接受**未验证**，稳妥做法是本地转码为 png 再上传。

---

## 10. 对 pixiv-tool 的落地建议

1. **api_key 为前置条件**：设置页引导注册（§7），无 key 时入口置灰 + 说明文案；key 走系统凭据存储。
2. 请求通道：本地图片文件 → POST multipart（`file` 字段）；应用内已有 URL（如用户粘贴）→ GET `url=`。全部走 wreq 直连，串行 + 间隔 ≥8s。
3. 解析顺序：HTTP 状态 → `header.status` → `results[]` 遍历筛 `index_id ∈ {5,6}`；`similarity` 用 `parseFloat`；`pixiv_id/member_id` 直接取 `data`，缺失走 §4.2 回退正则。
4. 结果跳转：pixiv 命中 → `browse_work_detail(pixiv_id)`（作品类型/是否 R-18 以应用内详情为准，不依赖 saucenao 侧字段）。
5. 缩略图：优先用应用内 pixiv 封面回源；若展示 saucenao 缩略图，签名 URL 仅短缓存（见 §5.2）。
6. 限额 UI：每次响应把 `short_remaining / long_remaining` 写入状态，逼近 0 时提示；收到 403 `-1` 统一提示「api_key 缺失或无效」。
7. 失败兜底：CF 挑战 / 非 JSON body → 明确报错不重试蛮干；429 → 指数退避。

---

## 11. 风险与备注

- **无兼容承诺**：官方 index_details.txt 原文 "This list is subject to change without notice"；实测口径已变过一次（匿名 API 2020 可用 → 2026 禁止）。解析代码必须宽容：字段缺失不致命、similarity 字符串、新字段忽略。
- **Cloudflare**：对低指纹客户端可能挑战（§1.4）；wreq Chrome147 指纹预期可过但上线前应实测一次。
- **测试配额纪律**：本调研全部匿名，未消耗任何注册账号配额；带 key 联调时注意 4 次/30 秒。
- 未实测清单（留给后续带 key 复核）：成功 JSON 的当日字段实样、numres=40 上限、minsim 在 JSON 通道的行为、429/超限响应体、无效图片/过大文件错误体、webp 支持、dedupe/testmode 效果、Cloudflare 对 wreq 的态度。

---

## 附录 A：响应 JSON 素材

### A1 【实测】匿名调 JSON API（GET，2026-10-02）

请求：`GET https://saucenao.com/search.php?db=999&output_type=2&numres=5&url=https%3A%2F%2Fcdn.donmai.us%2Fsample%2F6b%2F2a%2Fsample-6b2a245dca020b9afa14b531cb6a6536.jpg`

```json
// HTTP 403, Content-Type: application/json
{"header":{"status":-1,"message":"The anonymous account type does not permit API usage."}}
```

同形态复现实测（均 403/-1）：POST multipart `file` + `output_type=2`；`api_key=deadbeef00001111`；`numres=0`；`url=https://example.com/nonexistent_404.jpg`。

另实测：`output_type=1` → HTTP 200 `text/html`，正文 `XML not yet implemented.`

### A2 【实测】匿名网页 HTML 搜索（GET，无 output_type，2026-10-02）

请求：`GET https://saucenao.com/search.php?db=999&numres=5&url={同 A1 图}` → HTTP 200，结果页 5 张结果表，pixiv 命中条目文本与链接见 §4.3；缩略图位置为 `https://saucenao.com/images/static/blocked.gif`。

### A3 【社区记录】成功响应结构（nomnoms12/saucenao_api 测试套件记录的 2020 年真实响应片段，仅作解析单测参考形状，数值勿当现行契约）

```json
{
  "header": {
    "user_id": 0,
    "account_type": 0,
    "short_limit": "4",
    "long_limit": "100",
    "long_remaining": 79,
    "short_remaining": 2,
    "status": 0,
    "results_requested": 6,
    "search_depth": "128",
    "minimum_similarity": 24.6,
    "query_image_display": "userdata/O3bQNenXC.gif.png",
    "query_image": "O3bQNenXC.gif",
    "results_returned": 6
  },
  "results": [
    {
      "header": {
        "similarity": "23.50",
        "thumbnail": "https://img1.saucenao.com/res/pixiv/493/4933944_s.jpg?auth=kwJTn57-P4LeASMM5JTIeQ&exp=1596483386",
        "index_id": 5,
        "index_name": "Index #5: Pixiv Images - 4933944_s.jpg"
      },
      "data": {
        "ext_urls": ["https://www.pixiv.net/member_illust.php?mode=medium&illust_id=4933944"],
        "title": "妖キャラをカリスマ化してみた。",
        "pixiv_id": 4933944,
        "member_name": "佳虫",
        "member_id": 724886
      }
    },
    {
      "header": {
        "similarity": "21.48",
        "thumbnail": "https://img1.saucenao.com/res/pixiv_historical/383/3836606_s.jpg?auth=yA_uYFsJWaH0QTpyFFWXig&exp=1596483386",
        "index_id": 6,
        "index_name": "Index #6: Pixiv Historical - 3836606_s.jpg"
      },
      "data": {
        "ext_urls": ["https://www.pixiv.net/member_illust.php?mode=medium&illust_id=3836606"]
      }
    },
    {
      "header": {
        "similarity": "23.60",
        "thumbnail": "https://img3.saucenao.com/dA/51571/515715132.jpg",
        "index_id": 34,
        "index_name": "Index #34: deviantArt - 515715132.jpg"
      },
      "data": {
        "ext_urls": ["https://deviantart.com/view/515715132"],
        "title": "Koshitantan + video link+stagedl",
        "da_id": 515715132,
        "author_name": "SliverRose0916",
        "author_url": "http://sliverrose0916.deviantart.com"
      }
    }
  ]
}
```

要点核对清单（写解析器时对照）：`similarity` 字符串；`short_limit/long_limit` 字符串；`pixiv_id/member_id` 数值；
pixiv 缩略图在 `img1.saucenao.com/res/pixiv/` 下且带 `auth/exp` 签名；`index_name` 含命中缓存文件名后缀；非 pixiv 条目 `data` 字段集完全不同（按 `index_id` 分支取字段）。

## 附录 B：官方索引总表（tools/examples/api/index_details.txt 实抓全文，2026-10-02；官方注 2022-04-25 更新）

| mask | index | 名称 | mask | index | 名称 |
|---|---|---|---|---|---|
| 0x1 | #0 | h-mags | 0x1000000 | #25 | gelbooru |
| 0x2 | #1 | h-anime* | 0x2000000 | #26 | konachan |
| 0x4 | #2 | hcg | 0x4000000 | #27 | sankaku |
| 0x8 | #3 | ddb-objects* | 0x8000000 | #28 | anime-pictures |
| 0x10 | #4 | ddb-samples* | 0x10000000 | #29 | e621 |
| **0x20** | **#5** | **pixiv** | 0x20000000 | #30 | idol complex |
| **0x40** | **#6** | **pixivhistorical** | 0x40000000 | #31 | bcy illust |
| 0x80 | #7 | anime* | 0x80000000 | #32 | bcy cosplay |
| 0x100 | #8 | seiga_illust (nico nico seiga) | 0x100000000 | #33 | portalgraphics |
| 0x200 | #9 | danbooru | 0x200000000 | #34 | dA |
| 0x400 | #10 | drawr | 0x400000000 | #35 | pawoo |
| 0x800 | #11 | nijie | 0x800000000 | #36 | madokami |
| 0x1000 | #12 | yande.re | 0x1000000000 | #37 | mangadex |
| 0x2000 | #13 | animeop* | 0x2000000000 | #38 | H-Misc (ehentai) |
| 0x4000 | #14 | IMDb* | 0x4000000000 | #39 | ArtStation |
| 0x8000 | #15 | Shutterstock* | 0x8000000000 | #40 | FurAffinity |
| 0x10000 | #16 | FAKKU | 0x10000000000 | #41 | Twitter |
| 0x20000 | #18 | H-MISC (nhentai) | 0x20000000000 | #42 | Furry Network |
| 0x40000 | #19 | 2d_market | 0x40000000000 | #43 | Kemono |
| 0x80000 | #20 | medibang | 0x80000000000 | #44 | Skeb |
| 0x100000 | #21 | Anime | | | |
| 0x200000 | #22 | H-Anime | | | |
| 0x400000 | #23 | Movies | | | |
| 0x800000 | #24 | Shows | | | |

官方附注：`index #17 is reserved`；`*` 为已停用索引；掩码用法=按需把十六进制相加后转十进制传 `dbmask`；`db=999` 组合索引=除 #15 外全部。
（首页网页表单的 `dbs[]` 复选框取值与上表 index 一致【实测】，无 552 等其他编号。）

## 附录 C：本次实测请求台账（2026-10-02，全程匿名）

| # | 请求 | 结果 |
|---|---|---|
| 1 | 浏览器裸 fetch `search.php?db=999&output_type=2&numres=5&url=…`（无导航铺垫） | 403 Cloudflare "Just a moment..." |
| 2 | 顶层导航同一 URL | CF 过 → JSON：403 + `status:-1` 匿名错误 |
| 3 | 同源 fetch 复跑 #2 + `output_type=1` / `api_key=无效` / `numres=0` / `url=404 图` | 403 + `-1`（XML 变体除外：200 "XML not yet implemented."） |
| 4 | 同源 fetch HTML 搜索（无 output_type，GET `url=`） | 200 结果页，pixiv 条目见 §4.3 |
| 5 | POST multipart `file`（64×64 canvas PNG）+ `output_type=2` | 403 + `-1`（同匿名错误，通道通） |
| 6 | POST multipart `file`（同图）无 output_type | 200 结果页，61% 命中 pixiv ID 77175072 等 |
| 7 | POST multipart + `minsim=99`（32×32 PNG） | 200，仍返回 73%–75% 条目 + "Low similarity results have been hidden" 折叠提示 |
| 8 | `status.html` / 首页 DB 表单 / `index_details.txt` | 全部 200，见 §2 / 附录 B |
| 9 | `user.php`、`user.php?page=search-api`、`/tools/examples/api/`（目录） | 403/登录页（登录墙） |
