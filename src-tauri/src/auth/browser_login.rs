//! 用真实 Chromium 浏览器完成 Pixiv 登录并通过 CDP 读取 Cookie。
//!
//! 语义对齐 Python `auth/browser_login.py`（V1 无 pywebview 回退登录窗：
//! 找不到浏览器直接返回 error 终态，文案引导用户改用手动 Cookie 登录）。
//!
//! **Cookie 值不得写入日志**（日志只出现端口 / 状态 / 计数类信息）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_json::Value;
use tokio::process::Child;

use crate::auth::cdp::CdpClient;
use crate::pixiv::csrf::{ProbeError, fetch_session_probe};

pub const LOGIN_URL: &str = "https://accounts.pixiv.net/login";
pub const LOGIN_TIMEOUT_SEC: u64 = 300;
/// 每轮探测间隔。
pub const POLL_INTERVAL_MILLIS: u64 = 500;
/// CDP 调试端口就绪探测：50 次 × 间隔 0.1s，单次 HTTP 请求 0.3s 超时。
const CDP_POLL_ROUNDS: u32 = 50;
const CDP_POLL_INTERVAL_MILLIS: u64 = 100;
const CDP_POLL_TIMEOUT_MILLIS: u64 = 300;
/// 退出清理：Browser.close 后最多等 3s，未退则 kill。
const SHUTDOWN_GRACE_SEC: u64 = 3;

/// 系统没有可通过 CDP 控制的 Chrome / Edge / Chromium。
#[derive(thiserror::Error, Debug)]
#[error("未找到 Chrome、Edge 或 Chromium")]
pub struct BrowserNotFoundError;

/// 浏览器登录结果（status 与旧 Python 版一致）。
#[derive(Debug, Clone, Serialize)]
pub struct LoginResult {
    /// "success" | "cancelled" | "timeout" | "error"
    pub status: String,
    /// 仅 success：完整 pixiv.net cookie（含 PHPSESSID 与补入的 x-csrf-token）。
    pub cookies: Option<HashMap<String, String>>,
    /// 仅 success：探测到的用户信息。
    pub user: Option<crate::pixiv::csrf::UserInfo>,
    /// timeout/error 时的说明。
    pub message: Option<String>,
}

impl LoginResult {
    /// 构造非 success 终态（cancelled / timeout / error），供 webview_login 复用。
    pub(crate) fn terminal(status: &str, message: Option<String>) -> Self {
        Self {
            status: status.to_string(),
            cookies: None,
            user: None,
            message,
        }
    }
}

/// 返回首个已安装的 Chromium 浏览器可执行文件：
/// - macOS: /Applications 下 Google Chrome / Microsoft Edge / Chromium（按序）
/// - Windows: PROGRAMFILES / PROGRAMFILES(X86) / LOCALAPPDATA 下
///   Google/Chrome/Application/chrome.exe、Microsoft/Edge/Application/msedge.exe
/// - Linux: which google-chrome | microsoft-edge | chromium | chromium-browser
pub fn find_login_browser() -> Result<PathBuf, BrowserNotFoundError> {
    let candidates = browser_candidates();
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or(BrowserNotFoundError)
}

/// 分平台候选列表（顺序即优先级，对齐 Python `find_login_browser`）。
fn browser_candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
        .iter()
        .map(PathBuf::from)
        .collect()
    }
    #[cfg(target_os = "windows")]
    {
        let mut candidates = Vec::new();
        let roots = [
            std::env::var_os("PROGRAMFILES"),
            std::env::var_os("PROGRAMFILES(X86)"),
            std::env::var_os("LOCALAPPDATA"),
        ];
        for root in roots.iter().flatten() {
            let root = PathBuf::from(root);
            candidates.push(root.join(r"Google\Chrome\Application\chrome.exe"));
            candidates.push(root.join(r"Microsoft\Edge\Application\msedge.exe"));
        }
        candidates
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "windows")))]
    {
        [
            "google-chrome",
            "microsoft-edge",
            "chromium",
            "chromium-browser",
        ]
        .iter()
        .filter_map(|name| which(name))
        .collect()
    }
}

/// 极简 which：扫 PATH 找可执行文件（等价 Python `shutil.which` 的常用路径）。
#[cfg(all(unix, not(target_os = "macos"), not(target_os = "windows")))]
fn which(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// 从 CDP Storage.getCookies 结果（Value 数组）提取 Pixiv 域 Cookie：
/// domain 去掉前导 '.' 后以 "pixiv.net" 结尾，且 name/value 非空。
pub fn extract_pixiv_cookies(items: &[Value]) -> HashMap<String, String> {
    items
        .iter()
        .filter_map(|item| {
            let domain = item.get("domain").and_then(Value::as_str).unwrap_or("");
            let name = item.get("name").and_then(Value::as_str).unwrap_or("");
            let value = item.get("value").and_then(Value::as_str).unwrap_or("");
            // 与 Python 一致：剥前导 '.' 后大小写敏感地 endswith("pixiv.net")
            let is_pixiv = domain.trim_start_matches('.').ends_with("pixiv.net");
            (is_pixiv && !name.is_empty() && !value.is_empty())
                .then(|| (name.to_string(), value.to_string()))
        })
        .collect()
}

/// 登录页跳回 Pixiv 主站后才探测 Session（url host ∈ {pixiv.net, www.pixiv.net}，
/// accounts.pixiv.net 不算；type=="page"），避免验证码阶段制造额外请求。
pub fn has_pixiv_main_target(items: &[Value]) -> bool {
    items.iter().any(|item| {
        if item.get("type").and_then(Value::as_str) != Some("page") {
            return false;
        }
        let url = item.get("url").and_then(Value::as_str).unwrap_or("");
        matches!(
            url_host(url).as_deref(),
            Some("pixiv.net" | "www.pixiv.net")
        )
    })
}

/// URL → 小写主机名（剥 scheme / userinfo / 端口），等价 Python `urlparse().hostname`。
/// 供 webview_login 轮询窗口 URL 时复用。
pub fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?;
    // IPv6 字面量带方括号，端口在 "]" 之后
    let host = if let Some(stripped) = host.strip_prefix('[') {
        stripped.split(']').next()?
    } else {
        host.split(':').next()?
    };
    let host = host.to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// 打开隔离的真实浏览器等待登录完成：
///
/// 1. spawn：`<browser> --remote-debugging-port=<随机空闲端口>
///    --remote-debugging-address=127.0.0.1 --user-data-dir=<profile_dir>
///    --no-first-run --no-default-browser-check --app=about:blank`
///    （profile_dir 由调用方传 `config_dir/login-browser-profile`，先建目录；
///    登录页不由启动参数打开，见 ensure_fresh_login_page）
/// 2. CDP 连接后：清 profile 里 pixiv 会话 cookie（PHPSESSID，保留设备态），
///    再由 CDP 打开登录页——顺序保证登录页请求必然未登录，杜绝已登录
///    profile 在 accounts.pixiv.net/login 被 302 回主站导致轮询秒判成功
/// 3. 轮询 `http://127.0.0.1:{port}/json/version` 拿 webSocketDebuggerUrl
/// 4. 主循环至多 LOGIN_TIMEOUT_SEC（每轮 sleep 0.5s）：
///    - 进程退出 → {"status":"cancelled"}
///    - Target.getTargets 有 pixiv 主站 page → Storage.getCookies →
///      extract_pixiv_cookies → 有 PHPSESSID → fetch_session_probe 验证
///      （ProbeError::Invalid 忽略继续轮询；成功补 x-csrf-token 返回 success）
/// 5. 超时 → {"status":"timeout","message":"登录超时（300s）"}
/// 6. finally：CdpClient Browser.close → 进程 terminate → 3s 内不退则 kill
///
/// Cookie 值不得写入日志。
pub async fn open_browser_login(profile_dir: &Path) -> LoginResult {
    // Rust 版无 pywebview 回退登录窗：找不到浏览器即为终态，引导手动 Cookie 登录。
    let browser = match find_login_browser() {
        Ok(browser) => browser,
        Err(err) => {
            log::info!("浏览器登录不可用：{err}");
            return LoginResult::terminal(
                "error",
                Some("未找到 Chrome、Edge 或 Chromium，请使用手动 Cookie 登录".into()),
            );
        }
    };

    match run_browser_login(&browser, profile_dir).await {
        Ok(result) => result,
        Err(err) => {
            // 日志与返回文案都只含原因，不含任何 Cookie 值
            log::warn!("真实浏览器登录失败：{err:#}");
            LoginResult::terminal("error", Some(format!("{err:#}")))
        }
    }
}

/// 主流程（不含 find_login_browser）：任何 Err 都会走 finally 清理后转 error 终态。
async fn run_browser_login(browser: &Path, profile_dir: &Path) -> Result<LoginResult> {
    std::fs::create_dir_all(profile_dir)
        .map_err(|err| anyhow!("创建浏览器 profile 目录失败: {err}"))?;
    let port = free_port().map_err(|err| anyhow!("获取空闲端口失败: {err}"))?;

    let mut child = spawn_browser(browser, profile_dir, port)?;
    let ws_url = match wait_for_cdp(port).await {
        Ok(ws_url) => ws_url,
        Err(err) => {
            shutdown_browser(&mut child).await;
            return Err(err);
        }
    };
    let mut cdp = match CdpClient::connect(&ws_url).await {
        Ok(cdp) => cdp,
        Err(err) => {
            shutdown_browser(&mut child).await;
            return Err(anyhow!("连接 CDP WebSocket 失败: {err}"));
        }
    };

    // 登录窗语义：必然以未登录态打开（多账号下的加账号）。清旧会话后
    // 才打开登录页；失败仅记日志不阻塞——退化为旧行为（已登录 profile
    // 可能秒关），不应让登录流程整体失败。
    if let Err(err) = ensure_fresh_login_page(&mut cdp).await {
        log::warn!("重置浏览器登录态失败（可能回到已登录账号）: {err:#}");
    }

    let outcome = login_loop(&mut cdp, &mut child).await;
    // finally 语义：先 CDP Browser.close（失败忽略，连接随 drop 关闭），
    // 再等浏览器进程退出（至多 3s），仍未退则 kill。
    let _ = cdp.close().await;
    shutdown_browser(&mut child).await;
    outcome
}

/// spawn 隔离 profile 的浏览器（stdout/stderr 丢弃，避免管道写满死锁）。
/// tokio::process 让 wait 可异步（清理阶段带超时等待）。
/// 以 about:blank 启动：登录页由 CDP 在清掉旧会话后再打开
/// （ensure_fresh_login_page），登录页请求必然未登录。
fn spawn_browser(browser: &Path, profile_dir: &Path, port: u16) -> Result<Child> {
    tokio::process::Command::new(browser)
        .arg(format!("--remote-debugging-port={port}"))
        .arg("--remote-debugging-address=127.0.0.1")
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--app=about:blank")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| anyhow!("启动浏览器进程失败: {err}"))
}

/// 从 Storage.getCookies 结果提取 pixiv 域 PHPSESSID 的精确身份
/// `(name, domain, path)`——过期覆盖删除必须逐字段匹配原条目；
/// 缺/空 path 视为 "/"；非 pixiv 域与非 PHPSESSID 名不含。
pub fn pixiv_session_cookie_keys(items: &[Value]) -> Vec<(String, String, String)> {
    items
        .iter()
        .filter_map(|item| {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("");
            let domain = item.get("domain").and_then(Value::as_str).unwrap_or("");
            let is_pixiv_session = name == "PHPSESSID"
                && !domain.is_empty()
                && domain.trim_start_matches('.').ends_with("pixiv.net");
            is_pixiv_session.then(|| {
                let path = item
                    .get("path")
                    .and_then(Value::as_str)
                    .filter(|p| !p.is_empty())
                    .unwrap_or("/")
                    .to_string();
                (name.to_string(), domain.to_string(), path)
            })
        })
        .collect()
}

/// 清除 profile 浏览器里的 pixiv 会话 cookie（PHPSESSID），保留
/// device_token / cf_clearance 等设备态（ADR 0006 持久化 profile 的意义）。
/// 删除走「同名过期覆盖」（expires=0 的 Set-Cookie 语义），浏览器级 CDP
/// 即可完成，无需 page session。返回清理条数。
async fn clear_pixiv_session(cdp: &mut CdpClient) -> Result<usize> {
    let result = cdp
        .call("Storage.getCookies", serde_json::json!({}))
        .await?;
    let items = result
        .get("cookies")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let keys = pixiv_session_cookie_keys(&items);
    for (name, domain, path) in &keys {
        cdp.call(
            "Storage.setCookies",
            serde_json::json!({ "cookies": [{
                "name": name,
                "value": "",
                "domain": domain,
                "path": path,
                "secure": true,
                "expires": 0,
            }]}),
        )
        .await?;
    }
    Ok(keys.len())
}

/// 保证登录页以未登录态打开：清 pixiv 会话 cookie → CDP 打开登录页 →
/// 关掉 about:blank 启动页。顺序不可换——登录页请求必须发生在清会话
/// 之后，否则已登录 profile 会让 accounts.pixiv.net/login 302 回主站，
/// 轮询循环立即误判成功并秒关登录窗（加号永远拿到旧账号）。
async fn ensure_fresh_login_page(cdp: &mut CdpClient) -> Result<()> {
    let cleared = clear_pixiv_session(cdp).await?;
    if cleared > 0 {
        // 只报条数，不涉及任何 cookie 值
        log::info!("已清除登录 profile 旧会话（PHPSESSID ×{cleared}），设备态 cookie 保留");
    }
    cdp.call(
        "Target.createTarget",
        serde_json::json!({ "url": LOGIN_URL }),
    )
    .await?;
    let targets = cdp.call("Target.getTargets", serde_json::json!({})).await?;
    for info in targets
        .get("targetInfos")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let is_blank_page = info.get("type").and_then(Value::as_str) == Some("page")
            && info.get("url").and_then(Value::as_str) == Some("about:blank");
        if !is_blank_page {
            continue;
        }
        if let Some(id) = info.get("targetId").and_then(Value::as_str) {
            // 失败仅忽略：多留一个空白页不致命
            let _ = cdp
                .call("Target.closeTarget", serde_json::json!({ "targetId": id }))
                .await;
        }
    }
    Ok(())
}

/// CDP 主循环（轮询目标与 Cookie；连接由调用方建立并在 finally 里关闭）。
async fn login_loop(cdp: &mut CdpClient, child: &mut Child) -> Result<LoginResult> {
    let deadline = Instant::now() + Duration::from_secs(LOGIN_TIMEOUT_SEC);
    loop {
        if Instant::now() >= deadline {
            return Ok(LoginResult::terminal(
                "timeout",
                Some(format!("登录超时（{LOGIN_TIMEOUT_SEC}s）")),
            ));
        }
        // 浏览器被用户直接关闭 → cancelled
        match child.try_wait() {
            Ok(Some(_)) => return Ok(LoginResult::terminal("cancelled", None)),
            Ok(None) => {}
            Err(err) => return Err(anyhow!("检查浏览器进程状态失败: {err}")),
        }

        // 还停在 accounts.pixiv.net（登录页 / 验证码）时不发探测请求
        let targets = cdp.call("Target.getTargets", serde_json::json!({})).await?;
        let target_infos = targets
            .get("targetInfos")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !has_pixiv_main_target(&target_infos) {
            tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            continue;
        }

        let result = cdp
            .call("Storage.getCookies", serde_json::json!({}))
            .await?;
        let cookie_items = result
            .get("cookies")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut cookies = extract_pixiv_cookies(&cookie_items);

        if let Some(phpsessid) = cookies
            .get("PHPSESSID")
            .filter(|value| !value.is_empty())
            .cloned()
        {
            match fetch_session_probe(&phpsessid).await {
                Ok(probe) => {
                    cookies.insert("x-csrf-token".to_string(), probe.csrf_token);
                    return Ok(LoginResult {
                        status: "success".into(),
                        cookies: Some(cookies),
                        user: probe.user,
                        message: None,
                    });
                }
                // Session 尚未生效（还在登录跳转中）：静默进入下一轮
                Err(ProbeError::Invalid(_)) => {}
                Err(ProbeError::Csrf(msg)) => {
                    log::warn!("登录态有效但获取 csrf token 异常，继续轮询：{msg}");
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
    }
}

/// 轮询 `http://127.0.0.1:{port}/json/version` 取 webSocketDebuggerUrl。
/// 客户端显式 `.no_proxy()`：本机调试端口不得走系统代理（对齐 trust_env=False）。
async fn wait_for_cdp(port: u16) -> Result<String> {
    let url = format!("http://127.0.0.1:{port}/json/version");
    let client = wreq::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(CDP_POLL_TIMEOUT_MILLIS))
        .build()
        .map_err(|err| anyhow!("创建 HTTP 客户端失败: {err}"))?;
    for _ in 0..CDP_POLL_ROUNDS {
        if let Ok(response) = client.get(&url).send().await {
            if let Ok(text) = response.text().await {
                if let Ok(version) = serde_json::from_str::<Value>(&text) {
                    if let Some(ws_url) = version
                        .get("webSocketDebuggerUrl")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        return Ok(ws_url.to_string());
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(CDP_POLL_INTERVAL_MILLIS)).await;
    }
    Err(anyhow!("浏览器调试端口启动失败"))
}

/// finally 清理：Browser.close 已让浏览器自行退出；再等 3s，仍未退则 kill。
/// （std 无跨平台 SIGTERM，优雅路径依赖 CDP Browser.close，兜底直接 kill。）
async fn shutdown_browser(child: &mut Child) {
    let already_exited = child
        .try_wait()
        .map(|status| status.is_some())
        .unwrap_or(true);
    if !already_exited
        && tokio::time::timeout(Duration::from_secs(SHUTDOWN_GRACE_SEC), child.wait())
            .await
            .is_err()
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}

/// 随机空闲端口（bind 127.0.0.1:0 后立刻释放，存在微小竞态——与 Python 版一致）。
fn free_port() -> std::io::Result<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0))?
        .local_addr()
        .map(|addr| addr.port())
}

// ----------------------------------------------------------------------
// 单元测试（纯函数，内嵌样例 JSON，全部离线）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_filters_by_domain_and_empty_values() {
        let items = vec![
            json!({"name": "PHPSESSID", "value": "12345_abc", "domain": ".pixiv.net"}),
            json!({"name": "device_token", "value": "abc", "domain": ".www.pixiv.net"}),
            // 前导多点也剥
            json!({"name": "multi_dot", "value": "v", "domain": "..pixiv.net"}),
            // 非 pixiv 域
            json!({"name": "evil", "value": "v", "domain": "pixiv.net.evil.com"}),
            json!({"name": "google", "value": "v", "domain": ".google.com"}),
            // 空值 / 空 name 过滤
            json!({"name": "empty_value", "value": "", "domain": ".pixiv.net"}),
            json!({"name": "", "value": "v", "domain": ".pixiv.net"}),
            // 缺字段容错
            json!({"name": "no_domain", "value": "v"}),
            json!({"domain": ".pixiv.net"}),
        ];
        let cookies = extract_pixiv_cookies(&items);
        assert_eq!(cookies.len(), 3);
        assert_eq!(
            cookies.get("PHPSESSID").map(String::as_str),
            Some("12345_abc")
        );
        assert_eq!(cookies.get("device_token").map(String::as_str), Some("abc"));
        assert_eq!(cookies.get("multi_dot").map(String::as_str), Some("v"));
        assert!(!cookies.contains_key("evil"));
        assert!(!cookies.contains_key("google"));
        assert!(!cookies.contains_key("empty_value"));
    }

    #[test]
    fn extract_empty_input() {
        assert!(extract_pixiv_cookies(&[]).is_empty());
        assert!(extract_pixiv_cookies(&[json!({})]).is_empty());
    }

    #[test]
    fn main_target_accepts_pixiv_hosts_only() {
        let items = vec![
            json!({"type": "page", "url": "https://www.pixiv.net/"}),
            json!({"type": "page", "url": "https://pixiv.net/artworks"}),
        ];
        assert!(has_pixiv_main_target(&items));
        // 带端口 / 大写主机同样命中
        assert!(has_pixiv_main_target(&[json!({
            "type": "page", "url": "https://www.pixiv.net:443/"
        })]));
        assert!(has_pixiv_main_target(&[json!({
            "type": "page", "url": "HTTPS://WWW.PIXIV.NET/"
        })]));
    }

    #[test]
    fn main_target_rejects_accounts_and_non_page() {
        // 登录页（accounts 子域）不算主站
        assert!(!has_pixiv_main_target(&[json!({
            "type": "page", "url": "https://accounts.pixiv.net/login"
        })]));
        // 伪装域
        assert!(!has_pixiv_main_target(&[json!({
            "type": "page", "url": "https://pixiv.net.evil.com/"
        })]));
        // 非 page 类型（iframe / worker 等）拒绝
        assert!(!has_pixiv_main_target(&[json!({
            "type": "iframe", "url": "https://www.pixiv.net/"
        })]));
        assert!(!has_pixiv_main_target(&[json!({
            "type": "background_page", "url": "chrome-extension://x"
        })]));
        // 空列表 / 缺 url
        assert!(!has_pixiv_main_target(&[]));
        assert!(!has_pixiv_main_target(&[json!({"type": "page"})]));
    }

    #[test]
    fn url_host_variants() {
        assert_eq!(
            url_host("https://www.pixiv.net/").as_deref(),
            Some("www.pixiv.net")
        );
        assert_eq!(url_host("https://pixiv.net").as_deref(), Some("pixiv.net"));
        assert_eq!(
            url_host("https://user:pass@www.pixiv.net:8443/a?b=1").as_deref(),
            Some("www.pixiv.net")
        );
        assert_eq!(
            url_host("http://127.0.0.1:9222/json").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(url_host("about:blank"), None, "无 scheme");
        assert_eq!(url_host(""), None);
    }

    #[test]
    fn session_cookie_keys_filter_pixiv_phpsessid() {
        let items = vec![
            json!({"name": "PHPSESSID", "value": "s1", "domain": ".pixiv.net", "path": "/"}),
            // host-only 变体与缺 path（视为 /）
            json!({"name": "PHPSESSID", "value": "s2", "domain": "www.pixiv.net"}),
            json!({"name": "PHPSESSID", "value": "s3", "domain": "..pixiv.net", "path": "/sub"}),
            // 非 pixiv 域
            json!({"name": "PHPSESSID", "value": "x", "domain": "pixiv.net.evil.com", "path": "/"}),
            // pixiv 域但不是会话 cookie（设备态保留，不清）
            json!({"name": "device_token", "value": "d", "domain": ".pixiv.net", "path": "/"}),
            // 缺 domain / 空 name 容错
            json!({"name": "PHPSESSID", "value": "v"}),
            json!({"name": "", "value": "v", "domain": ".pixiv.net", "path": "/"}),
        ];
        let keys = pixiv_session_cookie_keys(&items);
        assert_eq!(
            keys,
            vec![
                (
                    "PHPSESSID".to_string(),
                    ".pixiv.net".to_string(),
                    "/".to_string()
                ),
                (
                    "PHPSESSID".to_string(),
                    "www.pixiv.net".to_string(),
                    "/".to_string()
                ),
                (
                    "PHPSESSID".to_string(),
                    "..pixiv.net".to_string(),
                    "/sub".to_string()
                ),
            ]
        );
        assert!(pixiv_session_cookie_keys(&[]).is_empty());
    }

    /// 真机验证「过期覆盖删除」语义：临时 headless profile 里种假 PHPSESSID →
    /// clear_pixiv_session → 回读确认已删。手工跑：
    /// `cargo test --release -- --ignored clear_pixiv_session_roundtrip`
    #[tokio::test]
    #[ignore = "依赖本机 Chrome/Edge/Chromium，仅在环境可控时手工跑"]
    async fn clear_pixiv_session_roundtrip() {
        let browser = find_login_browser().expect("需要本机 Chrome/Edge/Chromium");
        let dir =
            std::env::temp_dir().join(format!("pixiv-tool-cdp-clear-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let port = free_port().unwrap();
        // headless 临时 profile：不闪窗、不触真实账号数据
        let mut child = tokio::process::Command::new(&browser)
            .arg(format!("--remote-debugging-port={port}"))
            .arg("--remote-debugging-address=127.0.0.1")
            .arg(format!("--user-data-dir={}", dir.display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--headless=new")
            .arg("about:blank")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut cdp = async {
            let ws_url = wait_for_cdp(port).await.unwrap();
            let mut cdp = CdpClient::connect(&ws_url).await.unwrap();

            // 种假 PHPSESSID（临时 profile，与真实登录态无关）
            cdp.call(
                "Storage.setCookies",
                serde_json::json!({ "cookies": [{
                    "name": "PHPSESSID",
                    "value": "fake_test_session_123",
                    "domain": ".pixiv.net",
                    "path": "/",
                    "secure": true,
                }]}),
            )
            .await
            .unwrap();
            let raw = cdp
                .call("Storage.getCookies", serde_json::json!({}))
                .await
                .unwrap();
            let seeded = pixiv_session_cookie_keys(raw["cookies"].as_array().unwrap());
            assert_eq!(seeded.len(), 1, "假会话应已种入");

            let cleared = clear_pixiv_session(&mut cdp).await.unwrap();
            assert_eq!(cleared, 1);

            let raw = cdp
                .call("Storage.getCookies", serde_json::json!({}))
                .await
                .unwrap();
            let left = pixiv_session_cookie_keys(raw["cookies"].as_array().unwrap());
            assert!(
                left.is_empty(),
                "PHPSESSID 应已被过期覆盖删除，残留 {left:?}"
            );
            cdp
        }
        .await;
        let _ = cdp.close().await;
        shutdown_browser(&mut child).await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "依赖本机是否安装浏览器，仅在环境可控时手工跑（cargo test -- --ignored）"]
    fn find_login_browser_env_dependent() {
        // 在装了 Chrome/Edge/Chromium 的机器上应找到可执行文件；
        // 干净环境（容器）下应返回 Err。
        let _ = find_login_browser();
    }
}
