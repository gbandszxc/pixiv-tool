//! Settings —— JSON 配置管理，与旧 Python `storage/settings.py` / `config/settings.json`
//! 完全兼容（snake_case 键、默认值、损坏恢复、旧值迁移）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths::default_output_dir;

pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// 应用配置。JSON 键为 snake_case，与旧 Python 版逐字段兼容。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 输出目录（绝对路径或锚定 data_dir 的相对路径）。
    pub output_dir: String,
    /// 启用的导出格式（"txt" / "markdown"）。
    pub output_formats: Vec<String>,
    /// 界面语言（"zh-CN" / "en-US"）。
    pub language: String,
    /// 主题（"auto" / "light" / "dark"）。
    pub theme: String,
    /// Material 3 色板（"pixiv" / "indigo" / "jade" / "violet" / "amber"）。
    pub theme_color: String,
    /// 旧 Python 后端端口配置，Tauri 版无后端进程，仅保留字段兼容旧配置文件。
    pub backend_port: Option<i64>,
    /// 任务最大等待时间（秒）：运行超过该时长自动标记失败，不含暂停时间。
    pub max_wait_seconds: i64,
    /// 全局 R-18 展示开关：关闭后列表隐藏 x_restrict >= 1 的作品，详情页仍可访问。
    pub show_r18: bool,
    /// 列表/网格封面档位（见 [`THUMB_GRID_TIERS`]）。
    pub thumb_quality_grid: String,
    /// 详情页主图档位（见 [`THUMB_DETAIL_TIERS`]；medium = 接口 regular 原样）。
    pub thumb_quality_detail: String,
    /// 全屏浮层档位（见 [`THUMB_FULLSCREEN_TIERS`]）。
    pub thumb_quality_fullscreen: String,
    /// SauceNAO API Key（以图识图必需，saucenao.com 注册后获取；仅保存在本机
    /// 配置文件——不入库、不写日志、不进报错原文）。
    pub saucenao_api_key: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir().to_string_lossy().into_owned(),
            output_formats: vec!["txt".into(), "markdown".into()],
            language: "zh-CN".into(),
            theme: "auto".into(),
            theme_color: "pixiv".into(),
            backend_port: None,
            max_wait_seconds: 180,
            show_r18: true,
            thumb_quality_grid: "medium".into(),
            thumb_quality_detail: "medium".into(),
            thumb_quality_fullscreen: "large".into(),
            saucenao_api_key: String::new(),
        }
    }
}

/// 列表/网格封面可选档位。
pub const THUMB_GRID_TIERS: [&str; 3] = ["small", "medium", "large"];
/// 详情页主图可选档位（medium = 接口 regular 原样，不映射到 540px 档）。
pub const THUMB_DETAIL_TIERS: [&str; 3] = ["medium", "large", "original"];
/// 全屏浮层可选档位。
pub const THUMB_FULLSCREEN_TIERS: [&str; 2] = ["large", "original"];

/// 缩略图档位校验：非字符串或不在 `allowed` 集合内 → Err(message)。
pub fn validate_thumb_tier(value: &Value, allowed: &[&str], message: &str) -> Result<(), String> {
    match value.as_str() {
        Some(tier) if allowed.contains(&tier) => Ok(()),
        _ => Err(message.to_string()),
    }
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join(SETTINGS_FILE_NAME)
}

impl Settings {
    /// 从 `config_dir/settings.json` 加载：
    /// - 文件不存在 → 写入默认值并返回
    /// - JSON 损坏 / 读失败 → 备份为 `settings.json.corrupt-{mtime_ns}` 后重建默认
    /// - 旧值迁移：`output_dir == "downloads"`（早期默认相对路径）→ 系统下载目录/pixiv-tool
    /// - 加载期归一：非法缩略图档位重置为默认（不强制回写文件）
    /// - 缺键填默认（serde default）；未知键忽略（serde 默认行为）
    pub fn load_or_init(config_dir: &Path) -> Settings {
        let path = settings_path(config_dir);
        if !path.exists() {
            let settings = Settings::default();
            if let Err(err) = settings.save(config_dir) {
                eprintln!("写入默认 settings.json 失败 {}: {err}", path.display());
            }
            return settings;
        }

        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        match serde_json::from_str::<Settings>(&raw) {
            Ok(mut settings) => {
                // 旧默认值迁移：存了字面量 "downloads" 的配置直接迁移；
                // 用户显式改过的路径不动。
                if settings.output_dir == "downloads" {
                    settings.output_dir = default_output_dir().to_string_lossy().into_owned();
                }
                // 手改 settings.json 写入的非法档位回落到默认（不强制回写文件）。
                if !THUMB_GRID_TIERS.contains(&settings.thumb_quality_grid.as_str()) {
                    settings.thumb_quality_grid = "medium".into();
                }
                if !THUMB_DETAIL_TIERS.contains(&settings.thumb_quality_detail.as_str()) {
                    settings.thumb_quality_detail = "medium".into();
                }
                if !THUMB_FULLSCREEN_TIERS.contains(&settings.thumb_quality_fullscreen.as_str()) {
                    settings.thumb_quality_fullscreen = "large".into();
                }
                settings
            }
            Err(_) => {
                backup_corrupt(&path);
                let settings = Settings::default();
                if let Err(err) = settings.save(config_dir) {
                    eprintln!("重建 settings.json 失败 {}: {err}", path.display());
                }
                settings
            }
        }
    }

    /// 持久化（serde_json 默认不转义非 ASCII，等价 ensure_ascii=false；2 空格缩进）。
    pub fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir)?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        std::fs::write(settings_path(config_dir), format!("{json}\n"))
    }
}

/// 损坏文件备份：settings.json → settings.json.corrupt-{mtime_ns}（复制，失败仅忽略）。
fn backup_corrupt(path: &Path) {
    let mtime_ns = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| SETTINGS_FILE_NAME.to_string());
    let backup = path.with_file_name(format!("{file_name}.corrupt-{mtime_ns}"));
    if let Err(err) = std::fs::copy(path, &backup) {
        eprintln!(
            "备份损坏的 settings.json 失败 {} → {}: {err}",
            path.display(),
            backup.display()
        );
    }
}

/// 校验输出目录并返回锚定后的绝对路径。错误消息中文，与旧 Python 一致：
/// - 空白 → "输出目录不能为空"
/// - 含控制字符（< 0x20）→ "输出目录包含非法控制字符"
/// - Windows 含 `*?"<>|` → "输出目录包含非法字符: {found}"（按出现顺序去重）
/// - 相对路径 → 锚定 data_dir
/// - 指向已存在文件 → "输出目录指向一个已有文件"
/// - create_dir_all 失败 → "无法创建输出目录: {err}"
pub fn validate_output_dir(value: &str, data_dir: &Path) -> Result<PathBuf, String> {
    if value.trim().is_empty() {
        return Err("输出目录不能为空".into());
    }
    if value.chars().any(|c| (c as u32) < 32) {
        return Err("输出目录包含非法控制字符".into());
    }
    #[cfg(target_os = "windows")]
    {
        let illegal = ['*', '?', '"', '<', '>', '|'];
        let mut seen = std::collections::HashSet::new();
        let found: String = value
            .chars()
            .filter(|c| illegal.contains(c) && seen.insert(*c))
            .collect();
        if !found.is_empty() {
            return Err(format!("输出目录包含非法字符: {found}"));
        }
    }
    let mut path = PathBuf::from(value);
    if !path.is_absolute() {
        path = data_dir.join(path);
    }
    if path.exists() && !path.is_dir() {
        return Err("输出目录指向一个已有文件".into());
    }
    std::fs::create_dir_all(&path).map_err(|err| format!("无法创建输出目录: {err}"))?;
    Ok(path)
}

/// JSON 值形式的输出目录校验（非字符串 → "输出目录必须是字符串"）。
pub fn validate_output_dir_value(value: &Value, data_dir: &Path) -> Result<PathBuf, String> {
    match value.as_str() {
        Some(s) => validate_output_dir(s, data_dir),
        None => Err("输出目录必须是字符串".into()),
    }
}

/// 校验最大等待时间：必须是 30..=86400 的整数。
pub fn validate_max_wait(v: i64) -> Result<(), String> {
    if (30..=86400).contains(&v) {
        Ok(())
    } else {
        Err("最大等待时间必须是 30~86400 秒之间的整数".into())
    }
}

/// JSON 值形式的最大等待时间校验（bool / 非整数 / 越界都拒绝），通过时返回该整数。
pub fn validate_max_wait_value(v: &Value) -> Result<i64, String> {
    if v.is_boolean() {
        return Err("最大等待时间必须是 30~86400 秒之间的整数".into());
    }
    match v.as_i64() {
        Some(n) => {
            validate_max_wait(n)?;
            Ok(n)
        }
        None => Err("最大等待时间必须是 30~86400 秒之间的整数".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pixiv-tool-settings-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn defaults_match_python() {
        let s = Settings::default();
        assert_eq!(s.output_formats, vec!["txt", "markdown"]);
        assert_eq!(s.language, "zh-CN");
        assert_eq!(s.theme, "auto");
        assert_eq!(s.theme_color, "pixiv");
        assert_eq!(s.backend_port, None);
        assert_eq!(s.max_wait_seconds, 180);
        assert!(s.show_r18);
        assert_eq!(s.thumb_quality_grid, "medium");
        assert_eq!(s.thumb_quality_detail, "medium");
        assert_eq!(s.thumb_quality_fullscreen, "large");
        assert_eq!(s.saucenao_api_key, "");
        assert!(
            s.output_dir
                .replace('\\', "/")
                .ends_with("Downloads/pixiv-tool")
        );
    }

    #[test]
    fn init_creates_file_with_defaults() {
        let dir = temp_config_dir("init");
        let s = Settings::load_or_init(&dir);
        assert_eq!(s.max_wait_seconds, 180);
        let raw = std::fs::read_to_string(settings_path(&dir)).unwrap();
        assert!(raw.contains("\"max_wait_seconds\": 180"));
        assert!(raw.contains("\"language\": \"zh-CN\""));
        // 非 ASCII 不转义（ensure_ascii=false 等价）
        assert!(raw.contains("\"output_dir\": \""));
        cleanup(&dir);
    }

    #[test]
    fn missing_keys_filled_and_unknown_ignored() {
        let dir = temp_config_dir("partial");
        std::fs::write(
            settings_path(&dir),
            r#"{"output_dir":"/tmp/x","unknown_key":123}"#,
        )
        .unwrap();
        let s = Settings::load_or_init(&dir);
        assert_eq!(s.output_dir, "/tmp/x");
        assert_eq!(s.language, "zh-CN"); // 缺键 → 默认
        assert_eq!(s.output_formats.len(), 2); // 缺键 → 默认
        // 新增键缺省 → 默认
        assert!(s.show_r18);
        assert_eq!(s.thumb_quality_grid, "medium");
        assert_eq!(s.thumb_quality_detail, "medium");
        assert_eq!(s.thumb_quality_fullscreen, "large");
        assert_eq!(s.saucenao_api_key, "");
        cleanup(&dir);
    }

    #[test]
    fn invalid_thumb_tier_falls_back_to_default_on_load() {
        let dir = temp_config_dir("thumb-tier");
        std::fs::write(
            settings_path(&dir),
            r#"{"thumb_quality_grid":"huge","thumb_quality_detail":"small","thumb_quality_fullscreen":"medium"}"#,
        )
        .unwrap();
        let s = Settings::load_or_init(&dir);
        assert_eq!(s.thumb_quality_grid, "medium");
        assert_eq!(s.thumb_quality_detail, "medium");
        assert_eq!(s.thumb_quality_fullscreen, "large");
        // 合法档位原样保留
        std::fs::write(
            settings_path(&dir),
            r#"{"thumb_quality_grid":"small","thumb_quality_detail":"original","thumb_quality_fullscreen":"original"}"#,
        )
        .unwrap();
        let s = Settings::load_or_init(&dir);
        assert_eq!(s.thumb_quality_grid, "small");
        assert_eq!(s.thumb_quality_detail, "original");
        assert_eq!(s.thumb_quality_fullscreen, "original");
        cleanup(&dir);
    }

    #[test]
    fn validate_thumb_tier_rules() {
        let ok = validate_thumb_tier(
            &serde_json::json!("large"),
            &THUMB_GRID_TIERS,
            "列表缩略图档位无效",
        );
        assert!(ok.is_ok());
        for bad in [
            serde_json::json!("huge"),
            serde_json::json!(1),
            serde_json::json!(null),
        ] {
            assert_eq!(
                validate_thumb_tier(&bad, &THUMB_GRID_TIERS, "列表缩略图档位无效").unwrap_err(),
                "列表缩略图档位无效"
            );
        }
    }

    #[test]
    fn corrupt_file_backed_up_and_rebuilt() {
        let dir = temp_config_dir("corrupt");
        std::fs::write(settings_path(&dir), "{ not valid json !!!").unwrap();
        let s = Settings::load_or_init(&dir);
        assert_eq!(s.max_wait_seconds, 180); // 回落默认
        // 存在 corrupt-{mtime_ns} 备份
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(backups.len(), 1, "应恰好有一个损坏备份");
        cleanup(&dir);
    }

    #[test]
    fn migrates_legacy_downloads_output_dir() {
        let dir = temp_config_dir("migrate");
        std::fs::write(settings_path(&dir), r#"{"output_dir":"downloads"}"#).unwrap();
        let s = Settings::load_or_init(&dir);
        assert_ne!(s.output_dir, "downloads");
        assert!(
            s.output_dir
                .replace('\\', "/")
                .ends_with("Downloads/pixiv-tool")
        );
        cleanup(&dir);
    }

    #[test]
    fn save_roundtrip() {
        let dir = temp_config_dir("roundtrip");
        let s = Settings {
            max_wait_seconds: 3600,
            language: "en-US".into(),
            ..Default::default()
        };
        s.save(&dir).unwrap();
        let loaded = Settings::load_or_init(&dir);
        assert_eq!(loaded.max_wait_seconds, 3600);
        assert_eq!(loaded.language, "en-US");
        cleanup(&dir);
    }

    #[test]
    fn validate_output_dir_rules() {
        let data_dir = std::env::temp_dir();
        assert_eq!(
            validate_output_dir("   ", &data_dir).unwrap_err(),
            "输出目录不能为空"
        );
        assert_eq!(
            validate_output_dir("ab\u{1}cd", &data_dir).unwrap_err(),
            "输出目录包含非法控制字符"
        );
        // 相对路径锚定 data_dir
        let anchored = validate_output_dir("pixiv-out", &data_dir).unwrap();
        assert!(anchored.starts_with(&data_dir));
        assert!(anchored.ends_with("pixiv-out"));
        // 指向已有文件
        let file = data_dir.join(format!("pixiv-tool-test-file-{}", uuid::Uuid::new_v4()));
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            validate_output_dir(&file.to_string_lossy(), &data_dir).unwrap_err(),
            "输出目录指向一个已有文件"
        );
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn validate_max_wait_rules() {
        assert!(validate_max_wait(30).is_ok());
        assert!(validate_max_wait(86400).is_ok());
        assert!(validate_max_wait(29).is_err());
        assert!(validate_max_wait(86401).is_err());
        assert_eq!(
            validate_max_wait(10).unwrap_err(),
            "最大等待时间必须是 30~86400 秒之间的整数"
        );
        // JSON 值形式：bool / 字符串拒绝
        assert!(validate_max_wait_value(&serde_json::json!(180)).is_ok());
        assert!(validate_max_wait_value(&serde_json::json!(true)).is_err());
        assert!(validate_max_wait_value(&serde_json::json!("180")).is_err());
    }
}
