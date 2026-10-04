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

pub fn validate_settings(settings: &Settings) -> Result<(), String> {
    if !settings.translation_api_url.trim().is_empty() {
        completion_url(&settings.translation_api_url)?;
    }
    if settings.translation_model.len() > 256
        || settings.translation_model.chars().any(char::is_control)
    {
        return Err("翻译模型 ID 无效".into());
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
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "翻译服务返回 HTTP {}，请检查 Key、模型 ID 和高级 JSON",
                status.as_u16()
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
        parse_completion(serde_json::from_slice(&raw).map_err(|_| "翻译服务返回格式无效")?)
    }
}

#[tauri::command]
pub async fn novel_translation_get(
    state: State<'_, AppState>,
    novel: NovelInput,
) -> Result<TranslationBook, String> {
    pages(&novel)?;
    read_book(&book_path(&state.paths.data_dir, &novel))
}

#[tauri::command]
pub async fn novel_translate_page(
    state: State<'_, AppState>,
    novel: NovelInput,
    page: usize,
    force: bool,
    progress: Channel<String>,
) -> Result<Vec<TranslatedLine>, String> {
    let all_pages = pages(&novel)?;
    let current = page
        .checked_sub(1)
        .and_then(|index| all_pages.get(index))
        .ok_or("翻译页码无效")?;
    if current.len() > 120_000 {
        return Err("本页超过 120KB，请使用支持长上下文的独立翻译工具".into());
    }
    let source = source_lines(current);
    if source.is_empty() {
        return Err("本页没有可翻译文字".into());
    }
    let _ = progress.send("queued".into());
    // ponytail: 全局串行，防止设定集覆盖与重复扣费；需要并行多小说时改为按小说加锁。
    let _guard = state.translation_lock.lock().await;
    let path = book_path(&state.paths.data_dir, &novel);
    let mut book = read_book(&path)?;
    if !force {
        if let Some(cached) = book.pages.get(&page) {
            return Ok(cached.clone());
        }
    }
    let settings = state.settings_snapshot();
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
    translate_book(
        &client, &novel, &all_pages, page, &path, &mut book, &progress,
    )
    .await
}

async fn translate_book(
    client: &TranslationClient<'_>,
    novel: &NovelInput,
    all_pages: &[String],
    page: usize,
    path: &Path,
    book: &mut TranslationBook,
    progress: &Channel<String>,
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
    let candidate: StoryBible =
        serde_json::from_value(client.complete(PREPARE_PROMPT, input.clone()).await?)
            .map_err(|_| "模型设定集结构无效")?;
    book.bible = merge_bible(&book.bible, candidate)?;
    // Pass 1 先落盘；Pass 2 失败仍能沿用术语，不写半页译文。
    save_book(path, book)?;
    input["locked_bible"] = json!(book.bible);
    let _ = progress.send("translate".into());
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Output {
        lines: Vec<TranslatedLine>,
    }
    let output: Output = serde_json::from_value(client.complete(TRANSLATE_PROMPT, input).await?)
        .map_err(|_| "模型译文结构无效")?;
    let lines = validate_translation(output.lines, &source)?;
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
        let result: Result<(), String> = async {
            let mut book = TranslationBook::default();
            translate_book(&client, &novel, &all_pages, 1, &path, &mut book, &progress).await?;
            let first_bible = book.bible.clone();
            // 第二页从落盘记录恢复，验证跨页以及重新打开小说后的设定沿用。
            book = read_book(&path)?;
            translate_book(&client, &novel, &all_pages, 2, &path, &mut book, &progress).await?;
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
        let mut book = TranslationBook::default();
        for page in 1..=2 {
            translate_book(
                &client, &novel, &all_pages, page, &path, &mut book, &progress,
            )
            .await
            .unwrap();
        }
        assert!(
            translate_book(&client, &novel, &all_pages, 2, &path, &mut book, &progress)
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
