//! 浏览访问历史命令（自有浏览 UI 的访问记录）。
//!
//! - `browse_history_record`：作品详情页加载成功后上报一次（同 kind+work_id 覆写置顶）
//! - `browse_history_list`：按访问时间倒序分页
//! - `browse_history_clear`：一键清空
//!
//! 与「抓取历史」（history_list / novels+illustrations 联合查询）无关，勿混淆。

use serde_json::{Value, json};
use tauri::State;

use crate::db::BrowseHistoryEntry;
use crate::state::AppState;

/// 记录一次浏览访问。
#[tauri::command]
pub async fn browse_history_record(
    state: State<'_, AppState>,
    kind: String,
    work_id: i64,
    title: String,
    author_id: i64,
    author_name: String,
    cover: Option<String>,
    page_count: i64,
    x_restrict: i64,
) -> Result<Value, String> {
    browse_history_record_impl(
        &state,
        kind,
        work_id,
        title,
        author_id,
        author_name,
        cover,
        page_count,
        x_restrict,
    )
}

/// 命令实现（不依赖 tauri::State，可离线集成测试）。
#[allow(clippy::too_many_arguments)]
pub fn browse_history_record_impl(
    state: &AppState,
    kind: String,
    work_id: i64,
    title: String,
    author_id: i64,
    author_name: String,
    cover: Option<String>,
    page_count: i64,
    x_restrict: i64,
) -> Result<Value, String> {
    state.db.record_browse_history(&BrowseHistoryEntry {
        kind,
        work_id,
        title,
        author_id,
        author_name,
        cover,
        page_count,
        x_restrict,
        visited_at: crate::db::now_iso(),
    })?;
    Ok(json!({ "status": "success" }))
}

/// 分页查询浏览历史（page 从 1 起）。
#[tauri::command]
pub async fn browse_history_list(
    state: State<'_, AppState>,
    page: u32,
    page_size: u32,
) -> Result<Value, String> {
    browse_history_list_impl(&state, page, page_size)
}

/// 命令实现（离线可直调）。
pub fn browse_history_list_impl(
    state: &AppState,
    page: u32,
    page_size: u32,
) -> Result<Value, String> {
    let (items, total) = state.db.list_browse_history(page as i64, page_size as i64)?;
    Ok(json!({
        "items": items,
        "total": total,
        "page": page,
        "page_size": page_size,
    }))
}

/// 清空浏览历史。
#[tauri::command]
pub async fn browse_history_clear(state: State<'_, AppState>) -> Result<Value, String> {
    browse_history_clear_impl(&state)
}

/// 命令实现（离线可直调）。
pub fn browse_history_clear_impl(state: &AppState) -> Result<Value, String> {
    let deleted = state.db.clear_browse_history()?;
    Ok(json!({ "status": "success", "deleted": deleted }))
}
