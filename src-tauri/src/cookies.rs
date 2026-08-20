//! 登录态 Cookie 存储 —— 基于系统凭据管理器（keyring 3）。
//!
//! - Windows: Credential Manager（windows-native）
//! - macOS:   Keychain（apple-native）
//! - Linux:   Secret Service / kernel keyutils（linux-native-sync-persistent；
//!   无 Secret Service 时操作返回错误文案，可接受）
//!
//! 存储内容：cookies HashMap 的**紧凑 JSON**（键值原样，含 PHPSESSID /
//! x-csrf-token / 其他 pixiv.net cookie）。x-csrf-token 从 map 中拆出/合入是
//! 调用方（core）的事，这里只做原样存取。

use std::collections::HashMap;

pub const SERVICE: &str = "pixiv-tool.cookies";
pub const ACCOUNT: &str = "default";

pub struct CookieStore {
    service: String,
    account: String,
}

impl Default for CookieStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CookieStore {
    pub fn new() -> Self {
        Self {
            service: SERVICE.to_string(),
            account: ACCOUNT.to_string(),
        }
    }

    /// 保存（覆盖）登录态。
    pub fn save(&self, cookies: &HashMap<String, String>) -> Result<(), String> {
        let payload =
            serde_json::to_string(cookies).map_err(|err| format!("登录态序列化失败: {err}"))?;
        let entry = self.entry()?;
        entry
            .set_password(&payload)
            .map_err(|err| format!("保存登录态失败: {err}"))
    }

    /// 读取登录态。None = 从未存储过；
    /// 坏 JSON → Err("...登录态已损坏，请重新登录")；非对象 → Err("...格式无效...")。
    pub fn load(&self) -> Result<Option<HashMap<String, String>>, String> {
        let entry = self.entry()?;
        let payload = match entry.get_password() {
            Ok(payload) => payload,
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(err) => return Err(format!("读取登录态失败: {err}")),
        };
        let parsed: serde_json::Value = serde_json::from_str(&payload)
            .map_err(|_| "系统凭据存储中的登录态已损坏，请重新登录".to_string())?;
        let map = parsed
            .as_object()
            .ok_or_else(|| "系统凭据存储中的登录态格式无效，请重新登录".to_string())?;
        // 值非字符串时退化为空串（正常数据不会出现）
        Ok(Some(
            map.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect(),
        ))
    }

    /// 清除登录态。条目不存在视为已清空（忽略）；其他错误向上抛。
    pub fn clear(&self) -> Result<(), String> {
        let entry = self.entry()?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(format!("清除登录态失败: {err}")),
        }
    }

    /// keyring 3 的 Entry 不 Clone，按需创建（开销可忽略）。
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(&self.service, &self.account)
            .map_err(|err| format!("当前平台登录态存储不可用: {err}"))
    }
}

// 说明：CookieStore 不写单测——save/load/clear 直连真实系统凭据存储
// （macOS Keychain 会弹授权、CI 无 Secret Service），留给人工/集成验证。
