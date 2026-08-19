//! 任务命令（桩，phase C 实现）。
//!
//! 返回体与旧 HTTP 后端一致：
//! - tasks_list → {"items":[TaskRow...]}；未知分类 → Err("未知任务分类: {c}")
//! - task_create → {"task_id":"...","status":"pending"}（爬取后台执行，
//!   进度经 "task://progress" / "task://done" 事件推送）；
//!   未登录 → {"error":"未登录，请先登录"}；来源/分类非法 → {"error":"未知..."}
//! - task_pause / task_resume / task_cancel → {"status":"paused"|"running"|"canceled"}
//! - task_retry_failed → {"task_id":新id,"status":"pending"}；
//!   任务不存在 → {"error":"任务不存在"}；没有失败项 → {"error":"没有失败项"}
//! - task_delete → {"deleted":1}；不存在 → Err("任务不存在")
//! - tasks_delete → {"deleted":n}；空列表 → Err("task_ids 不能为空")；
//!   有不存在 → Err("任务不存在")
//! - tasks_delete_completed → {"deleted":n}
//! 删除进行中任务前先 cancel（设取消标志让后台循环退出）再删记录。
#![allow(unused)]

use serde_json::Value;
use tauri::{AppHandle, State};

use crate::state::AppState;

/// 任务列表，可按 category（novel/illustration）过滤。
#[tauri::command]
pub async fn tasks_list(
    state: State<'_, AppState>,
    category: Option<String>,
) -> Result<Value, String> {
    todo!("phase C")
}

/// 创建抓取任务。category 缺省 "novel"；formats 缺省 ["txt","markdown"]。
#[tauri::command]
pub async fn task_create(
    state: State<'_, AppState>,
    app: AppHandle,
    source_type: String,
    source_id: String,
    formats: Vec<String>,
    category: Option<String>,
) -> Result<Value, String> {
    todo!("phase C")
}

/// 暂停任务。
#[tauri::command]
pub async fn task_pause(state: State<'_, AppState>, task_id: String) -> Result<Value, String> {
    todo!("phase C")
}

/// 继续任务。
#[tauri::command]
pub async fn task_resume(state: State<'_, AppState>, task_id: String) -> Result<Value, String> {
    todo!("phase C")
}

/// 取消任务。
#[tauri::command]
pub async fn task_cancel(state: State<'_, AppState>, task_id: String) -> Result<Value, String> {
    todo!("phase C")
}

/// 重试任务失败项（返回新任务 id）。
#[tauri::command]
pub async fn task_retry_failed(
    state: State<'_, AppState>,
    app: AppHandle,
    task_id: String,
) -> Result<Value, String> {
    todo!("phase C")
}

/// 删除单个任务记录（含进行中任务，先 cancel）。
#[tauri::command]
pub async fn task_delete(state: State<'_, AppState>, task_id: String) -> Result<Value, String> {
    todo!("phase C")
}

/// 批量删除任务记录（含进行中任务，先 cancel）。
#[tauri::command]
pub async fn tasks_delete(
    state: State<'_, AppState>,
    task_ids: Vec<String>,
) -> Result<Value, String> {
    todo!("phase C")
}

/// 清除全部已完成任务记录。
#[tauri::command]
pub async fn tasks_delete_completed(state: State<'_, AppState>) -> Result<Value, String> {
    todo!("phase C")
}
