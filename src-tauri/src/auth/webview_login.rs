//! 内嵌 webview 登录：无 Chromium 系浏览器时的回退登录路径。
//!
//! 打开独立 Tauri 窗口加载 Pixiv 登录页，用户在窗口里完成真实登录；
//! 主循环轮询窗口 URL，跳回 pixiv 主站后经统一 cookie API 读取原生
//! cookie 存储（macOS WKHTTPCookieStore 等，含 HttpOnly 的 PHPSESSID），
//! 再用 fetch_session_probe 验证，成功补 x-csrf-token 返回 success。
//!
//! 终态语义与 browser_login 一致（复用其 LoginResult / LOGIN_URL /
//! LOGIN_TIMEOUT_SEC / POLL_INTERVAL_MILLIS / url_host）：
//! - 用户关闭窗口 → {"status":"cancelled"}
//! - 总超时 LOGIN_TIMEOUT_SEC → {"status":"timeout"}（同样关窗）
//! - 建窗等异常 → {"status":"error","message":...}
//!
//! **Cookie 值不得写入日志**（日志只出现名字 / 计数类信息）。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use tauri::webview::Cookie;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::auth::browser_login::{
    LOGIN_TIMEOUT_SEC, LOGIN_URL, LoginResult, POLL_INTERVAL_MILLIS, url_host,
};
use crate::pixiv::csrf::{ProbeError, fetch_session_probe};

/// 登录窗口 label。capabilities 只授予 main 窗口，本窗口无 IPC 权限，
/// 仅用于加载 pixiv 登录页并读取其 cookie。
pub const WEBVIEW_LABEL: &str = "login-webview";
/// 登录窗口逻辑尺寸（对齐规格 960×720）。
const WINDOW_WIDTH: f64 = 960.0;
const WINDOW_HEIGHT: f64 = 720.0;

/// 从 webview 原生 cookie 存储的全量快照中提取 Pixiv 域 Cookie：
/// domain 去掉前导 '.' 后以 "pixiv.net" 结尾，且 name/value 非空
/// （与 browser_login::extract_pixiv_cookies 同语义）。
///
/// 刻意不用 `cookies_for_url`：其对 domain 的匹配在部分平台是精确匹配，
/// 会漏掉 ".pixiv.net" 的域级 cookie；全量读取后本地过滤更可靠。
pub fn extract_pixiv_cookies_from_store(cookies: &[Cookie<'static>]) -> HashMap<String, String> {
    cookies
        .iter()
        .filter_map(|cookie| {
            let domain = cookie.domain().unwrap_or("");
            let name = cookie.name();
            let value = cookie.value();
            // 与 CDP 版一致：剥前导 '.' 后大小写敏感地 ends_with("pixiv.net")
            let is_pixiv = domain.trim_start_matches('.').ends_with("pixiv.net");
            (is_pixiv && !name.is_empty() && !value.is_empty())
                .then(|| (name.to_string(), value.to_string()))
        })
        .collect()
}

/// 轮询门禁：host 是否为 pixiv 主站（accounts 子域不算，对齐 CDP 版语义）。
fn is_pixiv_main_host(host: &str) -> bool {
    matches!(host, "pixiv.net" | "www.pixiv.net")
}

/// 打开隔离数据目录的内嵌 webview 等待登录完成：
///
/// 1. 建窗：label=login-webview、960×720、加载 LOGIN_URL，
///    data_directory 用调用方传入的 profile_dir（config_dir/login-webview-profile）
/// 2. 主循环至多 LOGIN_TIMEOUT_SEC（每轮 sleep POLL_INTERVAL_MILLIS）：
///    - 用户关闭窗口 → cancelled
///    - url host ∈ {pixiv.net, www.pixiv.net} → 读全量 cookie → 过滤 →
///      有非空 PHPSESSID → fetch_session_probe 验证
///      （ProbeError::Invalid 忽略继续轮询；成功补 x-csrf-token 返回 success）
/// 3. 超时 → timeout；终态统一关窗（destroy，窗口可能已不存在则忽略错误）
///
/// Cookie 值不得写入日志。
pub async fn open_webview_login(app: &AppHandle, profile_dir: &Path) -> LoginResult {
    match run_webview_login(app, profile_dir).await {
        Ok(result) => result,
        Err(err) => {
            // 日志与返回文案都只含原因，不含任何 Cookie 值
            log::warn!("内嵌 webview 登录失败：{err:#}");
            LoginResult::terminal("error", Some(format!("{err:#}")))
        }
    }
}

/// 重置内嵌登录 webview 的 profile 目录：登录窗语义是「必然未登录地打开」，
/// 持久化的旧会话会让 accounts.pixiv.net/login 302 回主站，轮询立即误判
/// 成功（加号秒关）。webview 无 CDP 通道可做定向会话清理，且该目录本就
/// 以「删目录=完整复测」为既定语义（ADR 0009 第 4 条），故启动前整目录
/// 重置。与浏览器路径（CDP 定向清 PHPSESSID、保留设备态，ADR 0011）不同：
/// webview 是无 Chromium 环境的回退路径，使用频率低，重置成本可接受。
/// 目录本就不存在时静默无操作；删不掉（如被占用）不阻塞登录。
fn reset_login_profile(profile_dir: &Path) {
    if let Err(err) = std::fs::remove_dir_all(profile_dir) {
        if err.kind() != std::io::ErrorKind::NotFound {
            log::warn!("重置 webview 登录 profile 失败（可能复发加号秒关）: {err}");
        }
    }
}

/// 主流程：任何 Err 都会在 finally 里关窗后由调用方转 error 终态。
async fn run_webview_login(app: &AppHandle, profile_dir: &Path) -> Result<LoginResult> {
    reset_login_profile(profile_dir);
    std::fs::create_dir_all(profile_dir)
        .map_err(|err| anyhow!("创建 webview profile 目录失败: {err}"))?;

    // 防御：重复发起登录时销毁上次残留的同 label 窗口，避免 build 撞 label 失败
    if let Some(existing) = app.get_webview_window(WEBVIEW_LABEL) {
        log::info!("发现残留的登录窗口，先销毁重建");
        let _ = existing.destroy();
    }

    let url = tauri::Url::parse(LOGIN_URL).map_err(|err| anyhow!("解析登录 URL 失败: {err}"))?;
    let window = WebviewWindowBuilder::new(app, WEBVIEW_LABEL, WebviewUrl::External(url))
        .title("Pixiv 登录")
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .data_directory(profile_dir.to_path_buf())
        .build()
        .map_err(|err| anyhow!("创建登录窗口失败: {err}"))?;

    let outcome = login_loop(&window).await;
    // finally 语义：cancelled 时窗口已被用户关闭，destroy 报错属预期，忽略
    let _ = window.destroy();
    outcome
}

/// 轮询主循环（窗口由调用方创建并在 finally 里销毁）。
async fn login_loop(window: &WebviewWindow) -> Result<LoginResult> {
    let deadline = Instant::now() + Duration::from_secs(LOGIN_TIMEOUT_SEC);
    let closed = window_closed_flag(window);
    loop {
        if Instant::now() >= deadline {
            return Ok(LoginResult::terminal(
                "timeout",
                Some(format!("登录超时（{LOGIN_TIMEOUT_SEC}s）")),
            ));
        }
        // 用户直接关闭登录窗 → cancelled
        if closed.load(Ordering::Relaxed) {
            return Ok(LoginResult::terminal("cancelled", None));
        }

        // 还停在 accounts.pixiv.net（登录页 / 验证码）时不读 cookie、不发探测请求
        let Some(host) = window_url_host(window).await else {
            // url() 出错多为窗口正在销毁的瞬间：交给下一轮的 closed 判定兜底
            tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            continue;
        };
        if !is_pixiv_main_host(&host) {
            tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MILLIS)).await;
            continue;
        }

        let raw = window_cookies(window).await.unwrap_or_default();
        let mut cookies = extract_pixiv_cookies_from_store(&raw);

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

/// 用户关闭窗口的检测标记：监听 Destroyed 事件置位。
fn window_closed_flag(window: &WebviewWindow) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let listener_flag = flag.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            listener_flag.store(true, Ordering::Relaxed);
        }
    });
    flag
}

/// 读当前页面 URL 并取 host（None = 读取失败，如窗口正在销毁）。
///
/// 线程模型说明：wry 对 url()/cookies() 的实现是把请求经事件循环代理
/// 派发到主线程执行、再在标准库 channel 上阻塞等结果（tauri-runtime-wry
/// 的 webview_getter! 宏），因此从任意线程调用都安全；但调用线程会被
/// 阻塞到主线程应答，且官方文档提示 Windows WebView2 在同步上下文里读
/// cookie 有死锁风险（wry#583），故统一包进 spawn_blocking 到独立阻塞
/// 线程再 await，不卡 tokio runtime worker。
async fn window_url_host(window: &WebviewWindow) -> Option<String> {
    let window = window.clone();
    let url = tokio::task::spawn_blocking(move || window.url().map(|url| url.to_string()))
        .await
        .ok()?
        .ok()?;
    url_host(&url)
}

/// 读 webview 原生 cookie 存储的全量快照（None = 读取失败）。线程模型同上。
async fn window_cookies(window: &WebviewWindow) -> Option<Vec<Cookie<'static>>> {
    let window = window.clone();
    tokio::task::spawn_blocking(move || window.cookies().ok())
        .await
        .ok()?
}

// ----------------------------------------------------------------------
// 单元测试（纯函数，离线构造 Cookie 样例）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    /// 离线构造 Cookie<'static>（Cookie::new 接受 owned 字符串即 'static）。
    fn make_cookie(name: &str, value: &str, domain: Option<&str>) -> Cookie<'static> {
        let mut cookie = Cookie::new(name.to_string(), value.to_string());
        if let Some(domain) = domain {
            cookie.set_domain(domain.to_string());
        }
        cookie
    }

    #[test]
    fn reset_login_profile_removes_directory() {
        let dir =
            std::env::temp_dir().join(format!("pixiv-tool-wv-reset-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("Default")).unwrap();
        std::fs::write(dir.join("Default").join("Cookies"), b"x").unwrap();

        reset_login_profile(&dir);
        assert!(!dir.exists(), "profile 目录应被整体重置");

        // 目录不存在时静默无操作、不报错
        reset_login_profile(&dir);
        assert!(!dir.exists());
    }

    #[test]
    fn extract_filters_by_domain_and_empty_values() {
        let cookies = vec![
            make_cookie("PHPSESSID", "12345_abc", Some(".pixiv.net")),
            make_cookie("device_token", "abc", Some(".www.pixiv.net")),
            // 前导多点也剥
            make_cookie("multi_dot", "v", Some("..pixiv.net")),
            // 无前导点的精确域
            make_cookie("plain", "v", Some("www.pixiv.net")),
            // 非 pixiv 域 / 伪装域
            make_cookie("evil", "v", Some("pixiv.net.evil.com")),
            make_cookie("google", "v", Some(".google.com")),
            // 空值 / 空 name 过滤
            make_cookie("empty_value", "", Some(".pixiv.net")),
            make_cookie("", "v", Some(".pixiv.net")),
        ];
        let extracted = extract_pixiv_cookies_from_store(&cookies);
        assert_eq!(extracted.len(), 4);
        assert_eq!(
            extracted.get("PHPSESSID").map(String::as_str),
            Some("12345_abc")
        );
        assert_eq!(
            extracted.get("device_token").map(String::as_str),
            Some("abc")
        );
        assert_eq!(extracted.get("multi_dot").map(String::as_str), Some("v"));
        assert_eq!(extracted.get("plain").map(String::as_str), Some("v"));
        assert!(!extracted.contains_key("evil"));
        assert!(!extracted.contains_key("google"));
        assert!(!extracted.contains_key("empty_value"));
    }

    #[test]
    fn extract_missing_domain_is_rejected() {
        // 原生存储理论上总有 domain，但缺字段容错须与 CDP 版一致：拒绝
        let cookies = vec![make_cookie("no_domain", "v", None)];
        assert!(extract_pixiv_cookies_from_store(&cookies).is_empty());
    }

    #[test]
    fn extract_empty_input() {
        assert!(extract_pixiv_cookies_from_store(&[]).is_empty());
    }

    #[test]
    fn main_host_accepts_pixiv_hosts_only() {
        assert!(is_pixiv_main_host("pixiv.net"));
        assert!(is_pixiv_main_host("www.pixiv.net"));
        // 登录页（accounts 子域）不算主站
        assert!(!is_pixiv_main_host("accounts.pixiv.net"));
        // 伪装域 / 大小写已在 url_host 归一，但空串必须拒绝
        assert!(!is_pixiv_main_host("pixiv.net.evil.com"));
        assert!(!is_pixiv_main_host(""));
    }
}
