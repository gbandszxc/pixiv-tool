# ADR 0014 — 浏览访问历史持久化（SQLite 表 + 3 个 IPC 命令）

日期：2026-10-02 · 状态：已接受

## 背景

自有浏览 UI（ADR 0012）已覆盖首页/频道/发现/动态/搜索/排行/收藏等列表页与作品查看器，
但用户浏览过的作品没有留下任何访问痕迹：离开查看器后无从回顾「看过什么」，也无法据此
回到此前看过的作品。需求：

- 记录浏览访问（插画 / 漫画 / 小说的作品级访问），网格回显封面；
- 支持分页浏览访问记录，最近访问在前；
- 支持一键清空全部访问历史；
- 访问记录跨重启持久化。

该约束要求方案具备：进程重启后仍在、查询可排序分页、清空为原子操作，且不引入新依赖、
不扩大登录态落点。

## 决策

### 1. SQLite 新表 `browse_history`

追加到 `db.rs::SCHEMA_SQL`（`CREATE TABLE IF NOT EXISTS` 幂等建表）：

```sql
CREATE TABLE IF NOT EXISTS browse_history (
    work_id     INTEGER NOT NULL,
    kind        TEXT    NOT NULL,
    title       TEXT    NOT NULL DEFAULT '',
    author_id   INTEGER NOT NULL DEFAULT 0,
    author_name TEXT    NOT NULL DEFAULT '',
    cover       TEXT    NOT NULL DEFAULT '',
    page_count  INTEGER NOT NULL DEFAULT 0,
    x_restrict  INTEGER NOT NULL DEFAULT 0,
    visited_at  TEXT    NOT NULL,
    PRIMARY KEY (kind, work_id)
);
CREATE INDEX IF NOT EXISTS idx_browse_history_visited ON browse_history(visited_at DESC);
```

- 主键 `(kind, work_id)`：同一作品重复访问用 `INSERT OR REPLACE` 覆写并按新
  `visited_at` 置顶，不产生重复行；
- `visited_at` 复用既有 `now_iso()`（UTC ISO8601 TEXT），与 novels/illustrations 一致；
- 只读列（title/author/cover/page_count/x_restrict）随访问快照写入，列表页直接回显、
  不再回源接口；`cover` 缺省归一为空串；
- 表名为 `browse_history`，与既有「抓取历史」（`history_list` 对 novels+illustrations
  的联合查询）命名空间分离，互不混淆。

### 2. 三个 IPC 命令（`commands/browse_history_cmds.rs`）

| 命令 | 返回体 |
|---|---|
| `browse_history_record(kind, workId, title, authorId, authorName, cover?, pageCount, xRestrict)` | `{status:"success"}` |
| `browse_history_list(page, pageSize, kind?)` | `{items:[…], total, page, page_size}`（`ORDER BY visited_at DESC, work_id DESC`；`kind` 省略=全部，否则 illust/manga/novel 过滤） |
| `browse_history_clear()` | `{status:"success", deleted:n}` |

沿用现有薄壳结构：`#[tauri::command] pub async fn` 转发到 `*_impl(&AppState, …)`，
后者不依赖 Tauri 运行时、可离线冒烟直调（同 `history_list` 先例）。参数 JS 侧 camelCase、
Rust snake_case，由 Tauri 2 自动映射。

### 3. 上报时机与入口

- 作品详情页（插画/漫画/小说）加载成功后由前端调用 `browse_history_record` 上报一次；
- 侧栏「收藏」下方新增「浏览历史」入口（`/browse/history`），网格回显 + 分页 + 一键清空。
  前端实现见 SPEC §6.1；本条只冻结后端契约。

## 备选方案

- **localStorage（前端自持）**：否决——多账号/多设备不共享、无跨端一致的重启语义、
  清空与分页都在前端重复实现，且与「后端数据统一落 app.db」的既有约定背离。
- **写入 `settings.json`**：否决——settings 是配置真相源（白名单 14 键 + 校验），
  访问历史是会持续增长的运行时数据，混入配置会污染设置读写与迁移路径。
- **复用 novels/illustrations 表加访问时间列**：否决——两表语义是「已抓取产物」，
  浏览访问不产生抓取产物；且跨 kind 统一行形状需再 UNION，不如独立表简洁。

## 后果

- 旧库无需 ALTER：下次 `Db::open` 时 `SCHEMA_SQL` 幂等补建新表与索引，已有数据不受影响；
- 访问记录进入 `data/app.db`（用户数据目录，绝不入库），不涉及任何凭据，边界不变；
- 命令计数由 49 增至 52（新增 browse_history 分组 3 个），SPEC §3.4 / §7 同步更新；
- 增长无上限：当前不做自动淘汰，由用户「一键清空」治理；若记录量级成为问题，再评估
  按时间/条数上限淘汰（本 ADR 不预设）。
