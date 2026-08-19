//! Pixiv Session 校验与 CSRF token 获取。
//!
//! **桩（phase B 实现）**：本文件只定稿公共契约与错误语义，函数体待填充。
#![allow(unused)]

use serde::Serialize;

pub const PIXIV_SELF_URL: &str = "https://www.pixiv.net/ajax/user/self?lang=zh";

/// 登录用户信息（字段与旧 Python 版一致，全字符串）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct UserInfo {
    pub user_id: String,
    pub pixiv_id: String,
    pub name: String,
    pub profile_img: String,
}

/// 登录态探测结果。
#[derive(Debug)]
pub struct SessionProbe {
    pub csrf_token: String,
    pub is_logged_in: bool,
    pub user: Option<UserInfo>,
}

/// 探测错误：
/// - `Invalid`：PHPSESSID 无效或已过期（401/403/重定向/无 userData.id）
/// - `Csrf`：登录态有效，但响应异常（非 JSON / 缺 csrf token）
#[derive(thiserror::Error, Debug)]
pub enum ProbeError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Csrf(String),
}

/// 兼容纯值、`PHPSESSID=值` 和 Cookie 请求头格式。
///
/// - strip 后长度 > 4096 → Err("PHPSESSID 格式不正确")
/// - 含 "PHPSESSID=" → 按 cookie 对解析取值（解析失败 → Err("PHPSESSID 格式不正确")）
/// - 空值 → Err("PHPSESSID 不能为空")
/// - 含 `\r` `\n` `;` `\0` → Err("PHPSESSID 格式不正确")
pub fn normalize_phpsessid(value: &str) -> Result<String, String> {
    todo!("phase B")
}

/// 用 /ajax/user/self 一次完成 Session、token 与用户信息校验。
///
/// 错误语义（phase B 实现，文案精确）：
/// - 401/403/重定向/无 userData.id → Invalid("PHPSESSID 无效或已过期")
/// - 非 JSON → Csrf("Pixiv 登录态响应不是有效 JSON")
/// - 缺 token → Csrf("登录态有效，但 Pixiv 响应缺少 csrf token")
///
/// 注意：Chrome 实测匿名 /ajax/user/self 同样返回 HTTP 200 和 token，
/// 只有 userData.id 能证明 PHPSESSID 真正有效。
/// HTTP 探测请求应复用 PixivClient 的指纹伪装（裸 httpx 式 UA 是强风控信号）。
pub async fn fetch_session_probe(phpsessid: &str) -> Result<SessionProbe, ProbeError> {
    todo!("phase B")
}
