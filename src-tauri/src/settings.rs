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
    /// 应用启动页（发现 / 关注 / 我的 / 下载，见 STARTUP_PAGES）。
    pub startup_page: String,
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
    /// 图片磁盘缓存上限（MiB），默认 [`DEFAULT_IMAGE_CACHE_MAX_MIB`]，
    /// 合法区间 [`IMAGE_CACHE_MIN_MIB`] ~ [`IMAGE_CACHE_MAX_MIB`]；
    /// 非法值在加载期回落默认。消费方为 `image_proxy` 的分区淘汰。
    pub image_cache_max_mib: i64,
    /// 小说正文字号缩放（小说阅读器底栏缩放控件写入），默认 1.0，
    /// 合法区间 [0.75, 2.0]；非法值在加载期回落 1.0。
    pub novel_font_scale: f64,
    /// 小说阅读背景色（小说阅读器底栏色块按钮写入），语义键，默认空串 = 跟随主题，
    /// 合法值见 [`NOVEL_BG_COLORS`]；非法值（含手改 settings.json）在加载期回落空串。
    pub novel_bg_color: String,
    /// SauceNAO API Key（以图识图必需，saucenao.com 注册后获取；仅保存在本机
    /// 配置文件——不入库、不写日志、不进报错原文）。
    pub saucenao_api_key: String,
    pub translation_api_url: String,
    /// 翻译接口协议（见 [`crate::translation::TRANSLATION_API_FORMATS`]），
    /// 默认 `chat_completions`（OpenAI 兼容）。
    pub translation_api_format: String,
    pub translation_model: String,
    /// 翻译目标语言（见 [`crate::translation::TARGET_LANGUAGES`]），空串 = 跟随界面语言 `language`。
    pub translation_target_language: String,
    /// 小说翻译单请求超时（秒），默认 [`DEFAULT_TRANSLATION_TIMEOUT_SECONDS`]（10 分钟）；
    /// 合法区间 [`TRANSLATION_TIMEOUT_MIN_SECONDS`] ~ [`TRANSLATION_TIMEOUT_MAX_SECONDS`]。
    pub translation_timeout_seconds: i64,
    /// 模型请求体扩展；不含凭据或应用保留字段，可用键随协议不同。
    pub translation_extra: Value,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir().to_string_lossy().into_owned(),
            output_formats: vec!["txt".into(), "markdown".into()],
            language: "zh-CN".into(),
            theme: "auto".into(),
            theme_color: "pixiv".into(),
            startup_page: "/browse/home".into(),
            backend_port: None,
            max_wait_seconds: 180,
            show_r18: true,
            thumb_quality_grid: "medium".into(),
            thumb_quality_detail: "medium".into(),
            thumb_quality_fullscreen: "large".into(),
            image_cache_max_mib: DEFAULT_IMAGE_CACHE_MAX_MIB,
            novel_font_scale: 1.0,
            novel_bg_color: String::new(),
            saucenao_api_key: String::new(),
            translation_api_url: String::new(),
            translation_api_format: "chat_completions".into(),
            translation_model: String::new(),
            translation_target_language: String::new(),
            translation_timeout_seconds: DEFAULT_TRANSLATION_TIMEOUT_SECONDS,
            translation_extra: serde_json::json!({}),
        }
    }
}

/// 列表/网格封面可选档位。
pub const THUMB_GRID_TIERS: [&str; 3] = ["small", "medium", "large"];
/// 详情页主图可选档位（medium = 接口 regular 原样，不映射到 540px 档）。
pub const THUMB_DETAIL_TIERS: [&str; 3] = ["medium", "large", "original"];
/// 全屏浮层可选档位。
pub const THUMB_FULLSCREEN_TIERS: [&str; 2] = ["large", "original"];
/// 图片磁盘缓存上限默认值（MiB）：取代写死的 1GB（ADR 0028）。
pub const DEFAULT_IMAGE_CACHE_MAX_MIB: i64 = 512;
/// 图片磁盘缓存上限合法区间下限（MiB）。
pub const IMAGE_CACHE_MIN_MIB: i64 = 256;
/// 图片磁盘缓存上限合法区间上限（MiB）= 2 GiB。
pub const IMAGE_CACHE_MAX_MIB: i64 = 2048;
/// 小说阅读背景色可选语义键（空串 = 跟随主题，不在此表内）。
pub const NOVEL_BG_COLORS: [&str; 5] = ["green", "kraft", "warm", "mist", "blush"];
/// 四个核心导航入口；发现默认展示推荐首页。
pub const STARTUP_PAGES: [&str; 4] = [
    "/browse/home",
    "/browse/feed",
    "/browse/bookmark",
    "/tools/tasks",
];
/// 小说翻译单请求超时默认值（秒）= 10 分钟（慢模型/长页远比旧的 180s 宽松）。
pub const DEFAULT_TRANSLATION_TIMEOUT_SECONDS: i64 = 600;
/// 小说翻译单请求超时合法区间下限（秒）：低于此值连正常首字延迟都等不到。
pub const TRANSLATION_TIMEOUT_MIN_SECONDS: i64 = 30;
/// 小说翻译单请求超时合法区间上限（秒）= 1 小时。
pub const TRANSLATION_TIMEOUT_MAX_SECONDS: i64 = 3600;

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
    /// - 加载期归一：非法启动页、缩略图档位、小说字号缩放、阅读背景色重置为默认（不强制回写文件）
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
                if !STARTUP_PAGES.contains(&settings.startup_page.as_str()) {
                    settings.startup_page = "/browse/home".into();
                }
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
                // 图片缓存上限：手改 settings.json 写入区间外的值（含 0 与负数）回落默认，
                // 否则 0 会让每次缓存写入都触发全量淘汰。
                if !(IMAGE_CACHE_MIN_MIB..=IMAGE_CACHE_MAX_MIB).contains(&settings.image_cache_max_mib)
                {
                    settings.image_cache_max_mib = DEFAULT_IMAGE_CACHE_MAX_MIB;
                }
                // 小说字号缩放：非有限值（NaN/inf，JSON 文本层面不可表达，纯防御）
                // 或越出 [0.75, 2.0] 回落默认 1.0（对齐缩略图档位的加载回落做法）。
                if !settings.novel_font_scale.is_finite()
                    || !(0.75..=2.0).contains(&settings.novel_font_scale)
                {
                    settings.novel_font_scale = 1.0;
                }
                // 小说阅读背景色：空串（跟随主题）与白名单内语义键原样保留，
                // 白名单外的值（含手改 settings.json）回落空串（对齐缩略图档位做法）。
                if !settings.novel_bg_color.is_empty()
                    && !NOVEL_BG_COLORS.contains(&settings.novel_bg_color.as_str())
                {
                    settings.novel_bg_color = String::new();
                }
                // 翻译接口协议：白名单外的值（含手改 settings.json）回落默认 Chat Completions。
                if !crate::translation::TRANSLATION_API_FORMATS
                    .contains(&settings.translation_api_format.as_str())
                {
                    settings.translation_api_format = "chat_completions".into();
                }
                // 翻译超时：手改 settings.json 写入区间外的值（含 0 与负数）回落默认，
                // 否则 0 秒会让全部翻译请求立刻超时。
                if !(TRANSLATION_TIMEOUT_MIN_SECONDS..=TRANSLATION_TIMEOUT_MAX_SECONDS)
                    .contains(&settings.translation_timeout_seconds)
                {
                    settings.translation_timeout_seconds = DEFAULT_TRANSLATION_TIMEOUT_SECONDS;
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

/// JSON 值形式的翻译超时校验（bool / 非整数 / 越界都拒绝），通过时返回该整数。
pub fn validate_translation_timeout_value(v: &Value) -> Result<i64, String> {
    if v.is_boolean() {
        return Err(translation_timeout_message());
    }
    match v.as_i64() {
        Some(n) if (TRANSLATION_TIMEOUT_MIN_SECONDS..=TRANSLATION_TIMEOUT_MAX_SECONDS).contains(&n) => {
            Ok(n)
        }
        _ => Err(translation_timeout_message()),
    }
}

/// 翻译超时校验文案：区间与常量同源，避免改常量忘改文案。
fn translation_timeout_message() -> String {
    format!(
        "翻译超时时间必须是 {TRANSLATION_TIMEOUT_MIN_SECONDS}~{TRANSLATION_TIMEOUT_MAX_SECONDS} 秒之间的整数"
    )
}

/// JSON 值形式的图片缓存上限校验（bool / 非整数 / 越界都拒绝），通过时返回该整数。
pub fn validate_image_cache_max_value(v: &Value) -> Result<i64, String> {
    let message = || {
        format!("图片缓存上限必须是 {IMAGE_CACHE_MIN_MIB}~{IMAGE_CACHE_MAX_MIB} MiB 之间的整数")
    };
    if v.is_boolean() {
        return Err(message());
    }
    match v.as_i64() {
        Some(n) if (IMAGE_CACHE_MIN_MIB..=IMAGE_CACHE_MAX_MIB).contains(&n) => Ok(n),
        _ => Err(message()),
    }
}

/// JSON 值形式的小说字号缩放校验：bool / 非数字 / 非有限值（NaN/inf）/ 越界都拒绝。
pub fn validate_novel_font_scale_value(v: &Value) -> Result<(), String> {
    let Some(scale) = v.as_f64().filter(|s| s.is_finite()) else {
        return Err("小说字号缩放必须是 0.75~2.0 之间的数字".into());
    };
    if (0.75..=2.0).contains(&scale) {
        Ok(())
    } else {
        Err("小说字号缩放必须是 0.75~2.0 之间的数字".into())
    }
}

/// JSON 值形式的小说阅读背景色校验：必须是白名单语义键或空串（空串 = 跟随主题），
/// 非字符串 / 白名单外的值一律拒绝。
pub fn validate_novel_bg_color_value(v: &Value) -> Result<(), String> {
    match v.as_str() {
        Some(color) if color.is_empty() || NOVEL_BG_COLORS.contains(&color) => Ok(()),
        _ => Err("阅读背景色无效".to_string()),
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
        assert_eq!(s.image_cache_max_mib, DEFAULT_IMAGE_CACHE_MAX_MIB);
        assert_eq!(s.novel_font_scale, 1.0);
        assert_eq!(s.novel_bg_color, "");
        assert_eq!(s.saucenao_api_key, "");
        assert_eq!(s.translation_api_format, "chat_completions");
        assert_eq!(
            s.translation_timeout_seconds,
            DEFAULT_TRANSLATION_TIMEOUT_SECONDS
        );
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
        assert!(raw.contains("\"translation_timeout_seconds\": 600"));
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
        assert_eq!(s.image_cache_max_mib, DEFAULT_IMAGE_CACHE_MAX_MIB);
        assert_eq!(s.novel_font_scale, 1.0);
        assert_eq!(s.novel_bg_color, "");
        assert_eq!(s.saucenao_api_key, "");
        assert_eq!(s.translation_api_format, "chat_completions");
        assert_eq!(
            s.translation_timeout_seconds,
            DEFAULT_TRANSLATION_TIMEOUT_SECONDS
        );
        assert_eq!(s.startup_page, "/browse/home");
        cleanup(&dir);
    }

    #[test]
    fn startup_page_roundtrip_and_invalid_fallback() {
        let dir = temp_config_dir("startup");
        for page in STARTUP_PAGES {
            let settings = Settings {
                startup_page: page.into(),
                ..Settings::default()
            };
            settings.save(&dir).unwrap();
            assert_eq!(Settings::load_or_init(&dir).startup_page, page);
        }
        std::fs::write(
            settings_path(&dir),
            r#"{"startup_page":"/unknown","theme":"dark"}"#,
        )
        .unwrap();
        let settings = Settings::load_or_init(&dir);
        assert_eq!(settings.startup_page, "/browse/home");
        assert_eq!(settings.theme, "dark");
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
    fn invalid_novel_font_scale_falls_back_to_default_on_load() {
        let dir = temp_config_dir("novelfont");
        // 手改 settings.json 写入越界值 → 加载期回落 1.0
        std::fs::write(settings_path(&dir), r#"{"novel_font_scale":9.9}"#).unwrap();
        assert_eq!(Settings::load_or_init(&dir).novel_font_scale, 1.0);
        // 合法值（含边界）原样保留
        for scale in [0.75, 1.25, 2.0] {
            std::fs::write(
                settings_path(&dir),
                format!(r#"{{"novel_font_scale":{scale}}}"#),
            )
            .unwrap();
            assert_eq!(Settings::load_or_init(&dir).novel_font_scale, scale);
        }
        cleanup(&dir);
    }

    #[test]
    fn validate_novel_font_scale_rules() {
        use serde_json::json;
        for ok in [json!(1.0), json!(0.75), json!(2.0), json!(1.25), json!(1)] {
            assert!(validate_novel_font_scale_value(&ok).is_ok(), "ok={ok}");
        }
        let message = "小说字号缩放必须是 0.75~2.0 之间的数字";
        for bad in [
            json!(0.5),
            json!(2.5),
            json!(9.9),
            json!("abc"),
            json!(true),
            json!(null),
        ] {
            assert_eq!(
                validate_novel_font_scale_value(&bad).unwrap_err(),
                message,
                "bad={bad}"
            );
        }
    }

    #[test]
    fn invalid_novel_bg_color_falls_back_to_default_on_load() {
        let dir = temp_config_dir("novelbg");
        // 手改 settings.json 写入白名单外的值 → 加载期回落空串（跟随主题）
        std::fs::write(settings_path(&dir), r#"{"novel_bg_color":"hotpink"}"#).unwrap();
        assert_eq!(Settings::load_or_init(&dir).novel_bg_color, "");
        // 合法值（空串 + 五个语义键）原样保留
        for color in ["", "green", "kraft", "warm", "mist", "blush"] {
            std::fs::write(
                settings_path(&dir),
                format!(r#"{{"novel_bg_color":"{color}"}}"#),
            )
            .unwrap();
            assert_eq!(Settings::load_or_init(&dir).novel_bg_color, color);
        }
        cleanup(&dir);
    }

    #[test]
    fn validate_novel_bg_color_rules() {
        use serde_json::json;
        for ok in ["", "green", "kraft", "warm", "mist", "blush"] {
            assert!(validate_novel_bg_color_value(&json!(ok)).is_ok(), "ok={ok}");
        }
        let message = "阅读背景色无效";
        for bad in [json!("hotpink"), json!(1), json!(true), json!(null)] {
            assert_eq!(
                validate_novel_bg_color_value(&bad).unwrap_err(),
                message,
                "bad={bad}"
            );
        }
    }

    #[test]
    fn invalid_translation_api_format_falls_back_to_default_on_load() {
        let dir = temp_config_dir("translate-format");
        // 手改 settings.json 写入白名单外的协议 → 加载期回落 chat_completions
        std::fs::write(settings_path(&dir), r#"{"translation_api_format":"grpc"}"#).unwrap();
        assert_eq!(
            Settings::load_or_init(&dir).translation_api_format,
            "chat_completions"
        );
        // 合法协议原样保留
        for format in ["responses", "anthropic"] {
            std::fs::write(
                settings_path(&dir),
                format!(r#"{{"translation_api_format":"{format}"}}"#),
            )
            .unwrap();
            assert_eq!(Settings::load_or_init(&dir).translation_api_format, format);
        }
        cleanup(&dir);
    }

    #[test]
    fn translation_timeout_roundtrip_and_invalid_fallback_on_load() {
        let dir = temp_config_dir("translate-timeout");
        // 合法值（含区间两端）原样保留
        for seconds in [
            TRANSLATION_TIMEOUT_MIN_SECONDS,
            900,
            TRANSLATION_TIMEOUT_MAX_SECONDS,
        ] {
            std::fs::write(
                settings_path(&dir),
                format!(r#"{{"translation_timeout_seconds":{seconds}}}"#),
            )
            .unwrap();
            assert_eq!(
                Settings::load_or_init(&dir).translation_timeout_seconds,
                seconds
            );
        }
        // 区间外（含 0 与负数）回落默认：否则 0 秒会让全部翻译请求立刻超时
        for seconds in [0, -1, TRANSLATION_TIMEOUT_MAX_SECONDS + 1] {
            std::fs::write(
                settings_path(&dir),
                format!(r#"{{"translation_timeout_seconds":{seconds}}}"#),
            )
            .unwrap();
            assert_eq!(
                Settings::load_or_init(&dir).translation_timeout_seconds,
                DEFAULT_TRANSLATION_TIMEOUT_SECONDS
            );
        }
        cleanup(&dir);
    }

    #[test]
    fn image_cache_max_roundtrip_and_invalid_fallback_on_load() {
        let dir = temp_config_dir("image-cache-max");
        // 合法值（含区间两端）原样保留
        for mib in [IMAGE_CACHE_MIN_MIB, 512, 1024, IMAGE_CACHE_MAX_MIB] {
            std::fs::write(
                settings_path(&dir),
                format!(r#"{{"image_cache_max_mib":{mib}}}"#),
            )
            .unwrap();
            assert_eq!(Settings::load_or_init(&dir).image_cache_max_mib, mib);
        }
        // 区间外（含 0 与负数）回落默认：否则 0 会让每次缓存写入都触发全量淘汰
        for mib in [0, -1, IMAGE_CACHE_MAX_MIB + 1] {
            std::fs::write(
                settings_path(&dir),
                format!(r#"{{"image_cache_max_mib":{mib}}}"#),
            )
            .unwrap();
            assert_eq!(
                Settings::load_or_init(&dir).image_cache_max_mib,
                DEFAULT_IMAGE_CACHE_MAX_MIB
            );
        }
        cleanup(&dir);
    }

    #[test]
    fn validate_image_cache_max_rules() {
        let message = "图片缓存上限必须是 256~2048 MiB 之间的整数";
        // 区间两端与默认值通过，并返回整数本身
        assert_eq!(
            validate_image_cache_max_value(&serde_json::json!(256)).unwrap(),
            256
        );
        assert_eq!(
            validate_image_cache_max_value(&serde_json::json!(2048)).unwrap(),
            2048
        );
        assert_eq!(
            validate_image_cache_max_value(&serde_json::json!(512)).unwrap(),
            512
        );
        // 越界 / 非整数 / bool / null 全部拒绝，文案精确匹配
        for bad in [255, 2049, 0, -5] {
            assert_eq!(
                validate_image_cache_max_value(&serde_json::json!(bad)).unwrap_err(),
                message
            );
        }
        for bad in [
            serde_json::json!(true),
            serde_json::json!("512"),
            serde_json::json!(null),
            serde_json::json!(512.5),
        ] {
            assert_eq!(
                validate_image_cache_max_value(&bad).unwrap_err(),
                message
            );
        }
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

    #[test]
    fn validate_translation_timeout_rules() {
        let message = "翻译超时时间必须是 30~3600 秒之间的整数";
        // 区间两端与默认值通过，并返回整数本身
        assert_eq!(
            validate_translation_timeout_value(&serde_json::json!(30)).unwrap(),
            30
        );
        assert_eq!(
            validate_translation_timeout_value(&serde_json::json!(3600)).unwrap(),
            3600
        );
        assert_eq!(
            validate_translation_timeout_value(&serde_json::json!(600)).unwrap(),
            600
        );
        // 越界 / 非整数 / bool / null 全部拒绝，文案精确匹配
        for bad in [29, 3601, 0, -5] {
            assert_eq!(
                validate_translation_timeout_value(&serde_json::json!(bad)).unwrap_err(),
                message
            );
        }
        for bad in [
            serde_json::json!(true),
            serde_json::json!("600"),
            serde_json::json!(null),
            serde_json::json!(600.5),
        ] {
            assert_eq!(
                validate_translation_timeout_value(&bad).unwrap_err(),
                message
            );
        }
    }
}
