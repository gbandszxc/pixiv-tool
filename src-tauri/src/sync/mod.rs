//! WebDAV 同步：把浏览历史与 pixiv 多账户登录态加密打包上传，或拉取远端包恢复。
//!
//! 模块划分：
//! - [`crypto`]：包格式与 AES-256-GCM 加解密（密钥由口令 PBKDF2 派生）
//! - [`webdav`]：独立 wreq 客户端（无浏览器指纹伪装）+ MKCOL / PUT / GET
//! - [`archive`]：载荷 JSON 形状、导出与历史合并
//! - [`keys`]：同步专用凭据（WebDAV 密码、同步口令）的 keyring 封装
//! - [`service`]：编排：配置解析 → 导出/导入 → 加解密 → 网络
//!
//! 错误一律是 [`SyncError`]（`{code, field, message}`，ADR 0044 形状）；
//! 任何日志与错误文案都不得出现 cookie、WebDAV 密码或同步口令原文。

pub mod archive;
pub mod crypto;
pub mod keys;
pub mod service;
pub mod webdav;

pub use service::{DownloadSummary, SyncConfigOverride, TestConnectionOutcome, UploadSummary};

use serde::Serialize;

/// 同步范围白名单（命令参数 `scopes` 的合法值）。
pub const SYNC_SCOPES: [&str; 2] = ["history", "auth"];

/// 远端固定文件名（`${base_url}/pixiv-tool-sync.bin`）。
pub const REMOTE_FILE_NAME: &str = "pixiv-tool-sync.bin";

/// 同步范围（命令参数白名单内的一对取值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// 浏览访问历史（`browse_history` 表）。
    History,
    /// pixiv 多账户登录态（账号索引 + 每账号 keyring 条目）。
    Auth,
}

/// 归一化 `scopes` 参数：去重 + 白名单过滤；空集或含未知值 → `sync_no_scope`。
pub fn normalize_scopes(raw: &[String]) -> Result<Vec<Scope>, SyncError> {
    let mut scopes = Vec::new();
    for value in raw {
        let scope = match value.trim() {
            "history" => Scope::History,
            "auth" => Scope::Auth,
            _ => return Err(SyncError::no_scope()),
        };
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    if scopes.is_empty() {
        return Err(SyncError::no_scope());
    }
    Ok(scopes)
}

/// 同步命令的结构化错误载荷（`{code, field, message}`，形状对齐 ADR 0044）。
///
/// `message` 是中文兜底文案，只描述失败类别与（必要时）HTTP 状态码；
/// 不含 cookie / 密码 / 口令 / 密文，也不回显底层解密错误细节。
#[derive(Debug, Clone, Serialize)]
pub struct SyncError {
    pub code: &'static str,
    pub field: Option<&'static str>,
    pub message: String,
}

impl SyncError {
    pub fn new(
        code: &'static str,
        field: Option<&'static str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            field,
            message: message.into(),
        }
    }

    /// WebDAV 地址 / 用户名缺失。
    pub fn config_incomplete(field: &'static str) -> Self {
        Self::new(
            "sync_config_incomplete",
            Some(field),
            "WebDAV 同步配置不完整",
        )
    }

    /// 地址不是 http/https，或不是可用的目录 URL。
    pub fn url_invalid() -> Self {
        Self::new(
            "sync_url_invalid",
            Some("webdav_url"),
            "WebDAV 地址必须是 http/https 链接",
        )
    }

    /// 401 / 403：地址对但凭据被拒。
    pub fn auth_failed() -> Self {
        Self::new(
            "sync_auth_failed",
            Some("webdav_password"),
            "WebDAV 认证失败，请检查用户名与密码",
        )
    }

    /// 远端包不存在（GET 404）。
    pub fn not_found() -> Self {
        Self::new(
            "sync_not_found",
            None,
            "远端没有可恢复的同步包，请先上传一次",
        )
    }

    /// 其他非 2xx 响应；文案只带状态码。
    pub fn http_status(status: u16) -> Self {
        Self::new(
            "sync_http_error",
            Some("webdav_url"),
            format!("WebDAV 服务返回 HTTP {status}"),
        )
    }

    /// 连接 / 超时 / TLS 等网络层错误。
    pub fn network(detail: impl Into<String>) -> Self {
        Self::new("sync_network_error", Some("webdav_url"), detail)
    }

    /// 下载体积超过上限。
    pub fn oversize(limit: u64) -> Self {
        Self::new(
            "sync_oversize",
            None,
            format!("远端同步包超过 {} MiB 上限", limit / (1024 * 1024)),
        )
    }

    /// 解密后的载荷不是本工具认识的同步包。
    pub fn payload_invalid() -> Self {
        Self::new("sync_payload_invalid", None, "同步包内容无效或版本不受支持")
    }

    /// 解密失败（认证失败 / 截断 / 头部参数异常）统一文案。
    pub fn decrypt_failed() -> Self {
        Self::new(
            "sync_decrypt_failed",
            Some("sync_passphrase"),
            "同步包解密失败，请检查同步口令",
        )
    }

    /// 范围参数为空或含未知值。
    pub fn no_scope() -> Self {
        Self::new("sync_no_scope", None, "请至少选择一个同步范围")
    }

    /// 本地状态读写失败（数据库 / 账号索引 / 凭据库）。
    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new("sync_internal", None, detail)
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SyncError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_dedupe_and_reject_unknown() {
        let scopes =
            normalize_scopes(&["history".into(), "auth".into(), "history".into()]).unwrap();
        assert_eq!(scopes, vec![Scope::History, Scope::Auth]);
        assert_eq!(
            normalize_scopes(&["history".into()]).unwrap(),
            vec![Scope::History]
        );
        for bad in [vec![], vec!["cookies".to_string()], vec!["".to_string()]] {
            assert_eq!(normalize_scopes(&bad).unwrap_err().code, "sync_no_scope");
        }
        // 去重后仍非空
        assert_eq!(
            normalize_scopes(&["auth".into(), "auth".into()]).unwrap(),
            vec![Scope::Auth]
        );
    }

    #[test]
    fn error_payload_shape() {
        let value = serde_json::to_value(SyncError::not_found()).unwrap();
        assert_eq!(value["code"], "sync_not_found");
        assert!(value["field"].is_null());
        assert!(value["message"].as_str().unwrap().contains("同步包"));
        let value = serde_json::to_value(SyncError::http_status(500)).unwrap();
        assert_eq!(value["code"], "sync_http_error");
        assert!(value["message"].as_str().unwrap().contains("500"));
    }
}
