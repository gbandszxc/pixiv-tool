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
use std::sync::RwLock;

pub const SERVICE: &str = "pixiv-tool.cookies";
pub const ACCOUNT: &str = "default";

pub struct CookieStore {
    service: String,
    account: String,
    /// 内存缓存：keychain 读取在 macOS 会弹授权框（dev 下二进制每次重编译
    /// 签名变化，「始终允许」失效），启动后只允许首次真实读取，
    /// 之后 load 全走缓存；save/clear 同步更新。
    /// None = 未读过；Some(map) = 已读（空 map 等价无登录态）。
    cache: RwLock<Option<HashMap<String, String>>>,
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
            cache: RwLock::new(None),
        }
    }

    /// 保存（覆盖）登录态。写入 keychain 成功后同步更新内存缓存。
    pub fn save(&self, cookies: &HashMap<String, String>) -> Result<(), String> {
        let payload =
            serde_json::to_string(cookies).map_err(|err| format!("登录态序列化失败: {err}"))?;
        let entry = self.entry()?;
        entry
            .set_password(&payload)
            .map_err(|err| format!("保存登录态失败: {err}"))?;
        if let Ok(mut c) = self.cache.write() {
            *c = Some(cookies.clone());
        }
        Ok(())
    }

    /// 读取登录态。None = 从未存储过；
    /// 坏 JSON → Err("...登录态已损坏，请重新登录")；非对象 → Err("...格式无效...")。
    ///
    /// 命中内存缓存直接返回（不触发 keychain 授权框）；仅进程内首次读取
    /// 落 keychain。读取失败（含用户拒绝授权）不写缓存，下次重试。
    pub fn load(&self) -> Result<Option<HashMap<String, String>>, String> {
        // 命中缓存：空 map 等价无登录态（None）
        if let Ok(c) = self.cache.read() {
            if let Some(map) = &*c {
                return Ok((!map.is_empty()).then(|| map.clone()));
            }
        }
        let entry = self.entry()?;
        let payload = match entry.get_password() {
            Ok(payload) => payload,
            Err(keyring::Error::NoEntry) => {
                self.remember(HashMap::new());
                return Ok(None);
            }
            Err(err) => return Err(format!("读取登录态失败: {err}")),
        };
        let parsed: serde_json::Value = serde_json::from_str(&payload)
            .map_err(|_| "系统凭据存储中的登录态已损坏，请重新登录".to_string())?;
        let map = parsed
            .as_object()
            .ok_or_else(|| "系统凭据存储中的登录态格式无效，请重新登录".to_string())?;
        // 值非字符串时退化为空串（正常数据不会出现）
        let cookies: HashMap<String, String> = map
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
            .collect();
        self.remember(cookies.clone());
        Ok(Some(cookies))
    }

    /// 清除登录态。条目不存在视为已清空（忽略）；其他错误向上抛。成功后清缓存。
    pub fn clear(&self) -> Result<(), String> {
        let entry = self.entry()?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {
                self.remember(HashMap::new());
                Ok(())
            }
            Err(err) => Err(format!("清除登录态失败: {err}")),
        }
    }

    /// 更新缓存（锁失败静默跳过，下次 load 落 keychain 重读）。
    fn remember(&self, cookies: HashMap<String, String>) {
        if let Ok(mut c) = self.cache.write() {
            *c = Some(cookies);
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
