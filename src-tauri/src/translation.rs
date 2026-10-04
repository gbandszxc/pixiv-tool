//! 单页两轮翻译：本机持久化设定集，已锁定译名只增不改。
use crate::{settings::Settings, state::AppState};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{State, ipc::Channel};

const PREPARE_PROMPT: &str = include_str!("translation_prepare.txt");
const TRANSLATE_PROMPT: &str = include_str!("translation_render.txt");
const KEY_ACCOUNT: &str = "novel-translation-api-key";

#[derive(Clone, Deserialize, Serialize)]
pub struct NovelInput {
    pub novel_id: i64,
    pub title: String,
    pub tags: Vec<String>,
    pub description: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Term {
    pub source: String,
    pub translation: String,
    pub kind: String,
    pub aliases: Vec<String>,
    pub notes: String,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoryBible {
    pub style: String,
    pub terms: Vec<Term>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TranslatedLine {
    pub line: usize,
    pub text: String,
}

#[derive(Default, Deserialize, Serialize)]
pub struct TranslationBook {
    pub bible: StoryBible,
    pub pages: BTreeMap<usize, Vec<TranslatedLine>>,
}

#[derive(Serialize)]
struct SourceLine {
    line: usize,
    text: String,
}

fn key_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new("pixiv-tool", KEY_ACCOUNT).map_err(|_| "无法访问翻译凭据存储".into())
}

pub fn read_api_key() -> Result<Option<String>, String> {
    match key_entry()?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("无法读取翻译 API Key，请检查系统凭据存储".into()),
    }
}

pub fn write_api_key(key: &str) -> Result<(), String> {
    let entry = key_entry()?;
    if key.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("无法清除翻译 API Key".into()),
        }
    } else {
        entry
            .set_password(key)
            .map_err(|_| "无法保存翻译 API Key".into())
    }
}

/// URL 可以是 API 基址或完整 chat/completions 端点；HTTP 仅允许本机模型。
pub fn completion_url(value: &str) -> Result<tauri::Url, String> {
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
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/chat/completions") {
        let path = if path.is_empty() {
            "/v1/chat/completions".into()
        } else {
            format!("{path}/chat/completions")
        };
        url.set_path(&path);
    }
    Ok(url)
}

pub fn validate_options(value: &Value) -> Result<(), String> {
    let object = value.as_object().ok_or("翻译高级 JSON 必须是对象")?;
    // 请求的边界由应用控制；其余供应商扩展（包括嵌套 thinking）原样传递。
    const RESERVED: &[&str] = &[
        "model",
        "messages",
        "stream",
        "stream_options",
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
    if object
        .keys()
        .any(|key| RESERVED.contains(&key.to_ascii_lowercase().as_str()))
    {
        return Err("高级 JSON 不能覆盖模型、消息、输出格式、流式、工具或凭据字段".into());
    }
    if value.to_string().len() > 16_384 {
        return Err("翻译高级 JSON 不能超过 16KB".into());
    }
    Ok(())
}

/// 目标语言白名单：code → 提示词中的语言名（附原文名便于模型对齐）。
/// code 同时用于「原文已是目标语言」的快速判定与前端选择项。
pub const TARGET_LANGUAGES: [(&str, &str); 9] = [
    ("zh-CN", "简体中文"),
    ("zh-TW", "繁體中文"),
    ("en", "英文（English）"),
    ("ja", "日文（日本語）"),
    ("ko", "韩文（한국어）"),
    ("es", "西班牙文（Español）"),
    ("fr", "法文（Français）"),
    ("de", "德文（Deutsch）"),
    ("ru", "俄文（Русский）"),
];

/// 归一化语言标签：忽略大小写与地区写法，兼容界面语言的 en-US、zh-Hant。
fn normalize_language_code(value: &str) -> String {
    let text = value.trim().replace('_', "-").to_ascii_lowercase();
    let base = text.split('-').next().unwrap_or_default();
    match base {
        "zh" if text.contains("tw") || text.contains("hk") || text.contains("hant") => {
            "zh-TW".into()
        }
        "zh" => "zh-CN".into(),
        other => other.to_string(),
    }
}

/// 本次生效的目标语言（code, 提示词语言名）：设置留空跟随界面语言；不在白名单内即拒绝。
fn resolve_target_language(settings: &Settings) -> Result<(&'static str, &'static str), String> {
    let raw = if settings.translation_target_language.trim().is_empty() {
        settings.language.trim()
    } else {
        settings.translation_target_language.trim()
    };
    let code = normalize_language_code(raw);
    TARGET_LANGUAGES
        .iter()
        .find(|(candidate, _)| *candidate == code)
        .copied()
        .ok_or_else(|| "翻译目标语言无效，请在设置中选择".into())
}

/// 原文主导语言；只服务「是否需要翻译」的快速判定，不做精确识别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceLanguage {
    Japanese,
    Korean,
    /// 中文；用字倾向只区分简繁，用于匹配 zh-CN / zh-TW。
    Chinese { traditional: bool },
    Russian,
    /// 拉丁文字：无法区分具体语种，只用于匹配 en。
    Latin,
    Unknown,
}

/// 仅繁体 / 仅简体用字（覆盖常用字，不追求完备）。
const TRADITIONAL_ONLY: &str = "們這說國學對時會來為麼種樣個東車過還進開關實應讓覺聽見點萬與專業體發長門問間無舊氣親愛單獨龍馬鳥魚頭風雲電語讀寫書認誰謝請講論議記試課銀錢張陽陰歲邊達遠運連極樂習義處備變層場團園圖聲報擔擁擇掛換據檢樓歡漢決沒淚滿熱環現畫療盡眾禮";
const SIMPLIFIED_ONLY: &str = "们这说国学对时会来为么种样个东车过还进开关实应让觉听见点万与专业体发长门问间无旧气亲爱单独龙马鸟鱼头风云电语读写书认谁谢请讲论议记试课银钱张阳阴岁边达远运连极乐习义处备变层场团园图声报担拥择挂换据检楼欢汉决没泪满热环现画疗尽众礼";

/// 采样正文前 4000 字判定主导语言：假名 / 谚文占比 ≥5% 即判日 / 韩；
/// 汉字占一半以上按简繁用字倾向区分；拉丁、西里尔按主导文字判定。
fn detect_language(text: &str) -> SourceLanguage {
    let (mut kana, mut hangul, mut han, mut latin, mut cyrillic) = (0, 0, 0, 0, 0);
    let (mut traditional, mut simplified) = (0, 0);
    for character in text.chars().take(4000) {
        match character {
            '\u{3040}'..='\u{30ff}' | '\u{31f0}'..='\u{31ff}' => kana += 1,
            '\u{ac00}'..='\u{d7af}' | '\u{1100}'..='\u{11ff}' | '\u{3130}'..='\u{318f}' => hangul += 1,
            '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}' => {
                han += 1;
                traditional += usize::from(TRADITIONAL_ONLY.contains(character));
                simplified += usize::from(SIMPLIFIED_ONLY.contains(character));
            }
            'a'..='z' | 'A'..='Z' => latin += 1,
            '\u{0400}'..='\u{04ff}' => cyrillic += 1,
            _ => {}
        }
    }
    let letters = kana + hangul + han + latin + cyrillic;
    if letters == 0 {
        return SourceLanguage::Unknown;
    }
    if kana * 20 >= letters {
        return SourceLanguage::Japanese;
    }
    if hangul * 20 >= letters {
        return SourceLanguage::Korean;
    }
    if han * 2 >= letters {
        return SourceLanguage::Chinese {
            traditional: traditional > simplified && traditional >= 3,
        };
    }
    if cyrillic * 3 >= letters {
        return SourceLanguage::Russian;
    }
    if latin * 2 >= letters {
        return SourceLanguage::Latin;
    }
    SourceLanguage::Unknown
}

/// 原文语言与目标语言一致。拉丁文字只匹配 en：es / fr / de 无法本地区分，始终走翻译。
fn same_target_language(source: SourceLanguage, target: &str) -> bool {
    match (source, target) {
        (SourceLanguage::Japanese, "ja")
        | (SourceLanguage::Korean, "ko")
        | (SourceLanguage::Russian, "ru")
        | (SourceLanguage::Latin, "en") => true,
        (SourceLanguage::Chinese { traditional }, "zh-CN") => !traditional,
        (SourceLanguage::Chinese { traditional }, "zh-TW") => traditional,
        _ => false,
    }
}

/// 提示词模板注入目标语言名（模板占位 `{target_language}`）。
fn with_target_language(template: &str, target: &str) -> String {
    template.replace("{target_language}", target)
}

pub fn validate_settings(settings: &Settings) -> Result<(), String> {
    if !settings.translation_api_url.trim().is_empty() {
        completion_url(&settings.translation_api_url)?;
    }
    if settings.translation_model.len() > 256
        || settings.translation_model.chars().any(char::is_control)
    {
        return Err("翻译模型 ID 无效".into());
    }
    if !settings.translation_target_language.trim().is_empty() {
        resolve_target_language(settings)?;
    }
    validate_options(&settings.translation_extra)
}

fn pages(input: &NovelInput) -> Result<Vec<String>, String> {
    if input.novel_id <= 0
        || input.content.len() > 8_000_000
        || input.title.len() > 4096
        || input.description.len() > 65_536
        || input.tags.len() > 100
        || input.tags.iter().any(|tag| tag.len() > 1024)
    {
        return Err("小说翻译输入无效或过大".into());
    }
    Ok(input
        .content
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split("[newpage]")
        .map(|page| page.trim().to_string())
        .collect())
}

fn book_path(data_dir: &Path, input: &NovelInput) -> PathBuf {
    // 原文与元信息变化创建新版本，避免旧译文与新段落错配。
    let hash = Sha256::digest(serde_json::to_vec(input).expect("NovelInput 可序列化"));
    data_dir
        .join("translations")
        .join(input.novel_id.to_string())
        .join(format!("{hash:x}.json"))
}

fn read_book(path: &Path) -> Result<TranslationBook, String> {
    match std::fs::read(path) {
        Ok(raw) => serde_json::from_slice(&raw)
            .map_err(|_| "本机翻译记录损坏，请检查 translations 目录".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(TranslationBook::default())
        }
        Err(_) => Err("无法读取本机翻译记录".into()),
    }
}

fn save_book(path: &Path, book: &TranslationBook) -> Result<(), String> {
    let dir = path.parent().ok_or("翻译记录路径无效")?;
    std::fs::create_dir_all(dir).map_err(|_| "无法创建翻译记录目录")?;
    let raw = serde_json::to_vec_pretty(book).map_err(|_| "无法序列化翻译记录")?;
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, raw).map_err(|_| "无法保存翻译记录")?;
    std::fs::rename(&temporary, path).map_err(|_| "无法替换翻译记录".into())
}

/// 后端和阅读器以同一原始行号关联译文；纯图片/跳转行不送去翻译。
fn source_lines(page: &str) -> Vec<SourceLine> {
    let markup =
        regex::Regex::new(r"\[(?:pixivimage|uploadedimage|jump|jumpurl):[^\]]*\]").unwrap();
    page.lines()
        .enumerate()
        .filter_map(|(line, raw)| {
            let text = raw.trim();
            if text.is_empty() || markup.replace_all(text, "").trim().is_empty() {
                return None;
            }
            Some(SourceLine {
                line,
                text: text.into(),
            })
        })
        .collect()
}

fn merge_bible(locked: &StoryBible, mut incoming: StoryBible) -> Result<StoryBible, String> {
    if incoming.style.len() > 4000 || incoming.terms.len() > 1000 {
        return Err("模型设定集过大".into());
    }
    let mut merged = locked.clone();
    if merged.style.is_empty() {
        merged.style = incoming.style;
    }
    for mut term in incoming.terms.drain(..) {
        term.source = term.source.trim().into();
        term.translation = term.translation.trim().into();
        if term.source.is_empty()
            || term.translation.is_empty()
            || term.source.len() > 512
            || term.translation.len() > 512
            || term.notes.len() > 2000
            || term.kind.len() > 100
            || term.aliases.len() > 50
            || term
                .aliases
                .iter()
                .any(|alias| alias.trim().is_empty() || alias.len() > 512)
        {
            return Err("模型设定集字段无效".into());
        }
        let index = merged
            .terms
            .iter()
            .position(|old| old.source == term.source || old.aliases.contains(&term.source));
        if let Some(index) = index {
            let old = &mut merged.terms[index];
            // 后页不得覆盖先前锁定译名。显式冲突失败，不能让译文悄悄漂移。
            if old.translation != term.translation {
                return Err("模型改写了锁定译名，请重试本页翻译".into());
            }
            if !term.notes.is_empty() && !old.notes.contains(&term.notes) {
                let notes = if term.notes.starts_with(&old.notes) {
                    term.notes
                } else {
                    format!("{}\n{}", old.notes, term.notes)
                };
                if notes.len() > 2000 {
                    return Err("模型人物设定说明过长".into());
                }
                old.notes = notes;
            }
            if old.source != term.source {
                term.aliases.push(term.source);
            }
            for alias in term.aliases {
                if alias != old.source && !old.aliases.contains(&alias) {
                    old.aliases.push(alias);
                }
            }
        } else {
            merged.terms.push(term);
        }
    }
    let mut names = HashSet::new();
    for term in &merged.terms {
        for name in std::iter::once(&term.source).chain(term.aliases.iter()) {
            if !names.insert(name) {
                return Err("模型设定集的别名指代冲突，请重试".into());
            }
        }
    }
    if serde_json::to_vec(&merged).unwrap().len() > 160_000 {
        return Err("小说设定集已超过 160KB".into());
    }
    Ok(merged)
}

fn validate_translation(
    lines: Vec<TranslatedLine>,
    source: &[SourceLine],
) -> Result<Vec<TranslatedLine>, String> {
    let mut result = lines;
    result.sort_by_key(|line| line.line);
    if result.len() != source.len()
        || result.iter().zip(source).any(|(output, input)| {
            output.line != input.line
                || output.text.trim().is_empty()
                || output.text.len() > 100_000
        })
    {
        return Err("模型译文缺段、重复或顺序不符，请重试本页翻译".into());
    }
    Ok(result)
}

fn request_body(settings: &Settings, prompt: &str, input: Value) -> Value {
    let mut body = settings
        .translation_extra
        .as_object()
        .cloned()
        .unwrap_or_default();
    body.insert("model".into(), json!(settings.translation_model.trim()));
    body.insert(
        "messages".into(),
        json!([
            {"role":"system", "content": prompt},
            {"role":"user", "content": input.to_string()}
        ]),
    );
    body.insert("stream".into(), json!(false));
    body.insert("store".into(), json!(false));
    // 不强制 response_format：许多兼容端点只支持普通文本，提示词明确要求 JSON。
    Value::Object(body)
}

fn parse_completion(body: Value) -> Result<Value, String> {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or("翻译服务未返回 choices")?;
    if choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .is_some_and(|reason| reason != "stop")
    {
        return Err(
            "模型未完整输出（可能达到 token 上限或被服务拒绝），请调整高级 JSON 后重试".into(),
        );
    }
    let text = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .ok_or("模型未返回文本译文")?
        .trim();
    let text = if text.starts_with("```") {
        text.split_once('\n')
            .and_then(|(_, text)| text.strip_suffix("```"))
            .ok_or("模型 JSON 代码块不完整")?
            .trim()
    } else {
        text
    };
    serde_json::from_str(text).map_err(|_| "模型未返回合法 JSON，请重试或更换模型".into())
}

struct TranslationClient<'a> {
    http: wreq::Client,
    url: tauri::Url,
    key: String,
    settings: &'a Settings,
}

impl TranslationClient<'_> {
    async fn complete(&self, prompt: &str, input: Value) -> Result<Value, String> {
        let response = self
            .http
            .post(self.url.as_str())
            .bearer_auth(&self.key)
            .header("Content-Type", "application/json")
            .body(request_body(self.settings, prompt, input).to_string())
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    "翻译请求超时（180 秒），可降低思考深度后重试"
                } else {
                    "无法连接翻译服务，请检查 URL 和网络"
                }
            })?;
        parse_completion(read_service_response(response).await?)
    }
}

/// HTTP 状态码 → 可执行提示。只给排查方向，不回显服务正文、请求或凭据（ADR 0017）。
fn http_status_hint(status: u16) -> &'static str {
    match status {
        401 | 403 => "请检查 API Key 是否有效",
        404 => "请检查 API URL 与模型 ID",
        429 => "请求过于频繁或额度不足，请稍后重试",
        400..=499 => "请检查模型 ID、高级 JSON 与请求参数",
        _ => "服务端暂时不可用，请稍后重试",
    }
}

async fn read_service_response(response: wreq::Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "翻译服务返回 HTTP {}：{}",
            status.as_u16(),
            http_status_hint(status.as_u16())
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > 4_000_000)
    {
        return Err("翻译响应过大".into());
    }
    let mut stream = std::pin::pin!(response.bytes_stream());
    let mut raw = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "读取翻译响应失败")?;
        if raw.len() + chunk.len() > 4_000_000 {
            return Err("翻译响应过大".into());
        }
        raw.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&raw).map_err(|_| "翻译服务返回格式无效".into())
}

/// 设置草稿只用于本次请求；Key 不写入配置或凭据库。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranslationProbe {
    pub api_url: String,
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default = "empty_options")]
    pub extra: Value,
}

fn empty_options() -> Value { json!({}) }

fn probe_client(probe: &TranslationProbe, timeout: u64) -> Result<(Settings, tauri::Url, String, wreq::Client), String> {
    let settings = Settings {
        translation_api_url: probe.api_url.clone(),
        translation_model: probe.model.clone(),
        translation_extra: probe.extra.clone(),
        ..Settings::default()
    };
    validate_settings(&settings)?;
    let url = completion_url(&probe.api_url)?;
    let key = match &probe.api_key {
        Some(key) => Some(key.trim().to_string()),
        None => read_api_key()?,
    }.filter(|key| !key.is_empty()).ok_or("请先配置翻译 API Key")?;
    if key.len() > 8192 || key.chars().any(char::is_control) {
        return Err("翻译 API Key 格式无效".into());
    }
    let http = wreq::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .redirect(wreq::redirect::Policy::none())
        .build().map_err(|_| "无法初始化翻译客户端")?;
    Ok((settings, url, key, http))
}

fn models_url(value: &str) -> Result<tauri::Url, String> {
    let mut url = completion_url(value)?;
    let base = url.path().strip_suffix("/chat/completions").ok_or("翻译 API URL 无效")?;
    let path = format!("{base}/models");
    url.set_path(&path);
    Ok(url)
}

fn parse_models(body: Value) -> Result<Vec<String>, String> {
    let data = body.get("data").and_then(Value::as_array).ok_or("服务未返回兼容的模型列表，可手动填写模型 ID")?;
    if data.len() > 2000 { return Err("模型列表超过 2000 项，请手动填写模型 ID".into()); }
    let mut models: Vec<String> = data.iter().filter_map(|item| item.get("id").and_then(Value::as_str))
        .filter(|id| !id.trim().is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
        .map(str::to_string).collect();
    models.sort();
    models.dedup();
    if models.is_empty() { return Err("服务未返回可选模型，可手动填写模型 ID".into()); }
    Ok(models)
}

#[tauri::command]
pub async fn translation_models(probe: TranslationProbe) -> Result<Vec<String>, String> {
    let (_, _, key, http) = probe_client(&probe, 30)?;
    let response = http.get(models_url(&probe.api_url)?.as_str()).bearer_auth(key)
        .send().await.map_err(|_| "无法获取模型列表，请检查 URL 和网络，或手动填写模型 ID")?;
    let body = read_service_response(response).await.map_err(|error| {
        if error.starts_with("翻译服务返回 HTTP") {
            format!("{error}；部分服务不提供 /models 列表，可手动填写模型 ID")
        } else {
            error
        }
    })?;
    parse_models(body)
}

#[tauri::command]
pub async fn translation_test(probe: TranslationProbe) -> Result<(), String> {
    let (settings, url, key, http) = probe_client(&probe, 180)?;
    if settings.translation_model.trim().is_empty() { return Err("请先填写或选择模型 ID".into()); }
    let client = TranslationClient { http, url, key, settings: &settings };
    let result = client.complete("这是连接检测。仅返回 JSON 对象 {\"ok\":true}，不输出其他文字。", json!("请确认服务可用。")).await?;
    if result.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err("服务已响应，但未返回预期 JSON，请调整高级配置或更换模型".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn novel_translation_get(
    state: State<'_, AppState>,
    novel: NovelInput,
) -> Result<TranslationBook, String> {
    pages(&novel)?;
    read_book(&book_path(&state.paths.data_dir, &novel))
}

/// 单页翻译结果：状态 + 译文 + 本次生效的目标语言 code。
#[derive(Serialize)]
pub struct PageTranslation {
    pub status: PageTranslationStatus,
    pub lines: Vec<TranslatedLine>,
    pub target_language: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PageTranslationStatus {
    /// 已产出译文（可能来自本机缓存）。
    Translated,
    /// 原文已是目标语言，未调用模型。
    AlreadyTargetLanguage,
}

#[tauri::command]
pub async fn novel_translate_page(
    state: State<'_, AppState>,
    novel: NovelInput,
    page: usize,
    force: bool,
    progress: Channel<String>,
) -> Result<PageTranslation, String> {
    translate_page_flow(&state, &novel, page, force, &progress).await
}

/// 单页翻译主流程（命令与单测共用）：原文语言与目标语言一致时快速返回，不发请求。
async fn translate_page_flow(
    state: &AppState,
    novel: &NovelInput,
    page: usize,
    force: bool,
    progress: &Channel<String>,
) -> Result<PageTranslation, String> {
    let all_pages = pages(novel)?;
    let current = page
        .checked_sub(1)
        .and_then(|index| all_pages.get(index))
        .ok_or("翻译页码无效")?;
    if current.len() > 120_000 {
        return Err("本页超过 120KB，请使用支持长上下文的独立翻译工具".into());
    }
    if source_lines(current).is_empty() {
        return Err("本页没有可翻译文字".into());
    }
    let _ = progress.send("queued".into());
    let settings = state.settings_snapshot();
    let (target_code, target_name) = resolve_target_language(&settings)?;
    // 原文已是目标语言：只回提示不进入两轮流程；显式重译（force）仍执行。
    if !force && same_target_language(detect_language(&novel.content), target_code) {
        return Ok(PageTranslation {
            status: PageTranslationStatus::AlreadyTargetLanguage,
            lines: Vec::new(),
            target_language: target_code.to_string(),
        });
    }
    // ponytail: 全局串行，防止设定集覆盖与重复扣费；需要并行多小说时改为按小说加锁。
    let _guard = state.translation_lock.lock().await;
    let path = book_path(&state.paths.data_dir, novel);
    let mut book = read_book(&path)?;
    if !force {
        if let Some(cached) = book.pages.get(&page) {
            return Ok(PageTranslation {
                status: PageTranslationStatus::Translated,
                lines: cached.clone(),
                target_language: target_code.to_string(),
            });
        }
    }
    validate_settings(&settings)?;
    if settings.translation_api_url.trim().is_empty()
        || settings.translation_model.trim().is_empty()
    {
        return Err("请先在设置的小说翻译中配置 API URL、API Key 和模型 ID".into());
    }
    let key = read_api_key()?
        .filter(|key| !key.is_empty())
        .ok_or("请先配置翻译 API Key")?;
    let url = completion_url(&settings.translation_api_url)?;
    let client = wreq::Client::builder()
        .timeout(Duration::from_secs(180))
        .redirect(wreq::redirect::Policy::none())
        .build()
        .map_err(|_| "无法初始化翻译客户端")?;
    let client = TranslationClient {
        http: client,
        url,
        key,
        settings: &settings,
    };
    let lines = translate_book(
        &client,
        novel,
        &all_pages,
        page,
        &path,
        &mut book,
        progress,
        target_name,
    )
    .await?;
    Ok(PageTranslation {
        status: PageTranslationStatus::Translated,
        lines,
        target_language: target_code.to_string(),
    })
}

/// 模型返回的候选设定集：忽略多余字段，可选字段缺失取默认值。
/// 语义校验（空值、长度、别名冲突）仍由 `merge_bible` 负责；落盘与读缓存保持严格 `StoryBible`。
#[derive(Deserialize)]
struct CandidateBible {
    style: String,
    terms: Vec<CandidateTerm>,
}

#[derive(Deserialize)]
struct CandidateTerm {
    source: String,
    translation: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    notes: String,
}

impl From<CandidateBible> for StoryBible {
    fn from(candidate: CandidateBible) -> Self {
        Self {
            style: candidate.style,
            terms: candidate
                .terms
                .into_iter()
                .map(|term| Term {
                    source: term.source,
                    translation: term.translation,
                    kind: term.kind,
                    aliases: term.aliases,
                    notes: term.notes,
                })
                .collect(),
        }
    }
}

async fn translate_book(
    client: &TranslationClient<'_>,
    novel: &NovelInput,
    all_pages: &[String],
    page: usize,
    path: &Path,
    book: &mut TranslationBook,
    progress: &Channel<String>,
    target_language: &str,
) -> Result<Vec<TranslatedLine>, String> {
    let source = source_lines(&all_pages[page - 1]);
    let prev = if page > 1 {
        all_pages[page - 2]
            .chars()
            .rev()
            .take(600)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>()
    } else {
        String::new()
    };
    let next = all_pages
        .get(page)
        .map(|text| text.chars().take(600).collect::<String>())
        .unwrap_or_default();
    let mut input = json!({ "metadata": { "title":novel.title, "tags":novel.tags, "description":novel.description },
        "page":page, "page_count":all_pages.len(), "previous_context":prev, "next_context":next,
        "locked_bible":book.bible, "source_lines":source });
    let _ = progress.send("prepare".into());
    let candidate: CandidateBible =
        serde_json::from_value(client.complete(&with_target_language(PREPARE_PROMPT, target_language), input.clone()).await?)
            .map_err(|_| "模型设定集结构无效")?;
    book.bible = merge_bible(&book.bible, candidate.into())?;
    // Pass 1 先落盘；Pass 2 失败仍能沿用术语，不写半页译文。
    save_book(path, book)?;
    input["locked_bible"] = json!(book.bible);
    let _ = progress.send("translate".into());
    // 模型译文同样容忍多余字段；缺 line/text 仍报错。
    #[derive(Deserialize)]
    struct Output {
        lines: Vec<OutputLine>,
    }
    #[derive(Deserialize)]
    struct OutputLine {
        line: usize,
        text: String,
    }
    let output: Output = serde_json::from_value(
        client
            .complete(&with_target_language(TRANSLATE_PROMPT, target_language), input)
            .await?,
    )
    .map_err(|_| "模型译文结构无效")?;
    let lines = validate_translation(
        output
            .lines
            .into_iter()
            .map(|line| TranslatedLine { line: line.line, text: line.text })
            .collect(),
        &source,
    )?;
    book.pages.insert(page, lines.clone());
    save_book(path, book)?;
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 显式运行的付费服务验收；凭据只从进程环境读取，不改应用设置或系统凭据。
    #[tokio::test]
    #[ignore = "需要显式授权访问模型服务和进程环境中的测试 Key"]
    async fn live_two_page_translation() {
        let key = std::env::var("PIXIV_TRANSLATION_TEST_KEY").expect("缺少测试 Key 环境变量");
        let settings = Settings {
            translation_api_url: std::env::var("PIXIV_TRANSLATION_TEST_URL")
                .expect("缺少测试 URL 环境变量"),
            translation_model: std::env::var("PIXIV_TRANSLATION_TEST_MODEL")
                .expect("缺少测试模型环境变量"),
            translation_extra: json!({"reasoning_effort":"low"}),
            ..Settings::default()
        };
        validate_settings(&settings).unwrap();
        let client = TranslationClient {
            http: wreq::Client::builder()
                .timeout(Duration::from_secs(180))
                .redirect(wreq::redirect::Policy::none())
                .build()
                .unwrap(),
            url: completion_url(&settings.translation_api_url).unwrap(),
            key: key.clone(),
            settings: &settings,
        };
        let novel = NovelInput {
            novel_id: 42,
            title: "バーでの再会（翻訳検証）".into(),
            tags: vec!["小説".into(), "百合".into()],
            description: "エリとレイナはバーで出会った夫婦。店主と話す短い場面。".into(),
            content: "[chapter:再会]\nエリとレイナは、このバーで出会った夫婦である。\n女店主は二人のもとに詰め寄り、理由を問いただした。[newpage]エリは妻のレイナを見た。\n「レイナ、帰ろう」\n彼女はうなずき、エリの手を握った。".into(),
        };
        let all_pages = pages(&novel).unwrap();
        let dir =
            std::env::temp_dir().join(format!("pixiv-translation-live-{}", uuid::Uuid::new_v4()));
        let path = book_path(&dir, &novel);
        let progress = Channel::<String>::new(|_| {
            println!("翻译阶段推进");
            Ok(())
        });
        let started = std::time::Instant::now();
        let target = resolve_target_language(&settings).unwrap().1;
        let result: Result<(), String> = async {
            let mut book = TranslationBook::default();
            translate_book(&client, &novel, &all_pages, 1, &path, &mut book, &progress, target)
                .await?;
            let first_bible = book.bible.clone();
            // 第二页从落盘记录恢复，验证跨页以及重新打开小说后的设定沿用。
            book = read_book(&path)?;
            translate_book(&client, &novel, &all_pages, 2, &path, &mut book, &progress, target)
                .await?;
            assert_eq!(book.bible.style, first_bible.style);
            for term in &first_bible.terms {
                let saved = book
                    .bible
                    .terms
                    .iter()
                    .find(|item| item.source == term.source)
                    .unwrap();
                assert_eq!(saved.translation, term.translation, "跨页锁定译名");
            }
            for name in ["エリ", "レイナ"] {
                let term = book
                    .bible
                    .terms
                    .iter()
                    .find(|term| {
                        term.source == name || term.aliases.iter().any(|alias| alias == name)
                    })
                    .expect("人物进入共享设定集");
                for page in 1..=2 {
                    assert!(
                        book.pages[&page]
                            .iter()
                            .any(|line| line.text.contains(&term.translation)),
                        "译文使用锁定人物名"
                    );
                }
            }
            assert_eq!(read_book(&path)?.pages.len(), 2);
            for (page, lines) in &book.pages {
                println!("第 {page} 页：");
                for line in lines {
                    println!("{}：{}", line.line, line.text.replace(&key, "[REDACTED]"));
                }
            }
            println!(
                "两页四轮请求通过，耗时 {} 秒，reasoning_effort=low",
                started.elapsed().as_secs()
            );
            Ok(())
        }
        .await;
        // 无论请求是否成功，均清理测试设定集与译文。
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
        result.expect("真实模型翻译验收失败");
    }

    /// 真实小说在线验收：只读本机 pixiv 登录态，从小说日榜取一篇真实日文小说，
    /// 走生产两轮管线翻译第 1 页；再故意用不存在的模型确认报错以可读文案暴露且不含凭据。
    /// 只读登录态（绝不 save/clear），不改应用设置，结束清理临时记录。
    #[tokio::test]
    #[ignore = "需要显式授权：读取本机登录态抓取真实小说并访问付费模型服务"]
    async fn live_real_novel_page_translation() {
        use crate::cookies::CookieStore;
        use crate::pixiv::{api::PixivApi, client::PixivClient};
        use std::sync::Arc;

        let key = std::env::var("PIXIV_TRANSLATION_TEST_KEY").expect("缺少测试 Key 环境变量");
        let settings = Settings {
            translation_api_url: std::env::var("PIXIV_TRANSLATION_TEST_URL")
                .expect("缺少测试 URL 环境变量"),
            translation_model: std::env::var("PIXIV_TRANSLATION_TEST_MODEL")
                .expect("缺少测试模型环境变量"),
            translation_extra: json!({"reasoning_effort":"low"}),
            ..Settings::default()
        };
        validate_settings(&settings).unwrap();
        let http = wreq::Client::builder()
            .timeout(Duration::from_secs(180))
            .redirect(wreq::redirect::Policy::none())
            .build()
            .unwrap();
        let client = TranslationClient {
            http: http.clone(),
            url: completion_url(&settings.translation_api_url).unwrap(),
            key: key.clone(),
            settings: &settings,
        };

        // 模型列表：验收服务应支持 /models，且列表包含配置的模型（对应设置页「获取模型」）。
        let listed = translation_models(TranslationProbe {
            api_url: settings.translation_api_url.clone(),
            api_key: Some(key.clone()),
            model: String::new(),
            extra: json!({}),
        })
        .await
        .expect("获取模型列表失败（不提供 /models 的服务请手动填写模型 ID）");
        assert!(
            listed.iter().any(|model| model == &settings.translation_model),
            "模型列表应包含配置的模型 {}，实际 {} 项",
            settings.translation_model,
            listed.len()
        );

        // 真实小说取样：日榜前 20 条里取第一篇非 R18、首页 400~4000 字且含日文假名的作品。
        let cookies = CookieStore::new()
            .load()
            .expect("读取登录态失败")
            .filter(|map| map.get("PHPSESSID").is_some_and(|value| !value.is_empty()))
            .expect("本机没有可用 pixiv 登录态，请先用应用登录一次");
        let api = PixivApi::new(Arc::new(
            PixivClient::new(&cookies).expect("创建 pixiv 客户端失败"),
        ));
        let ranking = api
            .get_ranking("novel", "daily", 1, None)
            .await
            .expect("小说日榜请求失败");
        let items = ranking
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(!items.is_empty(), "小说日榜 items 不应为空");
        // pixiv 的 id/计数在字符串与数字间漂移，宽松解析。
        fn loose_i64(value: &Value) -> Option<i64> {
            match value {
                Value::Number(number) => number.as_i64(),
                Value::String(text) => text.trim().parse().ok(),
                _ => None,
            }
        }
        let mut sample = None;
        for item in items.iter().take(20) {
            let id = item.get("id").and_then(loose_i64).unwrap_or(0);
            if id <= 0 {
                println!("跳过样本：id 无法解析");
                continue;
            }
            if item.get("x_restrict").and_then(loose_i64).unwrap_or(0) != 0 {
                println!("跳过样本 {id}：限制级作品");
                continue;
            }
            let Ok(novel) = api.get_novel(id).await else {
                println!("跳过样本 {id}：详情请求失败");
                continue;
            };
            let kana = novel
                .content
                .chars()
                .filter(|c| matches!(c, '\u{3040}'..='\u{30ff}'))
                .count();
            let first_page = novel.content.split("[newpage]").next().unwrap_or("").trim();
            let first_len = first_page.chars().count();
            if kana < 50 || !(200..=4000).contains(&first_len) {
                println!("跳过样本 {id}：首页 {first_len} 字、假名 {kana} 个");
                continue;
            }
            sample = Some((item.clone(), novel));
            break;
        }
        let (item, novel) = sample.expect("日榜前 20 条里没有取到可用的日文小说样本");
        let input = NovelInput {
            novel_id: novel.novel_id,
            title: novel.title.clone(),
            tags: item
                .get("tags")
                .and_then(Value::as_array)
                .map(|tags| {
                    tags.iter()
                        .filter_map(|tag| tag.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            description: String::new(),
            content: novel.content.clone(),
        };
        let all_pages = pages(&input).unwrap();
        let progress = Channel::<String>::new(|_| Ok(()));

        let dir = std::env::temp_dir().join(format!("pixiv-translation-live-real-{}", uuid::Uuid::new_v4()));
        let path = book_path(&dir, &input);
        let started = std::time::Instant::now();
        let target = resolve_target_language(&settings).unwrap().1;
        let result: Result<(), String> = async {
            let mut book = TranslationBook::default();
            let lines = translate_book(
                &client, &input, &all_pages, 1, &path, &mut book, &progress, target,
            )
            .await?;
            let source = source_lines(&all_pages[0]);
            assert_eq!(lines.len(), source.len(), "译文行数应与原文一致");
            for line in &lines {
                assert!(
                    source.iter().any(|item| item.line == line.line),
                    "行号必须在原文范围内"
                );
                assert!(!line.text.trim().is_empty(), "译文不应为空");
            }
            assert!(
                lines.iter().any(|line| line.text.chars().any(|c| matches!(c, '\u{4e00}'..='\u{9fff}'))),
                "译文应含中文"
            );
            println!(
                "真实小说《{}》共 {} 页；第 1 页 {} 行原文 → {} 行译文，设定集 {} 个词条，耗时 {} 秒",
                input.title,
                all_pages.len(),
                source.len(),
                lines.len(),
                book.bible.terms.len(),
                started.elapsed().as_secs()
            );
            println!(
                "译文首行示例（{} 字）：{}",
                lines[0].text.chars().count(),
                lines[0].text.chars().take(40).collect::<String>()
            );
            Ok(())
        }
        .await;
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
        result.expect("真实小说翻译验收失败");

        // 模型侧报错暴露：不存在的模型必须给出可读文案，且绝不包含凭据原文。
        let bad_settings = Settings {
            translation_model: "pixiv-tool-missing-model".into(),
            ..settings.clone()
        };
        let bad_client = TranslationClient {
            http,
            url: completion_url(&bad_settings.translation_api_url).unwrap(),
            key: key.clone(),
            settings: &bad_settings,
        };
        let bad_dir = std::env::temp_dir().join(format!("pixiv-translation-live-bad-{}", uuid::Uuid::new_v4()));
        let bad_path = book_path(&bad_dir, &input);
        let mut bad_book = TranslationBook::default();
        let error = translate_book(
            &bad_client, &input, &all_pages, 1, &bad_path, &mut bad_book, &progress, target,
        )
        .await
        .expect_err("不存在的模型应报错");
        if bad_dir.exists() {
            std::fs::remove_dir_all(bad_dir).unwrap();
        }
        assert!(
            error.starts_with("翻译服务返回") || error.contains("结构无效"),
            "模型侧报错须暴露为可读文案，实际：{error}"
        );
        assert!(!error.contains(&key), "错误文案不得包含凭据");
        println!("模型侧报错暴露（凭据已排除）：{error}");
    }

    #[test]
    fn detects_source_language_and_matches_target() {
        let japanese = "エリは妻のレイナを見た。「レイナ、帰ろう」彼女はうなずき、エリの手を握った。";
        let simplified = "她们看着窗外的云，说着话，点了点头，心里很暖。";
        let traditional = "她們看著窗外的雲，說著話，點了點頭，心裡很暖。";
        let korean = "그녀는 미소를 지었다. 그는 천천히 걸어갔다.";
        let english = "She looked at her wife and smiled. The bartender asked why they came.";
        let russian = "Она посмотрела на жену и улыбнулась. Барменша спросила, зачем они пришли.";
        assert_eq!(detect_language(japanese), SourceLanguage::Japanese);
        assert_eq!(detect_language(simplified), SourceLanguage::Chinese { traditional: false });
        assert_eq!(detect_language(traditional), SourceLanguage::Chinese { traditional: true });
        assert_eq!(detect_language(korean), SourceLanguage::Korean);
        assert_eq!(detect_language(english), SourceLanguage::Latin);
        assert_eq!(detect_language(russian), SourceLanguage::Russian);
        assert_eq!(detect_language("123 456 ……"), SourceLanguage::Unknown);
        // 一致判定：日→日跳过；日→中照常翻译；中文简繁互转仍要翻译；拉丁只匹配 en。
        assert!(same_target_language(detect_language(japanese), "ja"));
        assert!(!same_target_language(detect_language(japanese), "zh-CN"));
        assert!(same_target_language(detect_language(simplified), "zh-CN"));
        assert!(!same_target_language(detect_language(simplified), "zh-TW"));
        assert!(same_target_language(detect_language(traditional), "zh-TW"));
        assert!(!same_target_language(detect_language(traditional), "zh-CN"));
        assert!(same_target_language(detect_language(english), "en"));
        assert!(!same_target_language(detect_language(english), "es"));
        assert!(!same_target_language(SourceLanguage::Unknown, "zh-CN"));
    }

    #[test]
    fn resolves_target_language_from_settings_or_ui_language() {
        let follow = Settings {
            language: "en-US".into(),
            ..Settings::default()
        };
        assert_eq!(resolve_target_language(&follow).unwrap().0, "en");
        let explicit = Settings {
            language: "zh-CN".into(),
            translation_target_language: "zh-TW".into(),
            ..Settings::default()
        };
        assert_eq!(resolve_target_language(&explicit).unwrap().1, "繁體中文");
        let alias = Settings {
            translation_target_language: "zh_Hant".into(),
            ..Settings::default()
        };
        assert_eq!(resolve_target_language(&alias).unwrap().0, "zh-TW");
        let invalid = Settings {
            translation_target_language: "klingon".into(),
            ..Settings::default()
        };
        assert!(resolve_target_language(&invalid).is_err());
        assert!(validate_settings(&invalid).is_err(), "非法目标语言在保存时即拒绝");
        assert!(validate_settings(&Settings::default()).is_ok());
        // 界面语言非法只在翻译时拒绝（不阻塞无关设置保存）。
        assert!(resolve_target_language(&Settings { language: "xx".into(), ..Settings::default() }).is_err());
    }

    #[test]
    fn injects_target_language_into_prompts() {
        for template in [PREPARE_PROMPT, TRANSLATE_PROMPT] {
            let prompt = with_target_language(template, "英文（English）");
            assert!(prompt.contains("英文（English）"), "提示词须带上目标语言");
            assert!(!prompt.contains("{target_language}"), "占位符必须被替换");
        }
    }

    /// 原文已是目标语言：流程直接返回提示，不发任何请求（配置指向不可达端口）。
    #[tokio::test]
    async fn skips_translation_when_source_is_target_language() {
        let dir = std::env::temp_dir().join(format!(
            "pixiv-translation-skip-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = crate::db::Db::open(&dir.join("app.db")).unwrap();
        let paths = crate::paths::AppPaths {
            data_dir: dir.clone(),
            config_dir: dir.join("config"),
            logs_dir: dir.join("logs"),
        };
        let settings = Settings {
            translation_api_url: "http://127.0.0.1:9/v1".into(),
            translation_model: "mock-model".into(),
            language: "zh-CN".into(),
            ..Settings::default()
        };
        let state = AppState::new(paths, settings, db);
        let progress = Channel::<String>::new(|_| Ok(()));
        let chinese = NovelInput {
            novel_id: 7,
            title: "中文小说".into(),
            tags: vec![],
            description: String::new(),
            content: "她们看着窗外的云，说着话，点了点头，心里很暖。".into(),
        };
        let skipped = translate_page_flow(&state, &chinese, 1, false, &progress)
            .await
            .unwrap();
        assert!(matches!(
            skipped.status,
            PageTranslationStatus::AlreadyTargetLanguage
        ));
        assert!(skipped.lines.is_empty());
        assert_eq!(skipped.target_language, "zh-CN");
        // force 与不同语言都照常进入流程：配置不可达，必然报错（证明没有走快速返回）。
        assert!(translate_page_flow(&state, &chinese, 1, true, &progress).await.is_err());
        let japanese = NovelInput {
            novel_id: 8,
            title: "日本語".into(),
            tags: vec![],
            description: String::new(),
            content: "エリは妻のレイナを見た。「レイナ、帰ろう」".into(),
        };
        assert!(translate_page_flow(&state, &japanese, 1, false, &progress).await.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 目标语言在线验收：同一日文样章翻译为英文，验证提示词真的按目标语言生效
    /// （译文不得残留中日文，且必须出现拉丁字母）。只翻译第 1 页，两次请求。
    #[tokio::test]
    #[ignore = "需要显式授权访问模型服务和进程环境中的测试 Key"]
    async fn live_english_target_translation() {
        let key = std::env::var("PIXIV_TRANSLATION_TEST_KEY").expect("缺少测试 Key 环境变量");
        let settings = Settings {
            translation_api_url: std::env::var("PIXIV_TRANSLATION_TEST_URL")
                .expect("缺少测试 URL 环境变量"),
            translation_model: std::env::var("PIXIV_TRANSLATION_TEST_MODEL")
                .expect("缺少测试模型环境变量"),
            translation_target_language: "en".into(),
            translation_extra: json!({"reasoning_effort":"low"}),
            ..Settings::default()
        };
        let (code, target) = resolve_target_language(&settings).unwrap();
        assert_eq!(code, "en");
        let client = TranslationClient {
            http: wreq::Client::builder()
                .timeout(Duration::from_secs(180))
                .redirect(wreq::redirect::Policy::none())
                .build()
                .unwrap(),
            url: completion_url(&settings.translation_api_url).unwrap(),
            key,
            settings: &settings,
        };
        let novel = NovelInput {
            novel_id: 43,
            title: "バーでの再会（英語翻訳検証）".into(),
            tags: vec!["小説".into()],
            description: "エリとレイナはバーで出会った夫婦。".into(),
            content: "[chapter:再会]\nエリとレイナは、このバーで出会った夫婦である。\n「レイナ、帰ろう」".into(),
        };
        let all_pages = pages(&novel).unwrap();
        let dir = std::env::temp_dir().join(format!("pixiv-translation-live-en-{}", uuid::Uuid::new_v4()));
        let path = book_path(&dir, &novel);
        let progress = Channel::<String>::new(|_| Ok(()));
        let started = std::time::Instant::now();
        let result: Result<(), String> = async {
            let mut book = TranslationBook::default();
            let lines = translate_book(
                &client, &novel, &all_pages, 1, &path, &mut book, &progress, target,
            )
            .await?;
            let source = source_lines(&all_pages[0]);
            assert_eq!(lines.len(), source.len());
            for line in &lines {
                assert!(
                    !line.text.chars().any(|c| matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3040}'..='\u{30ff}')),
                    "英文目标不得残留中日文：{}",
                    line.text
                );
            }
            assert!(
                lines.iter().any(|line| line.text.chars().any(|c| c.is_ascii_alphabetic())),
                "英文译文应含拉丁字母"
            );
            println!(
                "英文目标：{} 行原文 → {} 行英文，设定集 {} 个词条，耗时 {} 秒",
                source.len(),
                lines.len(),
                book.bible.terms.len(),
                started.elapsed().as_secs()
            );
            for line in &lines {
                println!("{}：{}", line.line, line.text);
            }
            Ok(())
        }
        .await;
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
        result.expect("英文目标语言验收失败");
    }

    #[test]
    fn validates_endpoint_and_advanced_options() {
        assert_eq!(
            completion_url("https://example.com/v1/").unwrap().as_str(),
            "https://example.com/v1/chat/completions"
        );
        assert_eq!(
            completion_url("http://localhost:1234/v1/chat/completions")
                .unwrap()
                .as_str(),
            "http://localhost:1234/v1/chat/completions"
        );
        for url in [
            "http://example.com/v1",
            "https://user:key@example.com/v1",
            "https://example.com/v1?key=secret",
        ] {
            assert!(completion_url(url).is_err());
        }
        assert!(validate_options(&json!({"reasoning_effort":"high", "thinking":{"type":"enabled","budget_tokens":2000}})).is_ok());
        for value in [
            json!([]),
            json!({"messages":[]}),
            json!({"stream":true}),
            json!({"Authorization":"secret"}),
        ] {
            assert!(validate_options(&value).is_err());
        }
    }

    #[test]
    fn builds_models_url_and_parses_listing() {
        assert_eq!(
            models_url("https://example.com/v1").unwrap().as_str(),
            "https://example.com/v1/models"
        );
        assert_eq!(
            models_url("https://example.com/v1/chat/completions")
                .unwrap()
                .as_str(),
            "https://example.com/v1/models"
        );
        assert!(models_url("notaurl").is_err());
        let listed = parse_models(json!({"data":[{"id":"model-b"},{"id":"model-a"},{"object":"model"},{"id":""}]})).unwrap();
        assert_eq!(listed, vec!["model-a", "model-b"]);
        let huge: Vec<Value> = (0..2001).map(|index| json!({ "id": index })).collect();
        for body in [
            json!({"object":"list"}),
            json!({"data":[{"object":"model"}]}),
            json!({"data":[]}),
            json!({"data": huge}),
        ] {
            assert!(parse_models(body).is_err(), "空列表、缺 id 或超量须报错");
        }
    }

    #[tokio::test]
    async fn probe_requires_key_url_and_model_for_generation() {
        let keyed = |model: &str, api_url: &str, api_key: Option<&str>| TranslationProbe {
            api_url: api_url.into(),
            api_key: api_key.map(str::to_string),
            model: model.into(),
            extra: json!({}),
        };
        // 空 Key 与坏 URL 在发请求前就被拒绝；空模型只拦生成检测，不拦列表获取。
        assert!(
            translation_test(keyed("", "https://example.com/v1", Some("probe-key")))
                .await
                .is_err()
        );
        assert!(
            translation_models(keyed("", "ftp://example.com", Some("probe-key")))
                .await
                .is_err()
        );
        assert!(probe_client(&keyed("model-id", "https://example.com/v1", Some("")), 30).is_err());
        assert!(probe_client(&keyed("model-id", "https://example.com/v1", Some("probe-key")), 30).is_ok());
        assert!(
            probe_client(&keyed("", "https://example.com/v1", Some("probe-key")), 30).is_ok(),
            "列表请求允许空模型"
        );
        assert!(
            translation_test(keyed("", "https://example.com/v1", Some("probe-key")))
                .await
                .is_err(),
            "检测请求仍须模型 ID"
        );
    }

    #[test]
    fn settings_patch_keeps_key_out_of_serialized_settings() {
        let updated = crate::commands::settings_cmds::apply_settings_patch(&Settings::default(),
            &json!({"translation_api_url":"https://example.com/v1","translation_model":"model-id",
                "translation_extra":{"reasoning_effort":"high"}, "translation_api_key":"test-only-secret"}),
            &std::env::temp_dir()).unwrap();
        let saved = serde_json::to_value(updated).unwrap();
        assert_eq!(saved["translation_extra"]["reasoning_effort"], "high");
        assert!(saved.get("translation_api_key").is_none());
        assert!(!saved.to_string().contains("test-only-secret"));
        assert!(
            crate::commands::settings_cmds::apply_settings_patch(
                &Settings::default(),
                &json!({"translation_extra":{"stream":true}}),
                &std::env::temp_dir()
            )
            .is_err()
        );
    }
    #[test]
    fn tolerates_model_omitting_optional_bible_fields() {
        // 实测模型偶尔省略 aliases 等可选字段（2026-10-04 在线验收），多余字段同样忽略。
        let candidate: CandidateBible = serde_json::from_value(json!({
            "style": "简洁",
            "summary": "多余字段应忽略",
            "terms": [
                {"source": "エリ", "translation": "绘里", "notes": "只给部分字段"},
                {"source": "バー", "translation": "酒吧", "kind": "place", "aliases": ["酒馆"], "extra": 1}
            ]
        }))
        .unwrap();
        let merged = merge_bible(&StoryBible::default(), candidate.into()).unwrap();
        assert_eq!(merged.style, "简洁");
        assert_eq!(merged.terms[0].aliases, Vec::<String>::new());
        assert_eq!(merged.terms[1].aliases, vec!["酒馆"]);
        // 缺 terms、缺 source/translation 或类型不符仍拒绝。
        for body in [
            json!({"style":"x"}),
            json!({"style":"x","terms":[{"translation":"译"}]}),
            json!({"style":"x","terms":"nope"}),
        ] {
            assert!(
                serde_json::from_value::<CandidateBible>(body.clone()).is_err(),
                "结构不符须报错：{body}"
            );
        }
    }

    #[test]
    fn locks_names_and_rejects_alias_conflicts() {
        let old = StoryBible {
            style: "克制".into(),
            terms: vec![Term {
                source: "アリス".into(),
                translation: "爱丽丝".into(),
                kind: "character".into(),
                aliases: vec![],
                notes: "".into(),
            }],
        };
        let mut update = old.clone();
        update.style = "热烈".into();
        update.terms[0].aliases.push("少女".into());
        let merged = merge_bible(&old, update.clone()).unwrap();
        assert_eq!(merged.style, "克制");
        assert_eq!(merged.terms[0].aliases, vec!["少女"]);
        update.terms[0].translation = "艾莉丝".into();
        assert!(merge_bible(&old, update).is_err());
        let mut conflict = old.clone();
        let mut term = old.terms[0].clone();
        term.source = "別人".into();
        term.aliases = vec!["アリス".into()];
        conflict.terms.push(term);
        assert!(merge_bible(&old, conflict).is_err());
    }
    #[test]
    fn enforces_complete_aligned_output_and_does_not_translate_images() {
        let source =
            source_lines("[chapter:序章]\n\n少女。[rb:名前>なまえ]\n[uploadedimage:123]\n[jump:2]");
        assert_eq!(
            source.iter().map(|line| line.line).collect::<Vec<_>>(),
            vec![0, 2]
        );
        let valid = vec![
            TranslatedLine {
                line: 2,
                text: "少女。名字".into(),
            },
            TranslatedLine {
                line: 0,
                text: "序章".into(),
            },
        ];
        assert_eq!(validate_translation(valid, &source).unwrap()[0].line, 0);
        assert!(validate_translation(vec![], &source).is_err());
        assert!(
            validate_translation(
                vec![
                    TranslatedLine {
                        line: 0,
                        text: "序章".into()
                    },
                    TranslatedLine {
                        line: 0,
                        text: "重复".into()
                    }
                ],
                &source
            )
            .is_err()
        );
        assert!(
            parse_completion(
                json!({"choices":[{"finish_reason":"length","message":{"content":"{}"}}]})
            )
            .is_err()
        );
        assert_eq!(parse_completion(json!({"choices":[{"finish_reason":"stop","message":{"content":"```json\n{\"lines\":[]}\n```"}}]})).unwrap(),json!({"lines":[]}));
    }
    #[test]
    fn persists_bible_and_pages_and_invalidates_changed_source() {
        let dir = std::env::temp_dir().join(format!("pixiv-translation-{}", uuid::Uuid::new_v4()));
        let mut input = NovelInput {
            novel_id: 42,
            title: "标题".into(),
            tags: vec![],
            description: "".into(),
            content: "第一页\r\n[newpage]第二页".into(),
        };
        assert_eq!(pages(&input).unwrap(), vec!["第一页", "第二页"]);
        let path = book_path(&dir, &input);
        let mut book = TranslationBook::default();
        book.bible.style = "克制".into();
        book.pages.insert(
            1,
            vec![TranslatedLine {
                line: 0,
                text: "译文".into(),
            }],
        );
        save_book(&path, &book).unwrap();
        save_book(&path, &book).unwrap();
        assert_eq!(read_book(&path).unwrap().bible.style, "克制");
        input.content.push('改');
        assert!(
            read_book(&book_path(&dir, &input))
                .unwrap()
                .pages
                .is_empty()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn two_pass_http_preserves_shared_bible_and_page_scope() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let responses = vec![
            json!({"style":"克制、保留人物敬语距离", "terms":[{"source":"アリス","translation":"爱丽丝","kind":"character","aliases":[],"notes":"主角"}]}),
            json!({"lines":[{"line":0,"text":"爱丽丝。"}]}),
            json!({"style":"克制、保留人物敬语距离", "terms":[{"source":"アリス","translation":"爱丽丝","kind":"character","aliases":["アリスちゃん"],"notes":"新称呼"}]}),
            json!({"lines":[{"line":0,"text":"她微笑了。"}]}),
            json!({"style":"克制、保留人物敬语距离", "terms":[{"source":"アリス","translation":"爱丽丝","kind":"character","aliases":["アリス様"],"notes":"尊称"}]}),
            json!({"lines":[]}),
        ];
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for output in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let header_end;
                let content_length;
                loop {
                    let mut bytes = [0u8; 4096];
                    let size = socket.read(&mut bytes).await.unwrap();
                    assert!(size > 0);
                    raw.extend_from_slice(&bytes[..size]);
                    if let Some(end) = raw.windows(4).position(|part| part == b"\r\n\r\n") {
                        header_end = end + 4;
                        let headers = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                        assert!(headers.starts_with("post /v1/chat/completions http/1.1"));
                        content_length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .unwrap()
                            .parse::<usize>()
                            .unwrap();
                        break;
                    }
                }
                while raw.len() < header_end + content_length {
                    let mut bytes = [0u8; 4096];
                    let size = socket.read(&mut bytes).await.unwrap();
                    assert!(size > 0);
                    raw.extend_from_slice(&bytes[..size]);
                }
                requests.push(
                    serde_json::from_slice::<Value>(&raw[header_end..header_end + content_length])
                        .unwrap(),
                );
                let body = json!({"choices":[{"finish_reason":"stop","message":{"content":output.to_string()}}]}).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            requests
        });
        let dir =
            std::env::temp_dir().join(format!("pixiv-translation-http-{}", uuid::Uuid::new_v4()));
        let novel = NovelInput {
            novel_id: 42,
            title: "星の帰り道".into(),
            tags: vec!["同人".into()],
            description: "星空下重逢".into(),
            content: "アリス。[newpage]彼女は微笑んだ。".into(),
        };
        let all_pages = pages(&novel).unwrap();
        let path = book_path(&dir, &novel);
        let settings = Settings {
            translation_model: "mock-model".into(),
            translation_extra: json!({"reasoning_effort":"high","thinking":{"budget_tokens":2048}}),
            ..Settings::default()
        };
        let client = wreq::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let url = completion_url(&format!("http://{address}/v1")).unwrap();
        let client = TranslationClient {
            http: client,
            url,
            key: "test-key".into(),
            settings: &settings,
        };
        let progress = Channel::<String>::new(|_| Ok(()));
        let target = resolve_target_language(&settings).unwrap().1;
        let mut book = TranslationBook::default();
        for page in 1..=2 {
            translate_book(
                &client, &novel, &all_pages, page, &path, &mut book, &progress, target,
            )
            .await
            .unwrap();
        }
        assert!(
            translate_book(
                &client, &novel, &all_pages, 2, &path, &mut book, &progress, target
            )
            .await
            .is_err()
        );
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 6);
        for request in &requests {
            assert_eq!(request["model"], "mock-model");
            assert_eq!(request["stream"], false);
            assert_eq!(request["store"], false);
            assert_eq!(request["reasoning_effort"], "high");
            assert_eq!(request["thinking"]["budget_tokens"], 2048);
            // 两个提示词都必须注入目标语言，且不留占位符。
            let prompt = request["messages"][0]["content"].as_str().unwrap();
            assert!(prompt.contains("简体中文") && !prompt.contains("{target_language}"));
        }
        let pass2: Value =
            serde_json::from_str(requests[1]["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(pass2["metadata"]["tags"], json!(["同人"]));
        assert_eq!(pass2["source_lines"], json!([{"line":0,"text":"アリス。"}]));
        assert_eq!(pass2["locked_bible"]["terms"][0]["translation"], "爱丽丝");
        let later: Value =
            serde_json::from_str(requests[2]["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(later["locked_bible"]["terms"][0]["source"], "アリス");
        assert_eq!(
            later["source_lines"],
            json!([{"line":0,"text":"彼女は微笑んだ。"}])
        );
        let saved = read_book(&path).unwrap();
        assert_eq!(
            saved.bible.terms[0].aliases,
            vec!["アリスちゃん", "アリス様"]
        );
        assert_eq!(saved.pages.len(), 2);
        assert_eq!(saved.pages[&2][0].text, "她微笑了。");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
