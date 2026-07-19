# Triage Labels

Skills 用五个标准 triage role 交流。本文件把这些 role 映射到本仓库实际使用的标签字符串。

| mattpocock/skills 中的 role | 本仓库标签 | 含义 |
| --- | --- | --- |
| `needs-triage` | `needs-triage` | 维护者需要评估此 issue |
| `needs-info` | `needs-info` | 等待提交者补充信息 |
| `ready-for-agent` | `ready-for-agent` | 已完整规格化，AFK agent 可以领 |
| `ready-for-human` | `ready-for-human` | 需要人工实现 |
| `wontfix` | `wontfix` | 不会处理 |

当某个 skill 提到某个 role（如"apply the AFK-ready triage label"）时，使用上表对应的标签字符串。

本项目用 local markdown tracker，标签作为 issue 文件顶部的 `Status:` 行值。
