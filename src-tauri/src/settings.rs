//! Settings —— JSON 配置管理，与旧 Python `storage/settings.py` / `config/settings.json`
//! 完全兼容（snake_case 键、默认值、损坏恢复、旧值迁移）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::filename_template::{self, TemplateKind};
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
    /// 界面语言（"zh-CN" / "en-US" / "ja-JP"）。
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
    /// 插画/漫画详情页图片宽度占比（详情页顶栏缩放控件写入），默认 1.0（撑满舞台），
    /// 合法区间 [`DETAIL_IMAGE_SCALE_MIN`] ~ [`DETAIL_IMAGE_SCALE_MAX`]；
    /// 非法值在加载期回落 1.0。
    pub detail_image_scale: f64,
    /// 小说正文字号缩放（小说阅读器底栏缩放控件写入），默认 1.0，
    /// 合法区间 [0.75, 2.0]；非法值在加载期回落 1.0。
    pub novel_font_scale: f64,
    /// 小说阅读背景色（小说阅读器底栏色块按钮写入），语义键，默认空串 = 跟随主题，
    /// 合法值见 [`NOVEL_BG_COLORS`]；非法值（含手改 settings.json）在加载期回落空串。
    pub novel_bg_color: String,
    /// 插画（多页）下载文件名模板（占位符白名单与校验规则见
    /// [`filename_template`]），默认 [`filename_template::ILLUST_FILENAME_TEMPLATE_DEFAULT`]；
    /// 非法值（含手改 settings.json）在加载期回落默认。
    #[serde(default = "default_illust_filename_template")]
    pub illust_filename_template: String,
    /// 小说（单篇）下载文件名模板，默认
    /// [`filename_template::NOVEL_FILENAME_TEMPLATE_DEFAULT`]；非法值在加载期回落默认。
    #[serde(default = "default_novel_filename_template")]
    pub novel_filename_template: String,
    /// 系列各话下载文件名模板，默认
    /// [`filename_template::NOVEL_SERIES_FILENAME_TEMPLATE_DEFAULT`]；非法值在加载期回落默认。
    #[serde(default = "default_novel_series_filename_template")]
    pub novel_series_filename_template: String,
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
    /// 全局模型请求并发（所有小说共用），默认 4，区间 1~16。
    pub translation_agent_global_concurrency: i64,
    /// 单小说子任务并发，默认 4，区间 1~16（实际受全局上限约束）。
    pub translation_agent_novel_concurrency: i64,
    /// 同时活跃小说数，默认 2，区间 1~8。
    pub translation_agent_active_novels: i64,
    /// 自动网络重试次数（不含首次），默认 2，区间 0~5。
    pub translation_agent_retry_limit: i64,
    /// 自动退避基础延迟（毫秒），默认 1000，区间 500~30000。
    pub translation_agent_backoff_base_ms: i64,
    /// 自动退避上限（秒），默认 120，区间 10~600。
    pub translation_agent_backoff_max_seconds: i64,
    /// 质检返工轮数（修复 + 复检），默认 2，区间 0~5。
    pub translation_agent_rework_rounds: i64,
    /// 超限细分深度，默认 3，区间 0~5。
    pub translation_agent_split_depth: i64,
    /// 工具参数 / 结构纠正次数，默认 2，区间 0~3。
    pub translation_agent_tool_corrections: i64,
    /// 单子任务模型轮次，默认 24，区间 4~128。
    pub translation_agent_max_turns: i64,
    /// 主 Agent 无进展上限（轮），默认 8，区间 3~32。
    pub translation_agent_stall_limit: i64,
    /// 未知模型上下文窗口（token），默认 131072，区间 16384~2097152。
    pub translation_agent_context_window: i64,
    /// 单请求输出上限（token），默认 16384，区间 1024~262144，且不超过上下文窗口。
    pub translation_agent_output_cap: i64,
    /// 自动压缩触发阈值（可用输入预算百分比），默认 75，区间 50~90。
    pub translation_agent_compact_percent: i64,
    /// 保留近期历史（token），默认 4096，区间 512~131072。
    pub translation_agent_recent_history_tokens: i64,
    /// 摘要目标（token），默认 2048，区间 256~65536。
    pub translation_agent_summary_tokens: i64,
    /// 分块目标字符数，默认 3000，区间 500~100000，且不超过字符上限。
    pub translation_agent_chunk_target_chars: i64,
    /// 分块字符上限，默认 6000，区间 1000~200000。
    pub translation_agent_chunk_max_chars: i64,
    /// 邻接参考窗口（前后各多少字符），默认 600，区间 0~20000。
    pub translation_agent_context_chars: i64,
    /// 整篇累计模型尝试预算；0 = 自动公式（32 + 16 × 初始分块数），
    /// 自定义正整数区间 1~1000000。
    pub translation_call_budget: i64,
    /// 请求结果未知时是否自动重放；默认关闭（可能重复计费）。
    pub translation_agent_replay_unknown: bool,
    /// 启动前确认术语与风格；默认关闭，浮层可对本次任务覆盖。
    pub translation_confirm_before_start: bool,
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
            detail_image_scale: 1.0,
            novel_font_scale: 1.0,
            novel_bg_color: String::new(),
            illust_filename_template: default_illust_filename_template(),
            novel_filename_template: default_novel_filename_template(),
            novel_series_filename_template: default_novel_series_filename_template(),
            saucenao_api_key: String::new(),
            translation_api_url: String::new(),
            translation_api_format: "chat_completions".into(),
            translation_model: String::new(),
            translation_target_language: String::new(),
            translation_timeout_seconds: DEFAULT_TRANSLATION_TIMEOUT_SECONDS,
            translation_extra: serde_json::json!({}),
            translation_agent_global_concurrency: 4,
            translation_agent_novel_concurrency: 4,
            translation_agent_active_novels: 2,
            translation_agent_retry_limit: 2,
            translation_agent_backoff_base_ms: 1_000,
            translation_agent_backoff_max_seconds: 120,
            translation_agent_rework_rounds: 2,
            translation_agent_split_depth: 3,
            translation_agent_tool_corrections: 2,
            translation_agent_max_turns: 24,
            translation_agent_stall_limit: 8,
            translation_agent_context_window: 131_072,
            translation_agent_output_cap: 16_384,
            translation_agent_compact_percent: 75,
            translation_agent_recent_history_tokens: 4_096,
            translation_agent_summary_tokens: 2_048,
            translation_agent_chunk_target_chars: 3_000,
            translation_agent_chunk_max_chars: 6_000,
            translation_agent_context_chars: 600,
            translation_call_budget: 0,
            translation_agent_replay_unknown: false,
            translation_confirm_before_start: false,
        }
    }
}

/// serde default：插画文件名模板返回模块默认常量。
fn default_illust_filename_template() -> String {
    filename_template::ILLUST_FILENAME_TEMPLATE_DEFAULT.into()
}

/// serde default：小说文件名模板返回模块默认常量。
fn default_novel_filename_template() -> String {
    filename_template::NOVEL_FILENAME_TEMPLATE_DEFAULT.into()
}

/// serde default：系列文件名模板返回模块默认常量。
fn default_novel_series_filename_template() -> String {
    filename_template::NOVEL_SERIES_FILENAME_TEMPLATE_DEFAULT.into()
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
/// 详情页图片缩放下限（= 撑满舞台宽度的一半，再小已不可读）。
pub const DETAIL_IMAGE_SCALE_MIN: f64 = 0.5;
/// 详情页图片缩放上限（1.0 = 撑满舞台宽度，即既有默认观感）。
pub const DETAIL_IMAGE_SCALE_MAX: f64 = 1.0;
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

/// 通用 Agent 运行参数的整数区间定义。
///
/// 加载期回落与保存期校验共用本表（单一真相源），避免两处区间漂移。
#[derive(Debug, Clone, Copy)]
pub struct TranslationIntRule {
    /// settings.json 键名。
    pub key: &'static str,
    /// 缺省 / 越界回落值。
    pub default: i64,
    /// 合法区间下限（含）。
    pub min: i64,
    /// 合法区间上限（含）。
    pub max: i64,
    /// 中文提示用的字段名。
    pub label: &'static str,
}

impl TranslationIntRule {
    pub fn contains(self, value: i64) -> bool {
        (self.min..=self.max).contains(&value)
    }

    pub fn message(self) -> String {
        format!("{}必须是 {}~{} 之间的整数", self.label, self.min, self.max)
    }
}

/// 计划 §7.1 的运行参数（整数部分）。`translation_call_budget` 的 0 表示自动。
pub const TRANSLATION_AGENT_INT_RULES: &[TranslationIntRule] = &[
    TranslationIntRule {
        key: "translation_agent_global_concurrency",
        default: 4,
        min: 1,
        max: 16,
        label: "全局模型请求并发",
    },
    TranslationIntRule {
        key: "translation_agent_novel_concurrency",
        default: 4,
        min: 1,
        max: 16,
        label: "单小说子任务并发",
    },
    TranslationIntRule {
        key: "translation_agent_active_novels",
        default: 2,
        min: 1,
        max: 8,
        label: "同时活跃小说数",
    },
    TranslationIntRule {
        key: "translation_agent_retry_limit",
        default: 2,
        min: 0,
        max: 5,
        label: "自动网络重试次数",
    },
    TranslationIntRule {
        key: "translation_agent_backoff_base_ms",
        default: 1_000,
        min: 500,
        max: 30_000,
        label: "自动退避基础延迟（毫秒）",
    },
    TranslationIntRule {
        key: "translation_agent_backoff_max_seconds",
        default: 120,
        min: 10,
        max: 600,
        label: "自动退避上限（秒）",
    },
    TranslationIntRule {
        key: "translation_agent_rework_rounds",
        default: 2,
        min: 0,
        max: 5,
        label: "质检返工轮数",
    },
    TranslationIntRule {
        key: "translation_agent_split_depth",
        default: 3,
        min: 0,
        max: 5,
        label: "超限细分深度",
    },
    TranslationIntRule {
        key: "translation_agent_tool_corrections",
        default: 2,
        min: 0,
        max: 3,
        label: "工具参数纠错次数",
    },
    TranslationIntRule {
        key: "translation_agent_max_turns",
        default: 24,
        min: 4,
        max: 128,
        label: "单子任务模型轮次",
    },
    TranslationIntRule {
        key: "translation_agent_stall_limit",
        default: 8,
        min: 3,
        max: 32,
        label: "主 Agent 无进展上限",
    },
    TranslationIntRule {
        key: "translation_agent_context_window",
        default: 131_072,
        min: 16_384,
        max: 2_097_152,
        label: "未知模型上下文窗口",
    },
    TranslationIntRule {
        key: "translation_agent_output_cap",
        default: 16_384,
        min: 1_024,
        max: 262_144,
        label: "单请求输出上限",
    },
    TranslationIntRule {
        key: "translation_agent_compact_percent",
        default: 75,
        min: 50,
        max: 90,
        label: "自动压缩触发百分比",
    },
    TranslationIntRule {
        key: "translation_agent_recent_history_tokens",
        default: 4_096,
        min: 512,
        max: 131_072,
        label: "保留近期历史 token",
    },
    TranslationIntRule {
        key: "translation_agent_summary_tokens",
        default: 2_048,
        min: 256,
        max: 65_536,
        label: "摘要目标 token",
    },
    TranslationIntRule {
        key: "translation_agent_chunk_target_chars",
        default: 3_000,
        min: 500,
        max: 100_000,
        label: "分块目标字符数",
    },
    TranslationIntRule {
        key: "translation_agent_chunk_max_chars",
        default: 6_000,
        min: 1_000,
        max: 200_000,
        label: "分块字符上限",
    },
    TranslationIntRule {
        key: "translation_agent_context_chars",
        default: 600,
        min: 0,
        max: 20_000,
        label: "邻接参考窗口字符数",
    },
    TranslationIntRule {
        key: "translation_call_budget",
        default: 0,
        min: 0,
        max: 1_000_000,
        label: "整篇累计模型调用预算（0 = 自动）",
    },
];

/// 布尔运行参数键；保存时只校验类型。
pub const TRANSLATION_AGENT_BOOL_KEYS: [&str; 2] = [
    "translation_agent_replay_unknown",
    "translation_confirm_before_start",
];

/// 按键查整数区间定义。
pub fn translation_agent_int_rule(key: &str) -> Option<&'static TranslationIntRule> {
    TRANSLATION_AGENT_INT_RULES
        .iter()
        .find(|rule| rule.key == key)
}

/// JSON 值形式的运行参数校验；未知键返回 `None`。
pub fn validate_translation_agent_int_value(
    key: &str,
    value: &Value,
) -> Option<Result<i64, String>> {
    let rule = translation_agent_int_rule(key)?;
    Some(match value.as_i64() {
        Some(n) if !value.is_boolean() && rule.contains(n) => Ok(n),
        _ => Err(rule.message()),
    })
}

/// 保存前交叉校验：单项区间之外，还要保证输出 / 摘要 / 分块预算自洽。
pub fn validate_translation_agent_settings(settings: &Settings) -> Result<(), String> {
    for rule in TRANSLATION_AGENT_INT_RULES {
        let value = settings.agent_int(rule.key).unwrap_or(rule.default);
        if !rule.contains(value) {
            return Err(rule.message());
        }
    }
    if settings.translation_agent_chunk_target_chars > settings.translation_agent_chunk_max_chars {
        return Err("分块目标字符数不能超过分块字符上限".into());
    }
    if settings.translation_agent_output_cap > settings.translation_agent_context_window {
        return Err("单请求输出上限不能超过未知模型上下文窗口".into());
    }
    if settings.translation_agent_recent_history_tokens + settings.translation_agent_summary_tokens
        > settings.translation_agent_context_window
    {
        return Err("保留近期历史与摘要目标之和不能超过上下文窗口".into());
    }
    Ok(())
}

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

/// 单个下载文件名模板的加载期归一：非法值记日志并回落对应用途的默认模板
/// （对齐缩略图档位等「非法值回落」做法，不强制回写文件）。
fn normalize_filename_template(value: &mut String, kind: TemplateKind, key: &str) {
    if let Err(err) = filename_template::validate_template(value, kind) {
        eprintln!("settings.json 的 {key} 非法（{err}），回落默认模板");
        *value = match kind {
            TemplateKind::Illust => default_illust_filename_template(),
            TemplateKind::Novel => default_novel_filename_template(),
            TemplateKind::NovelSeries => default_novel_series_filename_template(),
        };
    }
}

impl Settings {
    /// 从 `config_dir/settings.json` 加载：
    /// - 文件不存在 → 写入默认值并返回
    /// - JSON 损坏 / 读失败 → 备份为 `settings.json.corrupt-{mtime_ns}` 后重建默认
    /// - 旧值迁移：`output_dir == "downloads"`（早期默认相对路径）→ 系统下载目录/pixiv-tool
    /// - 加载期归一：非法启动页、缩略图档位、小说字号缩放、阅读背景色、
    ///   下载文件名模板重置为默认（不强制回写文件）
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
        // 记录文件里实际出现的键：serde 的容器级 default 对缺失整数给 0，
        // 而 0 对部分运行参数是合法值，不能靠越界判定识别「缺键」。
        let present: std::collections::HashSet<String> = serde_json::from_str::<Value>(&raw)
            .ok()
            .and_then(|value| {
                value
                    .as_object()
                    .map(|object| object.keys().cloned().collect())
            })
            .unwrap_or_default();
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
                if !(IMAGE_CACHE_MIN_MIB..=IMAGE_CACHE_MAX_MIB)
                    .contains(&settings.image_cache_max_mib)
                {
                    settings.image_cache_max_mib = DEFAULT_IMAGE_CACHE_MAX_MIB;
                }
                // 详情页图片缩放：非有限值（NaN/inf，JSON 文本层面不可表达，纯防御）
                // 或越出 [0.5, 1.0] 回落默认 1.0（对齐小说字号缩放的加载回落做法）。
                if !settings.detail_image_scale.is_finite()
                    || !(DETAIL_IMAGE_SCALE_MIN..=DETAIL_IMAGE_SCALE_MAX)
                        .contains(&settings.detail_image_scale)
                {
                    settings.detail_image_scale = 1.0;
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
                // 翻译上下文窗口：32768 是 1.2.x 的旧默认值，一次性升级到 131072；
                // 显式改过其他值的用户不受影响（可在设置中改回）。
                if settings.translation_agent_context_window == 32_768 {
                    settings.translation_agent_context_window = 131_072;
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
                // 下载文件名模板：手改 settings.json 写入非法模板（未知占位符 /
                // 非法字符 / 缺必含项等）回落默认，否则每次下载都会渲染出异常文件名。
                normalize_filename_template(
                    &mut settings.illust_filename_template,
                    TemplateKind::Illust,
                    "illust_filename_template",
                );
                normalize_filename_template(
                    &mut settings.novel_filename_template,
                    TemplateKind::Novel,
                    "novel_filename_template",
                );
                normalize_filename_template(
                    &mut settings.novel_series_filename_template,
                    TemplateKind::NovelSeries,
                    "novel_series_filename_template",
                );
                settings.normalize_translation_agent(&present);
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

    /// 按键取运行参数整数值；未知键返回 `None`。
    fn agent_int(&self, key: &str) -> Option<i64> {
        Some(match key {
            "translation_agent_global_concurrency" => self.translation_agent_global_concurrency,
            "translation_agent_novel_concurrency" => self.translation_agent_novel_concurrency,
            "translation_agent_active_novels" => self.translation_agent_active_novels,
            "translation_agent_retry_limit" => self.translation_agent_retry_limit,
            "translation_agent_backoff_base_ms" => self.translation_agent_backoff_base_ms,
            "translation_agent_backoff_max_seconds" => self.translation_agent_backoff_max_seconds,
            "translation_agent_rework_rounds" => self.translation_agent_rework_rounds,
            "translation_agent_split_depth" => self.translation_agent_split_depth,
            "translation_agent_tool_corrections" => self.translation_agent_tool_corrections,
            "translation_agent_max_turns" => self.translation_agent_max_turns,
            "translation_agent_stall_limit" => self.translation_agent_stall_limit,
            "translation_agent_context_window" => self.translation_agent_context_window,
            "translation_agent_output_cap" => self.translation_agent_output_cap,
            "translation_agent_compact_percent" => self.translation_agent_compact_percent,
            "translation_agent_recent_history_tokens" => {
                self.translation_agent_recent_history_tokens
            }
            "translation_agent_summary_tokens" => self.translation_agent_summary_tokens,
            "translation_agent_chunk_target_chars" => self.translation_agent_chunk_target_chars,
            "translation_agent_chunk_max_chars" => self.translation_agent_chunk_max_chars,
            "translation_agent_context_chars" => self.translation_agent_context_chars,
            "translation_call_budget" => self.translation_call_budget,
            _ => return None,
        })
    }

    /// 按键取运行参数可变引用；未知键返回 `None`。
    fn agent_int_mut(&mut self, key: &str) -> Option<&mut i64> {
        Some(match key {
            "translation_agent_global_concurrency" => {
                &mut self.translation_agent_global_concurrency
            }
            "translation_agent_novel_concurrency" => &mut self.translation_agent_novel_concurrency,
            "translation_agent_active_novels" => &mut self.translation_agent_active_novels,
            "translation_agent_retry_limit" => &mut self.translation_agent_retry_limit,
            "translation_agent_backoff_base_ms" => &mut self.translation_agent_backoff_base_ms,
            "translation_agent_backoff_max_seconds" => {
                &mut self.translation_agent_backoff_max_seconds
            }
            "translation_agent_rework_rounds" => &mut self.translation_agent_rework_rounds,
            "translation_agent_split_depth" => &mut self.translation_agent_split_depth,
            "translation_agent_tool_corrections" => &mut self.translation_agent_tool_corrections,
            "translation_agent_max_turns" => &mut self.translation_agent_max_turns,
            "translation_agent_stall_limit" => &mut self.translation_agent_stall_limit,
            "translation_agent_context_window" => &mut self.translation_agent_context_window,
            "translation_agent_output_cap" => &mut self.translation_agent_output_cap,
            "translation_agent_compact_percent" => &mut self.translation_agent_compact_percent,
            "translation_agent_recent_history_tokens" => {
                &mut self.translation_agent_recent_history_tokens
            }
            "translation_agent_summary_tokens" => &mut self.translation_agent_summary_tokens,
            "translation_agent_chunk_target_chars" => {
                &mut self.translation_agent_chunk_target_chars
            }
            "translation_agent_chunk_max_chars" => &mut self.translation_agent_chunk_max_chars,
            "translation_agent_context_chars" => &mut self.translation_agent_context_chars,
            "translation_call_budget" => &mut self.translation_call_budget,
            _ => return None,
        })
    }

    /// 加载期归一：缺键或越界回落默认并记录；交叉约束冲突时整组回落默认，
    /// 不让手改 settings.json 写出会让运行层立刻拒绝的组合。
    fn normalize_translation_agent(&mut self, present: &std::collections::HashSet<String>) {
        for rule in TRANSLATION_AGENT_INT_RULES {
            let Some(value) = self.agent_int_mut(rule.key) else {
                continue;
            };
            if !present.contains(rule.key) {
                // 旧版 settings.json 没有这些键：serde 给的是 0，必须回填计划默认值。
                *value = rule.default;
                continue;
            }
            if !rule.contains(*value) {
                eprintln!(
                    "settings.json 的 {} 越界（{}），回落默认 {}",
                    rule.key, *value, rule.default
                );
                *value = rule.default;
            }
        }
        for key in TRANSLATION_AGENT_BOOL_KEYS {
            if !present.contains(key) {
                match key {
                    "translation_agent_replay_unknown" => {
                        self.translation_agent_replay_unknown = false;
                    }
                    "translation_confirm_before_start" => {
                        self.translation_confirm_before_start = false;
                    }
                    _ => {}
                }
            }
        }
        if self.translation_agent_chunk_target_chars > self.translation_agent_chunk_max_chars {
            eprintln!("settings.json 的分块目标超过字符上限，两项回落默认");
            self.translation_agent_chunk_target_chars = 3_000;
            self.translation_agent_chunk_max_chars = 6_000;
        }
        if self.translation_agent_output_cap > self.translation_agent_context_window {
            eprintln!("settings.json 的输出上限超过上下文窗口，回落默认");
            self.translation_agent_output_cap = 16_384.min(self.translation_agent_context_window);
        }
        if self.translation_agent_recent_history_tokens + self.translation_agent_summary_tokens
            > self.translation_agent_context_window
        {
            eprintln!("settings.json 的近期历史与摘要目标超过上下文窗口，两项回落默认");
            self.translation_agent_recent_history_tokens = 4_096;
            self.translation_agent_summary_tokens = 2_048;
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
        Some(n)
            if (TRANSLATION_TIMEOUT_MIN_SECONDS..=TRANSLATION_TIMEOUT_MAX_SECONDS).contains(&n) =>
        {
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
    let message =
        || format!("图片缓存上限必须是 {IMAGE_CACHE_MIN_MIB}~{IMAGE_CACHE_MAX_MIB} MiB 之间的整数");
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

/// JSON 值形式的详情页图片缩放校验：bool / 非数字 / 非有限值（NaN/inf）/ 越界都拒绝。
pub fn validate_detail_image_scale_value(v: &Value) -> Result<(), String> {
    let message = "详情页图片缩放必须是 0.5~1.0 之间的数字";
    let Some(scale) = v.as_f64().filter(|s| s.is_finite()) else {
        return Err(message.into());
    };
    if (DETAIL_IMAGE_SCALE_MIN..=DETAIL_IMAGE_SCALE_MAX).contains(&scale) {
        Ok(())
    } else {
        Err(message.into())
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

/// JSON 值形式的下载文件名模板校验：非字符串拒绝；内容按用途经
/// [`filename_template::validate_template`] 校验（错误文案可直接展示给用户）。
pub fn validate_filename_template_value(v: &Value, kind: TemplateKind) -> Result<(), String> {
    match v.as_str() {
        Some(template) => filename_template::validate_template(template, kind),
        None => Err("文件名模板必须是字符串".to_string()),
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
        assert_eq!(s.detail_image_scale, 1.0);
        assert_eq!(s.novel_font_scale, 1.0);
        assert_eq!(s.novel_bg_color, "");
        assert_eq!(
            s.illust_filename_template,
            filename_template::ILLUST_FILENAME_TEMPLATE_DEFAULT
        );
        assert_eq!(
            s.novel_filename_template,
            filename_template::NOVEL_FILENAME_TEMPLATE_DEFAULT
        );
        assert_eq!(
            s.novel_series_filename_template,
            filename_template::NOVEL_SERIES_FILENAME_TEMPLATE_DEFAULT
        );
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
        assert_eq!(s.detail_image_scale, 1.0);
        assert_eq!(s.novel_font_scale, 1.0);
        assert_eq!(s.novel_bg_color, "");
        assert_eq!(
            s.illust_filename_template,
            filename_template::ILLUST_FILENAME_TEMPLATE_DEFAULT
        );
        assert_eq!(
            s.novel_filename_template,
            filename_template::NOVEL_FILENAME_TEMPLATE_DEFAULT
        );
        assert_eq!(
            s.novel_series_filename_template,
            filename_template::NOVEL_SERIES_FILENAME_TEMPLATE_DEFAULT
        );
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
    fn invalid_detail_image_scale_falls_back_to_default_on_load() {
        let dir = temp_config_dir("detailimg");
        // 手改 settings.json 写入越界值（含 NaN 语义不可表达的上界外）→ 加载期回落 1.0
        for bad in [9.9, 0.1, 0.0] {
            std::fs::write(
                settings_path(&dir),
                format!(r#"{{"detail_image_scale":{bad}}}"#),
            )
            .unwrap();
            assert_eq!(Settings::load_or_init(&dir).detail_image_scale, 1.0);
        }
        // 合法值（含边界）原样保留
        for scale in [0.5, 0.75, 1.0] {
            std::fs::write(
                settings_path(&dir),
                format!(r#"{{"detail_image_scale":{scale}}}"#),
            )
            .unwrap();
            assert_eq!(Settings::load_or_init(&dir).detail_image_scale, scale);
        }
        cleanup(&dir);
    }

    #[test]
    fn validate_detail_image_scale_rules() {
        use serde_json::json;
        for ok in [json!(1.0), json!(0.5), json!(1), json!(0.75)] {
            assert!(validate_detail_image_scale_value(&ok).is_ok(), "ok={ok}");
        }
        let message = "详情页图片缩放必须是 0.5~1.0 之间的数字";
        for bad in [
            json!(0.4),
            json!(1.5),
            json!(9.9),
            json!("abc"),
            json!(true),
            json!(null),
        ] {
            assert_eq!(
                validate_detail_image_scale_value(&bad).unwrap_err(),
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
    fn filename_template_roundtrip_and_invalid_fallback_on_load() {
        use serde_json::json;
        let dir = temp_config_dir("filename-template");
        // 合法自定义模板（各用途各一）原样保留
        std::fs::write(
            settings_path(&dir),
            r#"{"illust_filename_template":"{user}_{pid}_p{page1}","novel_filename_template":"{nid}_{title}","novel_series_filename_template":"{sid}_{order}_{title}"}"#,
        )
        .unwrap();
        let s = Settings::load_or_init(&dir);
        assert_eq!(s.illust_filename_template, "{user}_{pid}_p{page1}");
        assert_eq!(s.novel_filename_template, "{nid}_{title}");
        assert_eq!(s.novel_series_filename_template, "{sid}_{order}_{title}");
        // 手改 settings.json 写入非法模板 → 加载期回落默认。键与用途一一对应：
        // 系列模板缺 {order} 判非法，即使同一串对小说单篇合法。
        let bad = json!({
            "illust_filename_template": "{foo}_{pid}_p{page}",
            "novel_filename_template": "",
            "novel_series_filename_template": "{title}_{nid}"
        });
        std::fs::write(settings_path(&dir), bad.to_string()).unwrap();
        let s = Settings::load_or_init(&dir);
        assert_eq!(
            s.illust_filename_template,
            filename_template::ILLUST_FILENAME_TEMPLATE_DEFAULT
        );
        assert_eq!(
            s.novel_filename_template,
            filename_template::NOVEL_FILENAME_TEMPLATE_DEFAULT
        );
        assert_eq!(
            s.novel_series_filename_template,
            filename_template::NOVEL_SERIES_FILENAME_TEMPLATE_DEFAULT
        );
        cleanup(&dir);
    }

    #[test]
    fn validate_filename_template_rules() {
        use serde_json::json;
        // 默认模板各自用途全部通过
        assert!(
            validate_filename_template_value(
                &json!(filename_template::ILLUST_FILENAME_TEMPLATE_DEFAULT),
                TemplateKind::Illust
            )
            .is_ok()
        );
        assert!(
            validate_filename_template_value(
                &json!(filename_template::NOVEL_FILENAME_TEMPLATE_DEFAULT),
                TemplateKind::Novel
            )
            .is_ok()
        );
        assert!(
            validate_filename_template_value(
                &json!(filename_template::NOVEL_SERIES_FILENAME_TEMPLATE_DEFAULT),
                TemplateKind::NovelSeries
            )
            .is_ok()
        );
        // 非字符串拒绝（文案统一）
        for bad in [json!(42), json!(true), json!(null)] {
            assert_eq!(
                validate_filename_template_value(&bad, TemplateKind::Illust).unwrap_err(),
                "文件名模板必须是字符串",
                "bad={bad}"
            );
        }
        // 各类非法内容：未知占位符 / 非法字符 / 缺必含项 / 空串
        for (template, kind, message_part) in [
            ("{foo}_{pid}_p{page}", TemplateKind::Illust, "未知占位符"),
            (
                "{title}_{pid}_p{page}/x",
                TemplateKind::Illust,
                "不允许的字符",
            ),
            ("{title}_{pid}", TemplateKind::Illust, "必须包含 {page}"),
            (
                "{title}_{nid}",
                TemplateKind::NovelSeries,
                "必须包含 {order}",
            ),
            ("", TemplateKind::Novel, "模板不能为空"),
        ] {
            let err = validate_filename_template_value(&json!(template), kind).unwrap_err();
            assert!(
                err.contains(message_part),
                "template={template:?} kind={kind:?} err={err}"
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
            assert_eq!(validate_image_cache_max_value(&bad).unwrap_err(), message);
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

    #[test]
    fn translation_agent_defaults_match_plan() {
        let settings = Settings::default();
        assert_eq!(settings.translation_agent_global_concurrency, 4);
        assert_eq!(settings.translation_agent_novel_concurrency, 4);
        assert_eq!(settings.translation_agent_active_novels, 2);
        assert_eq!(settings.translation_agent_retry_limit, 2);
        assert_eq!(settings.translation_agent_backoff_base_ms, 1_000);
        assert_eq!(settings.translation_agent_backoff_max_seconds, 120);
        assert_eq!(settings.translation_agent_rework_rounds, 2);
        assert_eq!(settings.translation_agent_split_depth, 3);
        assert_eq!(settings.translation_agent_tool_corrections, 2);
        assert_eq!(settings.translation_agent_max_turns, 24);
        assert_eq!(settings.translation_agent_stall_limit, 8);
        assert_eq!(settings.translation_agent_context_window, 131_072);
        assert_eq!(settings.translation_agent_output_cap, 16_384);
        assert_eq!(settings.translation_agent_compact_percent, 75);
        assert_eq!(settings.translation_agent_recent_history_tokens, 4_096);
        assert_eq!(settings.translation_agent_summary_tokens, 2_048);
        assert_eq!(settings.translation_agent_chunk_target_chars, 3_000);
        assert_eq!(settings.translation_agent_chunk_max_chars, 6_000);
        assert_eq!(settings.translation_agent_context_chars, 600);
        assert_eq!(settings.translation_call_budget, 0, "0 = 自动预算");
        assert!(!settings.translation_agent_replay_unknown);
        assert!(!settings.translation_confirm_before_start);
        validate_translation_agent_settings(&settings).unwrap();
    }

    #[test]
    fn translation_agent_out_of_range_falls_back_on_load() {
        let dir = temp_config_dir("agent-range");
        std::fs::write(
            settings_path(&dir),
            r#"{
                "translation_agent_global_concurrency": 0,
                "translation_agent_context_window": 100,
                "translation_agent_chunk_target_chars": 9999,
                "translation_agent_chunk_max_chars": 6000,
                "translation_call_budget": -1
            }"#,
        )
        .unwrap();
        let settings = Settings::load_or_init(&dir);
        assert_eq!(settings.translation_agent_global_concurrency, 4);
        assert_eq!(settings.translation_agent_context_window, 131_072);
        // 交叉冲突（目标 > 上限）整组回落默认。
        assert_eq!(settings.translation_agent_chunk_target_chars, 3_000);
        assert_eq!(settings.translation_agent_chunk_max_chars, 6_000);
        assert_eq!(settings.translation_call_budget, 0);
        // 文件里缺失的键回填计划默认值，而不是 serde 的整数 0。
        assert_eq!(settings.translation_agent_retry_limit, 2);
        assert_eq!(settings.translation_agent_split_depth, 3);
        assert_eq!(settings.translation_agent_tool_corrections, 2);
        assert_eq!(settings.translation_agent_context_chars, 600);
        assert_eq!(settings.translation_agent_recent_history_tokens, 4_096);
        assert!(!settings.translation_agent_replay_unknown);
        cleanup(&dir);
    }

    #[test]
    fn translation_agent_cross_validation_rejects_conflicts() {
        let mut settings = Settings::default();
        settings.translation_agent_output_cap = 100_000;
        settings.translation_agent_context_window = 32_768;
        assert_eq!(
            validate_translation_agent_settings(&settings).unwrap_err(),
            "单请求输出上限不能超过未知模型上下文窗口"
        );
        let mut settings = Settings::default();
        settings.translation_agent_chunk_target_chars = 10_000;
        assert_eq!(
            validate_translation_agent_settings(&settings).unwrap_err(),
            "分块目标字符数不能超过分块字符上限"
        );
    }
}
