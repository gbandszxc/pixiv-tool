//! 同步编排：配置解析（含草稿覆盖）→ 导出 / 导入 → 加解密 → WebDAV。
//!
//! 命令层只做参数转发，所有语义在这里；`sync_test_connection` 不解析同步口令
//! （设置弹窗可以先测连通性再设口令）。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::settings::Settings;
use crate::state::AppState;

use super::archive::{self, AccountsPayload};
use super::{crypto, keys, normalize_scopes, webdav, Scope, SyncError};

/// 命令级配置覆盖（设置弹窗草稿可直接测试 / 上传，不必先保存）。
///
/// 全部字段 `Option`：`Some` 优先于已保存值；`None` 读 settings / keyring。
/// 嵌套结构体不经 Tauri 的参数名 camelCase 重写，故两种命名都接受。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SyncConfigOverride {
    #[serde(alias = "webdavUrl")]
    pub webdav_url: Option<String>,
    #[serde(alias = "webdavUsername")]
    pub webdav_username: Option<String>,
    #[serde(alias = "webdavPassword")]
    pub webdav_password: Option<String>,
    #[serde(alias = "syncPassphrase")]
    pub sync_passphrase: Option<String>,
}

/// `sync_upload` 返回体。
#[derive(Debug, Clone, Serialize)]
pub struct UploadSummary {
    pub exported_at: String,
    pub bytes: usize,
    pub history_count: usize,
    pub account_count: usize,
}

/// `sync_download` 返回体。
#[derive(Debug, Clone, Serialize)]
pub struct DownloadSummary {
    pub exported_at: String,
    pub history_imported: usize,
    pub accounts_imported: usize,
    pub active_changed: bool,
    pub auth_changed: bool,
}

/// 测试连接结果：MKCOL 状态码 + 云端备份探测。
#[derive(Debug, Clone)]
pub struct TestConnectionOutcome {
    /// MKCOL 实际返回的 HTTP 状态码。
    pub http_status: u16,
    /// 云端同步文件的 `Last-Modified` 原文；无备份（404）→ `None`。
    pub backup_time: Option<String>,
}

/// 测试连接：建目录（已存在也算通）后探测远端是否有同步包，不读不写包内容，
/// 不要求同步口令。
pub async fn test_connection(
    state: &AppState,
    over: &SyncConfigOverride,
) -> Result<TestConnectionOutcome, SyncError> {
    let settings = state.settings_snapshot();
    let endpoint = resolve_endpoint(&settings, over, keys::read)?;
    let http_status = webdav::test_connection(&endpoint).await?;
    let backup_time = webdav::probe_backup(&endpoint).await?;
    Ok(TestConnectionOutcome {
        http_status,
        backup_time,
    })
}

/// 导出 + 加密 + 上传。
pub async fn upload(
    state: &AppState,
    over: &SyncConfigOverride,
    scopes: &[String],
) -> Result<UploadSummary, SyncError> {
    let scopes = normalize_scopes(scopes)?;
    let settings = state.settings_snapshot();
    let endpoint = resolve_endpoint(&settings, over, keys::read)?;
    let passphrase = resolve_passphrase(over, keys::read)?;

    let (payload, counts) = archive::build_payload(&state.db, &state.accounts, &scopes)?;
    let plaintext =
        serde_json::to_vec(&payload).map_err(|_| SyncError::internal("同步包序列化失败"))?;
    let packed = crypto::encrypt(&passphrase, &plaintext)?;
    let bytes = packed.len();
    webdav::upload(&endpoint, &packed).await?;

    Ok(UploadSummary {
        exported_at: payload.exported_at,
        bytes,
        history_count: counts.history_count,
        account_count: counts.account_count,
    })
}

/// 下载 + 解密 + 合并写回。
pub async fn download(
    state: &AppState,
    over: &SyncConfigOverride,
    scopes: &[String],
) -> Result<DownloadSummary, SyncError> {
    let scopes = normalize_scopes(scopes)?;
    let settings = state.settings_snapshot();
    let endpoint = resolve_endpoint(&settings, over, keys::read)?;
    let passphrase = resolve_passphrase(over, keys::read)?;

    let packed = webdav::download(&endpoint).await?;
    let plaintext = crypto::decrypt(&passphrase, &packed)?;
    let payload = archive::parse_payload(&plaintext)?;

    let history_imported = if scopes.contains(&Scope::History) {
        match payload.history.as_deref() {
            Some(items) => archive::import_history(&state.db, items)?,
            None => 0,
        }
    } else {
        0
    };

    let (accounts_imported, active_changed, auth_changed) = if scopes.contains(&Scope::Auth) {
        match payload.accounts.as_ref() {
            Some(accounts) => import_accounts(state, accounts)?,
            None => (0, false, false),
        }
    } else {
        (0, false, false)
    };

    Ok(DownloadSummary {
        exported_at: payload.exported_at,
        history_imported,
        accounts_imported,
        active_changed,
        auth_changed,
    })
}

/// 解析 WebDAV 端点：`over` 优先，其次 settings / keyring。
fn resolve_endpoint(
    settings: &Settings,
    over: &SyncConfigOverride,
    read_credential: impl Fn(&str) -> Result<Option<String>, String>,
) -> Result<webdav::WebDavEndpoint, SyncError> {
    let url = over
        .webdav_url
        .clone()
        .unwrap_or_else(|| settings.webdav_url.clone());
    if url.trim().is_empty() {
        return Err(SyncError::config_incomplete("webdav_url"));
    }
    let username = over
        .webdav_username
        .clone()
        .unwrap_or_else(|| settings.webdav_username.clone());
    let password = match &over.webdav_password {
        Some(value) => Some(value.clone()),
        None => read_credential(keys::ACCOUNT_WEBDAV_PASSWORD).map_err(SyncError::internal)?,
    }
    .filter(|value| !value.is_empty());

    if !username.trim().is_empty() && password.is_none() {
        return Err(SyncError::config_incomplete("webdav_password"));
    }
    webdav::WebDavEndpoint::new(&url, &username, password.as_deref().unwrap_or(""))
}

/// 解析同步口令：`over` 优先，其次 keyring；trim 后为空（未配置或显式清除）=
/// 使用内置默认口令派生密钥（ADR 0049）。不做长度限制。
fn resolve_passphrase(
    over: &SyncConfigOverride,
    read_credential: impl Fn(&str) -> Result<Option<String>, String>,
) -> Result<String, SyncError> {
    let value = match &over.sync_passphrase {
        Some(value) => Some(value.clone()),
        None => read_credential(keys::ACCOUNT_SYNC_PASSPHRASE).map_err(SyncError::internal)?,
    };
    let passphrase = value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| keys::DEFAULT_PASSPHRASE.to_string());
    Ok(passphrase)
}

/// 账户写回：
/// - 逐条 `store_cookies` + `upsert`（同名覆盖），跳过条目已在解析期过滤；
/// - active 规则：本地已有 active 则保持不变；仅当本地无 active 且云端 active
///   在导入集合内时才落 default 并切换；云端 active == 本地 active 时用新
///   cookies 刷新 default；
/// - 不删除本地额外账户；
/// - 任何账户写入后失效 pixiv 会话缓存。
fn import_accounts(
    state: &AppState,
    payload: &AccountsPayload,
) -> Result<(usize, bool, bool), SyncError> {
    let local_active = state.accounts.active();
    let mut imported: Vec<(&str, &HashMap<String, String>)> = Vec::new();

    for item in &payload.items {
        state
            .accounts
            .store_cookies(&item.user_id, &item.cookies)
            .map_err(|_| SyncError::internal(format!("保存账号 {} 登录态失败", item.user_id)))?;
        state
            .accounts
            .upsert(item.info.clone())
            .map_err(|_| SyncError::internal(format!("写入账号 {} 信息失败", item.user_id)))?;
        imported.push((&item.user_id, &item.cookies));
    }

    let mut active_changed = false;
    let mut default_written = false;
    let find_cookies = |user_id: &str| -> Option<&HashMap<String, String>> {
        imported
            .iter()
            .find(|(id, _)| *id == user_id)
            .map(|(_, cookies)| *cookies)
    };

    match local_active {
        Some(local) => {
            // upsert 会把 active 指向最后写入的账号，这里还原本地既有选择。
            state
                .accounts
                .set_active_pointer(Some(local.clone()))
                .map_err(|_| SyncError::internal("恢复当前账号失败"))?;
            if payload.active.as_deref() == Some(local.as_str()) {
                if let Some(cookies) = find_cookies(&local) {
                    state
                        .cookies
                        .save(cookies)
                        .map_err(|_| SyncError::internal("写入当前账号登录态失败"))?;
                    default_written = true;
                }
            }
        }
        None => match payload.active.as_deref() {
            Some(cloud) => match find_cookies(cloud) {
                Some(cookies) => {
                    state
                        .cookies
                        .save(cookies)
                        .map_err(|_| SyncError::internal("写入当前账号登录态失败"))?;
                    state
                        .accounts
                        .set_active_pointer(Some(cloud.to_string()))
                        .map_err(|_| SyncError::internal("切换当前账号失败"))?;
                    active_changed = true;
                    default_written = true;
                }
                // 云端 active 不在导入集合：保持没有 active（不擅自挑一个）。
                None => state
                    .accounts
                    .set_active_pointer(None)
                    .map_err(|_| SyncError::internal("更新当前账号失败"))?,
            },
            None => state
                .accounts
                .set_active_pointer(None)
                .map_err(|_| SyncError::internal("更新当前账号失败"))?,
        },
    }

    let accounts_imported = imported.len();
    let auth_changed = accounts_imported > 0 || default_written;
    if auth_changed {
        crate::pixiv::browse_api::invalidate_session_caches();
    }
    Ok((accounts_imported, active_changed, auth_changed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_credentials(_: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn credential(value: &'static str) -> impl Fn(&str) -> Result<Option<String>, String> {
        move |account: &str| {
            assert_eq!(account, keys::ACCOUNT_WEBDAV_PASSWORD);
            Ok(Some(value.to_string()))
        }
    }

    #[test]
    fn endpoint_override_wins_over_settings() {
        let mut settings = Settings::default();
        settings.webdav_url = "https://saved.example.com/dav".into();
        settings.webdav_username = "saved-user".into();

        let over = SyncConfigOverride {
            webdav_url: Some("http://draft.example.com/dav/".into()),
            webdav_username: Some("draft-user".into()),
            webdav_password: Some("draft-pass".into()),
            sync_passphrase: None,
        };
        let endpoint = resolve_endpoint(&settings, &over, no_credentials).unwrap();
        assert_eq!(endpoint.base_url, "http://draft.example.com/dav");
        assert_eq!(endpoint.username, "draft-user");
        assert_eq!(endpoint.password, "draft-pass");

        // 全部缺省 → 用已保存值 + keyring
        let endpoint =
            resolve_endpoint(&settings, &SyncConfigOverride::default(), credential("pw")).unwrap();
        assert_eq!(endpoint.base_url, "https://saved.example.com/dav");
        assert_eq!(endpoint.password, "pw");
    }

    #[test]
    fn endpoint_errors() {
        // 地址缺失
        let error = resolve_endpoint(
            &Settings::default(),
            &SyncConfigOverride::default(),
            no_credentials,
        )
        .unwrap_err();
        assert_eq!(error.code, "sync_config_incomplete");
        assert_eq!(error.field, Some("webdav_url"));

        // 地址非法
        let over = SyncConfigOverride {
            webdav_url: Some("ftp://dav.example.com".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_endpoint(&Settings::default(), &over, no_credentials)
                .unwrap_err()
                .code,
            "sync_url_invalid"
        );

        // 有用户名但 keyring 无密码
        let over = SyncConfigOverride {
            webdav_url: Some("https://dav.example.com".into()),
            webdav_username: Some("alice".into()),
            ..Default::default()
        };
        let error = resolve_endpoint(&Settings::default(), &over, no_credentials).unwrap_err();
        assert_eq!(error.code, "sync_config_incomplete");
        assert_eq!(error.field, Some("webdav_password"));

        // 匿名（无用户名）不要求密码
        let over = SyncConfigOverride {
            webdav_url: Some("https://dav.example.com".into()),
            ..Default::default()
        };
        assert!(resolve_endpoint(&Settings::default(), &over, no_credentials).is_ok());
    }

    #[test]
    fn passphrase_resolution() {
        // 草稿覆盖优先；不做长度限制（任意非空长度都可用）
        let over = SyncConfigOverride {
            sync_passphrase: Some("12345678".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_passphrase(&over, no_credentials).unwrap(),
            "12345678"
        );
        let over = SyncConfigOverride {
            sync_passphrase: Some("短".into()),
            ..Default::default()
        };
        assert_eq!(resolve_passphrase(&over, no_credentials).unwrap(), "短");

        // 未设置 / 显式清除（None 或空串）→ 内置默认口令（ADR 0049）
        assert_eq!(
            resolve_passphrase(&SyncConfigOverride::default(), no_credentials).unwrap(),
            keys::DEFAULT_PASSPHRASE
        );
        let over = SyncConfigOverride {
            sync_passphrase: Some("   ".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_passphrase(&over, no_credentials).unwrap(),
            keys::DEFAULT_PASSPHRASE
        );

        // 未覆盖时读 keyring
        let read = |account: &str| {
            assert_eq!(account, keys::ACCOUNT_SYNC_PASSPHRASE);
            Ok(Some("from-keyring-1234".to_string()))
        };
        assert_eq!(
            resolve_passphrase(&SyncConfigOverride::default(), read).unwrap(),
            "from-keyring-1234"
        );
    }
}
