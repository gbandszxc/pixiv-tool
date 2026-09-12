//! 设置命令。
//!
//! 返回体与旧 HTTP 后端一致：
//! - settings_get → Settings（snake_case 字段，等价旧 GET /api/settings）
//! - settings_save → {"status":"success"}；output_dir 非法 → Err(中文校验消息)；
//!   max_wait_seconds 非法 → Err("最大等待时间必须是 30~86400 秒之间的整数")
//! - clear_logs → {"status":"success"}（清空 <data_dir>/logs/app.log）

use std::path::Path;

use serde_json::{Value, json};
use tauri::State;

use crate::settings::{Settings, validate_max_wait_value, validate_output_dir_value};
use crate::state::AppState;

/// 可更新键白名单（其余键忽略，对齐旧 update_config 的 setattr 循环）。
const WRITABLE_KEYS: [&str; 7] = [
    "output_dir",
    "output_formats",
    "language",
    "theme",
    "theme_color",
    "backend_port",
    "max_wait_seconds",
];

/// 读取当前配置。
#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.settings_snapshot())
}

/// 更新配置并持久化。settings 是 partial JSON（只更新出现的键）。
#[tauri::command]
pub async fn settings_save(state: State<'_, AppState>, settings: Value) -> Result<Value, String> {
    let updated =
        apply_settings_patch(&state.settings_snapshot(), &settings, &state.paths.data_dir)?;
    // 先落盘再更新内存：磁盘失败时内存保持旧值，避免两处状态漂移
    updated
        .save(&state.paths.config_dir)
        .map_err(|err| format!("保存设置失败: {err}"))?;
    let mut guard = state
        .settings
        .lock()
        .map_err(|_| "设置表锁中毒".to_string())?;
    *guard = updated;
    Ok(json!({ "status": "success" }))
}

/// partial JSON → 新 Settings（纯函数，离线可测）：
/// 1. output_dir / max_wait_seconds 先做中文校验（失败即 Err，旧 400 文案）
/// 2. 白名单键覆盖到当前配置序列化结果上，再反解回 Settings
///    （类型不合法的值在反解时以中文错误拒绝）
pub fn apply_settings_patch(
    current: &Settings,
    patch: &Value,
    data_dir: &Path,
) -> Result<Settings, String> {
    let Some(patch_obj) = patch.as_object() else {
        return Err("settings 必须是 JSON 对象".to_string());
    };
    if let Some(value) = patch_obj.get("output_dir") {
        validate_output_dir_value(value, data_dir)?;
    }
    if let Some(value) = patch_obj.get("max_wait_seconds") {
        validate_max_wait_value(value)?;
    }
    if let Some(value) = patch_obj.get("theme_color") {
        let color = value.as_str().ok_or_else(|| "主题色板无效".to_string())?;
        if !["pixiv", "indigo", "jade", "violet", "amber"].contains(&color) {
            return Err("主题色板无效".to_string());
        }
    }
    let mut merged = serde_json::to_value(current)
        .map_err(|err| format!("序列化设置失败: {err}"))?
        .as_object()
        .cloned()
        .ok_or_else(|| "序列化设置失败: 结构异常".to_string())?;
    for key in WRITABLE_KEYS {
        if let Some(value) = patch_obj.get(key) {
            merged.insert(key.to_string(), value.clone());
        }
    }
    serde_json::from_value(Value::Object(merged))
        .map_err(|err| format!("设置字段格式不正确: {err}"))
}

/// 清空 app.log。
#[tauri::command]
pub async fn clear_logs(state: State<'_, AppState>) -> Result<Value, String> {
    let log_path = state.paths.logs_dir.join("app.log");
    if log_path.exists() {
        std::fs::write(&log_path, "")
            .map_err(|err| format!("清空日志失败 {}: {err}", log_path.display()))?;
    }
    Ok(json!({ "status": "success" }))
}

// ----------------------------------------------------------------------
// 单元测试（纯函数 + 临时目录，全部离线）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_data_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pixiv-tool-settingscmd-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn patch_updates_only_whitelisted_keys() {
        let data_dir = temp_data_dir("whitelist");
        let current = Settings::default();
        let patch = json!({
            "language": "en-US",
            "theme": "dark",
            "theme_color": "jade",
            "max_wait_seconds": 3600,
            "backend_port": null,
            "output_formats": ["txt"],
            // 非白名单键忽略
            "hacked_field": "x"
        });
        let updated = apply_settings_patch(&current, &patch, &data_dir).unwrap();
        assert_eq!(updated.language, "en-US");
        assert_eq!(updated.theme, "dark");
        assert_eq!(updated.theme_color, "jade");
        assert_eq!(updated.max_wait_seconds, 3600);
        assert_eq!(updated.backend_port, None);
        assert_eq!(updated.output_formats, vec!["txt".to_string()]);
        // 未出现的键保持原值
        assert_eq!(updated.output_dir, current.output_dir);
        cleanup(&data_dir);
    }

    #[test]
    fn patch_empty_is_noop() {
        let data_dir = temp_data_dir("noop");
        let current = Settings::default();
        let updated = apply_settings_patch(&current, &json!({}), &data_dir).unwrap();
        assert_eq!(updated.max_wait_seconds, current.max_wait_seconds);
        assert_eq!(updated.language, current.language);
        cleanup(&data_dir);
    }

    #[test]
    fn patch_rejects_bad_output_dir() {
        let data_dir = temp_data_dir("outdir");
        let err = apply_settings_patch(
            &Settings::default(),
            &json!({"output_dir": "   "}),
            &data_dir,
        )
        .unwrap_err();
        assert_eq!(err, "输出目录不能为空");
        // 非字符串
        assert_eq!(
            apply_settings_patch(&Settings::default(), &json!({"output_dir": 42}), &data_dir)
                .unwrap_err(),
            "输出目录必须是字符串"
        );
        // 合法相对路径锚定成功
        assert!(
            apply_settings_patch(
                &Settings::default(),
                &json!({"output_dir": "out"}),
                &data_dir
            )
            .is_ok()
        );
        cleanup(&data_dir);
    }

    #[test]
    fn patch_rejects_bad_max_wait() {
        let data_dir = temp_data_dir("maxwait");
        let message = "最大等待时间必须是 30~86400 秒之间的整数";
        for bad in [json!(10), json!(true), json!("180"), json!(86401)] {
            assert_eq!(
                apply_settings_patch(
                    &Settings::default(),
                    &json!({"max_wait_seconds": bad}),
                    &data_dir
                )
                .unwrap_err(),
                message,
                "bad={bad}"
            );
        }
        assert!(
            apply_settings_patch(
                &Settings::default(),
                &json!({"max_wait_seconds": 86400}),
                &data_dir
            )
            .is_ok()
        );
        cleanup(&data_dir);
    }

    #[test]
    fn patch_rejects_non_object_and_bad_types() {
        let data_dir = temp_data_dir("shape");
        assert_eq!(
            apply_settings_patch(&Settings::default(), &json!("x"), &data_dir).unwrap_err(),
            "settings 必须是 JSON 对象"
        );
        assert_eq!(
            apply_settings_patch(&Settings::default(), &json!(null), &data_dir).unwrap_err(),
            "settings 必须是 JSON 对象"
        );
        // 白名单键但类型不合法（output_formats 传字符串）
        let err = apply_settings_patch(
            &Settings::default(),
            &json!({"output_formats": "txt"}),
            &data_dir,
        )
        .unwrap_err();
        assert!(err.starts_with("设置字段格式不正确"), "got {err}");
        cleanup(&data_dir);
    }

    #[test]
    fn patch_rejects_unknown_theme_color() {
        let data_dir = temp_data_dir("palette");
        assert_eq!(
            apply_settings_patch(&Settings::default(), &json!({"theme_color": "neon"}), &data_dir)
                .unwrap_err(),
            "主题色板无效"
        );
        cleanup(&data_dir);
    }
}
