//! 小说/插画记录管理命令（桩，phase D 实现）。
//!
//! 返回体与旧 HTTP 后端一致（业务错误走 200 + {"error":...}，校验错误走 Err）：
//! - novel_delete：不存在 → {"error":"小说不存在"}；成功 → {"status":"success"}
//! - novels_batch_delete：空列表 → {"error":"novel_ids 不能为空"}；
//!   成功 → {"status":"success","deleted":n}
//! - novels_delete_all → {"status":"success","deleted":n}
//! - open_novel_file：不存在 → {"error":"小说不存在"}；无文件 → {"error":"文件不存在"}；
//!   成功 → {"status":"success"}（reveal_in_file_manager）
//! - illustration_*：同构（{"error":"插画记录不存在"}、{"error":"没有已保存的文件"}、
//!   {"error":"illustration_ids 不能为空"}）
#![allow(unused)]

use serde_json::Value;
use tauri::State;

use crate::state::AppState;

/// 删除单条小说记录（可选删文件）。
#[tauri::command]
pub async fn novel_delete(
    state: State<'_, AppState>,
    novel_id: i64,
    delete_file: bool,
) -> Result<Value, String> {
    todo!("phase D")
}

/// 批量删除小说记录（可选删文件）。
#[tauri::command]
pub async fn novels_batch_delete(
    state: State<'_, AppState>,
    novel_ids: Vec<i64>,
    delete_file: bool,
) -> Result<Value, String> {
    todo!("phase D")
}

/// 清空全部小说记录（可选删文件）。
#[tauri::command]
pub async fn novels_delete_all(
    state: State<'_, AppState>,
    delete_file: bool,
) -> Result<Value, String> {
    todo!("phase D")
}

/// 删除单条插画记录（可选删 saved_paths 内全部文件）。
#[tauri::command]
pub async fn illustration_delete(
    state: State<'_, AppState>,
    artwork_id: i64,
    delete_file: bool,
) -> Result<Value, String> {
    todo!("phase D")
}

/// 批量删除插画记录（可选删文件）。
#[tauri::command]
pub async fn illustrations_batch_delete(
    state: State<'_, AppState>,
    artwork_ids: Vec<i64>,
    delete_file: bool,
) -> Result<Value, String> {
    todo!("phase D")
}

/// 清空全部插画记录（可选删文件）。
#[tauri::command]
pub async fn illustrations_delete_all(
    state: State<'_, AppState>,
    delete_file: bool,
) -> Result<Value, String> {
    todo!("phase D")
}

/// 在系统文件管理器中定位小说文件（txt 优先 md 兜底）。
#[tauri::command]
pub async fn open_novel_file(state: State<'_, AppState>, novel_id: i64) -> Result<Value, String> {
    todo!("phase D")
}

/// 在系统文件管理器中打开插画所在目录（saved_paths[0]）。
#[tauri::command]
pub async fn open_illustration_folder(
    state: State<'_, AppState>,
    artwork_id: i64,
) -> Result<Value, String> {
    todo!("phase D")
}
