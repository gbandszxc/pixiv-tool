# 02 — SQLite + schema 初始化 + 基础模型

**What to build:**

从用户视角：App 首次启动时，自动在 `data/app.db` 创建 `novels` 和 `tasks` 两张表（含索引），用户无感知。其他模块能通过统一的数据库接口查询和写入小说/任务记录，例如 `db.insert_novel(...)` 写入后 `db.get_novel(id)` 能查出来。数据库使用 SQLite 标准库，零外部依赖。

**Blocked by:** None — 可立即开始

**Status:** ready-for-agent

**Acceptance criteria:**

- [ ] `backend/storage/db.py` 提供数据库连接管理（线程安全，FastAPI async 兼容）
- [ ] `backend/storage/models.py` 定义 `Novel` 和 `Task` 两个 dataclass（或 typed dict），字段对应 SPEC §5.1 schema
- [ ] `novels` 表字段完整：novel_id (PK)、title、series_id、series_order、author_id、author_name、page_count、text_length、captured_at、modification_date、txt_path、md_path、status
- [ ] `tasks` 表字段完整：task_id (PK)、source_type、source_id、status、total、done、skipped、failed_ids (JSON)、created_at、updated_at、error
- [ ] 索引正确建立：`idx_novels_series` on novels(series_id)、`idx_novels_author` on novels(author_id)
- [ ] App 启动时自动执行 schema 初始化（CREATE TABLE IF NOT EXISTS），幂等可重复执行
- [ ] 数据库文件位置正确：`data/app.db`（portable 模式，与 exe 同级）
- [ ] 提供 CRUD 接口：`insert_novel`、`get_novel`、`is_downloaded`、`list_novels`、`insert_task`、`update_task`、`get_task`
- [ ] 所有写入操作包在事务里（spec §11 R6）
- [ ] 单元测试覆盖：建表幂等性、CRUD 基础流程、索引命中
- [ ] `data/app.db` 已加入 `.gitignore`（前序工作已完成）
