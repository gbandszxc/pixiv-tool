//! 仅记录服务错误的诊断字段；不记录请求、小说或模型生成正文。
use serde_json::{Value, json};
use std::sync::LazyLock;

pub(crate) fn redact(text: &str, key: &str) -> String {
    static CREDENTIAL: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
        r#"(?i)(?:authorization|cookie|set-cookie)["']?\s*[=:]\s*[^\r\n]+|(?:bearer\s+|(?:api[_-]?key|x-api-key|access[_-]?token|refresh[_-]?token|password|PHPSESSID)["']?\s*[=:]\s*["']?)[^\s,;"'\}\]]+|sk-[A-Za-z0-9_-]+"#
    ).expect("固定凭据脱敏正则")
    });
    static URL: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r#"https?://[^\s"'<>]+"#).expect("固定 URL 正则"));
    let text = if key.is_empty() {
        text.to_owned()
    } else {
        text.replace(key, "[REDACTED]")
    };
    let text = CREDENTIAL.replace_all(&text, "[REDACTED]");
    let text = URL.replace_all(&text, "[URL]");
    text.chars()
        .take(4096)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

pub(crate) fn service_error(error: &Value, key: &str) -> String {
    let mut details = json!({});
    if let Some(message) = error.as_str() {
        details["message"] = json!(redact(message, key));
    } else {
        for field in ["code", "type", "message", "param", "request_id"] {
            if let Some(value) = error.get(field) {
                if let Some(text) = value.as_str() {
                    details[field] = json!(redact(text, key));
                } else if value.is_number() || value.is_boolean() {
                    details[field] = value.clone();
                }
            }
        }
    }
    details.to_string()
}

/// 错误响应有界读取；非 JSON 正文可能含代理页面/请求回显，不落日志。
pub(crate) async fn log_http_error(response: wreq::Response, key: &str) {
    use futures_util::StreamExt;
    let status = response.status().as_u16();
    let mut stream = std::pin::pin!(response.bytes_stream());
    let mut raw = Vec::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            log::warn!("翻译服务 HTTP {status}：错误正文读取失败");
            return;
        };
        if raw.len() + chunk.len() > 64 * 1024 {
            log::warn!("翻译服务 HTTP {status}：错误正文超过 64KiB");
            return;
        }
        raw.extend_from_slice(&chunk);
    }
    match serde_json::from_slice::<Value>(&raw) {
        Ok(body) => log::warn!(
            "翻译服务 HTTP {status}：{}",
            service_error(body.get("error").unwrap_or(&body), key)
        ),
        Err(_) => log::warn!(
            "翻译服务 HTTP {status}：错误正文不是 JSON（{} 字节）",
            raw.len()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_diagnostics_without_credentials_or_generated_content() {
        let details = service_error(
            &json!({"code":"1301","message":"审核失败 secret-key\nCookie: PHPSESSID=private; another=secret","api_key":"private","output":"novel"}),
            "secret-key",
        );
        assert!(details.contains("1301") && details.contains("审核失败"));
        for secret in ["secret-key", "private", "another", "novel"] {
            assert!(!details.contains(secret));
        }
        let text = redact(
            "https://user:pass@example.com/?token=secret api_key=private Bearer test-token sk-test-secret",
            "",
        );
        for secret in ["pass", "secret", "private", "test-token"] {
            assert!(!text.contains(secret));
        }
        assert!(!redact(r#"{"api_key":"other-secret"}"#, "").contains("other-secret"));
        assert!(redact(&"中".repeat(5000), "").chars().count() <= 4096);
    }
}
