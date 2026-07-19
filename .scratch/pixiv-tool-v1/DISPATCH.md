# 调度协议（Dispatch Protocol）

> 本文件是 pixiv-tool V1 多 agent 调度的执行协议。
> 主 agent（调度型）按此协议工作，子 agent（实施型）参考此协议对齐预期。
> 入库版本控制，可迭代。

## 角色定义

### 主 agent（调度型）

- **不写业务代码**，只做：读文件、起子 agent、验证、commit、更新状态
- 负责：依赖管理、冲突识别、验收把关、状态推进
- 决策权：可以自行决定起几个子 agent、串行还是并行
- 上报权：遇到 SPEC 矛盾、ADR 冲突、acceptance criteria 无法满足时**停下问用户**

### 子 agent（实施型）

- 一次只负责**一个 ticket**
- 完整权限：Read / Write / Edit / Bash
- 完成后返回：commit hash、勾选的 acceptance criteria、解锁的后续 ticket

---

## 主 agent 工作循环

```
loop:
  1. 读 .scratch/pixiv-tool-v1/spec.md 找当前 frontier
     (frontier = 所有 blocker 已完成的 ticket)
  2. 如果 frontier 为空：
     - 如果所有 16 个 ticket 都 done → 任务完成，报告总结
     - 否则 → 有 ticket 卡住未完成，停下问用户
  3. 从 frontier 选 1 个 ticket（默认编号最小的）
     - 检查它和正在进行的子 agent 是否有文件冲突
     - 无冲突 → 可以并行起新子 agent
     - 有冲突 → 等冲突子 agent 完成再起
  4. 用下面的"子 agent 调用模板"起一个 Agent tool 调用
  5. 等子 agent 返回
  6. 验证（见下文"验收步骤"）
  7. 验证通过：
     - 在 ticket 文件顶部把 Status 改为 done
     - 在 spec.md 索引里把对应行打勾
     - commit 这两个状态变更
     - 回到 step 1
  8. 验证失败：
     - 记录失败原因
     - 同一个 ticket 最多重试 2 次
     - 第 3 次失败 → 停下问用户
```

### 并行策略（默认串行，可选并行）

**默认串行**：一次只起一个子 agent，等它完成。简单、无冲突、可控。

**可选并行**：当 frontier 有多个 ticket 且**文件路径不重叠**时可并行起。判断规则：

- 检查 ticket 的 acceptance criteria 里提到的文件路径
- 如果两个 ticket 都会改 `backend/pyproject.toml` 或 `backend/main.py` → **串行**（共享文件）
- 如果两个 ticket 改的文件完全不同 → 可并行

**已知冲突点**（实施时注意）：

| 冲突文件 | 涉及 ticket | 处理 |
|---|---|---|
| `backend/pyproject.toml` | 01/02/03/04/08 都会加依赖 | 01 先完成建基础，后续增量 |
| `backend/main.py` | 01/02/04 都会改启动逻辑 | 01 先完成，后续串行 |
| `frontend/package.json` | 05/10/14/15 都会加前端依赖 | 05 先完成，后续串行 |
| `frontend/src/router/index.ts` | 10/14/15 都会加路由 | 10 先完成，后续串行 |

**建议执行顺序**（平衡并行与冲突）：

```
串行: 01
并行: 02 + 03 + 04  (文件基本不重叠：db/cookies/settings 各自独立)
串行: 05
串行: 06 (依赖 03+05)
串行: 07 (依赖 06)
并行: 08 (依赖 02+06) + 09 (依赖 08) 实际串行因为依赖
串行: 10 (依赖 07+08+09)
并行: 11 + 13 + 14 (都依赖 10，文件不重叠：source/task-controls/history-page)
串行: 12 (依赖 11)
并行: 15 (依赖 04+10)
串行: 16 (依赖全部)
```

---

## 子 agent 调用模板

主 agent 用 Agent tool 起子 agent，调用参数模板：

```
Agent tool 调用：

description: "实现 ticket <NN>"
subagent_type: general-purpose
model: <见下表>
prompt:
  <见下文"子 agent 提示词模板"，把 <NN> 和 ticket 路径替换>
```

### model 选择

| Ticket | model | 理由 |
|---|---|---|
| 01 (骨架) | opus | 基础设施，影响后续所有 ticket |
| 02 (SQLite) | sonnet | 标准 CRUD，难度中等 |
| 03 (DPAPI) | opus | ctypes + 加密，易踩坑 |
| 04 (Settings) | sonnet | 标准 JSON 配置 |
| 05 (通信) | sonnet | FastAPI+SSE 标准模式 |
| 06 (登录窗) | opus | 复用 spike，但 pywebview 时序复杂 |
| 07 (登录态) | sonnet | 简单状态管理 |
| 08 (PixivClient) | opus | async + 限速 + 重试状态机 |
| 09 (Exporter) | sonnet | 字符串处理 + 文件 IO |
| 10 (单篇链路) | opus | 端到端整合，最复杂 |
| 11 (系列) | sonnet | 复用 Source 抽象 |
| 12 (用户全集) | sonnet | 复用 Source 抽象 |
| 13 (任务控制) | opus | 状态机 + asyncio 协调 |
| 14 (历史页) | sonnet | CRUD + 表格 UI |
| 15 (设置+i18n) | sonnet | 表单 + i18n 标准 |
| 16 (打包+CI) | opus | PyInstaller spec + YAML，易踩坑 |

### 子 agent 提示词模板

```
你是 pixiv-tool 项目的实施 agent，负责完成单个 ticket。

## 必读文件（按顺序读完再动手）

1. AGENTS.md —— 项目入口指引
2. docs/SPEC.md —— 完整规格书，重点读与你 ticket 相关的章节
3. docs/adr/ —— 读与你 ticket 相关的 ADR（见 ticket 文件里的引用）
4. .scratch/pixiv-tool-v1/issues/<NN>-<slug>.md —— 你的 ticket，含 acceptance criteria
5. spike/cookie_probe/probe.py —— 仅 ticket 06/08 需要，参考已验证的实现

## 你的 ticket

<把 ticket 文件完整内容贴这里，或让子 agent 自己 Read>

## 实施规则

1. 严格按 acceptance criteria 逐条实现，每完成一条在 ticket 文件里把 [ ] 改成 [x]
2. 写单元测试覆盖核心逻辑（每张 ticket 至少 5 个测试 case）
3. 遇到 SPEC 和 ADR 矛盾时，以 ADR 为准；如果 ADR 之间也矛盾，停下报告
4. 不要修改其他 ticket 涉及的文件（主 agent 会协调冲突）
5. 不要修改 docs/SPEC.md 和 docs/adr/*.md（如需变更先停下报告）
6. 不要修改 .scratch/ 下其他 ticket 文件
7. 命名和代码风格参考仓库现有代码（当前主要是 spike/cookie_probe/probe.py）
8. 注释密度匹配现有代码；PowerShell/Shell 脚本注释要详尽（方便跨平台转换）

## 提交规范

完成后用 conventional commit 提交：

  feat(<scope>): <短描述>

scope 用 ticket 范围（如 auth/storage/api/frontend/scripts/build）。
commit body 含：
  - 实现了哪些 acceptance criteria
  - 解锁了哪些后续 ticket（"Unblocks: NN, NN"）
  - 任何需要主 agent 注意的点

## 返回给主 agent

完成后在你的最终消息里包含：

- commit hash（git rev-parse HEAD）
- 已勾选的 acceptance criteria 数量 / 总数
- 修改了哪些文件（git diff --name-only HEAD~1）
- 是否有未解决的设计问题（如有，详细描述）
- 建议解锁的后续 ticket
```

---

## 验收步骤（主 agent 对子 agent 的产出把关）

子 agent 返回后，主 agent **不能盲信**，必须：

```
1. 检查 commit 是否存在：git log --oneline -1
2. 检查文件变更：git diff --name-only HEAD~1 HEAD
3. 读 ticket 文件，确认所有 [ ] 都已勾选为 [x]
4. 跑测试：
   - 后端：cd backend && uv run pytest tests/ -v
   - 前端：cd frontend && pnpm test（如果配了）
5. 如果 ticket 涉及可运行的功能：
   - 启动 dev 服务（./scripts/dev.ps1 start）
   - 手动或用 chrome-devtools MCP 验证关键路径
   - 停止服务
6. 检查是否误改了不该改的文件（SPEC、ADR、其他 ticket）
7. 全部通过 → 在 ticket 文件顶部 Status 改为 done
8. 部分失败 → 在 ticket 文件底部加 ## Comments 记录问题，重新起子 agent 修复
```

---

## 状态更新规则

### ticket 文件

每个 ticket 文件顶部有 `**Status:**` 字段。主 agent 负责：

- 子 agent 开始前：`**Status:** ready-for-agent` → `**Status:** in-progress`
- 子 agent 完成 + 主 agent 验收通过：→ `**Status:** done`
- 验收失败 + 重试中：→ `**Status:** needs-rework` + 在底部 `## Comments` 记录失败原因
- 卡住问用户：→ `**Status:** blocked` + 详细描述阻塞原因

### spec.md 索引

`.scratch/pixiv-tool-v1/spec.md` 里每个 ticket 是一个 markdown 链接。完成的 ticket 在链接前加 `[x]`：

```markdown
- [x] [01 — 项目骨架](issues/01-xxx.md)
- [ ] [02 — SQLite](issues/02-xxx.md)
```

每次状态变更都 commit。

---

## 失败处理

### 子 agent 报错或返回不完整

1. 在 ticket 文件底部 `## Comments` 记录错误信息
2. 把 Status 设为 `needs-rework`
3. 重新起子 agent，**在提示词里加上**：
   ```
  上次实施失败，错误信息：<贴错误>。
  请先分析失败原因（读相关代码 + 测试输出），再决定是修复还是重构。
  不要重复上次的错误。
   ```
4. 同一 ticket 最多重试 2 次，第 3 次失败停下问用户

### SPEC 矛盾或需求不清

子 agent 报告"无法决定"时，主 agent：

1. 先尝试读相关 ADR 自行判断
2. ADR 也不清楚 → 把 ticket Status 设为 `blocked`，在 `## Comments` 写清楚问题
3. 累积 3 个以上 blocked ticket 或遇到关键路径阻塞 → 停下问用户

### 测试不通过

1. 让子 agent 自己修（在重试提示词里贴失败测试输出）
2. 如果是测试本身写错了（断言过严），子 agent 可以修测试，但要在 commit message 里说明
3. 如果是 SPEC 设计有缺陷导致测试无法通过 → 停下问用户

---

## 停止条件

主 agent 在以下情况停止并报告用户：

1. ✅ **全部完成**：16 个 ticket 都 done，跑一次完整 dev 服务验证
2. 🚫 **关键阻塞**：3 个以上 ticket 同时 blocked，或关键路径（如 01/06/10）卡住
3. ⚠️ **重复失败**：同一 ticket 重试 3 次都失败
4. 🔍 **SPEC 缺陷**：发现 SPEC 或 ADR 有矛盾，需要用户决策
5. 📦 **范围漂移**：子 agent 实施时发现需要做 SPEC 没写的功能

---

## Git 工作流

- 所有工作在 `main` 分支（V1 项目小，不引入 feature branch 复杂度）
- 子 agent 自己 commit（每个 acceptance criteria 一组 commit，或整个 ticket 一个 commit）
- 主 agent 只 commit 状态变更（ticket Status + spec.md 索引）
- commit message 规范：
  - 子 agent：`feat(<scope>): <desc>` / `fix(<scope>): <desc>` / `refactor(<scope>): <desc>`
  - 主 agent：`chore(tickets): mark <NN> as done` / `chore(tickets): update spec.md index`

---

## 进度报告（可选）

主 agent 每完成 4 个 ticket（25% / 50% / 75% / 100%）向用户报告：

```
进度：<done>/<total> ticket 完成

已完成：
- 01 项目骨架
- 02 SQLite
- ...

进行中：
- 06 登录窗（in-progress）

Frontier（接下来可做）：
- 07 登录态
- 08 PixivClient

阻塞：
- 无 / <列出 blocked ticket>

下一步：起子 agent 实施 <NN>
```
