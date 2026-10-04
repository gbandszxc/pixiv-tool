# 小说翻译在线验收

日期：2026-10-04。用户明确授权使用指定服务与 Key 测试。凭据只经测试进程环境提供，不进入源码、文件、日志或报告；测试不修改应用配置或系统凭据。

## 配置与方法

- 基址：`https://open.bigmodel.cn/api/coding/paas/v4`
- 实际端点：基址追加 `/chat/completions`
- 模型：`glm-5.3-flash`
- 高级 JSON：`{"reasoning_effort":"low"}`
- 测试：`translation::tests::live_two_page_translation`（默认忽略，显式在线运行）

[智谱接入文档](https://docs.bigmodel.cn/cn/coding-plan/quick-start)列出上述 OpenAI Chat Completion 基址；[模型发布方说明](https://huggingface.co/zai-org/GLM-5.3-Flash-BF16)列出 low/high/max 思考深度。本次实测验证给定服务接受 low 请求，不推断服务内部实际思考 token 数。

使用两个短日语小说页，携带标题、标签、简介与邻页上下文，调用生产 `TranslationClient` / `translate_book`，每页先准备设定集再精翻，共四次请求。第二页从第一页的落盘记录恢复共享设定，验证人物译名与文风保持、译文行号完整、两页持久化。结束清理全部临时记录。

## 结果

测试通过，约 14 秒，无自动重试。两页均采用“艾莉 / 蕾娜”，第二页人物与妻子指代保留。输出示例：

| 页 | 原文 | 译文 |
|---|---|---|
| 1 | エリとレイナは、このバーで出会った夫婦である。 | 艾莉和蕾娜是在这家酒吧相识的伴侣。 |
| 2 | エリは妻のレイナを見た。 | 艾莉看了看妻子蕾娜。 |
| 2 | 「レイナ、帰ろう」 | 「蕾娜，回家吧。」 |

这是连通性、结构和跨页一致性验收，不代表文学质量全量评测；样例第一行把“夫妇”译为“伴侣”，关系用词仍受模型语义遵从影响。

## 真实小说复验与报错暴露（同日追加）

用户授权后追加运行 `translation::tests::live_real_novel_page_translation`：只读本机 pixiv 登录态（绝不 save/clear），从小说日榜前 20 条取第一篇非限制级、首页 200–4000 字且含假名的日文作品，走生产两轮管线翻译第 1 页，再用不存在的模型确认模型侧报错的暴露方式。

- 取样：前 4 个候选首页仅 41–160 字（按筛选跳过），第 5 个候选命中；该作品共 6 页，第 1 页 11 行原文 → 11 行译文，设定集 13 个词条，耗时约 17 秒（两次请求，`reasoning_effort=low`）。
- 模型列表：`/models` 返回包含配置模型，与设置页「获取模型」同一路径。
- 模型侧报错暴露：不存在的模型返回 `翻译服务返回 HTTP 400：请检查模型 ID、高级 JSON 与请求参数`，文案可读、含排查方向，且不含凭据（HTTP 401/403 → 检查 Key，404 → 检查 URL 与模型，429 → 频率或额度）。
- 失败复现与修复：同日两页用例首次运行报「模型设定集结构无效」。三次采样诊断显示模型偶发省略 `aliases` 等可选字段（一次复现）。已改为容错解析（忽略多余字段、可选字段取默认值），语义校验仍由 `merge_bible` 承担，落盘与读缓存保持严格 `StoryBible`；修复后两页用例通过（艾莉 / 蕾娜 跨页锁定）。

真实作品正文与译文只在本机测试进程内使用，不写入仓库；本节只记录方法与结论。

## 目标语言与语言一致快速返回（同日追加）

- 提示词注入：`translation::tests::live_english_target_translation` 把同一日文样章的目标语言设为 `en` 翻译一页（两次请求）。译文为 `Reunion` / `Eri and Reina were a couple who had met at this bar.` / `"Reina, let's go home."`，无中日文残留；离线单测同时断言 Prepare 与 Render 两个提示词都完成 `{target_language}` 替换、无残留占位符。
- 语言一致快速返回：命令流程单测（配置指向不可达端口 + 中文原文 + 目标 zh-CN）直接返回 `already_target_language` 且 lines 为空，证明未发请求；`force` 与不同语言照常进入流程（连接失败），证明不会静默跳过。判定为本地启发式（假名/谚文占比、汉字简繁用字、拉丁/西里尔主导文字），拉丁语系之间不做跳过。
- 回归：`live_two_page_translation` 与 `live_real_novel_page_translation` 复跑通过，默认中文目标路径不受影响。样章章节标题在两次采样中一次译为「重逢」、一次原样保留 `[章节:再会]`，属模型遵从波动，与本改造无关，未做断言。

## 当前智谱配置的流式错误复验（2026-10-04）

用户授权重启、诊断并复测本机收藏夹第一篇小说。当前配置为智谱 Coding 端点、Chat Completions、`glm-5.3-flash`，高级参数为空；不修改设置或系统凭据。

- 原小说 ID `28154845`：真实阅读器输入含 318 行原文。服务返回 HTTP 200 后开始发送 SSE，随后直接追加没有 `data:` 前缀的 `{"error":{"code":"1301",...}}`，错误 message 表示内容审核拒绝，没有正常完成信号。旧解析器忽略该错误体，仅得到不完整 JSON，误报「模型未返回合法 JSON」。后端复现采样中累计文本为 868 字节，JSON 解析报 EOF；真实页面也复现同一症状。
- 排除思考深度：仅在临时测试设置中关闭思考仍收到同一审核错误；未把测试调整写入应用配置。不能通过 JSON 容错或补全残缺文本修复服务侧拒绝。
- 修复：识别 SSE 行与流末尾直接追加的 JSON 错误体，`1301` 显示内容审核拒绝；三协议必须收到完成信号，缺少信号的 EOF 显示流提前结束。服务 message、原文与凭据均不回显，不保存半页译文。
- 正常路径：同一真实配置使用两行普通日文样章，经生产 Pass 1 / Pass 2 得到 2 行非空译文，原始行号对齐，耗时 27.11 秒。临时记录已清理；真实作品正文、译文与诊断原始响应不进入仓库。
- 修复后重启应用，在真实阅读器中用该小说完整标题、7 个标签、324 字简介与 11,368 字正文再次点击翻译：最终显示「翻译服务因内容审核拒绝本页（错误码 1301），未保存本次译文」，翻译中状态正常结束、译文行数为 0，本小说没有新增翻译缓存。该篇在当前服务仍不能翻译，未将服务侧拒绝宣称为成功。全量离线验收 306 个单元测试、5 个接口守卫与 7 个命令冒烟通过；前端类型检查 / 生产构建与 Rust check 通过。

## 运行边界

默认 `./dev.ps1 test` 保持离线。在线测试需要在已初始化 MSVC 环境的终端，显式提供 `PIXIV_TRANSLATION_TEST_KEY`、`PIXIV_TRANSLATION_TEST_URL`、`PIXIV_TRANSLATION_TEST_MODEL` 三个进程环境变量后运行：

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml translation::tests::live_two_page_translation -- --ignored --exact --nocapture
cargo test --locked --manifest-path src-tauri/Cargo.toml translation::tests::live_english_target_translation -- --ignored --exact --nocapture
cargo test --locked --manifest-path src-tauri/Cargo.toml translation::tests::live_real_novel_page_translation -- --ignored --exact --nocapture
```

仅在获授权后运行；两页用例请求四次模型服务，英文目标用例两次，真实小说用例额外读取本机登录态抓取作品并请求两到三次（含一次故意的报错探测）。默认高级配置固定为 low。不把实际 Key 写进脚本、命令示例或报告，测试结束后清除临时环境变量。
