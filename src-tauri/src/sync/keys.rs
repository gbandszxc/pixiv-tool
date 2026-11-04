//! 同步专用凭据（系统 keyring）。
//!
//! service 固定 `pixiv-tool.sync`，两个条目：
//! - `webdav-password`：WebDAV 密码
//! - `sync-passphrase`：同步口令（AES-256-GCM PBKDF2）
//!
//! 绝不落 settings.json / 数据库 / 日志；[`read`] / [`write`] 的错误文案也不带原文。
//! 语义与翻译 API Key 一致：写入空串 = 删除条目。同步口令不再做长度校验、允许
//! 未配置（此时用 [`DEFAULT_PASSPHRASE`] 派生密钥，ADR 0049）。

/// keyring service（与 cookies / 翻译凭据的 service 区分）。
pub const SERVICE: &str = "pixiv-tool.sync";
/// WebDAV 密码条目名。
pub const ACCOUNT_WEBDAV_PASSWORD: &str = "webdav-password";
/// 同步口令条目名。
pub const ACCOUNT_SYNC_PASSPHRASE: &str = "sync-passphrase";

/// 内置默认同步口令：未设置自定义口令时用它派生加密密钥（ADR 0049）。
///
/// 保护强度是**混淆级**：可防止 WebDAV 托管方与偶然拿到文件的人直接看到明文，
/// 但不对抗拥有应用二进制（可提取此常量）的人；需要更强保护应设置自定义口令。
pub const DEFAULT_PASSPHRASE: &str = "pixiv-tool:default-sync-passphrase:v1";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_passphrase_is_usable_key_input() {
        // 内置默认口令非空且无控制字符，可安全进入 PBKDF2
        assert!(!DEFAULT_PASSPHRASE.is_empty());
        assert!(!DEFAULT_PASSPHRASE.chars().any(char::is_control));
    }
}
