//! 历史联合查询命令。
//!
//! 返回体与旧 HTTP 后端一致：
//! 成功 → {"items":[HistoryRow...],"total":n,"page":p,"page_size":ps}；
//! 未知分类 → Err("未知历史分类: {c}")。

use serde_json::{Value, json};
use tauri::State;

use crate::state::AppState;

/// 分页查询历史（小说 + 插画联合）。category: all | novel | illustration。
#[tauri::command]
pub async fn history_list(
    state: State<'_, AppState>,
    category: String,
    page: i64,
    page_size: i64,
    keyword: Option<String>,
) -> Result<Value, String> {
    history_list_impl(&state, &category, page, page_size, keyword.as_deref())
}

/// 命令实现（不依赖 tauri::State，可离线集成测试）。
/// 未知分类（db 层校验文案）→ reject，与桩 doc 契约一致。
pub fn history_list_impl(
    state: &AppState,
    category: &str,
    page: i64,
    page_size: i64,
    keyword: Option<&str>,
) -> Result<Value, String> {
    let (items, total) = state.db.list_history(category, page, page_size, keyword)?;
    Ok(json!({
        "items": items,
        "total": total,
        "page": page,
        "page_size": page_size,
    }))
}
