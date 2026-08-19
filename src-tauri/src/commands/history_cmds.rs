//! 历史联合查询命令（桩，phase D 实现）。
//!
//! 返回体与旧 HTTP 后端一致：
//! 成功 → {"items":[HistoryRow...],"total":n,"page":p,"page_size":ps}；
//! 未知分类 → Err("未知历史分类: {c}")。
#![allow(unused)]

use serde_json::Value;
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
    todo!("phase D")
}
