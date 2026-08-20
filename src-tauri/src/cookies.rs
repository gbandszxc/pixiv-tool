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
//!
//! **Windows 分片**：Credential Manager 单条 blob 上限 2560 UTF-16 字符
//! （CRED_MAX_CREDENTIAL_BLOB_SIZE），pixiv 完整 cookie JSON 超限。超限时拆成
//! 多条目存储：`default` 存头 `{"v":2,"parts":N}`（提交点，最后写入），
//! `default.p1..pN` 存分片内容。macOS/Linux 无此限制但走同一路径，行为一致。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const SERVICE: &str = "pixiv-tool.cookies";
pub const ACCOUNT: &str = "default";

/// 单分片字符数上限。Windows Credential Manager blob 上限 2560 字节
/// （UTF-16 每字符 2 字节 = 1280 字符），留余量取 1200。
const PART_CHAR_LIMIT: usize = 1200;
const HEADER_VERSION: u8 = 2;

#[derive(Serialize, Deserialize)]
struct ShardHeader {
    v: u8,
    parts: usize,
}

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

    /// 保存（覆盖）登录态。超限时自动分片。
    pub fn save(&self, cookies: &HashMap<String, String>) -> Result<(), String> {
        let payload =
            serde_json::to_string(cookies).map_err(|err| format!("登录态序列化失败: {err}"))?;

        // 先清掉可能的旧分片，避免残留（如之前存了 3 片、现在只需 2 片）
        self.clear_shards()?;

        let parts = chunk_payload(&payload, PART_CHAR_LIMIT);
        if parts.len() <= 1 {
            return self.entry(&self.account)?.set_password(&payload).map_err(|err| format!("保存登录态失败: {err}"));
        }

        // 内容分片先写，头最后写：读到头才算有效数据（部分写入 → 视为损坏重登）
        for (i, part) in parts.iter().enumerate() {
            self.entry(&self.part_account(i + 1))?
                .set_password(part)
                .map_err(|err| format!("保存登录态失败（分片 {}）: {err}", i + 1))?;
        }
        let header = serde_json::to_string(&ShardHeader {
            v: HEADER_VERSION,
            parts: parts.len(),
        })
        .map_err(|err| format!("登录态序列化失败: {err}"))?;
        self.entry(&self.account)?
            .set_password(&header)
            .map_err(|err| format!("保存登录态失败: {err}"))
    }

    /// 读取登录态。None = 从未存储过；
    /// 坏 JSON → Err("...登录态已损坏，请重新登录")；非对象 → Err("...格式无效...")。
    pub fn load(&self) -> Result<Option<HashMap<String, String>>, String> {
        let payload = match self.load_payload()? {
            Some(p) => p,
            None => return Ok(None),
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
        self.clear_shards()?;
        match self.entry(&self.account)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(format!("清除登录态失败: {err}")),
        }
    }

    /// 读取完整 payload：头是分片头则拼分片，否则整个条目即 payload（v1 格式）。
    fn load_payload(&self) -> Result<Option<String>, String> {
        let entry = self.entry(&self.account)?;
        let stored = match entry.get_password() {
            Ok(p) => p,
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(err) => return Err(format!("读取登录态失败: {err}")),
        };
        // 尝试按分片头解析；解析失败或非 v2 头 = v1 单条格式
        let header = serde_json::from_str::<ShardHeader>(&stored)
            .ok()
            .filter(|h| h.v == HEADER_VERSION);
        let header = match header {
            Some(h) => h,
            None => return Ok(Some(stored)),
        };
        let mut payload = String::new();
        for i in 1..=header.parts {
            let part = self
                .entry(&self.part_account(i))?
                .get_password()
                .map_err(|_| "系统凭据存储中的登录态已损坏，请重新登录".to_string())?;
            payload.push_str(&part);
        }
        Ok(Some(payload))
    }

    /// 删除全部分片条目。依据当前头里的 parts 数；无头/旧格式则无分片可清。
    fn clear_shards(&self) -> Result<(), String> {
        let parts = match self.entry(&self.account)?.get_password() {
            Ok(stored) => serde_json::from_str::<ShardHeader>(&stored)
                .ok()
                .filter(|h| h.v == HEADER_VERSION)
                .map(|h| h.parts)
                .unwrap_or(0),
            Err(_) => 0,
        };
        for i in 1..=parts {
            match self.entry(&self.part_account(i))?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(err) => return Err(format!("清除登录态分片失败: {err}")),
            }
        }
        Ok(())
    }

    fn part_account(&self, i: usize) -> String {
        format!("{}.p{i}", self.account)
    }

    /// keyring 3 的 Entry 不 Clone，按需创建（开销可忽略）。
    fn entry(&self, account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(&self.service, account)
            .map_err(|err| format!("当前平台登录态存储不可用: {err}"))
    }
}

/// 按字符切分（UTF-16 安全：BMP 内 1 char = 1 unit），末片为余数。
fn chunk_payload(payload: &str, limit: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::with_capacity(limit);
    let mut count = 0;
    for c in payload.chars() {
        current.push(c);
        count += 1;
        if count == limit {
            parts.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_roundtrip() {
        let payload = "x".repeat(4500); // 1200×3=3600 余 900 → 4 片
        let parts = chunk_payload(&payload, PART_CHAR_LIMIT);
        assert_eq!(parts.len(), 4);
        assert_eq!(parts.iter().map(|p| p.chars().count()).sum::<usize>(), 4500);
        assert_eq!(parts.concat(), payload);
    }

    #[test]
    fn chunk_short_is_single() {
        let payload = "{\"PHPSESSID\":\"abc\"}".to_string();
        let parts = chunk_payload(&payload, PART_CHAR_LIMIT);
        assert_eq!(parts, vec![payload]);
    }

    #[test]
    fn chunk_multibyte_safe() {
        // 多字节字符不被切断：每片 chars() 拼回完整
        let payload = "登录态测试".repeat(600);
        let parts = chunk_payload(&payload, PART_CHAR_LIMIT);
        assert_eq!(parts.concat(), payload);
        // 每片 UTF-16 编码后 ≤ 2560 字节（Windows CRED blob 限制）
        assert!(parts.iter().all(|p| p.encode_utf16().count() * 2 <= 2560));
    }

    #[test]
    fn header_roundtrip() {
        let h = ShardHeader { v: HEADER_VERSION, parts: 3 };
        let s = serde_json::to_string(&h).unwrap();
        let back: ShardHeader = serde_json::from_str(&s).unwrap();
        assert_eq!(back.parts, 3);
    }

    /// 真实系统凭据存储往返（超 2560 字符触发分片）。需真机跑：
    /// `cargo test --release -- --ignored store_`
    #[test]
    #[ignore]
    fn store_roundtrip_big() {
        let store = CookieStore::new();
        let mut cookies = HashMap::new();
        cookies.insert("PHPSESSID".to_string(), "p".repeat(4000));
        cookies.insert("device_token".to_string(), "d".repeat(800));
        store.clear().unwrap();
        store.save(&cookies).unwrap();
        let loaded = store.load().unwrap().expect("should load");
        assert_eq!(loaded, cookies);
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }
}
