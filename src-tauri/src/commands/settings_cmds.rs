//! 设置命令（桩，phase D 实现）。
//!
//! 返回体与旧 HTTP 后端一致：
//! - settings_get → Settings（snake_case 字段，等价旧 GET /api/settings）
//! - settings_save → {"status":"success"}；output_dir 非法 → Err(中文校验消息)；
//!   max_wait_seconds 非法 → Err("最大等待时间必须是 30~86400 秒之间的整数")
//! - clear_logs → {"status":"success"}（清空 <data_dir>/logs/app.log）
#![allow(unused)]

use serde_json::Value;
use tauri::State;

use crate::settings::Settings;
use crate::state::AppState;

/// 读取当前配置。
#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Settings, String> {
    todo!("phase D")
}

/// 更新配置并持久化。settings 是 partial JSON（只更新出现的键）。
#[tauri::command]
pub async fn settings_save(state: State<'_, AppState>, settings: Value) -> Result<Value, String> {
    todo!("phase D")
}

/// 清空 app.log。
#[tauri::command]
pub async fn clear_logs(state: State<'_, AppState>) -> Result<Value, String> {
    todo!("phase D")
}
