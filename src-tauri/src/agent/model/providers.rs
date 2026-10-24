//! 三协议 provider 装配。
//!
//! 端点推导、URL 白名单、协议字段归一化与 opencode 会话头判定沿用
//! `translation.rs` 的既有语义（阶段 1 以本模块为迁移目标；旧实现暂不改动，
//! 后续单元把旧函数切到这里的公开实现，避免长期维护两套规则）。
//!
//! 本模块只负责「配置 → Rig model」和「配置 → CompletionRequest 补丁」，
//! 不持有全局状态，可直接在离线测试中装配。

use std::{sync::Arc, time::Duration};

use rig_core::{
    completion::CompletionRequest,
    driver::Model,
    providers::{
        anthropic::{AnthropicConfig, Messages},
        openai::{OpenAIConfig, responses_api::wire::Responses, wire::Chat},
    },
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use crate::agent::model::transport::WreqTransport;

/// 支持的三种协议；`ChatCompletions` 为默认（OpenAI 兼容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiFormat {
    /// OpenAI 兼容 `/chat/completions`。
    ChatCompletions,
    /// OpenAI `/responses`。
    Responses,
    /// Anthropic `/messages`。
    Anthropic,
}

impl ApiFormat {
    /// 当前协议的端点路径后缀。
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::ChatCompletions => "/chat/completions",
            Self::Responses => "/responses",
            Self::Anthropic => "/messages",
        }
    }
}

/// 设置值 → 协议；空串按默认处理（兼容旧配置与旧前端草稿）。
pub fn resolve_api_format(value: &str) -> Result<ApiFormat, String> {
    match value.trim() {
        "" | "chat_completions" => Ok(ApiFormat::ChatCompletions),
        "responses" => Ok(ApiFormat::Responses),
        "anthropic" => Ok(ApiFormat::Anthropic),
        _ => Err("翻译接口协议无效，请在设置中选择".into()),
    }
}

/// Anthropic Messages API 要求的版本头（当前稳定值）。
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// 会话标识请求头：opencode zen 系网关（Go 档）要求每个会话一个稳定 ID。
pub const OPENCODE_SESSION_HEADER: &str = "x-opencode-session";

/// 默认思考档：三协议默认 medium；用户显式给出的思考字段优先。
pub const DEFAULT_REASONING_EFFORT: &str = "medium";

/// URL 可以是 API 基址或当前协议的完整端点；HTTP 仅允许本机模型。
///
/// 规则与 `translation.rs::completion_url` 保持一致：先剥掉已知端点后缀，
/// 再按当前协议拼接；基址路径为空时补 `/v1`（与旧实现完全相同）。
pub fn completion_endpoint(value: &str, format: ApiFormat) -> Result<tauri::Url, String> {
    let mut url = tauri::Url::parse(value.trim()).map_err(|_| "翻译 API URL 无效")?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || url.scheme() == "http" && local)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("翻译 URL 须为 HTTPS（本机可用 HTTP），不能包含凭据、查询参数或片段".into());
    }
    let mut path = url.path().trim_end_matches('/').to_string();
    if !path.ends_with(format.endpoint()) {
        // 允许粘贴任意协议的完整端点：先剥掉已知端点后缀，再按当前协议拼接。
        for endpoint in [
            ApiFormat::ChatCompletions.endpoint(),
            ApiFormat::Responses.endpoint(),
            ApiFormat::Anthropic.endpoint(),
        ] {
            if path.ends_with(endpoint) {
                path.truncate(path.len() - endpoint.len());
                break;
            }
        }
        path = if path.is_empty() {
            format!("/v1{}", format.endpoint())
        } else {
            format!("{path}{}", format.endpoint())
        };
    }
    url.set_path(&path);
    Ok(url)
}

/// 会话标识头只发给 opencode zen 系网关（Go 档缺它即 400），其它供应商不加无用头。
pub fn needs_session_header(url: &tauri::Url) -> bool {
    url.host_str()
        .is_some_and(|host| host.to_ascii_lowercase().contains("opencode"))
}

/// 稳定会话 ID（UUID 形态）：SHA-256 前 16 字节转 8-4-4-4-12 十六进制。
/// 同一 seed 恒等，跨进程与重启不变——网关的 prompt 缓存与路由依赖这种稳定性。
pub fn session_id(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let hex: String = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// 仅当端点 host 含 `opencode` 时返回会话头；其它 host 返回 `None`。
pub fn session_header_for(url: &tauri::Url, session: &str) -> Option<(String, String)> {
    needs_session_header(url).then(|| (OPENCODE_SESSION_HEADER.to_string(), session.to_string()))
}

/// 高级 JSON 的保留字段校验（与 `translation.rs::validate_options` 同规则）。
pub fn validate_extra(value: &Value, format: ApiFormat) -> Result<(), String> {
    let object = value.as_object().ok_or("翻译高级 JSON 必须是对象")?;
    const RESERVED: &[&str] = &[
        "model",
        "messages",
        "stream",
        "n",
        "tools",
        "tool_choice",
        "functions",
        "function_call",
        "response_format",
        "modalities",
        "audio",
        "store",
        "api_key",
        "authorization",
        "headers",
    ];
    let own: &[&str] = match format {
        ApiFormat::ChatCompletions => &[],
        ApiFormat::Responses => &["input", "instructions"],
        ApiFormat::Anthropic => &["system"],
    };
    if object.keys().any(|key| {
        let key = key.to_ascii_lowercase();
        RESERVED.contains(&key.as_str()) || own.contains(&key.as_str())
    }) {
        return Err(
            "高级 JSON 不能覆盖模型、消息、系统提示、输出格式、流式、工具或凭据字段".into(),
        );
    }
    if value.to_string().len() > 16_384 {
        return Err("翻译高级 JSON 不能超过 16KB".into());
    }
    Ok(())
}

/// 按输入字符数分档的输出上限（token），ADR 0033。
pub fn dynamic_output_cap(input_chars: usize) -> u64 {
    match input_chars {
        0..=6_000 => 16_384,
        6_001..=24_000 => 65_536,
        _ => 262_144,
    }
}

/// 按协议装配扩展字段（ADR 0032 / 0033）：
/// 1. 改写「Chat Completions 习惯字段」，让跨协议切换后语义一致；
/// 2. 补默认值——输出上限按输入分档、思考默认 medium；用户显式给出的同义字段
///    （任一协议形状）永远优先，这里只补缺。
pub fn normalize_extra(body: &mut Map<String, Value>, format: ApiFormat, chars: usize) {
    match format {
        ApiFormat::ChatCompletions => {
            if let Some(max) = body
                .remove("max_tokens")
                .or_else(|| body.remove("max_output_tokens"))
            {
                body.entry("max_completion_tokens").or_insert(max);
            }
            body.entry("max_completion_tokens".to_string())
                .or_insert_with(|| json!(dynamic_output_cap(chars)));
            if !body.contains_key("reasoning_effort")
                && !body.contains_key("thinking")
                && !body.contains_key("reasoning")
            {
                body.insert("reasoning_effort".into(), json!(DEFAULT_REASONING_EFFORT));
            }
        }
        ApiFormat::Responses => {
            body.remove("stream_options");
            if let Some(max) = body
                .remove("max_completion_tokens")
                .or_else(|| body.remove("max_tokens"))
            {
                body.entry("max_output_tokens").or_insert(max);
            }
            body.entry("max_output_tokens".to_string())
                .or_insert_with(|| json!(dynamic_output_cap(chars)));
            let effort = body
                .remove("reasoning_effort")
                .unwrap_or_else(|| json!(DEFAULT_REASONING_EFFORT));
            match body.get_mut("reasoning") {
                Some(Value::Object(reasoning)) => {
                    reasoning.entry("effort").or_insert(effort);
                    if reasoning
                        .get("effort")
                        .and_then(Value::as_str)
                        .is_some_and(|effort| effort != "none")
                    {
                        reasoning.entry("summary").or_insert(json!("auto"));
                    }
                }
                Some(_) => {}
                None => {
                    let mut reasoning = Map::new();
                    if effort.as_str().is_some_and(|effort| effort != "none") {
                        reasoning.insert("summary".into(), json!("auto"));
                    }
                    reasoning.insert("effort".into(), effort);
                    body.insert("reasoning".into(), Value::Object(reasoning));
                }
            }
        }
        ApiFormat::Anthropic => {
            body.remove("stream_options");
            if let Some(max) = body
                .remove("max_completion_tokens")
                .or_else(|| body.remove("max_output_tokens"))
            {
                body.entry("max_tokens").or_insert(max);
            }
            body.entry("max_tokens".to_string())
                .or_insert_with(|| json!(dynamic_output_cap(chars)));
            let reasoning = body.remove("reasoning");
            let effort = body.remove("reasoning_effort").or_else(|| {
                reasoning
                    .as_ref()
                    .and_then(|reasoning| reasoning.get("effort"))
                    .cloned()
            });
            if body.contains_key("thinking") {
                pin_anthropic_sampling(body);
                return;
            }
            let level = effort
                .as_ref()
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_REASONING_EFFORT);
            if matches!(level, "none" | "disabled" | "minimal") {
                return;
            }
            let max = body
                .get("max_tokens")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| dynamic_output_cap(chars));
            if max <= 1_024 {
                return;
            }
            let budget = match level {
                "low" => 2_048,
                "medium" => 4_096,
                _ => 8_192,
            }
            .min(max - 1_024)
            .max(1_024);
            body.insert(
                "thinking".into(),
                json!({"type": "enabled", "budget_tokens": budget}),
            );
            pin_anthropic_sampling(body);
        }
    }
}

/// Anthropic 开启思考时的采样约束：`temperature` 只能是 1，`top_p` 必须 ≥0.95
/// 或不传，否则 Messages API 直接 400（ADR 0032）。
fn pin_anthropic_sampling(body: &mut Map<String, Value>) {
    if body.contains_key("temperature") {
        body.insert("temperature".into(), json!(1));
    }
    let keep_top_p = body
        .get("top_p")
        .and_then(Value::as_f64)
        .is_some_and(|top_p| top_p >= 0.95);
    if !keep_top_p {
        body.remove("top_p");
    }
}

/// 一次请求的 Rig 侧补丁：typed 输出上限 + 其余协议扩展字段。
#[derive(Clone, Debug, PartialEq)]
pub struct RequestDefaults {
    /// 走 Rig typed 字段的输出上限（Responses `max_output_tokens` /
    /// Anthropic `max_tokens`）。Chat 的输出上限在 Rig 里只能拼成
    /// `max_tokens`，与旧行为 `max_completion_tokens` 不同，因此走
    /// `additional_params`，这里为 `None`。
    pub max_tokens: Option<u64>,
    /// 合并后的 `additional_params`；已剔除会与 typed 字段重复的输出上限。
    pub additional_params: Value,
}

/// 用户配置原样（含高级 JSON）→ Rig 请求补丁。
///
/// 优先级：内置默认 < 高级 JSON 显式字段；`input_chars` 只影响内置默认分档。
pub fn request_defaults(config: &ProviderConfig, input_chars: usize) -> RequestDefaults {
    let mut body = config.extra_json.as_object().cloned().unwrap_or_default();
    normalize_extra(&mut body, config.format, input_chars);
    let max_tokens = match config.format {
        // Chat 保留 max_completion_tokens 原字段名，避免 Rig 改写成 max_tokens。
        ApiFormat::ChatCompletions => None,
        ApiFormat::Responses => take_output_cap(&mut body, "max_output_tokens"),
        ApiFormat::Anthropic => take_output_cap(&mut body, "max_tokens"),
    };
    RequestDefaults {
        max_tokens,
        additional_params: Value::Object(body),
    }
}

fn take_output_cap(body: &mut Map<String, Value>, key: &str) -> Option<u64> {
    let cap = body.get(key).and_then(Value::as_u64)?;
    body.remove(key);
    Some(cap)
}

/// 把 [`request_defaults`] 合并进 Rig 的 [`CompletionRequest`]。
///
/// - Responses / Anthropic：输出上限走 `request.max_tokens`（Rig 会编码成
///   协议自己的字段名）；
/// - Chat：输出上限放进 `additional_params.max_completion_tokens`；
/// - 运行时已有 `additional_params`（Agent / 调用方显式值）覆盖配置默认值。
pub fn apply_request_defaults(
    config: &ProviderConfig,
    input_chars: usize,
    request: &mut CompletionRequest,
) {
    let mut defaults = request_defaults(config, input_chars);
    let runtime_cap = request.max_tokens.take();
    match config.format {
        ApiFormat::ChatCompletions => {
            if let Some(cap) = runtime_cap {
                if let Some(body) = defaults.additional_params.as_object_mut() {
                    body.insert("max_completion_tokens".into(), json!(cap));
                }
            }
            request.max_tokens = None;
        }
        ApiFormat::Responses | ApiFormat::Anthropic => {
            request.max_tokens = runtime_cap.or(defaults.max_tokens);
        }
    }
    let mut merged = defaults
        .additional_params
        .as_object()
        .cloned()
        .unwrap_or_default();
    if let Some(Value::Object(existing)) = request.additional_params.take() {
        for (key, value) in existing {
            merged.insert(key, value);
        }
    }
    request.additional_params = Some(Value::Object(merged));
}

/// 一次模型请求所需的完整配置。
///
/// `endpoint` 是 [`completion_endpoint`] 的推导结果；`session_header` 已经过
/// host 判定（非 opencode 的 `Some` 会被丢弃）。
#[derive(Clone)]
pub struct ProviderConfig {
    pub format: ApiFormat,
    /// 用户原样输入的基址 / 完整端点。
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    /// 高级 JSON（不含应用保留字段）。
    pub extra_json: Value,
    pub timeout_seconds: u64,
    /// 最终端点；Rig 追加的路径会被传输层整体替换。
    pub endpoint: tauri::Url,
    /// 需要附加的静态会话头（name, value）；仅 opencode host 生效。
    pub session_header: Option<(String, String)>,
    /// 全局模型请求闸门。
    pub gate: Option<Arc<Semaphore>>,
}

impl ProviderConfig {
    /// 校验 URL / 高级 JSON 并推导端点。`timeout_seconds` 由设置层保证 > 0。
    pub fn new(
        format: ApiFormat,
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: impl Into<String>,
        extra_json: Value,
        timeout_seconds: u64,
    ) -> Result<Self, String> {
        let base_url = base_url.into();
        if base_url.trim().is_empty() {
            return Err("翻译 API URL 无效".into());
        }
        if timeout_seconds == 0 {
            return Err("翻译超时时间必须大于 0".into());
        }
        let model = model.into();
        if model.len() > 256 || model.chars().any(char::is_control) {
            return Err("翻译模型 ID 无效".into());
        }
        let endpoint = completion_endpoint(&base_url, format)?;
        validate_extra(&extra_json, format)?;
        Ok(Self {
            format,
            base_url,
            model,
            api_key: api_key.into(),
            extra_json,
            timeout_seconds,
            endpoint,
            session_header: None,
            gate: None,
        })
    }

    /// 设置会话头；host 不含 `opencode` 时自动忽略。
    pub fn with_session_header(mut self, header: Option<(String, String)>) -> Self {
        self.session_header = header.filter(|_| needs_session_header(&self.endpoint));
        self
    }

    /// 注入全局请求闸门。
    pub fn with_gate(mut self, gate: Option<Arc<Semaphore>>) -> Self {
        self.gate = gate;
        self
    }

    /// 构造 wreq 传输：超时沿用配置，禁重定向，叠加会话头与闸门。
    pub fn transport(&self) -> Result<WreqTransport, String> {
        let client = wreq::Client::builder()
            .timeout(Duration::from_secs(self.timeout_seconds))
            .redirect(wreq::redirect::Policy::none())
            .build()
            .map_err(|_| "无法初始化翻译客户端")?;
        let mut transport =
            WreqTransport::new(client, self.endpoint.clone()).with_gate(self.gate.clone());
        if let Some((name, value)) = &self.session_header {
            let name = http::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| "opencode 会话头名称无效")?;
            let value = http::header::HeaderValue::from_str(value)
                .map_err(|_| "opencode 会话头内容无效")?;
            transport = transport.with_header(name, value);
        }
        Ok(transport)
    }

    /// Chat Completions 模型（OpenAI 兼容）。
    pub fn chat_model(&self) -> Result<Model<Chat>, String> {
        Ok(OpenAIConfig::new(self.api_key.clone())
            .with_base_url(self.base_url.clone())
            .connect(self.transport()?)
            .chat(self.model.clone()))
    }

    /// Responses 模型。
    pub fn responses_model(&self) -> Result<Model<Responses>, String> {
        Ok(OpenAIConfig::new(self.api_key.clone())
            .with_base_url(self.base_url.clone())
            .connect(self.transport()?)
            .responses(self.model.clone()))
    }

    /// Anthropic Messages 模型。
    pub fn anthropic_model(&self) -> Result<Model<Messages>, String> {
        Ok(AnthropicConfig::new(self.api_key.clone())
            .with_base_url(self.base_url.trim())
            .connect(self.transport()?)
            .completion(self.model.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(format: ApiFormat, base: &str, extra: Value) -> ProviderConfig {
        ProviderConfig::new(format, base, "mock-model", "secret-key", extra, 600).unwrap()
    }

    #[test]
    fn endpoint_derivation_matches_existing_translation_rules() {
        use ApiFormat::*;
        assert_eq!(
            completion_endpoint("https://example.com/v1/", ChatCompletions)
                .unwrap()
                .as_str(),
            "https://example.com/v1/chat/completions"
        );
        assert_eq!(
            completion_endpoint("http://localhost:1234/v1/chat/completions", ChatCompletions)
                .unwrap()
                .as_str(),
            "http://localhost:1234/v1/chat/completions"
        );
        assert_eq!(
            completion_endpoint("https://api.anthropic.com/v1", Anthropic)
                .unwrap()
                .as_str(),
            "https://api.anthropic.com/v1/messages"
        );
        // 自定义网关直接给完整 /messages 端点时保持原路径，不再叠 /v1。
        assert_eq!(
            completion_endpoint("http://127.0.0.1:3425/messages", Anthropic)
                .unwrap()
                .as_str(),
            "http://127.0.0.1:3425/messages"
        );
        assert_eq!(
            completion_endpoint("https://api.anthropic.com/v1/messages", ChatCompletions)
                .unwrap()
                .as_str(),
            "https://api.anthropic.com/v1/chat/completions"
        );
        for url in [
            "http://example.com/v1",
            "https://user:key@example.com/v1",
            "https://example.com/v1?key=secret",
            "https://example.com/v1#fragment",
        ] {
            assert!(completion_endpoint(url, ChatCompletions).is_err());
        }
    }

    #[test]
    fn chat_extra_keeps_user_fields_and_adds_defaults() {
        let config = config(
            ApiFormat::ChatCompletions,
            "https://example.com/v1",
            json!({"max_tokens": 123, "temperature": 0.2, "custom": true}),
        );
        let defaults = request_defaults(&config, 1_000);
        assert_eq!(defaults.max_tokens, None);
        assert_eq!(defaults.additional_params["max_completion_tokens"], 123);
        assert_eq!(defaults.additional_params["reasoning_effort"], "medium");
        assert_eq!(defaults.additional_params["temperature"], 0.2);
        assert_eq!(defaults.additional_params["custom"], true);
        assert!(defaults.additional_params.get("max_tokens").is_none());
    }

    #[test]
    fn empty_extra_keeps_adr_0033_defaults_per_format() {
        assert_eq!(dynamic_output_cap(6_000), 16_384);
        assert_eq!(dynamic_output_cap(6_001), 65_536);
        assert_eq!(dynamic_output_cap(24_001), 262_144);

        let chat = request_defaults(
            &config(
                ApiFormat::ChatCompletions,
                "https://example.com/v1",
                json!({}),
            ),
            10_000,
        );
        assert_eq!(chat.max_tokens, None);
        assert_eq!(chat.additional_params["max_completion_tokens"], 65_536);
        assert_eq!(chat.additional_params["reasoning_effort"], "medium");

        let responses = request_defaults(
            &config(ApiFormat::Responses, "https://example.com/v1", json!({})),
            1_000,
        );
        assert_eq!(responses.max_tokens, Some(16_384));
        assert_eq!(responses.additional_params["reasoning"]["effort"], "medium");
        assert_eq!(responses.additional_params["reasoning"]["summary"], "auto");

        let anthropic = request_defaults(
            &config(
                ApiFormat::Anthropic,
                "https://api.anthropic.com/v1",
                json!({}),
            ),
            30_000,
        );
        assert_eq!(anthropic.max_tokens, Some(262_144));
        assert_eq!(
            anthropic.additional_params["thinking"],
            json!({"type": "enabled", "budget_tokens": 4_096})
        );
    }

    #[test]
    fn responses_extra_moves_cap_and_reasoning() {
        let config = config(
            ApiFormat::Responses,
            "https://example.com/v1",
            json!({"max_completion_tokens": 999, "reasoning_effort": "low"}),
        );
        let defaults = request_defaults(&config, 1_000);
        assert_eq!(defaults.max_tokens, Some(999));
        assert_eq!(defaults.additional_params["reasoning"]["effort"], "low");
        assert_eq!(defaults.additional_params["reasoning"]["summary"], "auto");
        assert!(
            defaults
                .additional_params
                .get("max_output_tokens")
                .is_none()
        );
    }

    #[test]
    fn anthropic_explicit_thinking_wins_and_pins_sampling() {
        let config = config(
            ApiFormat::Anthropic,
            "https://api.anthropic.com/v1",
            json!({
                "thinking": {"type": "enabled", "budget_tokens": 2048},
                "temperature": 0.3,
                "top_p": 0.5
            }),
        );
        let defaults = request_defaults(&config, 1_000);
        assert!(defaults.max_tokens.unwrap() > 1_024);
        assert_eq!(
            defaults.additional_params["thinking"],
            json!({"type": "enabled", "budget_tokens": 2048})
        );
        assert_eq!(defaults.additional_params["temperature"], 1);
        assert!(defaults.additional_params.get("top_p").is_none());
    }

    #[test]
    fn runtime_additional_params_win_over_config_defaults() {
        let config = config(
            ApiFormat::Responses,
            "https://example.com/v1",
            json!({"reasoning_effort": "low", "max_output_tokens": 999}),
        );
        let mut request = CompletionRequest::new("hi");
        request.max_tokens = Some(2222);
        request.additional_params = Some(json!({"reasoning": {"effort": "high"}}));
        apply_request_defaults(&config, 1_000, &mut request);
        assert_eq!(request.max_tokens, Some(2222));
        assert_eq!(
            request.additional_params.as_ref().unwrap()["reasoning"]["effort"],
            "high"
        );
    }

    #[test]
    fn session_header_only_for_opencode_hosts() {
        let opencode = tauri::Url::parse("https://opencode.ai/v1").unwrap();
        let local = tauri::Url::parse("http://127.0.0.1:3425/v1").unwrap();
        assert!(session_header_for(&opencode, "abc").is_some());
        assert!(session_header_for(&local, "abc").is_none());
        let config = ProviderConfig::new(
            ApiFormat::ChatCompletions,
            "http://127.0.0.1:3425/v1",
            "m",
            "k",
            json!({}),
            600,
        )
        .unwrap()
        .with_session_header(Some((OPENCODE_SESSION_HEADER.into(), "abc".into())));
        assert!(config.session_header.is_none());
    }

    #[test]
    fn validates_reserved_extra_fields() {
        assert!(validate_extra(&json!({"tools": []}), ApiFormat::ChatCompletions).is_err());
        assert!(validate_extra(&json!({"instructions": "x"}), ApiFormat::Responses).is_err());
        assert!(validate_extra(&json!({"system": "x"}), ApiFormat::Anthropic).is_err());
        assert!(validate_extra(&json!({"max_tokens": 10}), ApiFormat::Anthropic).is_ok());
    }
}
