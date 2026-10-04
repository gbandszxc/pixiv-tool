//! 设置命令。
//!
//! 返回体与旧 HTTP 后端一致：
//! - settings_get → Settings（snake_case 字段，等价旧 GET /api/settings）
//! - settings_save → {"status":"success"}；output_dir 非法 → Err(中文校验消息)；
//!   max_wait_seconds 非法 → Err("最大等待时间必须是 30~86400 秒之间的整数")；
//!   novel_font_scale 非法 → Err("小说字号缩放必须是 0.75~2.0 之间的数字")；
//!   novel_bg_color 非法 → Err("阅读背景色无效")
//! - clear_logs → {"status":"success"}（清空 <data_dir>/logs/app.log）

use std::path::Path;

use serde_json::{Value, json};
use tauri::State;

use crate::settings::{
    STARTUP_PAGES, Settings, THUMB_DETAIL_TIERS, THUMB_FULLSCREEN_TIERS, THUMB_GRID_TIERS,
    validate_max_wait_value, validate_novel_bg_color_value, validate_novel_font_scale_value,
    validate_output_dir_value, validate_thumb_tier,
};
use crate::state::AppState;

/// 可更新键白名单（其余键忽略，对齐旧 update_config 的 setattr 循环）。
/// `saucenao_api_key` 属用户凭据，任何日志不得输出该值。
const WRITABLE_KEYS: [&str; 19] = [
    "translation_api_url",
    "translation_model",
    "translation_target_language",
    "translation_extra",
    "startup_page",
    "output_dir",
    "output_formats",
    "language",
    "theme",
    "theme_color",
    "backend_port",
    "max_wait_seconds",
    "show_r18",
    "thumb_quality_grid",
    "thumb_quality_detail",
    "thumb_quality_fullscreen",
    "novel_font_scale",
    "novel_bg_color",
    "saucenao_api_key",
];

/// 读取当前配置。
#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Value, String> {
    let mut result = serde_json::to_value(state.settings_snapshot()).map_err(|_| "无法读取设置")?;
    match crate::translation::read_api_key() {
        Ok(key) => {
            result["translation_key_configured"] = json!(key.is_some_and(|key| !key.is_empty()));
            result["translation_key_error"] = json!("");
        }
        Err(error) => {
            result["translation_key_configured"] = json!(false);
            result["translation_key_error"] = json!(error);
        }
    }
    Ok(result)
}

/// 更新配置并持久化。settings 是 partial JSON（只更新出现的键）。
#[tauri::command]
pub async fn settings_save(state: State<'_, AppState>, settings: Value) -> Result<Value, String> {
    let mut guard = state
        .settings
        .lock()
        .map_err(|_| "设置表锁中毒".to_string())?;
    let previous = guard.clone();
    let updated = apply_settings_patch(&previous, &settings, &state.paths.data_dir)?;
    let key_patch = settings
        .get("translation_api_key")
        .map(|value| {
            let key = value.as_str().ok_or("翻译 API Key 必须是字符串")?.trim();
            if key.encode_utf16().count() > 1200 || key.chars().any(char::is_control) {
                return Err("翻译 API Key 过长或含控制字符");
            }
            Ok(key)
        })
        .transpose()?;
    // 先落盘再更新内存：磁盘失败时内存保持旧值，避免两处状态漂移
    updated
        .save(&state.paths.config_dir)
        .map_err(|err| format!("保存设置失败: {err}"))?;
    if let Some(key) = key_patch {
        if let Err(error) = crate::translation::write_api_key(key) {
            previous
                .save(&state.paths.config_dir)
                .map_err(|_| "凭据保存失败且配置回滚失败，请重新保存设置")?;
            return Err(error);
        }
    }
    *guard = updated;
    Ok(json!({ "status": "success" }))
}

/// partial JSON → 新 Settings（纯函数，离线可测）：
/// 1. output_dir / max_wait_seconds / theme_color / 缩略图档位 / show_r18 /
///    novel_font_scale / novel_bg_color 先做中文校验（失败即 Err，旧 400 文案）
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
    if let Some(value) = patch_obj.get("startup_page") {
        validate_thumb_tier(value, &STARTUP_PAGES, "应用启动页无效")?;
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
    if let Some(value) = patch_obj.get("thumb_quality_grid") {
        validate_thumb_tier(value, &THUMB_GRID_TIERS, "列表缩略图档位无效")?;
    }
    if let Some(value) = patch_obj.get("thumb_quality_detail") {
        validate_thumb_tier(value, &THUMB_DETAIL_TIERS, "详情页缩略图档位无效")?;
    }
    if let Some(value) = patch_obj.get("thumb_quality_fullscreen") {
        validate_thumb_tier(value, &THUMB_FULLSCREEN_TIERS, "全屏缩略图档位无效")?;
    }
    if patch_obj
        .get("show_r18")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err("R-18 展示开关必须是布尔值".to_string());
    }
    if let Some(value) = patch_obj.get("novel_font_scale") {
        validate_novel_font_scale_value(value)?;
    }
    if let Some(value) = patch_obj.get("novel_bg_color") {
        validate_novel_bg_color_value(value)?;
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
    // saucenao_api_key 保存前 trim（必须字符串；不做长度上限，保持简单）。
    // key 属用户凭据：只进配置文件，任何日志不得输出该值。
    if let Some(value) = patch_obj.get("saucenao_api_key") {
        let key = value
            .as_str()
            .ok_or("SauceNAO API Key 必须是字符串".to_string())?;
        merged.insert(
            "saucenao_api_key".to_string(),
            Value::String(key.trim().to_string()),
        );
    }
    let updated: Settings = serde_json::from_value(Value::Object(merged))
        .map_err(|_| "设置字段格式不正确".to_string())?;
    crate::translation::validate_settings(&updated)?;
    Ok(updated)
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
    fn patch_validates_startup_page() {
        let data_dir = temp_data_dir("startup");
        for page in STARTUP_PAGES {
            let updated = apply_settings_patch(
                &Settings::default(),
                &json!({"startup_page": page}),
                &data_dir,
            )
            .unwrap();
            assert_eq!(updated.startup_page, page);
            assert_eq!(
                apply_settings_patch(&updated, &json!({}), &data_dir)
                    .unwrap()
                    .startup_page,
                page
            );
        }
        for bad in [json!("/unknown"), json!(42), json!(null)] {
            assert_eq!(
                apply_settings_patch(
                    &Settings::default(),
                    &json!({"startup_page": bad}),
                    &data_dir
                )
                .unwrap_err(),
                "应用启动页无效"
            );
        }
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
            apply_settings_patch(
                &Settings::default(),
                &json!({"theme_color": "neon"}),
                &data_dir
            )
            .unwrap_err(),
            "主题色板无效"
        );
        cleanup(&data_dir);
    }

    #[test]
    fn patch_updates_new_keys() {
        let data_dir = temp_data_dir("newkeys");
        let updated = apply_settings_patch(
            &Settings::default(),
            &json!({
                "show_r18": false,
                "thumb_quality_grid": "small",
                "thumb_quality_detail": "original",
                "thumb_quality_fullscreen": "original"
            }),
            &data_dir,
        )
        .unwrap();
        assert!(!updated.show_r18);
        assert_eq!(updated.thumb_quality_grid, "small");
        assert_eq!(updated.thumb_quality_detail, "original");
        assert_eq!(updated.thumb_quality_fullscreen, "original");
        // 只出现部分键时其余保持原值
        let updated =
            apply_settings_patch(&Settings::default(), &json!({"show_r18": false}), &data_dir)
                .unwrap();
        assert!(!updated.show_r18);
        assert_eq!(updated.thumb_quality_grid, "medium");
        cleanup(&data_dir);
    }

    #[test]
    fn patch_rejects_bad_thumb_tiers() {
        let data_dir = temp_data_dir("tiers");
        let cases: [(&str, &str); 3] = [
            ("thumb_quality_grid", "列表缩略图档位无效"),
            ("thumb_quality_detail", "详情页缩略图档位无效"),
            ("thumb_quality_fullscreen", "全屏缩略图档位无效"),
        ];
        for (key, message) in cases {
            for bad in [json!("huge"), json!(3)] {
                let mut patch = serde_json::Map::new();
                patch.insert(key.to_string(), bad.clone());
                assert_eq!(
                    apply_settings_patch(&Settings::default(), &Value::Object(patch), &data_dir)
                        .unwrap_err(),
                    message,
                    "key={key} bad={bad}"
                );
            }
        }
        // 各键合法值接受
        assert!(
            apply_settings_patch(
                &Settings::default(),
                &json!({
                    "thumb_quality_grid": "large",
                    "thumb_quality_detail": "large",
                    "thumb_quality_fullscreen": "large"
                }),
                &data_dir
            )
            .is_ok()
        );
        cleanup(&data_dir);
    }

    #[test]
    fn patch_rejects_non_bool_show_r18() {
        let data_dir = temp_data_dir("r18bool");
        for bad in [json!("true"), json!(1), json!(null)] {
            assert_eq!(
                apply_settings_patch(&Settings::default(), &json!({"show_r18": bad}), &data_dir)
                    .unwrap_err(),
                "R-18 展示开关必须是布尔值",
                "bad={bad}"
            );
        }
        assert!(
            apply_settings_patch(&Settings::default(), &json!({"show_r18": true}), &data_dir)
                .is_ok()
        );
        cleanup(&data_dir);
    }

    #[test]
    fn patch_novel_font_scale_accepts_bounds_and_overrides() {
        let data_dir = temp_data_dir("novelfont");
        // 默认值与区间边界（0.75 / 2.0）通过
        for ok in [json!(1.0), json!(0.75), json!(2.0)] {
            assert!(
                apply_settings_patch(
                    &Settings::default(),
                    &json!({ "novel_font_scale": ok }),
                    &data_dir
                )
                .is_ok(),
                "ok={ok}"
            );
        }
        // 合法值覆盖生效
        let updated = apply_settings_patch(
            &Settings::default(),
            &json!({ "novel_font_scale": 1.25 }),
            &data_dir,
        )
        .unwrap();
        assert_eq!(updated.novel_font_scale, 1.25);
        // 缺键时保持原值不变
        let untouched =
            apply_settings_patch(&updated, &json!({ "language": "en-US" }), &data_dir).unwrap();
        assert_eq!(untouched.novel_font_scale, 1.25);
        cleanup(&data_dir);
    }

    #[test]
    fn patch_rejects_bad_novel_font_scale() {
        let data_dir = temp_data_dir("novelfont-bad");
        let message = "小说字号缩放必须是 0.75~2.0 之间的数字";
        // 越界 / 非数字 / bool / null 全部拒绝，文案精确匹配
        for bad in [
            json!(0.5),
            json!(2.5),
            json!("abc"),
            json!(true),
            json!(null),
        ] {
            assert_eq!(
                apply_settings_patch(
                    &Settings::default(),
                    &json!({ "novel_font_scale": bad }),
                    &data_dir
                )
                .unwrap_err(),
                message,
                "bad={bad}"
            );
        }
        cleanup(&data_dir);
    }

    #[test]
    fn patch_novel_bg_color_accepts_valid_keys_and_overrides() {
        let data_dir = temp_data_dir("novelbg");
        // 空串（跟随主题）与五个语义键全部接受，且覆盖生效
        for color in ["", "green", "kraft", "warm", "mist", "blush"] {
            let updated = apply_settings_patch(
                &Settings::default(),
                &json!({ "novel_bg_color": color }),
                &data_dir,
            )
            .unwrap();
            assert_eq!(updated.novel_bg_color, color, "color={color}");
        }
        // 缺键时保持原值不变
        let with_color = apply_settings_patch(
            &Settings::default(),
            &json!({ "novel_bg_color": "kraft" }),
            &data_dir,
        )
        .unwrap();
        let untouched =
            apply_settings_patch(&with_color, &json!({ "language": "en-US" }), &data_dir).unwrap();
        assert_eq!(untouched.novel_bg_color, "kraft");
        cleanup(&data_dir);
    }

    #[test]
    fn patch_rejects_bad_novel_bg_color() {
        let data_dir = temp_data_dir("novelbg-bad");
        let message = "阅读背景色无效";
        // 白名单外的值 / 非字符串 / bool / null 全部拒绝，文案精确匹配
        for bad in [json!("hotpink"), json!(1), json!(true), json!(null)] {
            assert_eq!(
                apply_settings_patch(
                    &Settings::default(),
                    &json!({ "novel_bg_color": bad }),
                    &data_dir
                )
                .unwrap_err(),
                message,
                "bad={bad}"
            );
        }
        cleanup(&data_dir);
    }

    #[test]
    fn patch_trims_saucenao_api_key() {
        let data_dir = temp_data_dir("saucenao");
        // trim 首尾空白
        let updated = apply_settings_patch(
            &Settings::default(),
            &json!({"saucenao_api_key": "  abc123  "}),
            &data_dir,
        )
        .unwrap();
        assert_eq!(updated.saucenao_api_key, "abc123");
        // 空白串合法（trim 后等价未配置）
        let updated = apply_settings_patch(
            &Settings::default(),
            &json!({"saucenao_api_key": " "}),
            &data_dir,
        )
        .unwrap();
        assert_eq!(updated.saucenao_api_key, "");
        // 非字符串拒绝
        assert_eq!(
            apply_settings_patch(
                &Settings::default(),
                &json!({"saucenao_api_key": 42}),
                &data_dir
            )
            .unwrap_err(),
            "SauceNAO API Key 必须是字符串"
        );
        // 未出现在 patch 里保持原值
        let updated =
            apply_settings_patch(&updated, &json!({"language": "en-US"}), &data_dir).unwrap();
        assert_eq!(updated.saucenao_api_key, "");
        cleanup(&data_dir);
    }
}
