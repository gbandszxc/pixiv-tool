# Domain Docs

工程 skills 探索代码库时应如何消费本仓库的领域文档。

## 探索前必读

- **`docs/SPEC.md`** —— 本项目的单一真相源（13 章 + 风险登记 + ADR 索引）
- **`docs/adr/`** —— 读涉及你即将修改区域的 ADR。当前已有：
  - `0001-pywebview-fastapi-vue.md`：技术栈选型
  - `0002-login-strategy.md`：登录策略
  - `0003-task-model.md`：任务模型
  - `0004-cookie-storage.md`：cookie 存储
  - `0005-cookie-probe-result.md`：R1 spike 结论（含 5 个 bug 修复详情）

> 注：标准 mattpocock 流程用 `CONTEXT.md` 作为术语表。本项目目前用 `docs/SPEC.md` 统一承载规格 + 术语，未单独建 `CONTEXT.md`。如果术语开始膨胀（>20 个专有名词），再用 `/domain-modeling` 拆出 `CONTEXT.md`。

## 文件结构

单上下文仓库：

```
/
├── docs/
│   ├── SPEC.md                    ← 真相源（项目规格 + 风险登记）
│   ├── adr/                       ← 架构决策记录
│   │   ├── 0001-pywebview-fastapi-vue.md
│   │   └── ...
│   └── agents/                    ← skills 配置（本目录）
├── spike/                         ← 风险验证脚本
│   └── cookie_probe/
├── src/pixiv_tool/                ← Python FastAPI 后端
└── frontend/                      ← Vue3 + Vite
```

## 用 SPEC 的术语

当你的输出（issue 标题、重构提案、假设、测试名）命名一个领域概念时，使用 `docs/SPEC.md` 中定义的术语。不要漂移到同义词。

如果你需要的概念不在 SPEC 里——这是个信号：要么你在发明项目不用的语言（重新考虑），要么有真实空缺（标记给后续 grilling）。

## 标记 ADR 冲突

如果你的输出与某个 ADR 矛盾，**显式指出**而非悄悄覆盖：

> _与 ADR-0002（嵌入式 WebView 登录主导）矛盾 —— 但值得重开因为..._

## 不要做的事

- 不要悄悄改 `docs/SPEC.md` —— 任何变更要么先在对话里对齐，要么新增 ADR
- 不要假设旧 spec 内容仍然有效 —— 先读最新版（git 历史）
- 不要复用 spike 的代码到主代码 —— spike 是参考实现，主代码要重新组织（见 ADR 0005 复用清单）
