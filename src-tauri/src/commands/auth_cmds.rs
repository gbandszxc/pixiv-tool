//! 认证命令（桩，phase B 实现）。
//!
//! 返回体与旧 HTTP 后端一致：
//! - auth_status 成功 → {"is_logged_in":true,"user_id","pixiv_id","name","profile_img"}；
//!   未登录/校验失败/平台不支持 → {"is_logged_in":false}
//! - auth_login 成功 → {"status":"success","message":"登录成功","user":{...}}；
//!   其他 → {"status":"cancelled"|"timeout"|"error","message":...}
//! - auth_login_manual 成功 → {"status":"success","message":"Cookie 已保存","user":{...}}；
//!   失败 → Err(中文错误文案)
//! - auth_logout → {"status":"success"}
#![allow(unused)]

use serde_json::Value;
use tauri::State;

use crate::state::AppState;

/// 查询当前登录态（等价旧 GET /api/auth/status）。
/// 校验走 fetch_session_probe 同源指纹；启动期短暂超时按未登录处理。
#[tauri::command]
pub async fn auth_status(state: State<'_, AppState>) -> Result<Value, String> {
    todo!("phase B")
}

/// 打开真实浏览器登录（长阻塞，等价旧 POST /api/auth/login）。
/// 找不到浏览器 → {"status":"error","message":"未找到 Chrome、Edge 或 Chromium"}。
#[tauri::command]
pub async fn auth_login(state: State<'_, AppState>) -> Result<Value, String> {
    todo!("phase B")
}

/// 手动提交 PHPSESSID，用 /ajax/user/self 验证后保存
/// （等价旧 POST /api/auth/login/manual）。
/// 无效/过期 → Err("PHPSESSID 无效或已过期")；缺 token → Err(token 相关中文文案)。
#[tauri::command]
pub async fn auth_login_manual(
    state: State<'_, AppState>,
    phpsessid: String,
) -> Result<Value, String> {
    todo!("phase B")
}

/// 清空本地登录态（等价旧 POST /api/auth/logout）。
#[tauri::command]
pub async fn auth_logout(state: State<'_, AppState>) -> Result<Value, String> {
    todo!("phase B")
}
