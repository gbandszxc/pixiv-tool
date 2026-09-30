//! 多账号登录态管理。
//!
//! 数据分两层：
//! - 索引：`config/accounts.json`，**只存用户元信息**（user_id / pixiv_id /
//!   name / 头像缓存文件名等，绝不含 cookie——安全边界见 AGENTS.md）；
//! - 凭据：每账号一个系统凭据条目（service `pixiv-tool.cookies`、account
//!   `u-<user_id>`），复用 CookieStore 的分片机制。
//!
//! 兼容语义：keyring 的 `default` 条目**恒为当前激活账号的镜像**，
//! auth_status / 抓取客户端 / webview 自动注入等既有读取方零感知；
//! 显式登录/同步登录同时更新账号条目，切换时只需把目标条目写入 default。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::cookies::CookieStore;

/// 索引文件名（config_dir 下）。
pub const ACCOUNTS_FILE_NAME: &str = "accounts.json";
/// 每账号凭据条目名前缀：`u-<user_id>`。
pub const ENTRY_PREFIX: &str = "u-";

/// 账号元信息（不含任何凭据）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountInfo {
    pub user_id: String,
    #[serde(default)]
    pub pixiv_id: String,
    #[serde(default)]
    pub name: String,
    /// pixiv 头像原始 URL（i.pximg.net，有防盗链；展示走 avatar_file 缓存）。
    #[serde(default)]
    pub profile_img: String,
    /// 本地头像缓存文件名（data/cache 下）；空 = 未缓存（前端回退首字母）。
    #[serde(default)]
    pub avatar_file: String,
    /// 最近一次登记时间（unix 秒；仅展示用途）。
    #[serde(default)]
    pub saved_at: i64,
}

impl AccountInfo {
    /// 从统一用户信息（csrf::UserInfo 序列化形状，snake_case）构造。
    /// avatar_file 由命令层在头像缓存就绪后补填。
    pub fn from_user_value(user: &serde_json::Value) -> Option<Self> {
        let obj = user.as_object()?;
        let str_field = |key: &str| -> String {
            obj.get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let user_id = str_field("user_id");
        if user_id.is_empty() {
            return None;
        }
        Some(Self {
            user_id,
            pixiv_id: str_field("pixiv_id"),
            name: str_field("name"),
            profile_img: str_field("profile_img"),
            avatar_file: String::new(),
            saved_at: now_unix(),
        })
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 索引文件结构（active 指向当前激活账号的 user_id）。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct AccountsIndex {
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    accounts: Vec<AccountInfo>,
}

/// keyring 条目名（`u-<user_id>`）。user_id 只允许字母数字与 `-_`，
/// 防御异常数据把条目名带出 service 命名空间（pixiv id 实为纯数字）。
fn entry_name(user_id: &str) -> Option<String> {
    let id = user_id.trim();
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
    valid.then(|| format!("{ENTRY_PREFIX}{id}"))
}

/// 多账号管理器：索引文件读写 + 每账号凭据存取。
///
/// CookieStore 实例按 user_id 缓存（其内部自带 load 内存缓存），保证每个
/// keyring 条目每进程至多真实读取一次——macOS Keychain 首读会弹授权框，
/// 实例级缓存把弹框次数压到每账号一次。
pub struct AccountManager {
    config_dir: PathBuf,
    stores: Mutex<HashMap<String, CookieStore>>,
}

impl AccountManager {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            config_dir: config_dir.to_path_buf(),
            stores: Mutex::new(HashMap::new()),
        }
    }

    fn index_path(&self) -> PathBuf {
        self.config_dir.join(ACCOUNTS_FILE_NAME)
    }

    /// 读索引：文件不存在 / 读失败 / JSON 损坏 → 备份损坏文件后返回默认
    /// （凭据本体在 keyring，索引损坏只丢列表不丢登录态，重新登录即恢复）。
    pub(crate) fn load_index(&self) -> AccountsIndex {
        let path = self.index_path();
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return AccountsIndex::default();
            }
            Err(err) => {
                log::warn!("读取账号索引失败（按空处理）: {err}");
                return AccountsIndex::default();
            }
        };
        match serde_json::from_str(&raw) {
            Ok(idx) => idx,
            Err(err) => {
                log::warn!("账号索引损坏（{err}），移走备份后按空处理");
                // rename 而非 copy：原文件移走，下次读取按不存在处理，
                // 不会对同一份坏文件反复产生备份
                let backup = path.with_file_name(format!(
                    "{ACCOUNTS_FILE_NAME}.corrupt-{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0)
                ));
                let _ = std::fs::rename(&path, &backup);
                AccountsIndex::default()
            }
        }
    }

    fn save_index(&self, idx: &AccountsIndex) -> Result<(), String> {
        let json = serde_json::to_string_pretty(idx)
            .map_err(|err| format!("账号索引序列化失败: {err}"))?;
        std::fs::create_dir_all(&self.config_dir)
            .map_err(|err| format!("创建配置目录失败: {err}"))?;
        std::fs::write(self.index_path(), format!("{json}\n"))
            .map_err(|err| format!("写入账号索引失败: {err}"))
    }

    /// 当前激活账号（索引 active）。
    pub fn active(&self) -> Option<String> {
        self.load_index().active.filter(|id| !id.is_empty())
    }

    /// 已保存账号列表（登记顺序）。
    pub fn list(&self) -> Vec<AccountInfo> {
        self.load_index().accounts
    }

    /// 账号的 CookieStore（实例缓存；条目名非法 → None）。
    pub fn entry_store(&self, user_id: &str) -> Option<CookieStore> {
        let name = entry_name(user_id)?;
        if let Ok(map) = self.stores.lock() {
            if let Some(store) = map.get(&name) {
                return Some(store.clone());
            }
        }
        let store = CookieStore::with_account(&name);
        if let Ok(mut map) = self.stores.lock() {
            map.insert(name, store.clone());
        }
        Some(store)
    }

    /// 覆盖保存账号的独立凭据（登记 / 切出时归档当前镜像用）。
    pub fn store_cookies(
        &self,
        user_id: &str,
        cookies: &HashMap<String, String>,
    ) -> Result<(), String> {
        self.entry_store(user_id)
            .ok_or_else(|| "账号 ID 非法，无法保存登录态".to_string())?
            .save(cookies)
    }

    /// 读取账号的独立凭据（从未存过 → None；损坏向上抛错误文案）。
    pub fn load_cookies(&self, user_id: &str) -> Result<Option<HashMap<String, String>>, String> {
        self.entry_store(user_id)
            .ok_or_else(|| "账号 ID 非法，无法读取登录态".to_string())?
            .load()
    }

    /// 登记账号：覆盖凭据 + 索引 upsert（按 user_id 去重，保持登记顺序）
    /// + 设为激活。仅显式登录/同步登录调用；状态校验只刷新元信息。
    pub fn enroll(
        &self,
        info: AccountInfo,
        cookies: &HashMap<String, String>,
    ) -> Result<(), String> {
        if entry_name(&info.user_id).is_none() {
            return Err(format!("账号 ID 非法: {}", info.user_id));
        }
        self.store_cookies(&info.user_id, cookies)?;
        self.upsert(info)
    }

    /// 只刷新账号元信息与 active，不访问系统凭据存储。
    pub fn upsert(&self, info: AccountInfo) -> Result<(), String> {
        if entry_name(&info.user_id).is_none() {
            return Err(format!("账号 ID 非法: {}", info.user_id));
        }
        let user_id = info.user_id.clone();
        let mut idx = self.load_index();
        match idx.accounts.iter_mut().find(|a| a.user_id == user_id) {
            // 已登记：元信息就地刷新（头像缓存文件名空时不覆盖已有值）
            Some(existing) => {
                let avatar_file = if info.avatar_file.is_empty() {
                    existing.avatar_file.clone()
                } else {
                    info.avatar_file.clone()
                };
                *existing = AccountInfo {
                    avatar_file,
                    ..info
                };
            }
            None => idx.accounts.push(info),
        }
        idx.active = Some(user_id);
        self.save_index(&idx)
    }

    /// 仅更新激活指针（切换账号用；目标不在列表时返回错误）。
    pub fn set_active(&self, user_id: &str) -> Result<(), String> {
        let mut idx = self.load_index();
        if !idx.accounts.iter().any(|a| a.user_id == user_id) {
            return Err(format!("账号未登记: {user_id}"));
        }
        idx.active = Some(user_id.to_string());
        self.save_index(&idx)
    }

    /// 移除账号：删除其独立凭据条目 + 索引移除（active 指向它则一并清空）。
    /// 凭据条目本就不存在视为已清（对齐 CookieStore::clear 语义）。
    pub fn remove(&self, user_id: &str) -> Result<(), String> {
        if let Some(store) = self.entry_store(user_id) {
            store.clear()?;
        }
        let mut idx = self.load_index();
        idx.accounts.retain(|a| a.user_id != user_id);
        if idx.active.as_deref() == Some(user_id) {
            idx.active = None;
        }
        self.save_index(&idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pixiv-tool-accounts-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn info(user_id: &str) -> AccountInfo {
        AccountInfo {
            user_id: user_id.to_string(),
            pixiv_id: format!("pid_{user_id}"),
            name: format!("用户{user_id}"),
            profile_img: format!("https://i.pximg.net/{user_id}.jpg"),
            avatar_file: String::new(),
            saved_at: 0,
        }
    }

    /// 每测试唯一的合法 user_id：keyring 条目是进程级全局命名空间，
    /// 并行测试共用 "1"/"2" 会互踩（条目已存在 → duplicate 错误）。
    fn unique_id(tag: &str) -> String {
        format!("{tag}-{}", uuid::Uuid::new_v4().simple())
    }

    #[test]
    fn entry_name_rules() {
        assert_eq!(entry_name("28640").as_deref(), Some("u-28640"));
        assert_eq!(entry_name(" a-b_1 ").as_deref(), Some("u-a-b_1"));
        // 非法字符 / 空 / 超长拒绝
        assert_eq!(entry_name(""), None);
        assert_eq!(entry_name("../evil"), None);
        assert_eq!(entry_name("id with space"), None);
        assert_eq!(entry_name(&"x".repeat(65)), None);
    }

    #[test]
    fn from_user_value_reads_snake_case() {
        let user = serde_json::json!({
            "user_id": "28640",
            "pixiv_id": "pid",
            "name": "名",
            "profile_img": "https://i.pximg.net/a.jpg"
        });
        let parsed = AccountInfo::from_user_value(&user).unwrap();
        assert_eq!(parsed.user_id, "28640");
        assert_eq!(parsed.pixiv_id, "pid");
        assert_eq!(parsed.name, "名");
        assert_eq!(parsed.avatar_file, "");
        // 缺 user_id / 非对象 → None
        assert!(AccountInfo::from_user_value(&serde_json::json!({ "name": "x" })).is_none());
        assert!(AccountInfo::from_user_value(&serde_json::json!("x")).is_none());
    }

    #[test]
    fn enroll_upsert_keeps_order_and_sets_active() {
        let dir = temp_config_dir("enroll");
        let mgr = AccountManager::new(&dir);
        let cookies = HashMap::from([("PHPSESSID".to_string(), "s1".to_string())]);
        let id1 = unique_id("a");
        let id2 = unique_id("b");

        mgr.enroll(info(&id1), &cookies).unwrap();
        mgr.enroll(info(&id2), &cookies).unwrap();
        // 同 id 重复登记：就地刷新且保持位置，并补头像缓存名
        let mut updated = info(&id1);
        updated.avatar_file = "a_50.jpg".to_string();
        mgr.enroll(updated, &cookies).unwrap();

        let list = mgr.list();
        assert_eq!(
            list.iter().map(|a| a.user_id.as_str()).collect::<Vec<_>>(),
            [id1.as_str(), id2.as_str()]
        );
        assert_eq!(list[0].avatar_file, "a_50.jpg");
        assert_eq!(mgr.active().as_deref(), Some(id1.as_str()));

        // 显式切换激活
        mgr.set_active(&id2).unwrap();
        assert_eq!(mgr.active().as_deref(), Some(id2.as_str()));
        // 未登记账号不能设激活
        assert!(mgr.set_active("404").is_err());
    }

    #[test]
    fn enroll_rejects_invalid_user_id() {
        let dir = temp_config_dir("enroll-invalid");
        let mgr = AccountManager::new(&dir);
        let cookies = HashMap::from([("PHPSESSID".to_string(), "s".to_string())]);
        assert!(mgr.enroll(info("../evil"), &cookies).is_err());
        assert!(mgr.list().is_empty());
    }

    #[test]
    fn remove_drops_entry_and_clears_active() {
        let dir = temp_config_dir("remove");
        let mgr = AccountManager::new(&dir);
        let cookies = HashMap::from([("PHPSESSID".to_string(), "s".to_string())]);
        let id1 = unique_id("a");
        let id2 = unique_id("b");
        mgr.enroll(info(&id1), &cookies).unwrap();
        mgr.enroll(info(&id2), &cookies).unwrap();
        mgr.set_active(&id2).unwrap();

        mgr.remove(&id2).unwrap();
        assert!(mgr.list().iter().all(|a| a.user_id != id2));
        assert_eq!(mgr.active(), None);
        // 凭据条目已删：读取为 None
        assert_eq!(mgr.load_cookies(&id2).unwrap(), None);
        // 其余账号不受影响
        assert_eq!(mgr.list().len(), 1);
        assert_eq!(mgr.list()[0].user_id, id1);
    }

    #[test]
    fn corrupt_index_backed_up_and_reset() {
        let dir = temp_config_dir("corrupt");
        let mgr = AccountManager::new(&dir);
        std::fs::write(mgr.index_path(), "{ not json !!!").unwrap();
        assert_eq!(mgr.list(), Vec::new());
        assert_eq!(mgr.active(), None);
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(backups.len(), 1, "应恰好有一个损坏备份");
    }

    #[test]
    fn missing_index_is_empty() {
        let dir = temp_config_dir("missing");
        let mgr = AccountManager::new(&dir);
        assert_eq!(mgr.list(), Vec::new());
        assert_eq!(mgr.active(), None);
    }

    /// 真实系统凭据存储往返（分账号条目）。需真机跑：
    /// `cargo test --release -- --ignored accounts_`
    #[test]
    #[ignore]
    fn accounts_cookies_roundtrip() {
        let dir = temp_config_dir("roundtrip");
        let mgr = AccountManager::new(&dir);
        let cookies = HashMap::from([("PHPSESSID".to_string(), "x".repeat(4000))]);
        mgr.enroll(info("100"), &cookies).unwrap();
        let loaded = mgr.load_cookies("100").unwrap().expect("should load");
        assert_eq!(loaded, cookies);
        mgr.remove("100").unwrap();
        assert_eq!(mgr.load_cookies("100").unwrap(), None);
    }
}
