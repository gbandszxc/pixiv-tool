//! 在线实测公共设施：cookie 获取（三级回退）、PixivApi 构造、断言辅助。
//!
//! 凭据纪律（AGENTS.md 安全边界）：
//! - 只读系统凭据存储：`CookieStore::new().load()` 读真实 `default` 条目；
//!   **绝不对其调用 save()/clear()**（会毁掉用户真实登录态）；
//! - 断言消息与 eprintln! 不打印 cookie / csrf token 值，只打印字段名、
//!   长度与条数；pixiv CDN URL 不含凭据，可原样出现在消息里。
//!
//! 进程内共用同一个 `PixivApi`（OnceLock）：每个客户端实例自带并发 2 /
//! 请求间隔 400ms / 超时 15s / 重试 3 / 429 暂停 60s 的限速语义，串行运行时
//! 不会刷接口；`get_user_self` 的 uid 也缓存，避免重复探测。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde_json::Value;

use pixiv_tool_lib::cookies::CookieStore;
use pixiv_tool_lib::pixiv::api::PixivApi;
use pixiv_tool_lib::pixiv::client::PixivClient;

/// 在线用例共用的 PixivApi（真实登录态，懒加载 + 进程内缓存）。
///
/// 取不到登录态时 panic，消息给出可执行的两条路径（应用登录 / 环境变量）。
pub fn live_api() -> &'static PixivApi {
    static API: OnceLock<PixivApi> = OnceLock::new();
    API.get_or_init(|| {
        let cookies = load_cookies();
        let client =
            PixivClient::new(&cookies).expect("创建指纹伪装 HTTP 客户端失败（wreq 初始化异常）");
        PixivApi::new(Arc::new(client))
    })
}

/// 当前登录用户 uid（GET /ajax/user/self 的 userData.id，进程内缓存）。
/// 周期内多次调用只探测一次。
pub async fn live_uid() -> i64 {
    static UID: OnceLock<i64> = OnceLock::new();
    if let Some(uid) = UID.get() {
        return *uid;
    }
    let (user_data, token) = live_api()
        .get_user_self()
        .await
        .expect("GET /ajax/user/self 失败：登录态可能已失效");
    let uid = as_i64_loose(&user_data["id"]).unwrap_or(0);
    if uid <= 0 {
        panic!("userData.id 无法解析为正整数（该键存在: {}）", user_data.get("id").is_some());
    }
    if token.is_empty() {
        panic!("登录态缺少 csrf token");
    }
    let _ = UID.set(uid);
    uid
}

// ----------------------------------------------------------------------
// cookie 三级获取
// ----------------------------------------------------------------------

/// ① 环境变量 `PIXIV_TOOL_TEST_COOKIES`（JSON 对象或 `k=v; k2=v2`）
/// ② 应用 `default` 条目（当前激活账号镜像）
/// ③ 仓库 `config/accounts.json` 的 active → `u-<id>` 条目
fn load_cookies() -> HashMap<String, String> {
    if let Ok(raw) = std::env::var("PIXIV_TOOL_TEST_COOKIES") {
        match parse_env_cookies(&raw) {
            Some(map) if has_session(&map) => return map,
            Some(_) => panic!("PIXIV_TOOL_TEST_COOKIES 缺少 PHPSESSID，拒绝用它构造客户端"),
            None => panic!(
                "PIXIV_TOOL_TEST_COOKIES 无法解析（应为 JSON 对象 {{\"PHPSESSID\":\"...\"}} 或 \"k=v; k2=v2\"）"
            ),
        }
    }

    match CookieStore::new().load() {
        Ok(Some(map)) if has_session(&map) => return map,
        Ok(Some(_)) => eprintln!("default 登录态缺少 PHPSESSID，继续尝试账号条目"),
        Ok(None) => {}
        Err(err) => eprintln!("读取 default 登录态失败：{err}（继续尝试账号条目）"),
    }

    if let Some(map) = active_account_cookies() {
        return map;
    }

    panic!(
        "取不到可用登录态。先在本机用应用登录一次，或设置 PIXIV_TOOL_TEST_COOKIES（JSON 对象或 \"k=v; k2=v2\"）后重跑 ./dev.ps1 test-live"
    );
}

/// 解析环境变量形态：JSON 对象（值可为字符串/数字）或 Cookie 头式 `k=v; k2=v2`。
fn parse_env_cookies(raw: &str) -> Option<HashMap<String, String>> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    let map: HashMap<String, String> = if text.starts_with('{') {
        let value: Value = serde_json::from_str(text).ok()?;
        let obj = value.as_object()?;
        obj.iter()
            .map(|(k, v)| {
                let val = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                (k.clone(), val)
            })
            .collect()
    } else {
        text.split(';')
            .filter_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                let (k, v) = (k.trim(), v.trim());
                (!k.is_empty() && !v.is_empty()).then(|| (k.to_string(), v.to_string()))
            })
            .collect()
    };
    (!map.is_empty()).then_some(map)
}

/// 仓库根 `config/accounts.json` 的 active 账号凭据（只读 `u-<id>` 条目）。
fn active_account_cookies() -> Option<HashMap<String, String>> {
    let path = repo_root().join("config").join("accounts.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let active = value
        .get("active")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?;
    // with_account 的 load 为只读；绝不 save/clear
    let loaded = CookieStore::with_account(&format!("u-{active}"))
        .load()
        .ok()
        .flatten()?;
    has_session(&loaded).then_some(loaded)
}

fn has_session(cookies: &HashMap<String, String>) -> bool {
    cookies.get("PHPSESSID").is_some_and(|v| !v.is_empty())
}

/// 仓库根（`src-tauri/` 的上一级；测试进程 CWD 在 src-tauri 下，不依赖 CWD）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

// ----------------------------------------------------------------------
// 轻量断言辅助（live_* 用例共用）
// ----------------------------------------------------------------------

/// 宽松 i64（pixiv 的 id/计数在字符串与数字间漂移）。
pub fn as_i64_loose(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// 条目 id（解析失败返回 0，由断言报错）。
pub fn id_of(item: &Value) -> i64 {
    as_i64_loose(&item["id"]).unwrap_or(0)
}

/// 条目作者 id。
pub fn author_of(item: &Value) -> i64 {
    as_i64_loose(&item["author_id"]).unwrap_or(0)
}

/// 契约列表信封（BrowseList 等）：取出 items 数组，缺失即失败。
pub fn assert_list_envelope<'a>(value: &'a Value, what: &str) -> &'a [Value] {
    value
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_else(|| panic!("{what} 缺少 items 数组，实际键: {:?}", keys(value)))
}

/// 对象顶层键列表（仅用于失败诊断；值为空对象/非对象时给空表）。
pub fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

/// pixiv CDN 绝对 URL（i.pximg.net 图片/zip、embed.pixiv.net 嵌入图、s.pximg.net 静态图）。
pub fn assert_pixiv_url(url: &str, field: &str) {
    assert!(
        url.starts_with("https://i.pximg.net/")
            || url.starts_with("https://embed.pixiv.net/")
            || url.starts_with("https://s.pximg.net/"),
        "{field} 应为 pixiv CDN 绝对 URL，实际: {url}"
    );
}

/// 榜单日期等 yyyymmdd 字段。
pub fn assert_yyyymmdd(value: &Value, field: &str) {
    let text = value
        .as_str()
        .unwrap_or_else(|| panic!("{field} 应为字符串，实际: {value}"));
    assert!(
        text.len() == 8 && text.bytes().all(|b| b.is_ascii_digit()),
        "{field} 应为 yyyymmdd，实际: {text}"
    );
}

/// 作品条目最低语义：id 正整数、标题非空、kind 在白名单、作者 id 为正。
pub fn assert_work_item(item: &Value, what: &str, expect_kind: Option<&str>) {
    assert!(id_of(item) > 0, "{what}.id 应为正整数，实际: {}", item["id"]);
    assert!(
        item["title"].as_str().is_some_and(|s| !s.is_empty()),
        "{what}.title 不应为空，实际: {}",
        item["title"]
    );
    let kind = item["kind"].as_str().unwrap_or("");
    assert!(
        matches!(kind, "illust" | "manga" | "ugoira" | "novel"),
        "{what}.kind 非法: {kind}"
    );
    if let Some(expected) = expect_kind {
        assert_eq!(kind, expected, "{what}.kind");
    }
    assert!(
        author_of(item) > 0,
        "{what}.author_id 应为正整数，实际: {}",
        item["author_id"]
    );
}
