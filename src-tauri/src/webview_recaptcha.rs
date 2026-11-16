//! 关注人机验证 token 铸造桥（ADR 0055）。
//!
//! pixiv 对部分账号（`test.ab.recaptcha_follow_user` 实验开关 / 风控命中）
//! 的关注 `bookmark_add.php` 强制要求 reCAPTCHA Enterprise score token，
//! 缺 token 恒 400（PIXIV-API §4.10）。该 token 只能在 pixiv 页面 JS 上下文
//! 里经 `grecaptcha.enterprise.execute` 铸出，原生侧无法自行生成；本模块用
//! 独立 Tauri 窗口加载 www.pixiv.net（满足 recaptcha 的域绑定，页面本身无
//! 登录要求），注入 `resources/pixiv/recaptcha-mint.js` 铸 token，经
//! `pixivtool-recaptcha://` 自定义 scheme 导航拦截回收，窗口用完即销毁。
//!
//! site key 由调用方传入（登录态主页 `__NEXT_DATA__` 解析，csrf.rs）：
//! 匿名页面没有 preloaded state，WebView 侧解析不到。
//!
//! 安全姿态与登录 webview 一致：窗口无 IPC 权限（capabilities 只授予
//! main 窗口）；数据目录独立于 `login-webview-profile`（会被登录流程整目录
//! 重置，不能共用），持久保留以积累 reCAPTCHA 风险信誉。
//!
//! 日志纪律：token 与含 token 的导航 URL 一律不写日志。

use std::path::Path;
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// 铸造窗口 label（对齐 login-webview 的命名风格）。
const MINT_LABEL: &str = "recaptcha-mint";
/// 窗口尺寸：小窗口可见（规避 macOS 隐藏窗口 JS 执行不确定性），不抢焦点。
const WINDOW_WIDTH: f64 = 420.0;
const WINDOW_HEIGHT: f64 = 300.0;
/// recaptcha 域绑定载体：任一 www.pixiv.net 页面即可，无需登录态。
const MINT_PAGE_URL: &str = "https://www.pixiv.net/";
/// 铸造总超时：页面加载 + enterprise.js 下载 + execute 的宽裕上限。
const MINT_TIMEOUT_SECS: u64 = 40;
/// 回收 scheme（与 recaptcha-mint.js 约定一致）。
const MINT_SCHEME: &str = "pixivtool-recaptcha";

const MINT_JS_TEMPLATE: &str = include_str!("../resources/pixiv/recaptcha-mint.js");

/// 铸造结果（on_navigation → 通道 → 超时等待）。
#[derive(Debug, PartialEq)]
enum MintOutcome {
    Token(String),
    Failed(String),
}

/// JS 字符串字面量转义（site key 来自 pixiv 页面自身配置，纵深防御仍转义）。
fn js_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' | '"' => {
                out.push('\\');
                out.push(ch);
            }
            '<' => out.push_str("\\x3c"),
            '>' => out.push_str("\\x3e"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{{{:04x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// 导航拦截解析：识别 `pixivtool-recaptcha://done?t={token}` /
/// `pixivtool-recaptcha://error?m={message}`，其余 URL 返回 None（放行）。
/// 纯函数，离线可测。
fn parse_mint_result_url(url: &str) -> Option<MintOutcome> {
    let url = tauri::Url::parse(url).ok()?;
    if url.scheme() != MINT_SCHEME {
        return None;
    }
    match url.host_str()? {
        "done" => {
            let token = url
                .query_pairs()
                .find(|(key, _)| key == "t")?
                .1
                .into_owned();
            (!token.is_empty()).then_some(MintOutcome::Token(token))
        }
        "error" => {
            let message = url
                .query_pairs()
                .find(|(key, _)| key == "m")
                .map(|(_, value)| value.into_owned())
                .unwrap_or_else(|| "未知原因".into());
            Some(MintOutcome::Failed(message))
        }
        _ => None,
    }
}

/// 铸一枚关注用 recaptcha score token（单飞串行；窗口用完即销毁）。
///
/// `site_key`：登录态主页 `__NEXT_DATA__` 解析的 reCAPTCHA Enterprise site key
/// （`PixivApi::follow_recaptcha_config`）；缺失即无法铸造。
/// 失败返回可读原因（不含 token），调用方映射人机验证引导文案。
pub async fn mint_follow_token(
    app: &AppHandle,
    config_dir: &Path,
    _author_id: i64,
    site_key: Option<&str>,
) -> Result<String, String> {
    let Some(site_key) = site_key.filter(|key| !key.is_empty()) else {
        return Err("无法从 pixiv 页面解析 recaptcha site key".into());
    };

    // 单飞：铸造窗口全局唯一，并发关注请求串行排队
    static MINT_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _permit = MINT_GATE.lock().await;

    // site key 注入脚本模板（常量模板 + 编译期资源，无用户输入）
    let script =
        MINT_JS_TEMPLATE.replace("__PIXIVTOOL_RECAPTCHA_SITE_KEY__", &js_string_literal(site_key));

    // 独立数据目录：持久保留（不随登录重置），为 reCAPTCHA 积累风险信誉
    let profile_dir = config_dir.join("recaptcha-webview-profile");
    std::fs::create_dir_all(&profile_dir)
        .map_err(|err| format!("创建人机验证 webview 目录失败: {err}"))?;

    // 防御：销毁上次残留的同 label 窗口，避免 build 撞 label 失败
    if let Some(existing) = app.get_webview_window(MINT_LABEL) {
        log::info!("发现残留的人机验证窗口，先销毁重建");
        let _ = existing.destroy();
    }

    let url = tauri::Url::parse(MINT_PAGE_URL).map_err(|err| format!("解析铸造页 URL 失败: {err}"))?;
    // tokio mpsc：recv().await 在异步侧等待；on_navigation 闭包跑在主线程，
    // unbounded 的 send 是同步非阻塞，安全
    let (nav_tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<MintOutcome>();
    let window = WebviewWindowBuilder::new(app, MINT_LABEL, WebviewUrl::External(url))
        .title("pixiv 人机验证")
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .focused(false)
        .center()
        .data_directory(profile_dir)
        .initialization_script(script)
        .on_navigation(move |nav_url| {
            // 导航 URL 可能含 token：只解析回传，不写日志
            if let Some(outcome) = parse_mint_result_url(nav_url.as_str()) {
                let _ = nav_tx.send(outcome);
                // 拦截自定义 scheme 导航（未知 scheme 会落错误页），页面保持原状
                return false;
            }
            true
        })
        .build()
        .map_err(|err| format!("创建人机验证窗口失败: {err}"))?;

    let outcome = tokio::time::timeout(
        Duration::from_secs(MINT_TIMEOUT_SECS),
        async move {
            match rx.recv().await {
                Some(MintOutcome::Token(token)) => Ok(token),
                Some(MintOutcome::Failed(message)) => Err(message),
                None => Err("人机验证窗口已关闭".into()),
            }
        },
    )
    .await;

    // finally 语义：铸完即销毁（窗口可能已被用户关闭，destroy 报错属预期）
    let _ = window.destroy();

    match outcome {
        Ok(Ok(token)) => {
            log::info!("recaptcha token 铸造成功");
            Ok(token)
        }
        Ok(Err(message)) => {
            log::warn!("recaptcha token 铸造失败：{message}");
            Err(message)
        }
        Err(_) => {
            log::warn!("recaptcha token 铸造超时（{MINT_TIMEOUT_SECS}s）");
            Err(format!("人机验证获取超时（{MINT_TIMEOUT_SECS}s）"))
        }
    }
}

// ----------------------------------------------------------------------
// 单元测试（纯函数，全部离线）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mint_result_url_accepts_done_and_error() {
        let outcome = parse_mint_result_url("pixivtool-recaptcha://done?t=abc123").unwrap();
        assert_eq!(outcome, MintOutcome::Token("abc123".into()));

        // token 值经 percent-encode 回传
        let outcome =
            parse_mint_result_url("pixivtool-recaptcha://done?t=a%2Fb%3Dc").unwrap();
        assert_eq!(outcome, MintOutcome::Token("a/b=c".into()));

        let outcome =
            parse_mint_result_url("pixivtool-recaptcha://error?m=grecaptcha%20timeout").unwrap();
        assert_eq!(outcome, MintOutcome::Failed("grecaptcha timeout".into()));

        // 缺 m 参数 → 未知原因
        let outcome = parse_mint_result_url("pixivtool-recaptcha://error").unwrap();
        assert_eq!(outcome, MintOutcome::Failed("未知原因".into()));
    }

    #[test]
    fn parse_mint_result_url_rejects_other_urls_and_empty_token() {
        // 正常页面导航（含初始 https 导航）不回收
        assert!(parse_mint_result_url("https://www.pixiv.net/").is_none());
        assert!(parse_mint_result_url("https://www.google.com/recaptcha").is_none());
        // 非 本 scheme 的自定义 scheme
        assert!(parse_mint_result_url("pixiv-tool://done?t=x").is_none());
        // 非法 URL
        assert!(parse_mint_result_url("not a url").is_none());
        // 空 token 拒绝
        assert!(parse_mint_result_url("pixivtool-recaptcha://done?t=").is_none());
        // done 缺 t 参数
        assert!(parse_mint_result_url("pixivtool-recaptcha://done").is_none());
    }

    #[test]
    fn js_string_literal_escapes_breakout_chars() {
        assert_eq!(js_string_literal("6Lc123ABC-_x"), "6Lc123ABC-_x");
        assert_eq!(js_string_literal("a\"b'c\\d"), "a\\\"b\\'c\\\\d");
        assert_eq!(js_string_literal("<script>"), "\\x3cscript\\x3e");
        assert_eq!(js_string_literal("a\nb\rc"), "a\\nb\\rc");
    }
}
