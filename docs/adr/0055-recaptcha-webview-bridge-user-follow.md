# ADR 0055：关注人机验证经内置 WebView 铸 token 桥接

- 状态：已接受（扩展 [ADR 0012](0012-browse-mode.md) 浏览模式写操作、
  [ADR 0011](0011-fresh-login-window.md) 的 webview 窗口生命周期模式；
  勘误 PIXIV-API §4.10 2026-10-03 的「关注参数未经真实账号写实测」遗留风险）
- 日期：2026-11-16
- 关联 ADR：[ADR 0012](0012-browse-mode.md)（浏览模式）、
  [ADR 0011](0011-fresh-login-window.md)（登录窗）、
  [ADR 0013](0013-remove-embedded-browser.md)（移除内嵌浏览器，本桥非浏览能力）、
  [ADR 0009](0009-webview-login-fallback.md)（webview 内 reCAPTCHA 可用的探针结论）
- 规格：[SPEC](../SPEC.md) §3 / §7；契约与证据见
  [PIXIV-API](../PIXIV-API.md) §4.10 与
  [research](../research/pixiv-browse-api.md) §作者关注人机验证

## 背景

2026-11-16 mac 端关注任何作者恒失败：日志连续 3 次 `POST /bookmark_add.php → 400`
后报「客户端错误: HTTP 400」；Windows 端同版本同代码不复现。排查结论：**差异来自
账号而非操作系统**——pixiv 对部分账号（`test.ab.recaptcha_follow_user` 实验开关 /
风控命中，社区证据指向注册较晚的账号）关注强制要求 reCAPTCHA Enterprise score
token，缺 token 恒 400。我们的 form 参数（`mode=add&type=user&user_id={id}`）与
Pixiv Previewer / PixivBatchDownloader / DowneyRem PixivSource 等主流实现完全一致，
不是参数问题。

该 token 只能在 pixiv 页面 JS 上下文里经 `grecaptcha.enterprise.execute` 铸出；
PixivBatchDownloader 维护者明确记录其 ubd protobuf 载荷无法逆向生成（扩展只能
iframe 模拟点击）。原生 Rust 侧无法自行生成。ppixiv 给出了完整可复刻的页面侧
配方：site key 在登录态主页 `__NEXT_DATA__` preloaded state 的
`misc.grecaptcha.recaptchaEnterpriseScoreSiteKey`，加载
`https://www.recaptcha.net/recaptcha/enterprise.js?render={key}`（Google 面向
中国大陆的替代域，可达性与 pixiv 同级）后 `enterprise.ready` → 按官方页面行为发
一次 action `www/pageload` → 关注用 `execute` action `www/follow_user`，
token 作为 form 字段 `recaptcha_enterprise_score_token` 随请求携带。

另有三个既有事实支撑可行性：本应用是 Tauri（自带 WebView）；ADR 0009 登录探针
已确认 WKWebView 内 pixiv 页面的 reCAPTCHA Enterprise 可正常初始化；
`webview_login.rs` 提供了「按需建窗 + data_directory + finally destroy」的成熟
窗口模板。

## 决策

### 1. 400 → 铸 token → 重试一次的编排（命令层）

`browse_user_follow` 命令层编排，api 层保持纯 HTTP：

1. 取消关注不涉及验证（`rpc_group_setting.php` 无门槛），行为不变；
2. 取关注人机验证配置 `follow_recaptcha_config()`（登录态主页
   `__NEXT_DATA__` 解析，随 csrf 上下文 30min 缓存，不额外发请求）：
   `follow_requires_token == Some(true)`（`test.ab.recaptcha_follow_user`）→
   **铸 token 前置**直发；配置获取失败不阻塞关注；
3. 否则先直发（绝大多数账号零变化）；`is_bad_request`（HTTP 400）→ 铸 token
   后带 `recaptcha_enterprise_score_token` 字段重试一次；
4. 重试仍 400 或铸造失败 → 回退文案（`user_follow_recaptcha_guidance_error`）：
   「关注请求被 pixiv 拒绝：该账号关注前需通过人机验证，请点击作者页右上角按钮
   在浏览器中手动关注」；其他错误原样透传。
5. flag 缺失的账号若实际被风控，由 ②③ 的 400 回退兜底——flag 只是优化，
   400 才是权威信标。

### 2. token 铸造窗口（`webview_recaptcha.rs`）

- 独立窗口 label `recaptcha-mint`，420×300、**可见但不抢焦点**（`focused(false)`
  + 居中）：规避 macOS 隐藏窗口 JS 执行的不确定性（本 bug 即发现于 mac）；
  窗口加载 `https://www.pixiv.net/`（匿名即可，仅作 recaptcha 域绑定载体），
  铸完即 `destroy()`，对齐登录窗的防御性重建 + finally 销毁生命周期。
- `initialization_script` 注入 `resources/pixiv/recaptcha-mint.js`（编译期常量
  `include_str!`，site key 经哨兵值 replace 注入并做 JS 字面量转义；脚本含
  `__pixivtoolMintRan` 去重标志，防页面重定向多次注入重复铸造）。
- 回收通道：脚本铸成后导航 `pixivtool-recaptcha://done?t={token}`、失败导航
  `://error?m={原因}`；Rust 侧 `on_navigation` 拦截解析（自定义 scheme 导航被
  cancel，页面保持原状），经 tokio mpsc 交回异步等待（总超时 40s）。窗口销毁
  后导航闭包释放、sender 归零，用户手关窗口立即终止等待。
- **无 IPC**：capabilities 仍只授予 main 窗口（对齐登录窗的刻意设计），回收
  走导航拦截而非 invoke，不给 pixiv 页面任何 IPC 面。
- **数据目录独立**：`config/recaptcha-webview-profile`，与 `login-webview-profile`
  分离（后者会被登录流程整目录 `remove_dir_all`，共用会互相踩）；本目录持久
  保留不自动重置，为 reCAPTCHA 积累浏览器风险信誉。纳入安全边界豁免清单
  （与两个登录 profile 同档：webview 引擎自管数据）。
- 单飞：进程级 tokio Mutex 串行铸造（token 单次有效，不缓存不复用）。
- site key 只能 Rust 侧解析（匿名页无 preloaded state）：`parse_recaptcha_page_config`
  从 csrf 共用的主页 HTML 取 `misc.grecaptcha.recaptchaEnterpriseScoreSiteKey`，
  WebView 注入时模板替换。

### 3. 关注写端点改不重试（`post_form_once`）

关注 / 取消关注（旧式表单写端点）的 4xx 是确定性失败，退避重试只白等 ~4s 并对
pixiv 重复发起注定失败的写请求。`run_gated` 抽出 `run_gated_bounded(attempts)`
（闸门 / 限速 / 429 / finally 语义不变），新增 `post_form_once`；其余写路径
（收藏、评论）重试语义不变。错误自愈（Auth|Client 失效 csrf 缓存）沿用。

### 4. 测试钩子

`PIXIV_TOOL_FORCE_RECAPTCHA_MINT=1`：关注跳过首次直发、直接铸 token 携带发送——
在未触发风控的账号（如开发机）上端到端验证「铸造 + 带 token 请求被 pixiv 接受」，
对齐 `PIXIV_TOOL_FORCE_WEBVIEW_LOGIN` 先例。

### 5. 失败回退的产品姿态

本桥是「能自动化就自动化」：铸造失败（页面风控升级 / recaptcha.net 不可达 /
超时）不自动打开系统浏览器，只给回退文案；作者页右上角已有「浏览器打开」按钮，
用户一步可达。PixivBatchDownloader（iframe 模拟点击）与 DowneyRem PixivSource
（失败后拉起浏览器手动关注）的取舍亦如此。

## 后果

- 触发风控的账号在应用内可直接关注成功；未触发风控的账号行为零变化（多一次
  主页配置解析，随既有 csrf 抓取复用同一份 HTML，无额外请求）。
- 新增外部资源依赖：`recaptcha.net` / `www.gstatic.com`（recaptcha 脚本与
  badge 资源）。可达性与 pixiv 同级（走同一代理策略）；pixiv 更换 site key /
  action 名 / 启停该实验时，症状回到「直发 400 + 回退文案」，不比现状差。
- `config/` 豁免目录 +1（`recaptcha-webview-profile`，webview 引擎自管数据，
  与登录 profile 同档）；应用窗口家族 +1（login-webview / recaptcha-mint）。
- token 与含 token 的导航 URL 一律不落日志（沿用凭据纪律）；site key 是页面
  公开配置，非凭据。
- SPEC §7 命令数不变（铸造是 browse_user_follow 的内部编排，无新 IPC）；
  前端零改动（busy 态覆盖等待期，错误文案经 errorMessage 透传）。

## 替代方案

- **迁移 pixiv app-api + OAuth（PixEz 等三方客户端路线）**：`/v1/user/follow/add`
  无 reCAPTCHA 门槛，但需要全新凭据体系（refresh token OAuth / PKCE 登录），
  与现行「web PHPSESSID 单凭据」（ADR 0008/0011）是架构级偏离，收益面窄
  （仅关注一个动作），不采纳；未来若 app-api 需求增多再整体评估。
- **WebView 内人工点关注（PixivBatchDownloader iframe 思路 / 登录窗复用）**：
  打开内置窗口加载作者页让用户点击官方按钮，DOM 脆弱、页面重、且登录
  webview profile 会被登录流程重置无法复用登录态；不如无感铸造。
- **仅引导式修复（400 → 提示浏览器手动关注，不铸 token）**：改动最小，但
  触发风控的账号在应用内永远无法关注，与本应用「只读浏览之外的写操作尽量
  应用内闭环」的方向相悖；本 ADR 的回退文案即该方案的兜底形态。
- **隐藏窗口铸造**：避免可见窗口闪现，但 macOS 隐藏窗口的 JS 执行与计时器
  节流不可控，本 bug 恰好在 mac 暴露——不押注平台差异，选可见小窗口。
