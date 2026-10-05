//! 认证命令。
//!
//! 返回体与旧 HTTP 后端一致：
//! - auth_status 成功 → {"is_logged_in":true,"user_id","pixiv_id","name","profile_img"}；
//!   未登录/校验失败/平台不支持 → {"is_logged_in":false}
//! - auth_login 成功 → {"status":"success","message":"登录成功","user":{...}}；
//!   其他 → {"status":"cancelled"|"timeout"|"error","message":...}
//! - auth_login_manual 成功 → {"status":"success","message":"Cookie 已保存","user":{...}}；
//!   失败 → Err(中文错误文案)
//! - auth_logout → {"status":"success","fallback_applied":bool,"warning"?:str}
//!   （退出当前账号；有剩余账号则自动回退，回退失败降级为纯登出；
//!   warning 为非终态抓取任务提示）
//!
//! 多账号扩展（账号索引 + 每账号凭据条目，default 恒为当前账号镜像）：
//! - auth_accounts_list → {"active": str|null, "accounts": [AccountInfo...]}
//! - auth_account_switch(user_id) → {"status":"success","warning"?:str}；
//!   目标凭据缺失/失效 → Err(中文错误文案)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tauri::State;

use crate::accounts::AccountInfo;
use crate::auth::browser_login::{find_login_browser, open_browser_login};
use crate::auth::webview_login::open_webview_login;
use crate::db::TERMINAL_TASK_STATUSES;
use crate::pixiv::browse_api::invalidate_session_caches;
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
        let client = Arc::new(
            PixivClient::new(&cookies)
                .map_err(|err| ProbeFailure::Other(format!("创建 HTTP 客户端失败: {err}")))?,
        );
        let api = crate::pixiv::api::PixivApi::new(client.clone());
        let (user_data, _token) = api.get_user_self().await.map_err(|err| match err {
            PixivError::Auth => ProbeFailure::Auth,
            other => ProbeFailure::Other(other.to_string()),
        })?;
        Ok::<(Arc<PixivClient>, Value), ProbeFailure>((client, user_data))
    };

    match tokio::time::timeout(Duration::from_secs(AUTH_STATUS_TIMEOUT_SEC), attempt).await {
        Err(_) => {
            log::warn!("登录态验证超时（{AUTH_STATUS_TIMEOUT_SEC}s），按未登录处理（保留 cookie）");
            Ok(json!({ "is_logged_in": false }))
        }
        Ok(Err(ProbeFailure::Auth)) => {
            // 401/403：登录态确定失效——清除 default 镜像，并从账号列表移除
            // 该账号（含其独立凭据条目），避免「死账号」反复出现在切换列表里
            let active = state.accounts.active();
            if let Err(err) = state.cookies.clear() {
                log::warn!("清除失效登录态失败: {err}");
            }
            if let Some(uid) = active {
                if let Err(err) = state.accounts.remove(&uid) {
                    log::warn!("移除失效账号失败: {err}");
                }
            }
            // 登录身份已清，会话缓存（自 uid / csrf token）一并失效
            invalidate_session_caches();
            Ok(json!({ "is_logged_in": false }))
        }
        Ok(Err(ProbeFailure::Other(msg))) => {
            log::warn!("登录态验证失败（保留 cookie）: {msg}");
            Ok(json!({ "is_logged_in": false }))
        }
        Ok(Ok((client, user_data))) => {
            let mut body = auth_status_body(&user_data);
            // 头像回显：i.pximg.net 有 Referer 防盗链，webview 直连 403，
            // 需后端代下到 data/cache 并经 pixiv-avatar:// 协议供前端显示。
            // 下载失败不影响登录态（前端兜底首字母）。
            let mut avatar_file = String::new();
            if let Some(url) = body
                .get("profile_img")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                if let Some(file) = ensure_avatar_file(&state, &client, url).await {
                    avatar_file = file.clone();
                    body["avatar_url"] = json!(avatar_scheme_url(&file));
                    log::info!("头像缓存命中: {}", body["avatar_url"]);
                }
            }
            // 多账号：校验成功即登记/刷新当前账号（旧单账号 default 数据
            // 由此自动建档迁移进索引；失败只记日志，不影响状态返回）
            if body.get("is_logged_in").and_then(Value::as_bool) == Some(true) {
                // 状态校验只刷新索引，不读取/重写当前账号的 Keychain 副本。
                enroll_account(&state, &body, &cookies, &avatar_file, false);
            }
            Ok(body)
        }
    }
}

/// 头像下载超时（秒）。登录态验证本身 2s 内完成，头像慢些无碍 UI。
const AVATAR_DOWNLOAD_TIMEOUT_SEC: u64 = 5;

/// 头像 URL 最后段做缓存文件名，白名单字符过滤防路径穿越；空/非法 → None。
fn avatar_filename(url: &str) -> Option<String> {
    let name = url.rsplit('/').next()?.trim();
    let valid = !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    valid.then(|| name.to_string())
}

/// 缓存文件名 → 前端可用的自定义协议 URL。
///
/// Windows：wry WebView2 自定义协议走 WebResourceRequested 拦截，filter
/// 前缀由 use_https_scheme 决定（默认 false → `http://pixiv-avatar.localhost*`，
/// wry webview2/mod.rs:472,935）。URL 必须是 http 形态，https 会漏拦走真实
/// DNS（*.localhost 保留域必失败）。macOS 直接注册 scheme，无前缀转换。
fn avatar_scheme_url(filename: &str) -> String {
    if cfg!(windows) {
        format!("http://pixiv-avatar.localhost/{filename}")
    } else {
        format!("pixiv-avatar://localhost/{filename}")
    }
}

/// 确保 `data/cache/` 有头像缓存：有则直接返回文件名，无则用登录态客户端
/// 代下（download_bytes 已带 pixiv Referer 过防盗链）。返回缓存文件名。
async fn ensure_avatar_file(state: &AppState, client: &PixivClient, url: &str) -> Option<String> {
    let filename = avatar_filename(url)?;
    let dir = state.paths.data_dir.join("cache");
    let path = dir.join(&filename);
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Some(filename);
    }
    let bytes = tokio::time::timeout(
        Duration::from_secs(AVATAR_DOWNLOAD_TIMEOUT_SEC),
        client.download_bytes(url),
    )
    .await
    .ok()
    .and_then(|r| r.ok());
    let Some(bytes) = bytes else {
        log::warn!("头像下载失败（前端回退首字母）: {url}");
        return None;
    };
    let _ = tokio::fs::create_dir_all(&dir).await;
    if let Err(err) = tokio::fs::write(&path, &bytes).await {
        log::warn!("头像缓存写入失败: {err}");
        return None;
    }
    Some(filename)
}

/// 登记账号进多账号管理（凭据归档到 `u-<user_id>` + 索引 upsert + 激活）。
/// 失败只记日志不向调用方传播：default 镜像已写成功，索引可由后续
/// auth_status 校验成功补建档。
fn enroll_account(
    state: &AppState,
    user: &Value,
    cookies: &HashMap<String, String>,
    avatar_file: &str,
    persist_credentials: bool,
) {
    let Some(mut info) = AccountInfo::from_user_value(user) else {
        log::warn!("登录响应缺少用户信息，跳过多账号登记");
        return;
    };
    info.avatar_file = avatar_file.to_string();
    let known = state
        .accounts
        .list()
        .iter()
        .any(|account| account.user_id == info.user_id);
    let result = if should_persist_credentials(persist_credentials, known) {
        state.accounts.enroll(info, cookies)
    } else {
        state.accounts.upsert(info)
    };
    if let Err(err) = result {
        log::warn!("登记账号失败: {err}");
    }
}

fn should_persist_credentials(explicit_login: bool, known_account: bool) -> bool {
    explicit_login || !known_account
}

fn fallback_account_id<'a>(accounts: &'a [AccountInfo], removed: Option<&str>) -> Option<&'a str> {
    accounts
        .iter()
        .find(|account| Some(account.user_id.as_str()) != removed)
        .map(|account| account.user_id.as_str())
}

/// 非终态抓取任务提示：任务在创建时快照账号凭据、全程沿用（重试才读当前
/// 账号），切换 / 退出账号不影响其运行，但访问权限与限速按创建时账号执行。
/// 有非终态任务时返回提示文案，随 logout / account_switch 响应交前端提示；
/// 查询失败视为无提示（提示属增强信息，不得阻塞登出 / 切换主流程）。
fn running_task_warning(state: &AppState) -> Option<String> {
    let items = state.db.list_tasks(None).ok()?;
    let running = items
        .iter()
        .filter(|task| !TERMINAL_TASK_STATUSES.contains(&task.status.as_str()))
        .count();
    (running > 0).then(|| format!("有 {running} 个进行中的抓取任务仍按其创建时的账号执行"))
}

/// 多账号列表响应体（纯函数，离线可测）：active + 账号元信息数组，
/// 有本地头像缓存的账号附带协议 URL。
fn account_list_body(active: Option<String>, accounts: Vec<AccountInfo>) -> Value {
    let items: Vec<Value> = accounts
        .into_iter()
        .map(|a| {
            let mut v = serde_json::to_value(&a).unwrap_or_else(|_| json!({}));
            if !a.avatar_file.is_empty() {
                v["avatar_url"] = json!(avatar_scheme_url(&a.avatar_file));
            }
            v
        })
        .collect();
    json!({ "active": active, "accounts": items })
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
        // 多账号：新登录账号登记并激活（头像缓存由随后前端的 auth_status 补）
        enroll_account(&state, &user, cookies, "", true);
        // 登录身份已变，会话缓存（自 uid / csrf token）一并失效
        invalidate_session_caches();
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
    // 多账号：登记并激活（同 auth_login）
    enroll_account(&state, &user, &cookies, "", true);
    // 登录身份已变，会话缓存（自 uid / csrf token）一并失效
    invalidate_session_caches();
    Ok(json!({
        "status": "success",
        "message": "Cookie 已保存",
        "user": user,
    }))
}

/// 清空当前账号登录态（等价旧 POST /api/auth/logout）。
/// 多账号语义：退出 = 清 default 镜像 + 从账号列表移除该账号（含其独立
/// 凭据条目）；有剩余账号时自动激活首个。
/// 响应体：`fallback_applied` 是否成功回退到剩余账号；回退任一步失败只记
/// 日志按纯登出返回 success（此刻后端真实状态就是未登录，向上抛错会让
/// 用户看到「登出失败」却实际已登出）；`warning` 为非终态任务提示（可选）。
#[tauri::command]
pub async fn auth_logout(state: State<'_, AppState>) -> Result<Value, String> {
    let active = state.accounts.active();
    let fallback =
        fallback_account_id(&state.accounts.list(), active.as_deref()).map(str::to_owned);
    // 清除失败不影响响应（条目不存在视为已清空；其余失败仅记日志）
    if let Err(err) = state.cookies.clear() {
        log::warn!("清除登录态失败: {err}");
    }
    if let Some(uid) = active {
        if let Err(err) = state.accounts.remove(&uid) {
            log::warn!("移除已退出账号失败: {err}");
        }
    }
    let mut fallback_applied = false;
    if let Some(next) = fallback {
        match state.accounts.load_cookies(&next) {
            Ok(Some(cookies)) if cookies.get("PHPSESSID").is_some_and(|v| !v.is_empty()) => {
                if let Err(err) = state.cookies.save(&cookies) {
                    log::warn!("回退账号写入 default 失败，按纯登出处理: {err}");
                } else if let Err(err) = state.accounts.set_active(&next) {
                    log::warn!("回退账号激活失败，按纯登出处理: {err}");
                } else {
                    fallback_applied = true;
                }
            }
            Ok(_) => {
                // 凭据从未存过或缺 PHPSESSID：条目确定无法切换，移出列表
                // 维持「列表中的账号皆可切换」（同 auth_status 失效清理模式）
                log::warn!("回退账号凭据缺失，已从列表移除: {next}");
                if let Err(err) = state.accounts.remove(&next) {
                    log::warn!("移除缺失凭据的回退账号失败: {err}");
                }
            }
            Err(err) => {
                // 凭据读取失败（坏 JSON / keyring 暂时故障）：可能是暂时性
                // 不可读，保留条目供后续重试，本次按纯登出处理
                log::warn!("读取回退账号凭据失败，按纯登出处理: {err}");
            }
        }
    }
    // 登录身份已变（无论是否回退成功），会话缓存一并失效
    invalidate_session_caches();
    let mut body = json!({ "status": "success", "fallback_applied": fallback_applied });
    if let Some(warning) = running_task_warning(&state) {
        body["warning"] = json!(warning);
    }
    Ok(body)
}

/// 已保存账号列表（含当前激活标记与本地头像协议 URL）。
#[tauri::command]
pub async fn auth_accounts_list(state: State<'_, AppState>) -> Result<Value, String> {
    Ok(account_list_body(
        state.accounts.active(),
        state.accounts.list(),
    ))
}

/// 切换当前账号：
/// 1) 目标账号条目凭据写入 default（缺 PHPSESSID → Err）
/// 2) 索引 active 指向目标
/// 3) 会话缓存失效 + 非终态任务提示（`warning`，可选）
#[tauri::command]
pub async fn auth_account_switch(
    state: State<'_, AppState>,
    user_id: String,
) -> Result<Value, String> {
    if !state
        .accounts
        .list()
        .iter()
        .any(|account| account.user_id == user_id)
    {
        return Err("账号未登记，无法切换登录态".to_string());
    }
    let target = state
        .accounts
        .load_cookies(&user_id)?
        .filter(|c| c.get("PHPSESSID").is_some_and(|v| !v.is_empty()))
        .ok_or_else(|| "该账号的登录态不存在或已失效，请重新登录".to_string())?;

    state.cookies.save(&target)?;
    state.accounts.set_active(&user_id)?;
    // 登录身份已变：自 uid / csrf token 进程缓存立即失效，避免收藏列表 /
    // 写操作在 TTL 内仍按旧账号身份取数或携带旧会话 token
    invalidate_session_caches();
    log::info!("已切换当前账号");

    let mut body = json!({ "status": "success" });
    if let Some(warning) = running_task_warning(&state) {
        body["warning"] = json!(warning);
    }
    Ok(body)
}

// ----------------------------------------------------------------------
// 单元测试（纯函数，离线）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, TaskInsert, now_iso};
    use crate::paths::AppPaths;
    use crate::settings::Settings;

    /// 真实临时库构造（对齐 task_cmds tests 的 temp_state 模式）。
    fn temp_state(tag: &str) -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "pixiv-tool-authcmd-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("app.db")).unwrap();
        let paths = AppPaths {
            data_dir: dir.clone(),
            config_dir: dir.join("config"),
            logs_dir: dir.join("logs"),
        };
        (AppState::new(paths, Settings::default(), db), dir)
    }

    fn insert_task(state: &AppState, task_id: &str, status: &str) {
        let now = now_iso();
        state
            .db
            .insert_task(&TaskInsert {
                task_id: task_id.to_string(),
                source_type: "single".into(),
                source_id: "1".into(),
                status: status.into(),
                created_at: now.clone(),
                updated_at: now,
                ..Default::default()
            })
            .unwrap();
    }

    #[test]
    fn running_task_warning_counts_non_terminal_only() {
        let (state, dir) = temp_state("warn");
        // 空库 → 无提示
        assert!(running_task_warning(&state).is_none());
        // 非终态任务计入，终态任务不计入
        insert_task(&state, "r1", "running");
        insert_task(&state, "p1", "paused");
        insert_task(&state, "d1", "done");
        insert_task(&state, "c1", "canceled");
        let warning = running_task_warning(&state).unwrap();
        assert!(
            warning.contains("2"),
            "提示应含非终态任务数，实际: {warning}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

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

    #[test]
    fn avatar_filename_filters_and_extracts() {
        assert_eq!(
            avatar_filename("https://i.pximg.net/user-profile/img/2024/01/01/12345_abcdef_50.jpg"),
            Some("12345_abcdef_50.jpg".to_string())
        );
        // 路径穿越与非法字符被拒
        assert_eq!(avatar_filename("https://x/..%2Fevil"), None);
        assert_eq!(avatar_filename("https://x/"), None);
        assert_eq!(avatar_filename("https://x/a b.jpg"), None);
    }

    #[test]
    fn avatar_scheme_url_platform_shape() {
        let url = avatar_scheme_url("a_50.jpg");
        if cfg!(windows) {
            assert_eq!(url, "http://pixiv-avatar.localhost/a_50.jpg");
        } else {
            assert_eq!(url, "pixiv-avatar://localhost/a_50.jpg");
        }
    }

    #[test]
    fn account_list_body_shape() {
        let mut a1 = AccountInfo {
            user_id: "100".into(),
            pixiv_id: "pid_100".into(),
            name: "用户100".into(),
            profile_img: "https://i.pximg.net/100.jpg".into(),
            avatar_file: "100_50.jpg".into(),
            saved_at: 0,
        };
        let a2 = AccountInfo {
            avatar_file: String::new(),
            ..a1.clone()
        };
        a1.saved_at = 123;

        let body = account_list_body(Some("100".into()), vec![a1, a2]);
        assert_eq!(body["active"], json!("100"));
        let items = body["accounts"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["user_id"], json!("100"));
        assert_eq!(
            items[0]["avatar_url"],
            json!(avatar_scheme_url("100_50.jpg"))
        );
        assert_eq!(items[0]["saved_at"], json!(123));
        // 无头像缓存的账号不带 avatar_url 键（前端回退首字母）
        assert!(items[1].get("avatar_url").is_none());

        // 未登录 / 空列表
        let empty = account_list_body(None, Vec::new());
        assert_eq!(empty["active"], json!(null));
        assert_eq!(empty["accounts"], json!([]));
    }

    #[test]
    fn logout_falls_back_to_first_remaining_account() {
        let account = |user_id: &str| AccountInfo {
            user_id: user_id.into(),
            pixiv_id: String::new(),
            name: String::new(),
            profile_img: String::new(),
            avatar_file: String::new(),
            saved_at: 0,
        };
        let accounts = vec![account("100"), account("200")];

        assert_eq!(fallback_account_id(&accounts, Some("200")), Some("100"));
        assert_eq!(fallback_account_id(&accounts, Some("100")), Some("200"));
        assert_eq!(fallback_account_id(&[], Some("100")), None);
    }

    #[test]
    fn status_probe_only_persists_legacy_single_account() {
        assert!(should_persist_credentials(false, false));
        assert!(!should_persist_credentials(false, true));
        assert!(should_persist_credentials(true, true));
    }
}
