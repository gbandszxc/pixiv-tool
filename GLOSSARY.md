# Pixiv Tool

本地运行的 Pixiv 客户端：抓取下载、只读浏览与小说整篇翻译共享同一个领域上下文。
本文件是领域术语的权威来源：文档、issue、代码命名与 Agent 提示词中出现这些概念时以本表为准，不漂移到同义词。行为规格仍以 `docs/SPEC.md` 为准——本表只回答「这个概念叫什么、是什么」，不承载参数、状态机细节与实现。括号内英文 / 标识符是代码与契约中的锚点。

## Language

### 作品与来源

**作品 (Work)**:
pixiv 上一条可浏览的内容单元，插画 / 漫画 / 小说统称。
_Avoid_: 条目、内容项

**插画 (Illust)**:
以图片呈现的作品，含单幅与多页；本项目中插画与漫画同属 illust 类别（`IllustSource` 展开 illusts + manga）。
_Avoid_: 图片、图

**小说 (Novel)**:
以文字呈现的作品，正文可含 pixiv 标记与 ruby 注音。
_Avoid_: 文章

**系列 (Series)**:
作者组织的多话作品序列；抓取与翻译时各话独立处理，不合并、不共享术语。
_Avoid_: 合集、丛书

**单篇 (Single)**:
一种抓取来源类型：用户直接指定的一个作品（来源枚举 `Single`）。
_Avoid_: 与「散篇」混用

**散篇**:
用户全集语境下不属于任何系列的作品，与系列内各话一起平铺保存。
_Avoid_: 零散作品

**作品页 (Page)**:
多页作品中的一张原图（插画 `_p{N}`）或小说的一次 `[newpage]` 分页；与阅读器的屏幕翻页无关。
_Avoid_: 翻页

**pixiv 标记 (Markup)**:
正文中的方括号标记（`[newpage]` / `[chapter:X]` / `[rb:原文>读音]` / `[pixivimage:...]` 等）；导出转换与翻译校验都必须保真，ruby 标记的数量与结构在翻译中强制保持。
_Avoid_: 标签

### 登录与账号

**登录态 (Login State)**:
一组当前有效的 pixiv 会话 Cookie（PHPSESSID / x-csrf-token 等）；只进系统凭据存储，绝不入库、不落 config。
_Avoid_: Cookie 泛指、会话

**账号 (Account)**:
登记的 pixiv 用户身份 = 用户元信息（`accounts.json`）+ 独立凭据条目（`u-<user_id>`）；多账号可切换。
_Avoid_: 用户（指 pixiv 上任意作者 / 他人时除外）

**激活账号 (Active Account)**:
当前生效的那个账号；keyring 的 `default` 条目恒为它的镜像，读取方零感知。
_Avoid_: 当前用户

**登录窗未登录态**:
登录窗（浏览器 / webview）每次打开前必然重置为未登录再开始登录，杜绝已登录 profile 秒判成功（ADR 0011）。
_Avoid_: 干净 profile

**登录专用 profile (Login Profile)**:
`config/login-browser-profile` 与 `config/login-webview-profile`，由浏览器 / webview 引擎自管会话数据的隔离目录；是「config 下不得出现 cookie 文件」的唯一豁免。
_Avoid_: 浏览器缓存目录

**会话探测 (Session Probe)**:
以 `/ajax/user/self` 做服务端权威验证登录态，非空 `userData.id` 才算有效。
_Avoid_: 心跳、ping

**会话缓存失效 (Session Cache Invalidation)**:
凭据变更（登录 / 切换 / 登出 / 探测失效）后强制清空进程内的自 uid 与主站 csrf token 缓存，防止旧账号身份残留一个 TTL。
_Avoid_: 刷新登录

**翻译凭据 (Translation Credentials)**:
小说翻译模型网关的 API Key，与 pixiv 登录态完全独立；翻译请求不带 pixiv Cookie。
_Avoid_: 翻译 Token

### 抓取与下载

**抓取任务 (Task)**:
一条入库的下载工作，状态机 `pending → running ⇄ paused → done / failed / canceled`；任务页签与状态栏里的「任务」即指它。
_Avoid_: 把翻译 Job 称为任务

**来源 (Source)**:
抓取任务的目标范围枚举：单篇 / 系列 / 用户全集（插画无系列来源）。
_Avoid_: 类型

**爬虫 (Crawler)**:
执行一个抓取任务的后台流程：逐项下载、单项失败不中断、已下载跳过。
_Avoid_: worker

**失败队列 (Failed IDs)**:
抓取任务内逐项失败的 id 清单，落库并可拼接为新任务串行重试。
_Avoid_: 错误日志

**下载历史**:
已完成抓取任务的回显（列表 / 宫格双模式，本地文件丢失有专用空态）；与浏览历史无关。
_Avoid_: 与「浏览历史」混用

**ugoira**:
pixiv 动图格式；应用保存原始帧序列 zip，不转码。
_Avoid_: 泛指 gif 动图

**以图识图 (SauceNAO)**:
经 SauceNAO 服务以图反搜 pixiv 作品的功能，需单独配置其 API Key。
_Avoid_: 反搜（口语可，文档用全称）

### 浏览模式

**浏览模式 (Browse)**:
应用内自有 UI 经代理访问 pixiv 只读接口的浏览方式，是 pixiv 内容的唯一应用内入口（ADR 0012 / 0013）。
_Avoid_: 内嵌浏览器（已移除）

**浏览历史 (Browse History)**:
作品级访问记录的持久化（`browse_history` 表），可经同步上云恢复。
_Avoid_: 访问历史、浏览访问历史

**图片代理 (Image Proxy)**:
`pixiv-img` / `pixiv-ugoira` / `pixiv-download` 自定义协议下的图片通道：磁盘缓存 + CDN 闸门，前端永不直连 pixiv 图床。
_Avoid_: CDN 直链

**缩略图档位 (Thumb Tier)**:
缩略图 URL 按设置档位改写尺寸；档位是合法枚举，不是任意尺寸。
_Avoid_: 清晰度

### 整篇翻译

**整篇翻译 (Whole-novel Translation)**:
以整篇小说为单位的翻译功能：Rig Agent 内核上的 Job 流水线（ADR 0034 / 0035）；设置与界面入口沿用「小说翻译」名称。
_Avoid_: 单页翻译（v1 旧实现，命令入口已移除）

**翻译 Job (Job)**:
一篇作品一次完整翻译的工作单元；身份 = 作品 + 原文版本 + 目标语言，独占缓存目录并拥有全部产物。系列内各话各自成 Job。
_Avoid_: 翻译任务（与抓取 Task 混淆）

**协调者 (Coordinator)**:
串联扫描、冻结、翻译、审查、返工、整体复核与发布的主 Agent 会话。
_Avoid_: 主线程、master

**子 Agent (Sub-agent)**:
被协调者派发的翻译或审查会话，只服务单块，不再派生（默认一层）。
_Avoid_: worker

**子任务 (Subtask)**:
派发给子 Agent 的单块工作实例；「并发子任务」指同时进行的这些工作。
_Avoid_: 与「子 Agent」混用

**页 / 行 / 片段 (Page / Line / Segment)**:
Job 内原文的三层映射：分页 → 行 → 片段；片段是局部修订的最小替换单位。
_Avoid_: 段落

**块 (Chunk)**:
按语义边界与字符预算切出的翻译与审查单位，携带前后邻接窗口；超输入上限时自动细分（限层数）。
_Avoid_: 段落、分片、截断（截断指别的行为）

**扫描 (Scan)**:
翻译前的约束抽取阶段：按块并行派发扫描子 Agent（`dispatch_scan`），汇总术语、人物关系、风格与对白。
_Avoid_: Pass 1（旧称）

**约束 (Constraints)**:
扫描产出的术语表 + 风格 + 译注整体；冻结后成为翻译与审查的共同契约。
_Avoid_: 设定集、bible（ADR 0017 旧称，已被 ADR 0035 取代）

**冻结 (Freeze)**:
约束定版生效的动作；此后修改术语与风格走「确认并继续」。
_Avoid_: 锁定

**约束版本 (Constraint Version)**:
约束的版本号；确认或修订后 +1，草稿与审查带版本戳、拒绝旧版本回退。
_Avoid_: revision

**等待确认 (Awaiting Confirmation)**:
冻结约束后、翻译开始前的持久状态：可修改术语与风格后「确认并继续」；跨应用重启不丢失、仍可确认（ADR 0046）。
_Avoid_: 与「暂停」混用（语义完全不同）

**审查分级 (Verdict Severity)**:
审查问题分 minor / major / critical 三级；minor 不阻塞交付，major 与 critical 阻塞（ADR 0038）。
_Avoid_: error / warning

**局部修订 (revise_draft)**:
按片段定点替换译文的 Agent 工具：不重派翻译、不消耗返工额度，须在约束冻结后调用。
_Avoid_: 微调、重译

**按块返工 (Rework)**:
整块重新派发翻译；额度按块独立记录，某块用尽后只拒绝该块的返工。
_Avoid_: 全局重试轮数

**块闸门 (chunk_gate)**:
从磁盘产物计算的块状态判定：`draft_valid` / `review_pass` / `needs_rework`；待办清单（`pending_translate` / `pending_rework` / `pending_review`）与完成判据共用它，不各自实现。
_Avoid_: 各处自行判定块状态

**调用预算 (Call Budget)**:
整篇 Job 的累计模型尝试次数预算：自动按分块数计算、可手动追加、只增不减；父任务 / 子任务 / 压缩摘要 / 传输重试 / 返工共享（ADR 0042）。
_Avoid_: token 预算（不是 token 数）

**检查点 (Checkpoint)**:
Job 阶段产物的落盘点；暂停停在最近检查点，恢复从它继续。
_Avoid_: 存档（口语可，文档用全称）

**任务续跑 (Continuation)**:
一轮运行正常结束但完成校验未过时，带压缩后历史与续跑指令自动继续；连续无进展达上限才转「需要处理」。
_Avoid_: 自动重试（网络重试是传输层另一回事）

**恢复 (Resume)**:
用户手动让非终态任务继续的动作；会清空全部块的返工额度与未知结果记忆。暂停后继续、自动续跑、网络自动重试都不是它。
_Avoid_: 与「自动续跑」混用

**需要处理 (needs_attention)**:
任务无法自行收敛（无进展、额度用尽、需人工决策）时的持久状态，等待用户检查后恢复。
_Avoid_: failed（另有语义：任务终态失败）

**实时预览 (Live Preview)**:
模型输出的流式增量：只进界面预览，不落盘、不写日志；每槽只保留最近固定字符数的尾窗（ADR 0037）。
_Avoid_: 流式日志

**预览归属 (Preview Ownership)**:
预览槽按「主会话 / 翻译 / 审查 + 块」归属，主会话生成时优先，避免并发子任务的预览跳变（ADR 0037）。
_Avoid_: —（无同义词，防误用「全局预览」）

**整体复核 (Global Review)**:
发布前主 Agent 基于摘要与抽样的全文级复核，不把全文塞回上下文。
_Avoid_: 二审、全文审查

**发布 (Publish)**:
块覆盖、审查结论与合并校验全部通过后，才把正式结果写入阅读器投影；校验失败不发布、不覆盖旧可读版本。
_Avoid_: 保存、导出（导出另有命令语义）

**部分结果 (Partial)**:
存在被拒或跳过块时的结果状态：相应块回退原文，绝不冒充完整译文。
_Avoid_: 半成品

**内容拒绝 (content_refused)**:
子 Agent 按内容政策显式拒绝某块；重试 1 次后快速失败并记入被拒清单，resume 不清空（ADR 0043）。
_Avoid_: 与审查问题混用

**跳过块 (Skipped Chunks)**:
用户显式跳过的被拒块；不参与待办、完成判据与收尾计数。
_Avoid_: 忽略

**翻译缓存世代 (Cache Generation)**:
缓存清理后递增的世代号；旧世代的迟到写入被直接拒绝，不重建已删除的 Job 目录。
_Avoid_: 版本号（约束 / 草稿 / 审查各有自己的版本）

**v1 旧缓存 (Legacy Cache)**:
旧单页两轮翻译实现的只读缓存；仅作回退展示且标记 partial，绝不冒充已审查结果。
_Avoid_: 旧版翻译功能

### 同步

**同步 (Sync)**:
浏览历史与登录态经 WebDAV 上云与恢复（ADR 0048）；上传 / 恢复是两个显式动作。
_Avoid_: 备份泛称

**同步口令 (Passphrase)**:
可选的同步加密口令；缺省用内置默认密钥（ADR 0049）。与 WebDAV 账号密码是两回事。
_Avoid_: 密码（未限定时指 WebDAV 密码）

**测试连接 (Connection Test)**:
同步动作的门控：测试成功才允许上传 / 恢复，并回显云端备份信息。
_Avoid_: ping
