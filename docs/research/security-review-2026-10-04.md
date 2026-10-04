# 2026-10-04 安全与资源审查

审查基线：`fdd1089a1b37deb8e365c028550ea16a6888a75c`；Windows 11 x64，
PowerShell、MSVC。三名子代理分别负责前端生命周期、后端资源边界、凭据与
配置依赖，主代理负责翻译、更新及集成验证。以下为静态审查和离线回归结果，
不能解释为应用所有平台与所有第三方依赖均无漏洞。

## 已修复问题

| 范围 | 问题与处理 | 回归证据 |
|---|---|---|
| 下载凭据 | 默认头中的 CSRF 原本会继承到 CDN；关闭默认头继承，仅重附普通请求头，下载限制 pximg HTTPS 子域与 443 | 回环 HTTP 捕获合成 Cookie/CSRF，确认实际发送不携带凭据；非法 URL 发请求前拒绝 |
| 日志与错误 | wreq 原始错误可能带完整查询串；CDP 远端错误可能含请求参数 | 网络错误仅类别和脱敏地址，CDP 仅数字错误代码；合成秘密不出现在错误返回 |
| Cookie 域名 | ends_with("pixiv.net") 会误收 evilpixiv.net | CDP、webview 均要求 DNS 标签边界，过滤用例同步更新 |
| 登录资源 | CDP 不回包时无法进入登录清理；非 BMP 凭据分片超出 Windows UTF-16 容量 | 请求 10s / 连接关闭 3s 截止；模拟静默 WebSocket 和 UTF-16 拼接容量测试 |
| 凭据缓存 | 移除账号后保留 CookieStore；任意未登记切换会创建缓存；并发创建重复实例 | 移除驱逐、切换前守卫、单次短锁创建；既有临时索引/凭据 mock 测试 |
| 图片缓存 | 慢磁盘下后台写入无界排队，持续持有完整 Bytes | 最多 10 份响应；拥塞仅跳过缓存，不影响图片；满载合成下载检查返回、在途回收和无落盘 |
| 列表与订阅 | 重置后旧响应污染新列表；卸载后的订阅迟到完成会残留 | 请求世代保护含收藏游标；迟到订阅即反订阅，通知 timer 清理，停用作者 resize 和发现页补拉 |
| 输入与外链 | Session/Key 明文显示、Session 草稿长期保留；外部响应 URL 可传任意 scheme 给 opener | password 遮罩、关闭清草稿、防重复提交；HTTP/HTTPS 且无 userinfo |
| 任务与路径 | 空插画来源、非正 ID 可创建任务；暂停取消后可能启动下一项；扩展名可含 Windows 特殊路径字符 | 严格来源/正整数守卫，等待后重查取消，扩展名限定 1–8 位 ASCII 字母数字 |
| 历史分页 | LIMIT=-1 可全表加载，页码乘法可溢出 | 页码≥1、每页1–200、checked_mul；两类历史极值回归 |
| 更新元数据 | HTML/Release JSON 无实际字节上限，通常还多下载整个版本页 | 最终 URL 命中版本即返回；回退/JSON 实收上限4MB，Content-Length及chunked超限测试 |
| 内容安全与 Git | 主窗 CSP 为空；用户翻译缓存、webview profile 未完整忽略 | 生产/开发 CSP，整个 data/config 忽略；等价生产 CSP 浏览器测试 |

## 性能与内存优化

- AJAX body 改为移动 JSON 值，避免深克隆完整响应。
- 登录轮询借用 Cookie/页面数组，避免每轮 JSON 深克隆。
- 图片缓存 SHA256 复用已有 sha2，删去手写算法与完整 padding 输入副本，
  保留旧缓存键与官方向量回归。
- 翻译空正文检查不再将第二份行数组保留在串行排队/网络等待中。
- 停用页面停止无效监听与自动补拉；常规分页、列表内容、图片画质、抓取限速
  和两轮翻译契约保持原有语义。
- 滚动锚点遍历在首张可见卡片处停止，不再复制完整 NodeList 或重复读取几何。
  既有首次主页返回位置丢失、历史加载异常未处理也已修复，原回归测试不改。

没有进行真实负载吞吐或跨平台堆分析，因而不声称节省了特定 MB 或百分比。
原图/ugoira 仍整包缓冲，大文件峰值与可释放的浏览器图片缓存不等于持续泄漏。

## 依赖审计

- npm 官方 registry audit：原 11 条报告修复后为 **0 条已知漏洞**。
  Vite 6.4.3、esbuild 0.25.12、brace-expansion/postcss/nanoid 传递补丁。
- OSV 官方 querybatch：锁文件 **570 个 registry crate**；time 0.3.47、
  serde_with 3.21.0、plist 1.10.1 / quick-xml 0.42.0 消除原相关告警。
  这是版本范围扫描，不等价于完整 cargo-audit 或漏洞路径可达性证明。
- 残余：glib 0.18.5 的 VariantStrIter 安全性问题由 Linux GTK/WebKit 依赖树
  引入，Windows 构建不使用此树。补丁在 glib≥0.20，不能跨 GTK 版本直接替换；
  保留上游升级风险，不宣称 Linux 已安全。另有 proc-macro-error 与 unic 系列
  共 6 条未维护提示，属于维护风险，不作为密钥泄露或已利用漏洞证据。

官方证据：[Vite 公告](https://github.com/vitejs/vite/security/advisories/GHSA-fx2h-pf6j-xcff)、
[UNC/NTLM 公告](https://github.com/vitejs/launch-editor/security/advisories/GHSA-v6wh-96g9-6wx3)、
[time](https://rustsec.org/advisories/RUSTSEC-2026-0009.html)、
[serde_with](https://github.com/jonasbb/serde_with/security/advisories/GHSA-7gcf-g7xr-8hxj)、
[quick-xml 发布说明](https://github.com/tafia/quick-xml/releases/tag/v0.41.0)、
[glib](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)。构建基线改变见 ADR 0019。

## 验证与边界

源码 HEAD 已跟踪文件秘密格式扫描未发现实际 token/私钥/Session，且无已跟踪
data/config。没有扫描全部 Git 历史、读取本机系统凭据或证明秘密扫描绝无漏报。
SauceNAO Key 本机 settings.json 落点沿用已有规范；翻译 Key 与登录态仍只进
系统凭据库。CDP profile 是既有登录机制的运行数据，整目录排除提交。

浏览器新增 `security-lifecycle.html`：App 与 SauceNAO 各 10 次挂载/卸载后迟到
订阅清理验证通过；同时验证列表旧响应/游标隔离、秘密遮罩与外链限制。
生产构建等价 CSP header 测试验证 Material Web、设置六分组、登录字段以及
data/blob 图片正常，无意外 violation/pageerror，注入的内联脚本被拒绝。
本轮未调用真实 Pixiv/付费模型、系统安装器或进行真实登录；这些留在既有
显式在线验收入口，不在离线检查中伪造通过。

集成验证：`.\dev.ps1 check` 通过前端类型检查、Vite 6.4.3 生产构建与
`cargo check --locked`；`.\dev.ps1 test` 共 **300 项通过、0 失败、32 项忽略**
（289 单元 + 4 离线接口 + 7 IPC 冒烟；忽略项为原有在线/系统凭据环境测试）。
13 个既有浏览器回归加新生命周期回归共 **14 页全部通过、pageerror=0**，
包括 loading、workspace、detail、translation、author-follow、help-tooltips、
search-counts、navigation-context、channel、page-scroll、navigation、update、
startup-viewer 与 security-lifecycle。生产 CSP 测试再次通过。

构建仍有既有主入口约 604KB 的分块体积提示，以及 MSVC 链接器的信息提示，
均非失败；未为消除提示而掩盖阈值或引入未经测量的分块策略。Chrome DevTools
MCP 本轮挂起，浏览器验证使用本机独立 Playwright；未捕获原生 WebView 堆快照。
