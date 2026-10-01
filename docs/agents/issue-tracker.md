# Issue tracker: Local Markdown

本项目用仓库内的 markdown 文件追踪 issue 和 spec。

## 约定

- 一个 feature 一个目录：`.scratch/<feature-slug>/`
- spec 文件：`.scratch/<feature-slug>/spec.md`
- 实现 issue 一票一文件：`.scratch/<feature-slug>/issues/<NN>-<slug>.md`，从 `01` 起编号，**绝不要合并成单个 tickets 文件**
- triage 状态记录在每个 issue 文件顶部的 `Status:` 行（标签字符串见 `triage-labels.md`）
- 评论和对话历史追加到文件底部 `## Comments` 标题下

## 当某个 skill 说"发布到 issue tracker"

在 `.scratch/<feature-slug>/` 下创建新文件（必要时建目录）。

## 当某个 skill 说"获取相关 ticket"

读取被引用路径的文件。用户通常会直接传路径或 issue 编号。

## Wayfinding 操作

`/wayfinder` 使用。**map** 是一个文件，每个 **child ticket** 一个子文件。

- **Map**：`.scratch/<effort>/map.md` —— Notes / Decisions-so-far / Fog body
- **Child ticket**：`.scratch/<effort>/issues/NN-<slug>.md`，从 `01` 起编号，body 里写问题。`Type:` 行记录 ticket 类型（`research`/`prototype`/`grilling`/`task`）；`Status:` 行记录 `claimed`/`resolved`
- **Blocking**：顶部 `Blocked by: NN, NN` 行。当它列的所有文件都 `resolved` 时该 ticket 解锁
- **Frontier**：扫 `.scratch/<effort>/issues/` 找打开、未阻塞、未 claim 的文件；按编号小的优先
- **Claim**：开工前设 `Status: claimed` 并保存
- **Resolve**：在 `## Answer` 标题下追加答案，设 `Status: resolved`，然后给 map 的 Decisions-so-far 追加上下文指针（gist + link）

## 当前进度

- **当前 feature**：`pixiv-tool-v1`（路径 `.scratch/pixiv-tool-v1/`；slug 沿用旧栈命名，新 ticket 继续在此递增编号）
- **上游 spec**：`docs/SPEC.md`（13 章）
- **关联 ADR**：`docs/adr/0001` ~ `0012`

## 版本控制

`.scratch/` **不入库**（见 `.gitignore`）——它是本机的 local markdown tracker，
换台机器 clone 不到 ticket，属预期行为。

pywebview 旧栈的内容已于 2026-10-01 移出 git：`.scratch/pixiv-tool-v1` 的
`DISPATCH.md` / `SESSION-2026-07-19.md` / `spec.md` / `issues/01`~`18`、整个
`.scratch/pixiv-tool-v2/`，以及 `.zcode/plans/` 下的旧栈规划，均归档至
`.archive/pywebview-era/`（同样不入库，见其 README）。它们描述的是已不存在的
`backend/`、`dev.ps1`、pywebview 登录窗与 PyInstaller 链路，不构成项目文档。

