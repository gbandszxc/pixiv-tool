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

## 运行边界

默认 `./dev.ps1 test` 保持离线。在线测试需要在已初始化 MSVC 环境的终端，显式提供 `PIXIV_TRANSLATION_TEST_KEY`、`PIXIV_TRANSLATION_TEST_URL`、`PIXIV_TRANSLATION_TEST_MODEL` 三个进程环境变量后运行：

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml translation::tests::live_two_page_translation -- --ignored --exact --nocapture
```

仅在获授权后运行；该测试会请求四次模型服务，默认高级配置固定为 low。不把实际 Key 写进脚本、命令示例或报告，测试结束后清除临时环境变量。
