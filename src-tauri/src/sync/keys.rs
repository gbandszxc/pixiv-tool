//! 同步专用凭据（系统 keyring）。
//!
//! service 固定 `pixiv-tool.sync`，两个条目：
//! - `webdav-password`：WebDAV 密码
//! - `sync-passphrase`：同步口令（AES-256-GCM PBKDF2）
//!
//! 绝不落 settings.json / 数据库 / 日志；[`read`] / [`write`] 的错误文案也不带原文。
//! 语义与翻译 API Key 一致：写入空串 = 删除条目。

use super::SyncError;

/// keyring service（与 cookies / 翻译凭据的 service 区分）。
pub const SERVICE: &str = "pixiv-tool.sync";
/// WebDAV 密码条目名。
pub const ACCOUNT_WEBDAV_PASSWORD: &str = "webdav-password";
/// 同步口令条目名。
pub const ACCOUNT_SYNC_PASSPHRASE: &str = "sync-passphrase";

/// 同步口令长度下限（字符）。
pub const PASSPHRASE_MIN_CHARS: usize = 8;
/// 同步口令长度上限（字符）。
pub const PASSPHRASE_MAX_CHARS: usize = 256;

fn entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, account).map_err(|_| "无法访问同步凭据存储".to_string())
}

/// 读取条目；不存在 → `Ok(None)`，凭据库不可用 → `Err(安全文案)`。
pub fn read(account: &str) -> Result<Option<String>, String> {
    match entry(account)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("无法读取同步凭据，请检查系统凭据存储".to_string()),
    }
}

/// 写入条目；空串 = 删除（不存在视为已清）。
pub fn write(account: &str, value: &str) -> Result<(), String> {
    let entry = entry(account)?;
    if value.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("无法清除同步凭据".to_string()),
        }
    } else {
        entry
            .set_password(value)
            .map_err(|_| "无法保存同步凭据".to_string())
    }
}

/// 同步口令校验：长度（字符数）必须在 [`PASSPHRASE_MIN_CHARS`]..=[`PASSPHRASE_MAX_CHARS`]。
/// 口令不做 trim（前后空格也是口令的一部分）。
pub fn validate_passphrase(value: &str) -> Result<(), SyncError> {
    let len = value.chars().count();
    if (PASSPHRASE_MIN_CHARS..=PASSPHRASE_MAX_CHARS).contains(&len) {
        Ok(())
    } else {
        Err(SyncError::passphrase_invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_length_bounds() {
        assert!(validate_passphrase("12345678").is_ok());
        assert!(validate_passphrase(&"x".repeat(PASSPHRASE_MAX_CHARS)).is_ok());
        for value in ["", "1234567", &"x".repeat(PASSPHRASE_MAX_CHARS + 1)] {
            let error = validate_passphrase(value).unwrap_err();
            assert_eq!(error.code, "sync_passphrase_invalid");
            assert_eq!(error.field, Some("sync_passphrase"));
            // 不得回显口令原文（空串不参与包含判断）
            if !value.is_empty() {
                assert!(!error.message.contains(value));
            }
        }
        // 字符数而非字节数（中文口令按字符计）
        assert!(validate_passphrase("同步口令测试一二").is_ok());
    }
}
