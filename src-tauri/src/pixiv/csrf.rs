//! Pixiv Session 校验与 CSRF token 获取。
//!
//! 语义对齐 Python `core/csrf.py`。错误两分类：
//! - `Invalid`：PHPSESSID 无效或已过期（401/403/非 200/无 userData.id）
//! - `Csrf`：登录态有效，但响应异常（非 JSON / 缺 csrf token）
//!
//! 注意：Chrome 实测匿名 /ajax/user/self 同样返回 HTTP 200 和 token，
//! 只有 userData.id 能证明 PHPSESSID 真正有效。
//! 探测请求复用 PixivClient 的 wreq 指纹伪装（裸 UA 是强风控信号）。

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use super::client::{PixivClient, PixivError, json_truthy, mask_url};

pub const PIXIV_SELF_URL: &str = "https://www.pixiv.net/ajax/user/self?lang=zh";
/// 相对路径形式（browse 层复用同一端点取自 uid，走 PixivApi 的 get_json）。
pub(crate) const SELF_PATH: &str = "/ajax/user/self?lang=zh";

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
    let text = value.trim();
    if text.len() > 4096 {
        return Err("PHPSESSID 格式不正确".into());
    }
    let candidate = if text.contains("PHPSESSID=") {
        extract_cookie_value(text).ok_or_else(|| "PHPSESSID 格式不正确".to_string())?
    } else {
        text.to_string()
    };
    if candidate.is_empty() {
        return Err("PHPSESSID 不能为空".into());
    }
    if ['\r', '\n', ';', '\0']
        .iter()
        .any(|c| candidate.contains(*c))
    {
        return Err("PHPSESSID 格式不正确".into());
    }
    Ok(candidate)
}

/// Cookie 请求头 / `PHPSESSID=值` 中取 PHPSESSID 的值（分号分段、首等号取值）。
fn extract_cookie_value(text: &str) -> Option<String> {
    text.split(';').find_map(|pair| {
        let pair = pair.trim();
        let (name, value) = pair.split_once('=')?;
        (name.trim() == "PHPSESSID").then(|| value.trim().to_string())
    })
}

/// JSON 值 → 字符串（字符串原样、数字转字符串，其余空串），对齐 Python str()。
fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// /ajax/user/self 响应分类（纯函数，离线可测）。
/// `body` 是整个解析后的 JSON（该端点是扁平结构：顶层 userData/token）。
pub(crate) fn parse_self_response(
    status: u16,
    body: Option<Value>,
) -> Result<SessionProbe, ProbeError> {
    if matches!(status, 401 | 403) {
        return Err(ProbeError::Invalid("PHPSESSID 无效或已过期".into()));
    }
    if status != 200 {
        return Err(ProbeError::Invalid(format!(
            "Pixiv 登录态校验失败：HTTP {status}"
        )));
    }
    let Some(body) = body else {
        return Err(ProbeError::Csrf("Pixiv 登录态响应不是有效 JSON".into()));
    };
    let user_data = body.get("userData");
    let user_data = match user_data {
        Some(v) if v.is_object() => v,
        _ => return Err(ProbeError::Invalid("PHPSESSID 无效或已过期".into())),
    };
    // 只有 userData.id 能证明 PHPSESSID 有效（匿名 self 也返回 200+token）
    let id = user_data.get("id").filter(|v| json_truthy(v));
    if id.is_none() {
        return Err(ProbeError::Invalid("PHPSESSID 无效或已过期".into()));
    }
    let token = body
        .get("token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty());
    let token = token.ok_or(ProbeError::Csrf(
        "登录态有效，但 Pixiv 响应缺少 csrf token".into(),
    ))?;

    let profile_img = user_data
        .get("profileImg")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            user_data
                .get("profileImgBig")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default()
        .to_string();

    Ok(SessionProbe {
        csrf_token: token.to_string(),
        is_logged_in: true,
        user: Some(UserInfo {
            user_id: value_to_string(id.unwrap()),
            pixiv_id: user_data
                .get("pixivId")
                .map(value_to_string)
                .unwrap_or_default(),
            name: user_data
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            profile_img,
        }),
    })
}

/// 用 /ajax/user/self 一次完成 Session、token 与用户信息校验。
///
/// 错误语义（文案精确）：
/// - 401/403/非 200/无 userData.id → Invalid("PHPSESSID 无效或已过期" 等)
/// - 非 JSON → Csrf("Pixiv 登录态响应不是有效 JSON")
/// - 缺 token → Csrf("登录态有效，但 Pixiv 响应缺少 csrf token")
pub async fn fetch_session_probe(phpsessid: &str) -> Result<SessionProbe, ProbeError> {
    let sid = normalize_phpsessid(phpsessid).map_err(ProbeError::Invalid)?;
    let mut cookies = HashMap::new();
    cookies.insert("PHPSESSID".to_string(), sid);

    let client = PixivClient::new(&cookies)
        .map_err(|err| ProbeError::Invalid(format!("创建 HTTP 客户端失败: {err}")))?;
    let (status, body) = match client.get_json_probe(SELF_PATH).await {
        Ok(v) => v,
        // 探测专用客户端只可能因网络错误（重试耗尽）失败；ProbeError 只有
        // Invalid/Csrf 两类，归入 Invalid 并保留原始错误文案。
        Err(PixivError::Network(msg)) => {
            return Err(ProbeError::Invalid(format!("Pixiv 登录态校验失败：{msg}")));
        }
        Err(other) => return Err(ProbeError::Invalid(other.to_string())),
    };
    parse_self_response(status, body)
}

// ----------------------------------------------------------------------
// 主站 CSRF token 抓取（浏览层 POST 接口用，如 /ajax/street/v2/main）
// ----------------------------------------------------------------------
//
// 实测（docs/research/pixiv-browse-api.md §10）：旧版 `pixiv.context.token` /
// `global-data` meta 已不存在，POST 接口需要的 x-csrf-token 在 pixiv-web-next
// 的 SSR 序列化状态里：`<script id="__NEXT_DATA__">` JSON 的
// props.pageProps.serverSerializedPreloadedState.api.token。

/// 从主站首页 HTML 解析 `__NEXT_DATA__` 的 serverSerializedPreloadedState
/// 状态对象（纯函数，离线可测；csrf token 与关注人机验证配置共用此入口）。
/// 任一环节失败返回 None（调用方转可读错误）。
pub(crate) fn parse_next_data_preloaded_state(html: &str) -> Option<Value> {
    const MARKER: &str = "id=\"__NEXT_DATA__\"";
    let idx = html.find(MARKER)?;
    let after_marker = &html[idx + MARKER.len()..];
    // script 开标签结束（属性顺序不定，取 marker 之后第一个 '>'）
    let gt = after_marker.find('>')?;
    let script_body = &after_marker[gt + 1..];
    let end = script_body.find("</script>")?;
    let json: Value = serde_json::from_str(script_body[..end].trim()).ok()?;
    // 实测（2026-10-01 登录态）serverSerializedPreloadedState 是**JSON 字符串**，
    // 需再 parse 一次才是状态对象；历史/部分场景也曾直接下发对象，两种都兼容。
    let state_raw = json.pointer("/props/pageProps/serverSerializedPreloadedState")?;
    let state: Value = match state_raw {
        Value::String(s) => serde_json::from_str(s).ok()?,
        v => v.clone(),
    };
    Some(state)
}

/// 从主站首页 HTML 解析会话 csrf token（纯函数，离线可测）。
pub(crate) fn parse_next_data_token(html: &str) -> Option<String> {
    let state = parse_next_data_preloaded_state(html)?;
    let token = state.pointer("/api/token")?;
    token.as_str().filter(|t| !t.is_empty()).map(String::from)
}

/// 主页 `__NEXT_DATA__`（preloaded state）里的关注人机验证配置（ADR 0055，
/// 路径对齐 ppixiv 的读取实现）：
/// - site_key：`misc.grecaptcha.recaptchaEnterpriseScoreSiteKey`（全局站点配置）；
/// - follow_requires_token：`test.ab.recaptcha_follow_user`（按账号启用的
///   实验开关，页面缺失/未下发 → None，由关注 400 回退兜底）。
/// 仅登录态首页含 gaUserData/preloaded state；匿名页两字段均缺席。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecaptchaPageConfig {
    pub site_key: Option<String>,
    pub follow_requires_token: Option<bool>,
}

impl RecaptchaPageConfig {
    /// 是否具备铸造条件（site key 已解析）。flag 只影响「铸 token 是否前置」，
    /// 不参与此判定。
    pub fn mintable(&self) -> bool {
        self.site_key.as_deref().is_some_and(|k| !k.is_empty())
    }
}

/// 从主站首页 HTML 解析关注人机验证配置（纯函数，离线可测）。
pub(crate) fn parse_recaptcha_page_config(html: &str) -> RecaptchaPageConfig {
    let Some(state) = parse_next_data_preloaded_state(html) else {
        return RecaptchaPageConfig::default();
    };
    let site_key = state
        .pointer("/misc/grecaptcha/recaptchaEnterpriseScoreSiteKey")
        .and_then(Value::as_str)
        .filter(|k| !k.is_empty())
        .map(String::from);
    let follow_requires_token = state
        .pointer("/test/ab/recaptcha_follow_user")
        .and_then(Value::as_bool);
    RecaptchaPageConfig {
        site_key,
        follow_requires_token,
    }
}

/// 主站首页一次抓取得到的会话上下文：csrf token + 关注人机验证配置。
#[derive(Debug, Clone)]
pub struct WebSessionContext {
    pub csrf_token: String,
    pub recaptcha: RecaptchaPageConfig,
}

/// GET 主站首页，解析 `__NEXT_DATA__` 取会话 csrf token 与人机验证配置。
/// 走 PixivClient 的闸门/限速/重试（与 ajax 请求同一套风控语义）。
/// 路径缺失/非 JSON → `Client("无法获取会话令牌，请重新登录")`。
/// 注意：token 值不得写入日志。
pub(crate) async fn fetch_web_session_context(
    client: &PixivClient,
) -> Result<WebSessionContext, PixivError> {
    let html = client
        .run_gated(|| async {
            let resp = client.send_ajax(super::client::BASE_URL).await?;
            let text = resp.text().await.map_err(|e| {
                PixivError::Network(format!("{e}: {}", mask_url(super::client::BASE_URL)))
            })?;
            Ok::<String, PixivError>(text)
        })
        .await?;
    log::debug!("已抓取主站 __NEXT_DATA__（{} 字节）", html.len());
    let csrf_token = parse_next_data_token(&html)
        .ok_or_else(|| PixivError::Client("无法获取会话令牌，请重新登录".into()))?;
    let recaptcha = parse_recaptcha_page_config(&html);
    Ok(WebSessionContext {
        csrf_token,
        recaptcha,
    })
}

// ----------------------------------------------------------------------
// 单元测试（全部离线）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_plain_value() {
        assert_eq!(
            normalize_phpsessid("12345_deadbeef").unwrap(),
            "12345_deadbeef"
        );
        assert_eq!(
            normalize_phpsessid("  12345_x  ").unwrap(),
            "12345_x",
            "strip 两侧空白"
        );
    }

    #[test]
    fn normalize_accepts_prefix_and_cookie_header_forms() {
        assert_eq!(normalize_phpsessid("PHPSESSID=12345_x").unwrap(), "12345_x");
        assert_eq!(
            normalize_phpsessid("a=1; PHPSESSID=12345_x; b=2").unwrap(),
            "12345_x",
            "完整 Cookie 请求头取 PHPSESSID 对"
        );
    }

    #[test]
    fn normalize_rejects_empty() {
        assert_eq!(normalize_phpsessid("").unwrap_err(), "PHPSESSID 不能为空");
        assert_eq!(
            normalize_phpsessid("   ").unwrap_err(),
            "PHPSESSID 不能为空"
        );
        assert_eq!(
            normalize_phpsessid("PHPSESSID=").unwrap_err(),
            "PHPSESSID 不能为空",
            "前缀样式但值为空"
        );
    }

    #[test]
    fn normalize_rejects_too_long() {
        let long = "a".repeat(4097);
        assert_eq!(
            normalize_phpsessid(&long).unwrap_err(),
            "PHPSESSID 格式不正确"
        );
        // 4096 恰好放行
        assert!(normalize_phpsessid(&"a".repeat(4096)).is_ok());
    }

    #[test]
    fn normalize_rejects_control_and_separators() {
        for bad in ["123\r45", "123\n45", "123;45", "123\x0045"] {
            assert_eq!(
                normalize_phpsessid(bad).unwrap_err(),
                "PHPSESSID 格式不正确",
                "非法字符: {bad:?}"
            );
        }
        // cookie 头样式：非法字符检查针对提取出的值（原始串的分号不误伤）
        assert!(normalize_phpsessid("a=1; PHPSESSID=ok_value").is_ok());
    }

    #[test]
    fn parse_self_success() {
        let body = serde_json::json!({
            "token": "csrf-tok",
            "userData": {
                "id": "28640",
                "pixivId": "pixiv_id_x",
                "name": "用户名",
                "profileImg": "",
                "profileImgBig": "https://i.pximg.net/user-profile-big/img.png"
            }
        });
        let probe = parse_self_response(200, Some(body)).unwrap();
        assert!(probe.is_logged_in);
        assert_eq!(probe.csrf_token, "csrf-tok");
        let user = probe.user.unwrap();
        assert_eq!(user.user_id, "28640");
        assert_eq!(user.pixiv_id, "pixiv_id_x");
        assert_eq!(user.name, "用户名");
        assert_eq!(
            user.profile_img, "https://i.pximg.net/user-profile-big/img.png",
            "profileImg 空则兜底 profileImgBig"
        );
    }

    #[test]
    fn parse_self_anonymous_is_invalid() {
        // 匿名 self：200 + token 但 userData 缺 id → Invalid
        let body = serde_json::json!({"token": "t", "userData": {"name": "_guest_"}});
        match parse_self_response(200, Some(body)) {
            Err(ProbeError::Invalid(msg)) => assert_eq!(msg, "PHPSESSID 无效或已过期"),
            other => panic!("预期 Invalid，实际 {other:?}"),
        }
        // userData 缺失 / 非对象
        for body in [
            serde_json::json!({"token": "t"}),
            serde_json::json!({"token": "t", "userData": "x"}),
            serde_json::json!({"token": "t", "userData": {}}),
        ] {
            assert!(matches!(
                parse_self_response(200, Some(body)),
                Err(ProbeError::Invalid(_))
            ));
        }
    }

    #[test]
    fn parse_self_status_classification() {
        // 401/403 → Invalid（固定文案）
        for status in [401u16, 403] {
            match parse_self_response(status, Some(serde_json::json!({}))) {
                Err(ProbeError::Invalid(msg)) => assert_eq!(msg, "PHPSESSID 无效或已过期"),
                other => panic!("预期 Invalid，实际 {other:?}"),
            }
        }
        // 其他非 200 → Invalid（带状态码）
        match parse_self_response(500, None) {
            Err(ProbeError::Invalid(msg)) => {
                assert_eq!(msg, "Pixiv 登录态校验失败：HTTP 500")
            }
            other => panic!("预期 Invalid，实际 {other:?}"),
        }
    }

    #[test]
    fn parse_self_bad_json_and_missing_token() {
        // 200 但非 JSON → Csrf
        match parse_self_response(200, None) {
            Err(ProbeError::Csrf(msg)) => assert_eq!(msg, "Pixiv 登录态响应不是有效 JSON"),
            other => panic!("预期 Csrf，实际 {other:?}"),
        }
        // userData 有效但缺 token → Csrf
        let body = serde_json::json!({"userData": {"id": "1", "name": "n"}});
        match parse_self_response(200, Some(body)) {
            Err(ProbeError::Csrf(msg)) => {
                assert_eq!(msg, "登录态有效，但 Pixiv 响应缺少 csrf token")
            }
            other => panic!("预期 Csrf，实际 {other:?}"),
        }
        // 空 token 同样拒绝
        let body = serde_json::json!({"userData": {"id": "1"}, "token": ""});
        assert!(matches!(
            parse_self_response(200, Some(body)),
            Err(ProbeError::Csrf(_))
        ));
    }

    fn next_data_html(token: &str) -> String {
        format!(
            r#"<!DOCTYPE html><html><body><script>other</script><script type="application/json" id="__NEXT_DATA__">{{"props":{{"pageProps":{{"serverSerializedPreloadedState":{{"api":{{"token":"{token}"}}}}}}}}}}</script></body></html>"#
        )
    }

    #[test]
    fn parse_next_data_token_success() {
        // 属性顺序在前（id 在 type 前）同样能定位
        let html = next_data_html("abc123def");
        assert_eq!(parse_next_data_token(&html).as_deref(), Some("abc123def"));
    }

    #[test]
    fn parse_next_data_token_string_wrapped_state() {
        // 2026-10-01 登录态实测：serverSerializedPreloadedState 是 JSON 字符串，需再 parse 一次
        let inner = serde_json::json!({"api": {"token": "ed56bce3"}, "ads": {}});
        let html = format!(
            r#"<script id="__NEXT_DATA__">{{"props":{{"pageProps":{{"serverSerializedPreloadedState":{}}}}}}}</script>"#,
            serde_json::to_string(&serde_json::to_string(&inner).unwrap()).unwrap()
        );
        assert_eq!(parse_next_data_token(&html).as_deref(), Some("ed56bce3"));
    }

    #[test]
    fn parse_next_data_token_missing_or_broken() {
        // 无 __NEXT_DATA__ script
        assert_eq!(parse_next_data_token("<html><body>403</body></html>"), None);
        // script 内不是 JSON
        let html = r#"<script id="__NEXT_DATA__">not json</script>"#;
        assert_eq!(parse_next_data_token(html), None);
        // JSON 里缺 api.token 路径
        let html = r#"<script id="__NEXT_DATA__">{"props":{"pageProps":{}}}</script>"#;
        assert_eq!(parse_next_data_token(html), None);
        // token 为空串视为缺失
        assert_eq!(parse_next_data_token(&next_data_html("")), None);
        // 内层字符串不是合法 JSON
        let html = r#"<script id="__NEXT_DATA__">{"props":{"pageProps":{"serverSerializedPreloadedState":"not json"}}}</script>"#;
        assert_eq!(parse_next_data_token(html), None);
    }

    /// 合成登录态首页 preloaded state（含 recaptcha 配置）的 __NEXT_DATA__ HTML。
    fn recaptcha_next_data_html(state: &serde_json::Value) -> String {
        let wrapped = serde_json::to_string(&serde_json::to_string(state).unwrap()).unwrap();
        format!(
            r#"<script id="__NEXT_DATA__">{{"props":{{"pageProps":{{"serverSerializedPreloadedState":{wrapped}}}}}}}</script>"#
        )
    }

    #[test]
    fn parse_recaptcha_page_config_full_and_partial() {
        // 完整形态：site key + flag（路径对齐 ppixiv 读取实现）
        let state = serde_json::json!({
            "api": {"token": "t"},
            "misc": {"grecaptcha": {"recaptchaEnterpriseScoreSiteKey": "6LcKEY"}},
            "test": {"ab": {"recaptcha_follow_user": true}}
        });
        let config = parse_recaptcha_page_config(&recaptcha_next_data_html(&state));
        assert_eq!(config.site_key.as_deref(), Some("6LcKEY"));
        assert_eq!(config.follow_requires_token, Some(true));
        assert!(config.mintable());

        // flag=false：不可铸造前置，但 site key 仍在（400 回退兜底可用）
        let state = serde_json::json!({
            "misc": {"grecaptcha": {"recaptchaEnterpriseScoreSiteKey": "6LcKEY"}},
            "test": {"ab": {"recaptcha_follow_user": false}}
        });
        let config = parse_recaptcha_page_config(&recaptcha_next_data_html(&state));
        assert_eq!(config.site_key.as_deref(), Some("6LcKEY"));
        assert_eq!(config.follow_requires_token, Some(false));
        assert!(config.mintable());
    }

    #[test]
    fn parse_recaptcha_page_config_missing_fields_is_none_not_error() {
        // 匿名页 / 页面变体：缺 preloaded state 或缺 recaptcha 路径 → 字段缺席
        assert_eq!(
            parse_recaptcha_page_config("<html><body>login</body></html>"),
            RecaptchaPageConfig::default()
        );
        let state = serde_json::json!({"api": {"token": "t"}});
        let config = parse_recaptcha_page_config(&recaptcha_next_data_html(&state));
        assert_eq!(config, RecaptchaPageConfig::default());
        assert!(!config.mintable());

        // 空 key 视为缺失
        let state = serde_json::json!({
            "misc": {"grecaptcha": {"recaptchaEnterpriseScoreSiteKey": ""}}
        });
        let config = parse_recaptcha_page_config(&recaptcha_next_data_html(&state));
        assert_eq!(config.site_key, None);
        assert!(!config.mintable());
    }
}
