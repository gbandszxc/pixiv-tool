# 资源来源与许可说明

## 来源

- 源目录：`D:/ProjectSpace/skills/my-skills/common/smart-translator`
- 源仓库 HEAD（导入时）：`99beb614573c3200f24f15b5f359b8459c93b17e`，提交时间 2026-10-18
- 导入日期：2026-10-24
- 导入范围：`SKILL.md`、`references/domain-acg.md`、`references/domain-academic.md`、`references/domain-legal.md`、`references/domain-literature.md`、`references/domain-news.md`、`references/domain-tech.md`
- 未导入：`evals/`、`.idea/`，以及源目录 `scripts/` 下的五个辅助程序。辅助程序仅用于核对确定性职责，其能力已由 Rust 工具承接。

导入时源文件 SHA-256（原始字节）：

| 源文件 | SHA-256 |
| --- | --- |
| `SKILL.md` | `53e80ab3f5a1e3cb49de118ae63534e24f56c8357dd1e058a58564924bf05b07` |
| `references/domain-academic.md` | `17bc89755ae6efb4438182a7b3e118200de0c1161e5dfd4d010fd143e8260f44` |
| `references/domain-acg.md` | `82f5561813d7b8e7a385281657d604f5f5a0717a5782d2537bb77da9051584a4` |
| `references/domain-legal.md` | `9fc485fe70540a88c4dd729122620ac4405b1b0b35658ef6d5eddce03b85c653` |
| `references/domain-literature.md` | `fbde48b4b25d612f7d2edd8a75bb0a6bf9e0b4760970cc3bec92ef1a76cc5fd9` |
| `references/domain-news.md` | `62735391958f0d8a6b91e03863b0373e41475c1275bc7217316240fff84d5f98` |
| `references/domain-tech.md` | `63038178b6501b11bd96055ed40120c6de62a2f80791ce7dd336470cc8a2e28b` |

## 版本与适配说明

- 本目录内容对应源 skill 的 2026-10-01 版本。源 SKILL 为 677 行，本仓库保留其 Pass 0～3、术语锁定、风格约束、逐块对照、整体复核与局部返工方法论。
- 适配目标：运行时不依赖源目录或任何本机路径，不调用外部解释器或进程；原文注册、切块、术语聚合、译文合并、审查汇总与阶段重置全部由 Rust 工具完成。
- 适配删改：删除工作目录、文件落盘、并行子任务文件归属、人工编辑译文与外部进程操作；替换为受控工具契约、资源 ID、结构化产物与运行层校验。
- 适配保留：领域词库、日译中专用规则、Markdown 保真清单、术语冲突处理、风格维度、审查问题分级与修复路径。
- 新增内容：`prompts/system.md` 占位系统提示词、`agents/` 三个角色定义、本说明与 `README.md`。这些内容不是源 skill 原文。
- 2026-10-27 适配修订（内容过滤触发率优化）：子 Agent 系统提示词瘦身为仅 `prompts/system.md`，角色契约与工具契约改随首条用户消息注入；冻结术语与风格从派发消息迁移到 `read_chunk` 工具返回；`role:translator` 的日译中拟声拟态词示例改为中性表述。资源包 hash 由编译期自动重算，进行中的旧 Job 按恢复规则校验。

## 许可核对结论

源目录及其上级 `D:/ProjectSpace/skills/my-skills` 中未发现 `LICENSE`、`COPYING` 或其他独立许可证文件。

结论：该 skill 为用户自有技能，未附带独立许可证，随本仓库分发。若后续上游补充许可证或变更授权条款，以补充文件为准，并在同一变更中更新本说明与 `README.md` 的资源版本记录。

## 分发注意

- 领域参考含 ACG 与同人常用词，仅供翻译时统一译法，不代表产品允许抓取或展示超出既有范围的内容。
- 本目录不得写入登录凭据、Cookie、token、用户原文或译文。
- 资源文件使用 UTF-8 无 BOM、LF 换行；修改后必须按 `README.md` 的口径更新资源 hash 记录。
