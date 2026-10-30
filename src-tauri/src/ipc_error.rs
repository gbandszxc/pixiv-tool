//! IPC 命令结构化错误载荷（ADR 0044）。
//!
//! `settings_save` 率先采用：校验错误不再是中文自由字符串，而是可序列化的
//! `{ code, field, message }` 对象。Tauri 2 会把 command 的 `Err` 值经 serde
//! 序列化后原样回传前端（`InvokeError(serde_json::Value)` → JSON 响应体），
//! 前端据此按 `field` 停靠字段错误、按 `code` 取 i18n 文案。其余命令仍返回
//! `Err(String)`，后续按同模式渐进迁移。

use serde::Serialize;

/// `settings_save` 的结构化校验错误载荷。
#[derive(Debug, Clone, Serialize)]
pub struct SettingsError {
    /// 稳定错误码，前端 i18n 键 `settings.errors.<code>` 的依据。
    pub code: &'static str,
    /// 触发错误的设置键名（SPEC §5.2）；整体性错误为 `None`（前端落 Snackbar 兜底）。
    pub field: Option<&'static str>,
    /// 中文兜底文案：日志展示与 i18n 键缺失时的降级显示。
    /// 不得包含凭据原文（脱敏口径见 ADR 0027）。
    pub message: String,
}

impl SettingsError {
    pub fn new(
        code: &'static str,
        field: Option<&'static str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            field,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IPC 契约形状：Err 值经 serde 序列化后必须是 { code, field, message } 对象
    /// （Tauri 2 的 InvokeError 按 serde_json::to_value 原样回传，ADR 0044）。
    #[test]
    fn serializes_to_payload_object() {
        let value = serde_json::to_value(SettingsError::new(
            "output_dir_create_failed",
            Some("output_dir"),
            "无法创建输出目录: 权限不足",
        ))
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "code": "output_dir_create_failed",
                "field": "output_dir",
                "message": "无法创建输出目录: 权限不足"
            })
        );
        // 整体性错误 field 为 null；Display 输出兜底文案
        let value =
            serde_json::to_value(SettingsError::new("internal", None, "设置表锁中毒")).unwrap();
        assert_eq!(value["code"], "internal");
        assert!(value["field"].is_null());
        assert_eq!(
            SettingsError::new("internal", None, "设置表锁中毒").to_string(),
            "设置表锁中毒"
        );
    }
}
