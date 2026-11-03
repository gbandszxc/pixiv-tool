//! 同步载荷（加密前的 JSON）：导出、解析与浏览历史合并。
//!
//! ```json
//! {
//!   "version": 1,
//!   "exported_at": "<UTC RFC3339>",
//!   "history": [ ... ] | null,
//!   "accounts": { "active": "uid" | null, "items": [ ... ] } | null
//! }
//! ```
//!
//! 未选范围对应字段为 `null`。导出侧只写「已登记 + 有 PHPSESSID」的账户；
//! 导入侧逐条校验，非法条目静默跳过（历史按「仅更晚覆盖」合并，账户索引同名覆盖）。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::accounts::{entry_name, AccountInfo, AccountManager};
use crate::db::{BrowseHistoryRow, Db};

use super::{Scope, SyncError};

/// 载荷版本；不认识的一律 `sync_payload_invalid`。
pub const PAYLOAD_VERSION: u32 = 1;

/// 浏览历史条目（字段与 `browse_history` 表一一对应）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    #[serde(default)]
    pub work_id: i64,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author_id: i64,
    #[serde(default)]
    pub author_name: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub page_count: i64,
    #[serde(default)]
    pub x_restrict: i64,
    #[serde(default)]
    pub visited_at: String,
}

/// 单账户载荷：元信息 + 完整 cookie map（含 PHPSESSID / x-csrf-token 等）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountItem {
    pub user_id: String,
    pub info: AccountInfo,
    #[serde(default)]
    pub cookies: HashMap<String, String>,
}

/// 账户范围载荷。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsPayload {
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub items: Vec<AccountItem>,
}

/// 加密前的完整载荷。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPayload {
    pub version: u32,
    pub exported_at: String,
    pub history: Option<Vec<HistoryItem>>,
    pub accounts: Option<AccountsPayload>,
}

/// 导出计数（命令返回用）。
#[derive(Debug, Clone, Copy, Default)]
pub struct ExportCounts {
    pub history_count: usize,
    pub account_count: usize,
}

/// 解析后的载荷（已过滤非法条目）。
#[derive(Debug, Clone, Default)]
pub struct ParsedPayload {
    pub exported_at: String,
    pub history: Option<Vec<HistoryItem>>,
    pub accounts: Option<AccountsPayload>,
}

/// 浏览历史 kind 白名单（与 `list_browse_history` 的过滤值一致）。
pub fn is_valid_kind(kind: &str) -> bool {
    matches!(kind, "illust" | "manga" | "novel")
}

/// 访问时间归一：解析任意 RFC3339（容忍时区偏移）→ UTC 微秒精度 + `Z`。
///
/// 归一后的长度固定 27，因此可直接用字符串比较判断「更晚」。
/// 解析失败返回 `None`（导入时按非法行跳过）。
pub fn normalize_visited_at(raw: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(raw.trim())
        .ok()
        .map(|dt| {
            dt.with_timezone(&chrono::Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
        })
}

/// 统计状态里的浏览历史与登录态，构造载荷（未选范围字段为 `None`）。
pub fn build_payload(
    db: &Db,
    accounts: &AccountManager,
    scopes: &[Scope],
) -> Result<(SyncPayload, ExportCounts), SyncError> {
    let mut counts = ExportCounts::default();

    let history = if scopes.contains(&Scope::History) {
        let rows = db
            .export_browse_history()
            .map_err(|_| SyncError::internal("读取浏览历史失败"))?;
        counts.history_count = rows.len();
        Some(rows.into_iter().map(history_item_from_row).collect())
    } else {
        None
    };

    let accounts_payload = if scopes.contains(&Scope::Auth) {
        let mut items = Vec::new();
        for info in accounts.list() {
            match accounts.load_cookies(&info.user_id) {
                // 无 PHPSESSID 的账号无法恢复登录态，跳过（不导出空壳）。
                Ok(Some(cookies)) if has_php_sessid(&cookies) => items.push(AccountItem {
                    user_id: info.user_id.clone(),
                    info,
                    cookies,
                }),
                Ok(_) => {}
                Err(_) => {
                    // 只记账号标识，不记凭据内容。
                    log::warn!("同步导出跳过账号 {}：登录态读取失败", info.user_id);
                }
            }
        }
        counts.account_count = items.len();
        Some(AccountsPayload {
            active: accounts.active(),
            items,
        })
    } else {
        None
    };

    Ok((
        SyncPayload {
            version: PAYLOAD_VERSION,
            exported_at: crate::db::now_iso(),
            history,
            accounts: accounts_payload,
        },
        counts,
    ))
}

fn history_item_from_row(row: BrowseHistoryRow) -> HistoryItem {
    HistoryItem {
        work_id: row.work_id,
        kind: row.kind,
        title: row.title,
        author_id: row.author_id,
        author_name: row.author_name,
        cover: row.cover,
        page_count: row.page_count,
        x_restrict: row.x_restrict,
        visited_at: row.visited_at,
    }
}

fn has_php_sessid(cookies: &HashMap<String, String>) -> bool {
    cookies
        .get("PHPSESSID")
        .is_some_and(|value| !value.trim().is_empty())
}

/// 原样载荷的形状（宽松解析；逐条校验交给 [`parse_payload`]）。
#[derive(Deserialize)]
struct RawPayload {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    exported_at: String,
    #[serde(default)]
    history: Option<Vec<Value>>,
    #[serde(default)]
    accounts: Option<Value>,
}

/// 解析解密后的 JSON：版本校验 + 历史行过滤（kind / work_id / visited_at）+
/// 账户条目过滤（user_id 词法、PHPSESSID、info 与条目 user_id 一致）。
///
/// 非法行不是错误——历史行跳过，账户条目跳过；只有整体 JSON 或版本不对才是
/// `sync_payload_invalid`。
pub fn parse_payload(bytes: &[u8]) -> Result<ParsedPayload, SyncError> {
    let raw: RawPayload =
        serde_json::from_slice(bytes).map_err(|_| SyncError::payload_invalid())?;
    if raw.version != PAYLOAD_VERSION {
        return Err(SyncError::payload_invalid());
    }

    let history = raw.history.map(|values| {
        values
            .into_iter()
            .filter_map(|value| serde_json::from_value::<HistoryItem>(value).ok())
            .filter_map(normalize_history_item)
            .collect::<Vec<_>>()
    });

    let accounts = raw.accounts.map(parse_accounts).transpose()?;

    Ok(ParsedPayload {
        exported_at: raw.exported_at,
        history,
        accounts,
    })
}

/// 单条历史行校验与归一；不合法 → `None`。
fn normalize_history_item(mut item: HistoryItem) -> Option<HistoryItem> {
    item.kind = item.kind.trim().to_string();
    if !is_valid_kind(&item.kind) || item.work_id <= 0 {
        return None;
    }
    item.visited_at = normalize_visited_at(&item.visited_at)?;
    Some(item)
}

/// 账户载荷解析：结构不对（不是对象 / items 不是数组）→ 整体 `payload_invalid`；
/// 单条非法 → 跳过。
fn parse_accounts(value: Value) -> Result<AccountsPayload, SyncError> {
    #[derive(Deserialize)]
    struct RawAccounts {
        #[serde(default)]
        active: Option<String>,
        #[serde(default)]
        items: Vec<Value>,
    }
    let raw: RawAccounts =
        serde_json::from_value(value).map_err(|_| SyncError::payload_invalid())?;
    let mut items = Vec::new();
    for entry in raw.items {
        let Ok(mut item) = serde_json::from_value::<AccountItem>(entry) else {
            continue;
        };
        let user_id = item.user_id.trim().to_string();
        if entry_name(&user_id).is_none()
            || item.info.user_id != user_id
            || !has_php_sessid(&item.cookies)
        {
            continue;
        }
        item.user_id = user_id;
        items.push(item);
    }
    let active = raw
        .active
        .map(|id| id.trim().to_string())
        .filter(|id| entry_name(id).is_some());
    Ok(AccountsPayload { active, items })
}

/// 浏览历史合并写库：返回实际插入 + 更新的行数。
pub fn import_history(db: &Db, items: &[HistoryItem]) -> Result<usize, SyncError> {
    let rows: Vec<BrowseHistoryRow> = items
        .iter()
        .filter_map(|item| {
            // parse_payload 已归一；这里再兜一层，非法行不写库。
            let visited_at = normalize_visited_at(&item.visited_at)?;
            if !is_valid_kind(&item.kind) || item.work_id <= 0 {
                return None;
            }
            Some(BrowseHistoryRow {
                work_id: item.work_id,
                kind: item.kind.clone(),
                title: item.title.clone(),
                author_id: item.author_id,
                author_name: item.author_name.clone(),
                cover: item.cover.clone(),
                page_count: item.page_count,
                x_restrict: item.x_restrict,
                visited_at,
            })
        })
        .collect();
    db.import_browse_history(&rows)
        .map_err(|_| SyncError::internal("写入浏览历史失败"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{BrowseHistoryEntry, Db};
    use std::path::{Path, PathBuf};

    fn temp_db(tag: &str) -> (Db, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "pixiv-tool-sync-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("app.db")).unwrap();
        (db, dir)
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    fn item(work_id: i64, kind: &str, visited_at: &str) -> HistoryItem {
        HistoryItem {
            work_id,
            kind: kind.into(),
            title: format!("title-{work_id}"),
            author_id: 1,
            author_name: "author".into(),
            cover: String::new(),
            page_count: 1,
            x_restrict: 0,
            visited_at: visited_at.into(),
        }
    }

    #[test]
    fn visited_at_normalizes_to_utc_micros() {
        // 时区偏移统一换算成 UTC，并补足微秒
        assert_eq!(
            normalize_visited_at("2026-01-01T08:00:00+08:00").as_deref(),
            Some("2026-01-01T00:00:00.000000Z")
        );
        assert_eq!(
            normalize_visited_at(" 2026-01-01T00:00:00.123Z ").as_deref(),
            Some("2026-01-01T00:00:00.123000Z")
        );
        // 归一后长度固定，字符串序 == 时间序
        let earlier = normalize_visited_at("2026-01-01T07:59:59+08:00").unwrap();
        let later = normalize_visited_at("2026-01-01T00:00:00Z").unwrap();
        assert_eq!(earlier.len(), later.len());
        assert!(earlier < later);
        // 非法值
        for bad in ["", "not-a-date", "2026-13-01T00:00:00Z", "1700000000"] {
            assert!(normalize_visited_at(bad).is_none(), "{bad} 不应通过");
        }
    }

    #[test]
    fn import_only_overwrites_with_later_visit() {
        let (db, dir) = temp_db("merge");
        db.record_browse_history(&BrowseHistoryEntry {
            kind: "illust".into(),
            work_id: 7,
            title: "local".into(),
            author_id: 1,
            author_name: "a".into(),
            cover: None,
            page_count: 1,
            x_restrict: 0,
            visited_at: "2026-01-02T00:00:00.000000Z".into(),
        })
        .unwrap();

        // 更早 → 跳过，返回 0
        let written =
            import_history(&db, &[item(7, "illust", "2026-01-01T08:00:00+08:00")]).unwrap();
        assert_eq!(written, 0);
        let (rows, _) = db.list_browse_history(1, 10, None).unwrap();
        assert_eq!(rows[0].title, "local");
        assert_eq!(rows[0].visited_at, "2026-01-02T00:00:00.000000Z");

        // 更晚 → 覆盖（1 行），且时间被归一
        let written =
            import_history(&db, &[item(7, "illust", "2026-01-03T00:00:00+08:00")]).unwrap();
        assert_eq!(written, 1);
        let (rows, _) = db.list_browse_history(1, 10, None).unwrap();
        assert_eq!(rows[0].title, "title-7");
        assert_eq!(rows[0].visited_at, "2026-01-02T16:00:00.000000Z");

        // 同一时间 → 不覆盖
        let written =
            import_history(&db, &[item(7, "illust", "2026-01-02T16:00:00.000000Z")]).unwrap();
        assert_eq!(written, 0);

        // 新键 → 插入
        let written = import_history(&db, &[item(8, "novel", "2025-01-01T00:00:00Z")]).unwrap();
        assert_eq!(written, 1);
        let (_, total) = db.list_browse_history(1, 10, None).unwrap();
        assert_eq!(total, 2);

        cleanup(&dir);
    }

    #[test]
    fn import_skips_invalid_history_rows() {
        let (db, dir) = temp_db("invalid");
        let written = import_history(
            &db,
            &[
                item(0, "illust", "2026-01-01T00:00:00Z"), // work_id 非法
                item(1, "video", "2026-01-01T00:00:00Z"),  // kind 非法
                item(2, "illust", "garbage"),              // 时间非法
                item(3, "manga", "2026-01-01T00:00:00Z"),  // 合法
            ],
        )
        .unwrap();
        assert_eq!(written, 1);
        let (rows, total) = db.list_browse_history(1, 10, None).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].work_id, 3);
        cleanup(&dir);
    }

    #[test]
    fn parse_filters_invalid_rows_and_accounts() {
        let payload = serde_json::json!({
            "version": 1,
            "exported_at": "2026-01-01T00:00:00.000000Z",
            "history": [
                {"work_id": 1, "kind": "illust", "visited_at": "2026-01-01T08:00:00+08:00"},
                {"work_id": 2, "kind": "bogus", "visited_at": "2026-01-01T00:00:00Z"},
                {"work_id": 0, "kind": "novel", "visited_at": "2026-01-01T00:00:00Z"},
                {"work_id": 3, "kind": "manga", "visited_at": "nope"}
            ],
            "accounts": {
                "active": "111",
                "items": [
                    {"user_id": "111", "info": {"user_id": "111", "name": "A"},
                     "cookies": {"PHPSESSID": "s1"}},
                    {"user_id": "bad id", "info": {"user_id": "bad id"},
                     "cookies": {"PHPSESSID": "s2"}},
                    {"user_id": "222", "info": {"user_id": "222"}, "cookies": {}},
                    {"user_id": "333", "info": {"user_id": "444"},
                     "cookies": {"PHPSESSID": "s3"}},
                    {"user_id": "444", "info": {"user_id": "444"},
                     "cookies": {"PHPSESSID": "s4"}}
                ]
            }
        })
        .to_string();
        let parsed = parse_payload(payload.as_bytes()).unwrap();
        assert_eq!(parsed.exported_at, "2026-01-01T00:00:00.000000Z");
        let history = parsed.history.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].work_id, 1);
        assert_eq!(history[0].visited_at, "2026-01-01T00:00:00.000000Z");

        let accounts = parsed.accounts.unwrap();
        assert_eq!(accounts.active.as_deref(), Some("111"));
        let ids: Vec<&str> = accounts.items.iter().map(|a| a.user_id.as_str()).collect();
        assert_eq!(ids, vec!["111", "444"]);
        // 未选范围 → 字段为 null
        let none =
            parse_payload(br#"{"version":1,"exported_at":"x","history":null,"accounts":null}"#)
                .unwrap();
        assert!(none.history.is_none());
        assert!(none.accounts.is_none());
    }

    #[test]
    fn parse_rejects_bad_version_and_json() {
        for raw in [
            br#"{"version":2,"exported_at":"x"}"#.as_slice(),
            br#"{"version":1,"exported_at":"x","history":"nope"}"#.as_slice(),
            br#"not json"#.as_slice(),
            br#"[]"#.as_slice(),
        ] {
            assert_eq!(parse_payload(raw).unwrap_err().code, "sync_payload_invalid");
        }
    }
}
