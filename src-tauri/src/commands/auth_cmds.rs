//! 认证命令。
//!
//! 返回体与旧 HTTP 后端一致：
//! - auth_status 成功 → {"is_logged_in":true,"user_id","pixiv_id","name","profile_img"}；
//!   未登录/校验失败/平台不支持 → {"is_logged_in":false}
//! - auth_login 成功 → {"status":"success","message":"登录成功","user":{...}}；
//!   其他 → {"status":"cancelled"|"timeout"|"error","message":...}
//! - auth_login_manual 成功 → {"status":"success","message":"Cookie 已保存","user":{...}}；
//!   失败 → Err(中文错误文案)
//! - auth_logout → {"status":"success"}

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tauri::State;

use crate::auth::browser_login::{find_login_browser, open_browser_login};
use crate::auth::webview_login::open_webview_login;
use crate::pixiv::client::{PixivClient, PixivError};
use crate::pixiv::csrf::{ProbeError, fetch_session_probe, normalize_phpsessid};
use crate::state::AppState;

/// 启动期登录态验证的整体超时（秒），对齐 Python `_AUTH_STATUS_TIMEOUT_SEC`。
const AUTH_STATUS_TIMEOUT_SEC: u64 = 2;

/// auth_status 的校验错误分类（Auth → 清 cookie；其他 → 保留 cookie 记日志）。
#[derive(Debug)]
enum ProbeFailure {
    Auth,
    Other(String),
}

/// 查询当前登录态（等价旧 GET /api/auth/status）。
/// 校验走 fetch_session_probe 同源指纹；启动期短暂超时按未登录处理。
#[tauri::command]
pub async fn auth_status(state: State<'_, AppState>) -> Result<Value, String> {
    let cookies = match state.cookies.load() {
        Ok(Some(cookies)) if !cookies.is_empty() => cookies,
        // 未存储过 / 平台不支持 / 登录态损坏 → 一律按未登录（对齐 Python 吞异常）
        Ok(_) => return Ok(json!({ "is_logged_in": false })),
        Err(err) => {
            log::warn!("读取登录态失败: {err}");
            return Ok(json!({ "is_logged_in": false }));
        }
    };

    let attempt = async {
        // 与抓取 / fetch_session_probe 共用同一套 wreq 指纹伪装，避免裸 UA 风控
        let client = PixivClient::new(&cookies)
            .map_err(|err| ProbeFailure::Other(format!("创建 HTTP 客户端失败: {err}")))?;
        let api = crate::pixiv::api::PixivApi::new(Arc::new(client));
        let (user_data, _token) = api.get_user_self().await.map_err(|err| match err {
            PixivError::Auth => ProbeFailure::Auth,
            other => ProbeFailure::Other(other.to_string()),
        })?;
        Ok::<Value, ProbeFailure>(user_data)
    };

    match tokio::time::timeout(Duration::from_secs(AUTH_STATUS_TIMEOUT_SEC), attempt).await {
        Err(_) => {
            log::warn!("登录态验证超时（{AUTH_STATUS_TIMEOUT_SEC}s），按未登录处理（保留 cookie）");
            Ok(json!({ "is_logged_in": false }))
        }
        Ok(Err(ProbeFailure::Auth)) => {
            // 401/403：登录态确定失效，主动清除
            if let Err(err) = state.cookies.clear() {
                log::warn!("清除失效登录态失败: {err}");
            }
            Ok(json!({ "is_logged_in": false }))
        }
        Ok(Err(ProbeFailure::Other(msg))) => {
            log::warn!("登录态验证失败（保留 cookie）: {msg}");
            Ok(json!({ "is_logged_in": false }))
        }
        Ok(Ok(user_data)) => Ok(auth_status_body(&user_data)),
    }
}

/// /ajax/user/self 的 userData → auth_status 响应体（纯函数，离线可测）。
/// userData 空 → 未登录；profileImg 空则兜底 profileImgBig（对齐 Python 字段语义）。
fn auth_status_body(user_data: &Value) -> Value {
    let Some(fields) = user_data.as_object().filter(|obj| !obj.is_empty()) else {
        log::warn!("登录态验证响应缺少 userData");
        return json!({ "is_logged_in": false });
    };
    let value_to_string = |v: &Value| -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        }
    };
    let profile_img = fields
        .get("profileImg")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            fields
                .get("profileImgBig")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default();
    json!({
        "is_logged_in": true,
        "user_id": fields.get("id").map(value_to_string).unwrap_or_default(),
        "pixiv_id": fields.get("pixivId").and_then(Value::as_str).unwrap_or_default(),
        "name": fields.get("name").and_then(Value::as_str).unwrap_or_default(),
        "profile_img": profile_img,
    })
}

/// 测试钩子：PIXIV_TOOL_FORCE_WEBVIEW_LOGIN=1 时跳过浏览器探测，强制内嵌 webview 登录。
fn force_webview_login() -> bool {
    std::env::var("PIXIV_TOOL_FORCE_WEBVIEW_LOGIN").as_deref() == Ok("1")
}

/// 打开真实浏览器登录（长阻塞，等价旧 POST /api/auth/login）。
/// 未装 Chrome/Edge/Chromium 时回退内嵌 webview 登录（浏览器路径中途的
/// 其他错误不触发回退）；PIXIV_TOOL_FORCE_WEBVIEW_LOGIN=1 强制走 webview。
#[tauri::command]
pub async fn auth_login(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let result = if force_webview_login() {
        log::info!("PIXIV_TOOL_FORCE_WEBVIEW_LOGIN=1，强制使用内嵌 webview 登录");
        open_webview_login(&app, &state.paths.config_dir.join("login-webview-profile")).await
    } else {
        match find_login_browser() {
            Ok(_) => {
                let profile_dir = state.paths.config_dir.join("login-browser-profile");
                open_browser_login(&profile_dir).await
            }
            // 无 Chromium 系浏览器 → 回退内嵌 webview 登录
            Err(err) => {
                log::info!("未找到可用浏览器（{err}），回退内嵌 webview 登录");
                open_webview_login(&app, &state.paths.config_dir.join("login-webview-profile"))
                    .await
            }
        }
    };

    if result.status == "success" {
        let Some(cookies) = result.cookies.as_ref().filter(|c| !c.is_empty()) else {
            log::warn!("浏览器登录返回 success 但缺少 cookie");
            return Ok(json!({
                "status": "error",
                "message": "登录取消",
            }));
        };
        state.cookies.save(cookies)?;
        let user = result
            .user
            .as_ref()
            .and_then(|u| serde_json::to_value(u).ok())
            .unwrap_or_else(|| json!({}));
        return Ok(json!({
            "status": "success",
            "message": "登录成功",
            "user": user,
        }));
    }

    // cancelled → "登录取消"；timeout/error → 透传 browser_login 的 message
    Ok(json!({
        "status": result.status,
        "message": result.message.unwrap_or_else(|| "登录取消".to_string()),
    }))
}

/// 手动提交 PHPSESSID，用 /ajax/user/self 验证后保存
/// （等价旧 POST /api/auth/login/manual）。
/// 无效/过期 → Err("PHPSESSID 无效或已过期")；缺 token → Err(token 相关中文文案)。
#[tauri::command]
pub async fn auth_login_manual(
    state: State<'_, AppState>,
    phpsessid: String,
) -> Result<Value, String> {
    // 校验失败：旧 400/502 的 detail 文案直接 reject
    let php = normalize_phpsessid(&phpsessid)?;
    let probe = fetch_session_probe(&php).await.map_err(|err| match err {
        ProbeError::Invalid(msg) | ProbeError::Csrf(msg) => msg,
    })?;

    let mut cookies = HashMap::new();
    cookies.insert("PHPSESSID".to_string(), php);
    cookies.insert("x-csrf-token".to_string(), probe.csrf_token);
    state.cookies.save(&cookies)?;

    let user = probe
        .user
        .as_ref()
        .and_then(|u| serde_json::to_value(u).ok())
        .unwrap_or_else(|| json!({}));
    Ok(json!({
        "status": "success",
        "message": "Cookie 已保存",
        "user": user,
    }))
}

/// 清空本地登录态（等价旧 POST /api/auth/logout）。
#[tauri::command]
pub async fn auth_logout(state: State<'_, AppState>) -> Result<Value, String> {
    // 清除失败不影响响应（条目不存在视为已清空；其余失败仅记日志）
    if let Err(err) = state.cookies.clear() {
        log::warn!("清除登录态失败: {err}");
    }
    Ok(json!({ "status": "success" }))
}

// ----------------------------------------------------------------------
// 单元测试（纯函数，离线）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_status_body_full_fields() {
        let user_data = json!({
            "id": "28640",
            "pixivId": "pixiv_id_x",
            "name": "用户名",
            "profileImg": "https://i.pximg.net/user-profile/img.png",
            "profileImgBig": "https://i.pximg.net/user-profile-big/img.png"
        });
        let body = auth_status_body(&user_data);
        assert_eq!(body["is_logged_in"], json!(true));
        assert_eq!(body["user_id"], json!("28640"));
        assert_eq!(body["pixiv_id"], json!("pixiv_id_x"));
        assert_eq!(body["name"], json!("用户名"));
        assert_eq!(
            body["profile_img"],
            json!("https://i.pximg.net/user-profile/img.png")
        );
    }

    #[test]
    fn auth_status_body_profile_img_fallback() {
        let user_data = json!({
            "id": 123,  // 数字 id 转字符串（对齐 Python str()）
            "profileImg": "",
            "profileImgBig": "https://i.pximg.net/big.png"
        });
        let body = auth_status_body(&user_data);
        assert_eq!(body["user_id"], json!("123"));
        assert_eq!(body["profile_img"], json!("https://i.pximg.net/big.png"));
    }

    #[test]
    fn auth_status_body_empty_user_data_is_logged_out() {
        for empty in [json!({}), json!(null), json!("x"), json!([])] {
            assert_eq!(
                auth_status_body(&empty)["is_logged_in"],
                json!(false),
                "userData={empty}"
            );
        }
    }
}
