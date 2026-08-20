//! SQLite 数据库封装 —— 表结构与旧 Python `storage/db.py` 完全一致
//! （可无缝共用同一个 `data/app.db`）。
//!
//! 连接模型：`Arc<Mutex<rusqlite::Connection>>`（打开时 `journal_mode=WAL`、
//! `foreign_keys=ON`）。Db 可 Clone（共享同一连接）。

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, params, params_from_iter};
use serde::Serialize;
use serde_json::Value as JsonValue;

/// 终态任务状态集合（删除任务前判断是否需要先 cancel）。
pub const TERMINAL_TASK_STATUSES: [&str; 3] = ["done", "failed", "canceled"];

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS novels (
    novel_id          INTEGER PRIMARY KEY,
    title             TEXT NOT NULL,
    series_id         INTEGER,
    series_order      INTEGER,
    author_id         INTEGER NOT NULL,
    author_name       TEXT,
    page_count        INTEGER,
    text_length       INTEGER,
    captured_at       TEXT NOT NULL,
    modification_date TEXT,
    txt_path          TEXT,
    md_path           TEXT,
    status            TEXT NOT NULL DEFAULT 'ok'
);
CREATE INDEX IF NOT EXISTS idx_novels_series ON novels(series_id);
CREATE INDEX IF NOT EXISTS idx_novels_author ON novels(author_id);

CREATE TABLE IF NOT EXISTS tasks (
    task_id     TEXT PRIMARY KEY,
    source_type TEXT NOT NULL,
    source_id   TEXT NOT NULL,
    category    TEXT NOT NULL DEFAULT 'novel',
    status      TEXT NOT NULL DEFAULT 'pending',
    total       INTEGER DEFAULT 0,
    done        INTEGER DEFAULT 0,
    skipped     INTEGER DEFAULT 0,
    failed_ids  TEXT DEFAULT '[]',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    error       TEXT
);

CREATE TABLE IF NOT EXISTS illustrations (
    artwork_id  INTEGER PRIMARY KEY,
    title       TEXT NOT NULL,
    author_id   INTEGER NOT NULL,
    author_name TEXT,
    illust_type INTEGER DEFAULT 0,
    page_count  INTEGER DEFAULT 1,
    saved_paths TEXT DEFAULT '[]',
    captured_at TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'ok'
);
CREATE INDEX IF NOT EXISTS idx_illustrations_author ON illustrations(author_id);
";

/// 统一 UTC ISO 时间戳（等价 Python `datetime.now(timezone.utc).isoformat()`）。
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// novels 表整行。
#[derive(Debug, Clone, Serialize)]
pub struct NovelRow {
    pub novel_id: i64,
    pub title: String,
    pub series_id: Option<i64>,
    pub series_order: Option<i64>,
    pub author_id: i64,
    pub author_name: Option<String>,
    pub page_count: Option<i64>,
    pub text_length: Option<i64>,
    pub captured_at: String,
    pub modification_date: Option<String>,
    pub txt_path: Option<String>,
    pub md_path: Option<String>,
    pub status: String,
}

/// tasks 表整行（failed_ids 是原始 JSON 文本，前端自行解析）。
#[derive(Debug, Clone, Serialize)]
pub struct TaskRow {
    pub task_id: String,
    pub source_type: String,
    pub source_id: String,
    pub category: String,
    pub status: String,
    pub total: i64,
    pub done: i64,
    pub skipped: i64,
    pub failed_ids: String,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<String>,
}

/// illustrations 表整行（saved_paths 是原始 JSON 文本）。
#[derive(Debug, Clone, Serialize)]
pub struct IllustrationRow {
    pub artwork_id: i64,
    pub title: String,
    pub author_id: i64,
    pub author_name: Option<String>,
    pub illust_type: i64,
    pub page_count: i64,
    pub saved_paths: String,
    pub captured_at: String,
    pub status: String,
}

/// history 联合查询行（统一形状，见 `Db::list_history`）。
#[derive(Debug, Clone, Serialize)]
pub struct HistoryRow {
    pub id: i64,
    /// "novel" | "illustration"
    pub category: String,
    pub title: String,
    pub author_name: Option<String>,
    pub pages: Option<i64>,
    /// 仅小说有；插画为 None。
    pub series_id: Option<i64>,
    /// 仅插画有；小说为 None。
    pub illust_type: Option<i64>,
    pub captured_at: String,
}

/// novels 插入参数（INSERT OR REPLACE）。
#[derive(Debug, Clone)]
pub struct NovelInsert {
    pub novel_id: i64,
    pub title: String,
    pub series_id: Option<i64>,
    pub series_order: Option<i64>,
    pub author_id: i64,
    pub author_name: Option<String>,
    pub page_count: Option<i64>,
    pub text_length: Option<i64>,
    pub captured_at: String,
    pub modification_date: Option<String>,
    pub txt_path: Option<String>,
    pub md_path: Option<String>,
    pub status: String,
}

impl Default for NovelInsert {
    fn default() -> Self {
        Self {
            novel_id: 0,
            title: String::new(),
            series_id: None,
            series_order: None,
            author_id: 0,
            author_name: None,
            page_count: None,
            text_length: None,
            captured_at: String::new(),
            modification_date: None,
            txt_path: None,
            md_path: None,
            status: "ok".into(),
        }
    }
}

/// illustrations 插入参数（INSERT OR REPLACE）。saved_paths 是 JSON 数组文本。
#[derive(Debug, Clone)]
pub struct IllustrationInsert {
    pub artwork_id: i64,
    pub title: String,
    pub author_id: i64,
    pub author_name: Option<String>,
    pub illust_type: i64,
    pub page_count: i64,
    pub saved_paths: String,
    pub captured_at: String,
    pub status: String,
}

impl Default for IllustrationInsert {
    fn default() -> Self {
        Self {
            artwork_id: 0,
            title: String::new(),
            author_id: 0,
            author_name: None,
            illust_type: 0,
            page_count: 1,
            saved_paths: "[]".into(),
            captured_at: String::new(),
            status: "ok".into(),
        }
    }
}

/// tasks 插入参数。
#[derive(Debug, Clone)]
pub struct TaskInsert {
    pub task_id: String,
    pub source_type: String,
    pub source_id: String,
    pub category: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Default for TaskInsert {
    fn default() -> Self {
        Self {
            task_id: String::new(),
            source_type: String::new(),
            source_id: String::new(),
            category: "novel".into(),
            status: "pending".into(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}

/// novels 分页过滤条件。
#[derive(Debug, Clone)]
pub struct NovelFilter {
    pub series_id: Option<i64>,
    pub author_id: Option<i64>,
    /// title LIKE '%kw%'
    pub keyword: Option<String>,
    pub page: i64,
    pub page_size: i64,
}

impl Default for NovelFilter {
    fn default() -> Self {
        Self {
            series_id: None,
            author_id: None,
            keyword: None,
            page: 1,
            page_size: 50,
        }
    }
}

/// illustrations 分页过滤条件（无 series_id）。
#[derive(Debug, Clone)]
pub struct IllustrationFilter {
    pub author_id: Option<i64>,
    /// title LIKE '%kw%'
    pub keyword: Option<String>,
    pub page: i64,
    pub page_size: i64,
}

impl Default for IllustrationFilter {
    fn default() -> Self {
        Self {
            author_id: None,
            keyword: None,
            page: 1,
            page_size: 50,
        }
    }
}

/// 线程安全的 SQLite 封装（可 Clone 共享连接）。
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

/// 动态 SQL 的 IN 占位符："?,?,?"。
fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// serde_json::Value → rusqlite 值（数组/对象序列化为 JSON 文本）。
fn json_to_sql(value: &JsonValue) -> SqlValue {
    match value {
        JsonValue::Null => SqlValue::Null,
        JsonValue::Bool(b) => SqlValue::Integer(i64::from(*b)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                SqlValue::Real(f)
            } else {
                SqlValue::Text(n.to_string())
            }
        }
        JsonValue::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

impl Db {
    /// 打开（或创建）数据库并初始化 schema + 轻量迁移。
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建数据库目录失败: {e}"))?;
        }
        let conn = Connection::open(path)
            .map_err(|e| format!("打开数据库失败 {}: {e}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("设置数据库 PRAGMA 失败: {e}"))?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, rusqlite::Error>,
    ) -> Result<T, String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| "数据库连接锁中毒".to_string())?;
        f(&conn).map_err(|e| e.to_string())
    }

    fn init_schema(&self) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute_batch(SCHEMA_SQL)?;
            // 轻量迁移：旧库的 tasks 表没有 category 列（CREATE TABLE IF NOT
            // EXISTS 不会补列，需要显式 ALTER）。
            let has_category: bool = conn
                .prepare("PRAGMA table_info(tasks)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<String>, _>>()?
                .iter()
                .any(|c| c == "category");
            if !has_category {
                conn.execute(
                    "ALTER TABLE tasks ADD COLUMN category TEXT NOT NULL DEFAULT 'novel'",
                    [],
                )?;
            }
            Ok(())
        })
    }

    // ------------------------------------------------------------------
    // novels
    // ------------------------------------------------------------------

    /// 插入（INSERT OR REPLACE，同 id 覆写）。
    pub fn insert_novel(&self, row: &NovelInsert) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO novels
                 (novel_id, title, series_id, series_order, author_id, author_name,
                  page_count, text_length, captured_at, modification_date, txt_path, md_path, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    row.novel_id,
                    row.title,
                    row.series_id,
                    row.series_order,
                    row.author_id,
                    row.author_name,
                    row.page_count,
                    row.text_length,
                    row.captured_at,
                    row.modification_date,
                    row.txt_path,
                    row.md_path,
                    row.status,
                ],
            )?;
            Ok(())
        })
    }

    /// 更新 txt_path / md_path（只更新非 None 字段，动态 SET）。
    pub fn update_novel_paths(
        &self,
        novel_id: i64,
        txt_path: Option<&str>,
        md_path: Option<&str>,
    ) -> Result<(), String> {
        let mut sets: Vec<(&str, SqlValue)> = Vec::new();
        if let Some(t) = txt_path {
            sets.push(("txt_path", SqlValue::from(t.to_string())));
        }
        if let Some(m) = md_path {
            sets.push(("md_path", SqlValue::from(m.to_string())));
        }
        if sets.is_empty() {
            return Ok(());
        }
        let set_clause = sets
            .iter()
            .map(|(k, _)| format!("{k} = ?"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("UPDATE novels SET {set_clause} WHERE novel_id = ?");
        self.with_conn(move |conn| {
            conn.execute(
                &sql,
                params_from_iter(
                    sets.iter()
                        .map(|(_, v)| v.clone())
                        .chain(std::iter::once(SqlValue::Integer(novel_id))),
                ),
            )?;
            Ok(())
        })
    }

    /// 去重判断：novel_id 已存在（错误吞掉返回 false 并打日志）。
    pub fn is_novel_downloaded(&self, novel_id: i64) -> bool {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM novels WHERE novel_id = ?1",
                    params![novel_id],
                    |_| Ok(()),
                )
                .is_ok())
        })
        .unwrap_or_else(|err| {
            log::warn!("is_novel_downloaded({novel_id}) 查询失败: {err}");
            false
        })
    }

    pub fn get_novel(&self, novel_id: i64) -> Option<NovelRow> {
        match self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM novels WHERE novel_id = ?1",
                params![novel_id],
                novel_from_row,
            )
        }) {
            Ok(row) => Some(row),
            Err(err) => {
                if !err.contains("Query returned no rows") {
                    log::warn!("get_novel({novel_id}) 查询失败: {err}");
                }
                None
            }
        }
    }

    /// 分页查询（title LIKE，ORDER BY captured_at DESC LIMIT/OFFSET）→ (rows, total)。
    pub fn list_novels(&self, filter: &NovelFilter) -> Result<(Vec<NovelRow>, i64), String> {
        let (where_clause, mut sql_params): (String, Vec<SqlValue>) = novel_where(filter);
        let total = self.with_conn(|conn| {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM novels{where_clause}"),
                params_from_iter(sql_params.iter()),
                |row| row.get::<_, i64>(0),
            )
        })?;
        let offset = (filter.page - 1).max(0) * filter.page_size;
        sql_params.push(SqlValue::Integer(filter.page_size));
        sql_params.push(SqlValue::Integer(offset));
        let rows = self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT * FROM novels{where_clause} ORDER BY captured_at DESC LIMIT ? OFFSET ?"
            ))?;
            let rows = stmt
                .query_map(params_from_iter(sql_params.iter()), novel_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        Ok((rows, total))
    }

    /// 批量删除 novel 记录，返回删除条数。
    /// delete_files=true 时先取 txt/md 路径，逐个 unlink（失败仅忽略、打日志）。
    pub fn delete_novels(&self, ids: &[i64], delete_files: bool) -> Result<usize, String> {
        if ids.is_empty() {
            return Ok(0);
        }
        let id_params: Vec<SqlValue> = ids.iter().map(|i| SqlValue::Integer(*i)).collect();
        let ph = placeholders(ids.len());
        self.with_conn(|conn| {
            if delete_files {
                let mut stmt = conn.prepare(&format!(
                    "SELECT txt_path, md_path FROM novels WHERE novel_id IN ({ph})"
                ))?;
                let paths: Vec<Option<String>> = stmt
                    .query_map(params_from_iter(id_params.iter()), |row| {
                        Ok([
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, Option<String>>(1)?,
                        ])
                    })?
                    .flatten()
                    .flat_map(|pair| pair.into_iter())
                    .collect();
                for path in paths.into_iter().flatten() {
                    if let Err(err) = std::fs::remove_file(&path) {
                        if err.kind() != std::io::ErrorKind::NotFound {
                            log::warn!("删除文件失败 {path}: {err}");
                        }
                    }
                }
            }
            let deleted = conn.execute(
                &format!("DELETE FROM novels WHERE novel_id IN ({ph})"),
                params_from_iter(id_params.iter()),
            )?;
            Ok(deleted)
        })
    }

    /// 清空 novels 表，返回删除条数（delete_files 语义同上）。
    pub fn delete_all_novels(&self, delete_files: bool) -> Result<usize, String> {
        self.with_conn(|conn| {
            if delete_files {
                let mut stmt = conn.prepare("SELECT txt_path, md_path FROM novels")?;
                let paths: Vec<Option<String>> = stmt
                    .query_map([], |row| {
                        Ok([
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, Option<String>>(1)?,
                        ])
                    })?
                    .flatten()
                    .flat_map(|pair| pair.into_iter())
                    .collect();
                for path in paths.into_iter().flatten() {
                    if let Err(err) = std::fs::remove_file(&path) {
                        if err.kind() != std::io::ErrorKind::NotFound {
                            log::warn!("删除文件失败 {path}: {err}");
                        }
                    }
                }
            }
            let total: i64 = conn.query_row("SELECT COUNT(*) FROM novels", [], |row| row.get(0))?;
            conn.execute("DELETE FROM novels", [])?;
            Ok(total as usize)
        })
    }

    // ------------------------------------------------------------------
    // illustrations
    // ------------------------------------------------------------------

    /// 插入（INSERT OR REPLACE，同 id 覆写）。
    pub fn insert_illustration(&self, row: &IllustrationInsert) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO illustrations
                 (artwork_id, title, author_id, author_name, illust_type, page_count,
                  saved_paths, captured_at, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    row.artwork_id,
                    row.title,
                    row.author_id,
                    row.author_name,
                    row.illust_type,
                    row.page_count,
                    row.saved_paths,
                    row.captured_at,
                    row.status,
                ],
            )?;
            Ok(())
        })
    }

    /// 去重判断：artwork_id 已存在。
    pub fn is_illust_downloaded(&self, artwork_id: i64) -> bool {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM illustrations WHERE artwork_id = ?1",
                    params![artwork_id],
                    |_| Ok(()),
                )
                .is_ok())
        })
        .unwrap_or_else(|err| {
            log::warn!("is_illust_downloaded({artwork_id}) 查询失败: {err}");
            false
        })
    }

    pub fn get_illustration(&self, artwork_id: i64) -> Option<IllustrationRow> {
        match self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM illustrations WHERE artwork_id = ?1",
                params![artwork_id],
                illustration_from_row,
            )
        }) {
            Ok(row) => Some(row),
            Err(err) => {
                if !err.contains("Query returned no rows") {
                    log::warn!("get_illustration({artwork_id}) 查询失败: {err}");
                }
                None
            }
        }
    }

    /// 分页查询 → (rows, total)。
    pub fn list_illustrations(
        &self,
        filter: &IllustrationFilter,
    ) -> Result<(Vec<IllustrationRow>, i64), String> {
        let mut conditions: Vec<&str> = Vec::new();
        let mut sql_params: Vec<SqlValue> = Vec::new();
        if let Some(author_id) = filter.author_id {
            conditions.push("author_id = ?");
            sql_params.push(SqlValue::Integer(author_id));
        }
        if let Some(keyword) = filter.keyword.as_deref().filter(|k| !k.is_empty()) {
            conditions.push("title LIKE ?");
            sql_params.push(SqlValue::from(format!("%{keyword}%")));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        let total = self.with_conn(|conn| {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM illustrations{where_clause}"),
                params_from_iter(sql_params.iter()),
                |row| row.get::<_, i64>(0),
            )
        })?;
        let offset = (filter.page - 1).max(0) * filter.page_size;
        sql_params.push(SqlValue::Integer(filter.page_size));
        sql_params.push(SqlValue::Integer(offset));
        let rows = self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT * FROM illustrations{where_clause} ORDER BY captured_at DESC LIMIT ? OFFSET ?"
            ))?;
            let rows = stmt
                .query_map(params_from_iter(sql_params.iter()), illustration_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        Ok((rows, total))
    }

    /// 批量删除 illustration 记录，返回删除条数。
    /// delete_files=true 时删除 saved_paths JSON 数组内的全部文件（失败仅忽略）。
    pub fn delete_illustrations(&self, ids: &[i64], delete_files: bool) -> Result<usize, String> {
        if ids.is_empty() {
            return Ok(0);
        }
        let id_params: Vec<SqlValue> = ids.iter().map(|i| SqlValue::Integer(*i)).collect();
        let ph = placeholders(ids.len());
        self.with_conn(|conn| {
            if delete_files {
                let mut stmt = conn.prepare(&format!(
                    "SELECT saved_paths FROM illustrations WHERE artwork_id IN ({ph})"
                ))?;
                let raw_paths: Vec<String> = stmt
                    .query_map(params_from_iter(id_params.iter()), |row| {
                        row.get::<_, Option<String>>(0)
                    })?
                    .flatten()
                    .flatten()
                    .collect();
                for raw in raw_paths {
                    let paths: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
                    for path in paths {
                        if let Err(err) = std::fs::remove_file(&path) {
                            if err.kind() != std::io::ErrorKind::NotFound {
                                log::warn!("删除文件失败 {path}: {err}");
                            }
                        }
                    }
                }
            }
            let deleted = conn.execute(
                &format!("DELETE FROM illustrations WHERE artwork_id IN ({ph})"),
                params_from_iter(id_params.iter()),
            )?;
            Ok(deleted)
        })
    }

    /// 清空 illustrations 表，返回删除条数（delete_files 语义同上）。
    pub fn delete_all_illustrations(&self, delete_files: bool) -> Result<usize, String> {
        self.with_conn(|conn| {
            if delete_files {
                let mut stmt = conn.prepare("SELECT saved_paths FROM illustrations")?;
                let raw_paths: Vec<String> = stmt
                    .query_map([], |row| row.get::<_, Option<String>>(0))?
                    .flatten()
                    .flatten()
                    .collect();
                for raw in raw_paths {
                    let paths: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
                    for path in paths {
                        if let Err(err) = std::fs::remove_file(&path) {
                            if err.kind() != std::io::ErrorKind::NotFound {
                                log::warn!("删除文件失败 {path}: {err}");
                            }
                        }
                    }
                }
            }
            let total: i64 =
                conn.query_row("SELECT COUNT(*) FROM illustrations", [], |row| row.get(0))?;
            conn.execute("DELETE FROM illustrations", [])?;
            Ok(total as usize)
        })
    }

    // ------------------------------------------------------------------
    // history（novels + illustrations UNION ALL 联合查询）
    // ------------------------------------------------------------------

    /// 分页联合查询，统一行形状 HistoryRow。
    ///
    /// - novel 子查询：novel_id AS id, 'novel' AS category, title, author_name,
    ///   page_count AS pages, series_id, NULL AS illust_type, captured_at
    /// - illustration 子查询：artwork_id, 'illustration', title, author_name,
    ///   page_count AS pages, NULL AS series_id, illust_type, captured_at
    /// - keyword 两个子查询都用（title LIKE '%kw%'）
    /// - ORDER BY captured_at DESC LIMIT/OFFSET；total 用 COUNT(*) 包裹
    /// - category 非法 → Err("未知历史分类: {c}")
    pub fn list_history(
        &self,
        category: &str,
        page: i64,
        page_size: i64,
        keyword: Option<&str>,
    ) -> Result<(Vec<HistoryRow>, i64), String> {
        if !matches!(category, "all" | "novel" | "illustration") {
            return Err(format!("未知历史分类: {category}"));
        }
        let kw = keyword.filter(|k| !k.is_empty());
        let kw_param = |params: &mut Vec<SqlValue>| {
            if kw.is_some() {
                params.push(SqlValue::from(format!("%{}%", kw.unwrap_or_default())));
            }
        };

        let novel_where = if kw.is_some() {
            " WHERE title LIKE ?"
        } else {
            ""
        };
        let illust_where = if kw.is_some() {
            " WHERE title LIKE ?"
        } else {
            ""
        };
        let novel_sql = format!(
            "SELECT novel_id AS id, 'novel' AS category, title, author_name, \
             page_count AS pages, series_id, NULL AS illust_type, captured_at \
             FROM novels{novel_where}"
        );
        let illust_sql = format!(
            "SELECT artwork_id AS id, 'illustration' AS category, title, author_name, \
             page_count AS pages, NULL AS series_id, illust_type, captured_at \
             FROM illustrations{illust_where}"
        );
        let union = match category {
            "novel" => novel_sql,
            "illustration" => illust_sql,
            _ => format!("{novel_sql} UNION ALL {illust_sql}"),
        };

        let mut params: Vec<SqlValue> = Vec::new();
        if category == "all" {
            kw_param(&mut params);
        }
        kw_param(&mut params);
        let total = self.with_conn(|conn| {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM ({union})"),
                params_from_iter(params.iter()),
                |row| row.get::<_, i64>(0),
            )
        })?;

        let offset = (page - 1).max(0) * page_size;
        params.push(SqlValue::Integer(page_size));
        params.push(SqlValue::Integer(offset));
        let rows = self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT * FROM ({union}) ORDER BY captured_at DESC LIMIT ? OFFSET ?"
            ))?;
            let rows = stmt
                .query_map(params_from_iter(params.iter()), history_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })?;
        Ok((rows, total))
    }

    // ------------------------------------------------------------------
    // tasks
    // ------------------------------------------------------------------

    /// 插入任务记录。
    pub fn insert_task(&self, row: &TaskInsert) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO tasks (task_id, source_type, source_id, category, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    row.task_id,
                    row.source_type,
                    row.source_id,
                    row.category,
                    row.status,
                    row.created_at,
                    row.updated_at,
                ],
            )?;
            Ok(())
        })
    }

    /// 动态 SET 更新任务字段（自动附带 `updated_at = now_utc_iso`）。
    /// fields 例：&[("status", json!("running")), ("done", json!(3))]。
    /// 字段名来自内部调用方（非用户输入），直接拼接 SQL。
    pub fn update_task_fields(
        &self,
        task_id: &str,
        fields: &[(&str, JsonValue)],
    ) -> Result<(), String> {
        if fields.is_empty() {
            return Ok(());
        }
        let set_clause = fields
            .iter()
            .map(|(k, _)| format!("{k} = ?"))
            .chain(std::iter::once("updated_at = ?".to_string()))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("UPDATE tasks SET {set_clause} WHERE task_id = ?");
        let values: Vec<SqlValue> = fields
            .iter()
            .map(|(_, v)| json_to_sql(v))
            .chain(std::iter::once(SqlValue::Text(now_iso())))
            .chain(std::iter::once(SqlValue::from(task_id.to_string())))
            .collect();
        self.with_conn(|conn| {
            conn.execute(&sql, params_from_iter(values))?;
            Ok(())
        })
    }

    /// 任务列表（ORDER BY created_at DESC），可按 category 过滤。
    pub fn list_tasks(&self, category: Option<&str>) -> Result<Vec<TaskRow>, String> {
        self.with_conn(|conn| {
            let rows: Vec<TaskRow> = match category.filter(|c| !c.is_empty()) {
                Some(c) => {
                    let mut stmt = conn.prepare(
                        "SELECT * FROM tasks WHERE category = ?1 ORDER BY created_at DESC",
                    )?;
                    stmt.query_map(params![c], task_from_row)?
                        .collect::<Result<Vec<_>, _>>()?
                }
                None => {
                    let mut stmt = conn.prepare("SELECT * FROM tasks ORDER BY created_at DESC")?;
                    stmt.query_map([], task_from_row)?
                        .collect::<Result<Vec<_>, _>>()?
                }
            };
            Ok(rows)
        })
    }

    pub fn get_task(&self, task_id: &str) -> Result<Option<TaskRow>, String> {
        self.with_conn(|conn| {
            let row = conn
                .query_row(
                    "SELECT * FROM tasks WHERE task_id = ?1",
                    params![task_id],
                    task_from_row,
                )
                .ok();
            Ok(row)
        })
    }

    /// 刹除任务记录（任意状态）。返回 `(deleted, missing)`：
    /// 有不存在任务时**整个批次不删除**，missing 返回缺失 id 列表，deleted 为 0。
    pub fn delete_tasks(&self, ids: &[String]) -> Result<(usize, Vec<String>), String> {
        // 去重保持顺序
        let mut unique: Vec<String> = Vec::new();
        for id in ids {
            if !unique.contains(id) {
                unique.push(id.clone());
            }
        }
        if unique.is_empty() {
            return Ok((0, Vec::new()));
        }
        let id_params: Vec<SqlValue> = unique.iter().map(|s| SqlValue::from(s.clone())).collect();
        let ph = placeholders(unique.len());
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT task_id FROM tasks WHERE task_id IN ({ph})"
            ))?;
            let existing: Vec<String> = stmt
                .query_map(params_from_iter(id_params.iter()), |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let missing: Vec<String> = unique
                .iter()
                .filter(|id| !existing.contains(id))
                .cloned()
                .collect();
            if !missing.is_empty() {
                return Ok((0, missing));
            }
            let deleted = conn.execute(
                &format!("DELETE FROM tasks WHERE task_id IN ({ph})"),
                params_from_iter(id_params.iter()),
            )?;
            Ok((deleted, Vec::new()))
        })
    }

    /// 删除全部已完成（status = 'done'）任务记录，不影响已导出的文件。
    pub fn delete_completed_tasks(&self) -> Result<usize, String> {
        self.with_conn(|conn| {
            let deleted = conn.execute("DELETE FROM tasks WHERE status = ?1", params!["done"])?;
            Ok(deleted)
        })
    }
}

// ----------------------------------------------------------------------
// 行映射
// ----------------------------------------------------------------------

fn novel_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NovelRow> {
    Ok(NovelRow {
        novel_id: row.get("novel_id")?,
        title: row.get("title")?,
        series_id: row.get("series_id")?,
        series_order: row.get("series_order")?,
        author_id: row.get("author_id")?,
        author_name: row.get("author_name")?,
        page_count: row.get("page_count")?,
        text_length: row.get("text_length")?,
        captured_at: row.get("captured_at")?,
        modification_date: row.get("modification_date")?,
        txt_path: row.get("txt_path")?,
        md_path: row.get("md_path")?,
        status: row.get("status")?,
    })
}

fn illustration_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IllustrationRow> {
    Ok(IllustrationRow {
        artwork_id: row.get("artwork_id")?,
        title: row.get("title")?,
        author_id: row.get("author_id")?,
        author_name: row.get("author_name")?,
        illust_type: row.get("illust_type")?,
        page_count: row.get("page_count")?,
        saved_paths: row.get("saved_paths")?,
        captured_at: row.get("captured_at")?,
        status: row.get("status")?,
    })
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRow> {
    Ok(TaskRow {
        task_id: row.get("task_id")?,
        source_type: row.get("source_type")?,
        source_id: row.get("source_id")?,
        category: row.get("category")?,
        status: row.get("status")?,
        total: row.get("total")?,
        done: row.get("done")?,
        skipped: row.get("skipped")?,
        failed_ids: row.get("failed_ids")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        error: row.get("error")?,
    })
}

fn history_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryRow> {
    Ok(HistoryRow {
        id: row.get("id")?,
        category: row.get("category")?,
        title: row.get("title")?,
        author_name: row.get("author_name")?,
        pages: row.get("pages")?,
        series_id: row.get("series_id")?,
        illust_type: row.get("illust_type")?,
        captured_at: row.get("captured_at")?,
    })
}

/// novels 过滤条件 → (WHERE 子句, 参数)。
fn novel_where(filter: &NovelFilter) -> (String, Vec<SqlValue>) {
    let mut conditions: Vec<&str> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();
    if let Some(series_id) = filter.series_id {
        conditions.push("series_id = ?");
        params.push(SqlValue::Integer(series_id));
    }
    if let Some(author_id) = filter.author_id {
        conditions.push("author_id = ?");
        params.push(SqlValue::Integer(author_id));
    }
    if let Some(keyword) = filter.keyword.as_deref().filter(|k| !k.is_empty()) {
        conditions.push("title LIKE ?");
        params.push(SqlValue::from(format!("%{keyword}%")));
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    (where_clause, params)
}

// ----------------------------------------------------------------------
// 单元测试（临时目录建库）
// ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_db(tag: &str) -> (Db, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("pixiv-tool-db-test-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.db");
        let db = Db::open(&path).unwrap();
        (db, dir)
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    fn novel(id: i64, title: &str, captured_at: &str) -> NovelInsert {
        NovelInsert {
            novel_id: id,
            title: title.into(),
            captured_at: captured_at.into(),
            ..Default::default()
        }
    }

    #[test]
    fn schema_is_idempotent() {
        let (db, dir) = temp_db("idempotent");
        let path = dir.join("app.db");
        // 同一文件二次打开（schema 幂等 + category 迁移不炸）
        let db2 = Db::open(&path).unwrap();
        db.insert_novel(&novel(1, "t", "2026-01-01T00:00:00+00:00"))
            .unwrap();
        assert!(db2.get_novel(1).is_some());
        cleanup(&dir);
    }

    #[test]
    fn novel_insert_query_pagination() {
        let (db, dir) = temp_db("novels");
        for (id, ts) in [
            (1, "2026-01-01T00:00:00+00:00"),
            (2, "2026-01-03T00:00:00+00:00"),
            (3, "2026-01-02T00:00:00+00:00"),
        ] {
            db.insert_novel(&novel(id, &format!("小说{id}"), ts))
                .unwrap();
        }
        // 全量：按 captured_at 倒序
        let (rows, total) = db
            .list_novels(&NovelFilter {
                page: 1,
                page_size: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(total, 3);
        assert_eq!(
            rows.iter().map(|r| r.novel_id).collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
        // keyword
        let filter = NovelFilter {
            keyword: Some("小说2".into()),
            page: 1,
            page_size: 10,
            ..Default::default()
        };
        let (rows, total) = db.list_novels(&filter).unwrap();
        assert_eq!((total, rows.len()), (1, 1));
        assert_eq!(rows[0].novel_id, 2);
        // 分页
        let filter = NovelFilter {
            page: 2,
            page_size: 2,
            ..Default::default()
        };
        let (rows, total) = db.list_novels(&filter).unwrap();
        assert_eq!(total, 3);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].novel_id, 1);
        // 去重判断
        assert!(db.is_novel_downloaded(1));
        assert!(!db.is_novel_downloaded(99));
        cleanup(&dir);
    }

    #[test]
    fn insert_or_replace_overwrites() {
        let (db, dir) = temp_db("replace");
        db.insert_novel(&novel(7, "旧标题", "2026-01-01T00:00:00+00:00"))
            .unwrap();
        db.insert_novel(&NovelInsert {
            novel_id: 7,
            title: "新标题".into(),
            page_count: Some(3),
            ..Default::default()
        })
        .unwrap();
        let row = db.get_novel(7).unwrap();
        assert_eq!(row.title, "新标题");
        assert_eq!(row.page_count, Some(3));
        // 仍是 1 行
        let (_, total) = db
            .list_novels(&NovelFilter {
                page: 1,
                page_size: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(total, 1);
        cleanup(&dir);
    }

    #[test]
    fn update_novel_paths_partial() {
        let (db, dir) = temp_db("paths");
        db.insert_novel(&novel(5, "t", "2026-01-01T00:00:00+00:00"))
            .unwrap();
        db.update_novel_paths(5, Some("/tmp/a.txt"), None).unwrap();
        assert_eq!(
            db.get_novel(5).unwrap().txt_path.as_deref(),
            Some("/tmp/a.txt")
        );
        assert_eq!(db.get_novel(5).unwrap().md_path, None);
        db.update_novel_paths(5, None, Some("/tmp/a.md")).unwrap();
        let row = db.get_novel(5).unwrap();
        assert_eq!(row.txt_path.as_deref(), Some("/tmp/a.txt"));
        assert_eq!(row.md_path.as_deref(), Some("/tmp/a.md"));
        // 空参数 no-op 不报错
        db.update_novel_paths(5, None, None).unwrap();
        cleanup(&dir);
    }

    #[test]
    fn history_union_ordering_and_filter() {
        let (db, dir) = temp_db("history");
        db.insert_novel(&NovelInsert {
            novel_id: 1,
            title: "小说A".into(),
            series_id: Some(10),
            page_count: Some(2),
            captured_at: "2026-02-01T00:00:00+00:00".into(),
            ..Default::default()
        })
        .unwrap();
        db.insert_novel(&novel(2, "小说B", "2026-03-01T00:00:00+00:00"))
            .unwrap();
        db.insert_illustration(&IllustrationInsert {
            artwork_id: 100,
            title: "插画C".into(),
            illust_type: 2,
            captured_at: "2026-04-01T00:00:00+00:00".into(),
            ..Default::default()
        })
        .unwrap();
        // all：按 captured_at 倒序（插画C > 小说B > 小说A）
        let (rows, total) = db.list_history("all", 1, 10, None).unwrap();
        assert_eq!(total, 3);
        assert_eq!(rows[0].category, "illustration");
        assert_eq!(rows[0].illust_type, Some(2));
        assert_eq!(rows[0].series_id, None);
        assert_eq!(rows[1].category, "novel");
        assert_eq!(rows[1].series_id, None); // 小说B 无系列
        assert_eq!(rows[2].series_id, Some(10));
        assert_eq!(rows[2].pages, Some(2));
        // 分类过滤
        let (rows, total) = db.list_history("novel", 1, 10, None).unwrap();
        assert_eq!(total, 2);
        assert!(rows.iter().all(|r| r.category == "novel"));
        // keyword 过滤（两个子查询都用）
        let (rows, total) = db.list_history("all", 1, 10, Some("插画")).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].id, 100);
        // 非法分类
        assert_eq!(
            db.list_history("manga", 1, 10, None).unwrap_err(),
            "未知历史分类: manga"
        );
        cleanup(&dir);
    }

    #[test]
    fn task_batch_delete_missing_is_all_or_nothing() {
        let (db, dir) = temp_db("tasks");
        let now = now_iso();
        for id in ["a", "b"] {
            db.insert_task(&TaskInsert {
                task_id: id.into(),
                source_type: "single".into(),
                source_id: "1".into(),
                created_at: now.clone(),
                updated_at: now.clone(),
                ..Default::default()
            })
            .unwrap();
        }
        // 有 missing → 整批不删
        let (deleted, missing) = db.delete_tasks(&["a".into(), "missing".into()]).unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(missing, vec!["missing"]);
        assert!(db.get_task("a").unwrap().is_some());
        // 全存在 → 删除
        let (deleted, missing) = db
            .delete_tasks(&["a".into(), "b".into(), "a".into()]) // 重复 id 去重
            .unwrap();
        assert_eq!(deleted, 2);
        assert!(missing.is_empty());
        assert!(db.get_task("a").unwrap().is_none());
        // 空列表 no-op
        let (deleted, missing) = db.delete_tasks(&[]).unwrap();
        assert_eq!((deleted, missing), (0, Vec::<String>::new()));
        cleanup(&dir);
    }

    #[test]
    fn update_task_fields_dynamic() {
        let (db, dir) = temp_db("update-task");
        db.insert_task(&TaskInsert {
            task_id: "t1".into(),
            source_type: "series".into(),
            source_id: "9".into(),
            created_at: now_iso(),
            updated_at: now_iso(),
            ..Default::default()
        })
        .unwrap();
        db.update_task_fields(
            "t1",
            &[
                ("status", serde_json::json!("running")),
                ("done", serde_json::json!(2)),
                ("failed_ids", serde_json::json!("[1,2]")),
            ],
        )
        .unwrap();
        let row = db.get_task("t1").unwrap().unwrap();
        assert_eq!(row.status, "running");
        assert_eq!(row.done, 2);
        assert_eq!(row.failed_ids, "[1,2]");
        // updated_at 自动附带（与 created_at 不同字符串即可——精度微秒，隔断几乎必然不同）
        // done 默认列存在
        db.update_task_fields("t1", &[]).unwrap(); // 空 no-op
        // list_tasks + category 过滤
        let rows = db.list_tasks(None).unwrap();
        assert_eq!(rows.len(), 1);
        let rows = db.list_tasks(Some("illustration")).unwrap();
        assert!(rows.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn delete_completed_tasks_only_done() {
        let (db, dir) = temp_db("completed");
        let now = now_iso();
        for (id, status) in [
            ("d1", "done"),
            ("d2", "done"),
            ("f1", "failed"),
            ("r1", "running"),
        ] {
            db.insert_task(&TaskInsert {
                task_id: id.into(),
                source_type: "single".into(),
                source_id: "1".into(),
                status: status.into(),
                created_at: now.clone(),
                updated_at: now.clone(),
                ..Default::default()
            })
            .unwrap();
        }
        assert_eq!(db.delete_completed_tasks().unwrap(), 2);
        let rows = db.list_tasks(None).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(db.get_task("f1").unwrap().is_some());
        cleanup(&dir);
    }

    #[test]
    fn illustration_saved_paths_and_delete_files() {
        let (db, dir) = temp_db("illust");
        let sub = dir.join("files");
        std::fs::create_dir_all(&sub).unwrap();
        let f1 = sub.join("a.png");
        let f2 = sub.join("b.png");
        std::fs::write(&f1, b"x").unwrap();
        std::fs::write(&f2, b"x").unwrap();
        let saved = serde_json::to_string(&vec![
            f1.to_string_lossy().into_owned(),
            f2.to_string_lossy().into_owned(),
        ])
        .unwrap();
        db.insert_illustration(&IllustrationInsert {
            artwork_id: 42,
            title: "T".into(),
            saved_paths: saved.clone(),
            captured_at: now_iso(),
            ..Default::default()
        })
        .unwrap();
        assert!(db.is_illust_downloaded(42));
        // 只删记录
        db.delete_illustrations(&[42], false).unwrap();
        assert!(!db.is_illust_downloaded(42));
        assert!(f1.exists());
        // 再插入，删记录 + 删文件
        db.insert_illustration(&IllustrationInsert {
            artwork_id: 42,
            title: "T".into(),
            saved_paths: saved,
            captured_at: now_iso(),
            ..Default::default()
        })
        .unwrap();
        db.delete_illustrations(&[42], true).unwrap();
        assert!(!f1.exists());
        assert!(!f2.exists());
        // delete_all
        db.insert_illustration(&IllustrationInsert {
            artwork_id: 43,
            title: "T2".into(),
            captured_at: now_iso(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(db.delete_all_illustrations(false).unwrap(), 1);
        assert!(db.get_illustration(43).is_none());
        cleanup(&dir);
    }

    #[test]
    fn novel_delete_all_with_files() {
        let (db, dir) = temp_db("del-all");
        let sub = dir.join("novels");
        std::fs::create_dir_all(&sub).unwrap();
        let f = sub.join("n.txt");
        std::fs::write(&f, b"x").unwrap();
        db.insert_novel(&NovelInsert {
            novel_id: 1,
            title: "T".into(),
            captured_at: now_iso(),
            txt_path: Some(f.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(db.delete_all_novels(true).unwrap(), 1);
        assert!(!f.exists());
        assert!(db.get_novel(1).is_none());
        cleanup(&dir);
    }
}
