# V2-02 — 前端 + 依赖技术债清理

**What to build:**

清理 3 个记录在 SESSION-2026-07-19 §2.2 的低风险技术债，让 `pnpm build` 和 `pnpm install` 流程完全干净。不动功能。

**Blocked by:** None — 独立。但建议在 V2-01（src layout 重构）之后做，避免重构期两摊改动混在一起。

**Status:** done（2026-07-22 实施，commit `af6aa75`）

## 子项 1：`frontend/pnpm-lock.yaml` 入库

**现状**：`frontend/pnpm-lock.yaml` 是 untracked（历史遗留）。团队成员 clone 后 `pnpm install` 可能装到不一致的版本，破坏可重现构建。

**修法**：
```bash
git add frontend/pnpm-lock.yaml
git commit -m "chore(frontend): 跟踪 pnpm-lock.yaml 保证可重现安装"
```

**验收**：
- [ ] `git ls-files frontend/pnpm-lock.yaml` 返回该文件
- [ ] 在新目录 clone + `pnpm install --frozen-lockfile --dir frontend` 不报错
- [ ] `.gitignore` 没有意外 ignore 它（检查 `frontend/` 下没有 `pnpm-lock.yaml` 规则）

## 子项 2：`auth.ts` 动态 + 静态导入冲突

**现状**：`pnpm build` 时 Vite 报警告：
```
(!) D:/...frontend/src/stores/auth.ts is dynamically imported by .../src/api/index.ts
but also statically imported by .../src/App.vue?... , .../src/views/SettingsView.vue?...
dynamic import will not move module into another chunk.
```

**根因**：`frontend/src/api/index.ts` 用了 `const authStore = useAuthStore()` 或类似动态 `import()` 引入 `stores/auth`，但 `App.vue` 和 `SettingsView.vue` 又静态 `import` 了它。Vite 想做 code-split 但被静态 import 拉回主 bundle，只能放弃并警告。

**修法**：把 `api/index.ts` 里的动态 import 改成静态 import。auth store 本来就该在主 bundle（每个页面都要看登录态），没必要 code-split。

```typescript
// api/index.ts 改前(大概):
const { useAuthStore } = await import('@/stores/auth')

// 改后:
import { useAuthStore } from '@/stores/auth'
```

实际代码位置和写法以 `frontend/src/api/index.ts` 现状为准——先读文件确认动态 import 的确切形态再改。

**验收**：
- [ ] `cd frontend && pnpm build` 输出里 `dynamically imported but also statically imported` 警告消失
- [ ] 前端功能不破：浏览器打开 dev 页面，登录态显示、抓取提交、设置页跳转都正常

## 子项 3：`httpx2` 弃用警告跟踪（不主动改）

**现状**：pytest 跑完会输出：
```
StarletteDeprecationWarning: Using `httpx` with `starlette.testclient` is deprecated;
install `httpx2` instead.
```

**根因**：FastAPI/Starlette 的 TestClient 在新版开始推荐 httpx2（httpx 的异步分支）。但 httpx2 API 和 httpx 不完全兼容，主动换会破坏现有测试。

**版本组合（2026-07-22 记录，来源 `.venv` 里的 `python -c "import ...; print(...)"`）**：
- fastapi 0.139.2
- starlette 1.3.1
- httpx 0.28.1

**修法**：**不主动改**。只做两件事：
1. 在本 ticket 记录该警告来自哪个 starlette/fastapi/httpx 版本组合（见上）
2. 留 TODO：未来 fastapi/starlette 主版本升级时，检查 httpx2 是否成熟，再统一迁移

TODO 已写入 `pyproject.toml`（`[project]` 段后的注释块，`TODO(V2-02)`），指向本 ticket。

**验收**：
- [x] ticket 里记录当前 fastapi/starlette/httpx 版本号（`pip show` 或 `uv pip list`）
- [x] 在 `pyproject.toml` 或某个 README/CHANGELOG 里留个 TODO 注释指向本 ticket
- [x] 不改任何测试代码

## Acceptance criteria（整 ticket）

- [ ] 3 个子项都有对应 commit
- [ ] `cd frontend && pnpm install --frozen-lockfile` 干净通过
- [ ] `cd frontend && pnpm build` 零警告（至少 auth.ts 那条消失）
- [ ] `pytest tests/` 90 个测试全过
- [ ] 前端功能不破（dev 模式打开浏览器肉眼检查菜单 + 抓取页 + 设置页）

## 实施建议

按子项顺序做，每个一个 commit。子项 1 最简单（1 行 git add）；子项 2 要读代码确认动态 import 形态；子项 3 纯记录不动代码。

**预计工作量**：30 分钟（主要是子项 2 读代码 + 测）。
