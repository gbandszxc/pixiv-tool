//! 检查应用更新命令（GitHub Releases）。
//!
//! 请求 `releases/latest`（GitHub 302 到最新稳定版发布页），从最终生效 URL
//! （wreq 的 `Response::uri()`，等价 reqwest 的 `url()`）解析 tag，拿不到再
//! 回退扫描响应 HTML 里的第一个 `releases/tag/<tag>`。只认 `^v?\d+\.\d+\.\d+$`
//! 的稳定版 tag（手写字符校验，不引入 regex 依赖），预发布（含 `-` 后缀）
//! 跳过继续找下一个。
//!
//! 网络策略（双通道）：wreq 默认**不跟随重定向**（builder 默认
//! `redirect_policy = none()`），必须显式 `Policy::limited`，否则 `releases/latest`
//! 停在 302、永远解析不到 tag。先走系统代理感知通道（用户开系统代理时与其
//! 浏览器同链路），网络层失败（连接错误或非 2xx）再用 `.no_proxy()` 强制直连
//! 通道重试一次，两者都失败才报错。
//!
//! 安全边界：HTTP 客户端独立构建、进程内缓存，**绝不复用 PixivClient**——
//! 它带 pixiv cookie，发给 GitHub 属于凭据泄漏。错误文案一律用固定中文短句，
//! 不回显网络错误原文（错误 Display 可能内嵌完整 URL）。

use std::cmp::Ordering;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Serialize;
use wreq::header::{HeaderMap, HeaderValue};
use wreq::redirect::Policy as RedirectPolicy;
use wreq_util::Emulation;

use crate::pixiv::client::{ACCEPT_LANGUAGE, USER_AGENT};

/// 最新稳定版发布页（`/releases/latest` 由 GitHub 302 到 `/releases/tag/<tag>`）。
const RELEASES_LATEST_URL: &str = "https://github.com/gbandszxc/pixiv-tool/releases/latest";
/// 发布列表页（无更新时给前端的跳转目标）。
const RELEASES_URL: &str = "https://github.com/gbandszxc/pixiv-tool/releases";
/// 单请求超时（秒）。直连 TLS 握手可能慢，比常规接口放宽。
const REQUEST_TIMEOUT_SECS: u64 = 15;
/// 最终 URL / HTML 里 tag 路径的公共前缀。
const TAG_PATH_MARKER: &str = "/releases/tag/";

/// 单通道请求结果。
enum FetchOutcome {
    /// 拿到最新稳定版 tag。
    Tag(String),
    /// 网络层通了但页面里解析不出稳定版 tag（不重试另一条通道）。
    NoTag,
    /// 网络层失败（连接错误或非 2xx），值得用另一条通道重试。
    Unreachable,
}

/// 更新检查结果（IPC 返回体，字段保持 snake_case）。
#[derive(Debug, Serialize)]
pub struct UpdateCheckInfo {
    /// 最新稳定版大于当前版本。
    pub has_update: bool,
    /// 当前应用版本（package_info）。
    pub current_version: String,
    /// releases 页找到的最新稳定版；无更新时等于 current_version。
    pub latest_version: String,
    /// 有更新 → 该 tag 发布页；无更新 → 发布列表页。
    pub release_url: String,
    pub platform: String,
    pub package_type: Option<String>,
}

/// 检查应用更新（前端 IPC 入口）。
#[tauri::command]
pub async fn check_app_update(app: tauri::AppHandle) -> Result<UpdateCheckInfo, String> {
    let current_version = app.package_info().version.to_string();
    check_app_update_impl(&current_version).await
}

/// 命令实现（current_version 参数化，便于离线测试构造）。
pub async fn check_app_update_impl(current_version: &str) -> Result<UpdateCheckInfo, String> {
    let (system_client, direct_client) = shared_clients()?;
    // 双通道：先走系统代理感知通道，网络层失败再用强制直连通道重试一次。
    let outcome = match fetch_release_tag(system_client).await {
        FetchOutcome::Unreachable => fetch_release_tag(direct_client).await,
        outcome => outcome,
    };
    match outcome {
        FetchOutcome::Tag(tag) => Ok(update_info(&tag, current_version)),
        FetchOutcome::NoTag => Err("未找到有效的发布版本".to_string()),
        FetchOutcome::Unreachable => Err("无法访问 GitHub 发布页".to_string()),
    }
}

/// 单通道取最新稳定版 tag：请求 `releases/latest`（跟随 302 到 tag 页），
/// 先从最终生效 URL 取 tag，拿不到再回退扫描响应 HTML。
async fn fetch_release_tag(client: &wreq::Client) -> FetchOutcome {
    let response = match client.get(RELEASES_LATEST_URL).send().await {
        Ok(response) => response,
        Err(_) => return FetchOutcome::Unreachable,
    };
    if !response.status().is_success() {
        return FetchOutcome::Unreachable;
    }
    // uri() 即跟随重定向后的最终 URL；releases/latest 302 后形如
    // .../releases/tag/v1.2.3，直接从中取 tag，HTML 扫描只做回退。
    let final_url = response.uri().to_string();
    let body = match response.text().await {
        Ok(body) => body,
        Err(_) => return FetchOutcome::Unreachable,
    };
    match tag_from_url(&final_url).or_else(|| first_stable_tag_in_html(&body)) {
        Some(tag) => FetchOutcome::Tag(tag),
        None => FetchOutcome::NoTag,
    }
}

/// 由稳定版 tag 与当前版本组装返回体（tag 允许 v 前缀，版本号本体统一
/// 去掉 v 再比较与展示）。
fn update_info(tag: &str, current_version: &str) -> UpdateCheckInfo {
    let latest = tag.strip_prefix('v').unwrap_or(tag).to_string();
    let has_update = compare_versions(&latest, current_version) == Ordering::Greater;
    UpdateCheckInfo {
        has_update,
        current_version: current_version.to_string(),
        latest_version: if has_update {
            latest
        } else {
            current_version.to_string()
        },
        release_url: if has_update {
            format!("{RELEASES_URL}/tag/{tag}")
        } else {
            RELEASES_URL.to_string()
        },
        platform: std::env::consts::OS.to_string(),
        package_type: tauri::utils::platform::bundle_type().map(|kind| kind.to_string()),
    }
}

/// 双通道无 cookie 客户端（Chrome147 指纹，与 saucenao 同款 builder），
/// 进程内缓存：(系统代理感知, 强制直连)。
pub(super) fn shared_clients() -> Result<&'static (wreq::Client, wreq::Client), String> {
    static CLIENTS: OnceLock<Option<(wreq::Client, wreq::Client)>> = OnceLock::new();
    CLIENTS
        .get_or_init(build_clients)
        .as_ref()
        .ok_or_else(|| "创建 HTTP 客户端失败".to_string())
}

/// 构建双通道客户端。两条通道共用同一 builder，仅代理语义不同：
/// - system：默认 builder，跟随系统代理 / 环境变量（用户开代理时同浏览器链路）；
/// - direct：`.no_proxy()` 强制直连，system 通道失败后的兜底。
/// 重定向必须显式开启：wreq 默认 `redirect_policy = none()`，不跟随的话
/// `releases/latest` 停在 302，永远拿不到最终 tag 页。
fn build_clients() -> Option<(wreq::Client, wreq::Client)> {
    let mut default_headers = HeaderMap::new();
    default_headers.insert("user-agent", HeaderValue::from_static(USER_AGENT));
    default_headers.insert("accept-language", HeaderValue::from_static(ACCEPT_LANGUAGE));
    let builder = || {
        wreq::Client::builder()
            .emulation(Emulation::Chrome147)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .redirect(RedirectPolicy::limited(10))
            .default_headers(default_headers.clone())
    };
    Some((builder().build().ok()?, builder().no_proxy().build().ok()?))
}

/// 语义化版本比较：按 `.` 分段转 u64 逐段比较，段数不齐按 0 补齐
/// （1.2 == 1.2.0）；容忍前导 v（tag 与 package_info 两种来源统一处理）。
fn compare_versions(a: &str, b: &str) -> Ordering {
    let mut sa: Vec<u64> = strip_v_prefix(a)
        .split('.')
        .map(|s| s.parse().unwrap_or(0))
        .collect();
    let mut sb: Vec<u64> = strip_v_prefix(b)
        .split('.')
        .map(|s| s.parse().unwrap_or(0))
        .collect();
    let len = sa.len().max(sb.len());
    sa.resize(len, 0);
    sb.resize(len, 0);
    sa.cmp(&sb)
}

/// 去掉前导 `v`（无前缀原样返回）。
fn strip_v_prefix(s: &str) -> &str {
    s.strip_prefix('v').unwrap_or(s)
}

/// 稳定版 tag 判定：`^v?\d+\.\d+\.\d+$`（恰好三段非空纯数字）。
pub(super) fn is_stable_tag(tag: &str) -> bool {
    let core = tag.strip_prefix('v').unwrap_or(tag);
    let mut parts = core.split('.');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), Some(c), None) => [a, b, c]
            .iter()
            .all(|seg| !seg.is_empty() && seg.bytes().all(|byte| byte.is_ascii_digit())),
        _ => false,
    }
}

/// 从最终生效 URL（`.../releases/tag/<tag>`）提取 tag，仅接受合法稳定版。
fn tag_from_url(url: &str) -> Option<String> {
    let rest = url.split(TAG_PATH_MARKER).nth(1)?;
    // 截断后续路径 / query / fragment（稳定版 tag 只含 [v0-9.]，不会被 URL 编码）
    let tag = rest.split(|c| c == '/' || c == '?' || c == '#').next()?;
    if is_stable_tag(tag) {
        Some(tag.to_string())
    } else {
        None
    }
}

/// 在 HTML 正文扫描第一个合法稳定版 tag（`.../releases/tag/<tag>`）。
/// 命中预发布（如 v1.2.3-beta.1）或非版本串（如 abc）则跳过，继续找下一个。
fn first_stable_tag_in_html(html: &str) -> Option<String> {
    let mut rest = html;
    while let Some(idx) = rest.find(TAG_PATH_MARKER) {
        let after = &rest[idx + TAG_PATH_MARKER.len()..];
        let end = after
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-')))
            .map(|(i, _)| i)
            .unwrap_or(after.len());
        let tag = &after[..end];
        if is_stable_tag(tag) {
            return Some(tag.to_string());
        }
        rest = after;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 版本比较 ----

    #[test]
    fn version_equal() {
        assert_eq!(compare_versions("1.2.3", "1.2.3"), Ordering::Equal);
    }

    #[test]
    fn version_segment_greater() {
        assert_eq!(compare_versions("1.2.4", "1.2.3"), Ordering::Greater);
        assert_eq!(compare_versions("1.3.0", "1.2.9"), Ordering::Greater);
        assert_eq!(compare_versions("2.0.0", "1.9.9"), Ordering::Greater);
        assert_eq!(compare_versions("0.9.9", "1.0.0"), Ordering::Less);
    }

    #[test]
    fn version_pads_missing_segments() {
        assert_eq!(compare_versions("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.2.0", "1.2"), Ordering::Equal);
        assert_eq!(compare_versions("1.2", "1.2.1"), Ordering::Less);
    }

    #[test]
    fn version_multi_segment() {
        assert_eq!(compare_versions("1.2.3.1", "1.2.3"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.0.0", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn version_tolerates_leading_v() {
        assert_eq!(compare_versions("v1.2.3", "1.2.3"), Ordering::Equal);
        assert_eq!(compare_versions("v1.2.3", "v1.2.4"), Ordering::Less);
    }

    // ---- tag 判定与解析 ----

    #[test]
    fn stable_tag_accepts_v_and_bare() {
        assert!(is_stable_tag("v1.2.3"));
        assert!(is_stable_tag("1.2.3"));
        assert!(is_stable_tag("v0.0.1"));
    }

    #[test]
    fn stable_tag_rejects_prerelease_and_junk() {
        assert!(!is_stable_tag("v1.2.3-beta.1"));
        assert!(!is_stable_tag("1.2.3-rc.1"));
        assert!(!is_stable_tag("abc"));
        assert!(!is_stable_tag("1.2"));
        assert!(!is_stable_tag("1.2.3.4"));
        assert!(!is_stable_tag("v"));
        assert!(!is_stable_tag(""));
    }

    #[test]
    fn html_scan_skips_noise_and_prerelease() {
        // noise（列表页链接 / 非版本 tag）在前，预发布其次，首个稳定版在 v1.2.3
        let html = concat!(
            "<nav><a href=\"/gbandszxc/pixiv-tool/releases\">All releases</a></nav>",
            "<a href=\"/gbandszxc/pixiv-tool/releases/tag/notes\">release notes</a>",
            "<a href=\"https://github.com/gbandszxc/pixiv-tool/releases/tag/v1.2.3-beta.1\">v1.2.3-beta.1</a>",
            "<a href=\"/gbandszxc/pixiv-tool/releases/tag/v1.2.3\">pixiv-tool v1.2.3</a>",
            "<a href=\"/gbandszxc/pixiv-tool/releases/tag/v1.1.0\">v1.1.0</a>",
        );
        assert_eq!(first_stable_tag_in_html(html).as_deref(), Some("v1.2.3"));
    }

    #[test]
    fn html_scan_returns_none_when_no_stable_tag() {
        let html = concat!(
            "<a href=\"/gbandszxc/pixiv-tool/releases/tag/abc\">abc</a>",
            "<a href=\"/gbandszxc/pixiv-tool/releases/tag/v2.0.0-beta.1\">beta</a>",
        );
        assert_eq!(first_stable_tag_in_html(html), None);
        assert_eq!(first_stable_tag_in_html("完全没有 tag 的正文"), None);
    }

    #[test]
    fn tag_from_final_url() {
        assert_eq!(
            tag_from_url("https://github.com/gbandszxc/pixiv-tool/releases/tag/v1.2.3").as_deref(),
            Some("v1.2.3")
        );
        assert_eq!(
            tag_from_url("https://github.com/gbandszxc/pixiv-tool/releases/tag/1.2.3").as_deref(),
            Some("1.2.3")
        );
        // 无 tag / 预发布 tag → None（走 HTML 回退）
        assert_eq!(
            tag_from_url("https://github.com/gbandszxc/pixiv-tool/releases"),
            None
        );
        assert_eq!(
            tag_from_url("https://github.com/gbandszxc/pixiv-tool/releases/tag/v1.2.3-beta.1"),
            None
        );
    }
}
