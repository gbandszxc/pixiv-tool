//! 翻译流式进度的摘要行：从 Pass 2 的译文增量里扫出「最后一个完整的 text 值」。
//!
//! 模型译文是流式下发的 JSON（`{"lines":[{"line":0,"text":"…"},…]}`），增量可以把
//! 键名、字符串、转义序列任意切断。完整 JSON 解析由 [`crate::translation::parse_model_json`]
//! 收尾；这里只做一个跨增量保留状态的轻量扫描——只认识 `text` 一个键，
//! 见到完整的 text 值就记下（后出现的覆盖先出现的），取不到完整值就不出摘要，
//! 由前端降级显示已接收字节数。

/// 摘要文本的字符上限（char 边界截断，超出部分交给前端 ellipsis）。
pub const SUMMARY_CHAR_LIMIT: usize = 60;
/// 单个字符串值的累积上限：只需要前缀，超限即停止累积，但继续扫到收尾引号，
/// 保证扫描状态不错位。
const STRING_ACCUMULATE_LIMIT: usize = 4096;

/// 一行流式摘要：最近一个完整的译文 text 值。
#[derive(Debug, PartialEq, Eq)]
pub struct StreamSummary {
    pub text: String,
}

impl StreamSummary {
    /// 展示形态：只输出摘要文本，按 60 字符 char 边界截断（不追加省略号）。
    pub fn display(&self) -> String {
        self.text.chars().take(SUMMARY_CHAR_LIMIT).collect()
    }
}

/// 字符串内转义的中间态：`\uXXXX` 要跨 4 个字符，代理对要跨 12 个。
#[derive(Default)]
enum Escape {
    #[default]
    None,
    Backslash,
    /// `\uXXXX` 的 4 位十六进制累积中。
    Unicode {
        digits: String,
    },
}

/// 跨增量 JSON 摘要扫描器：喂入译文增量，随时可取出最近一个完整的 text 值。
#[derive(Default)]
pub struct SummaryExtractor {
    /// 最近一个完整 text 值（后出现的覆盖先出现的）。
    latest: Option<StreamSummary>,
    /// 正在读的字符串（转义已还原）。
    text: String,
    /// 正在读的字符串是否已超过累积上限（只保留前缀）。
    overflow: bool,
    /// 字符串内的转义中间态。
    escape: Escape,
    /// 已读到的高代理项，等它的低代理项（`\uD83D\uDE00` 成对出现）。
    surrogate_high: Option<u16>,
    /// 是否在字符串里。
    in_string: bool,
    /// 刚读完的字符串，还要看下一个非空白字符是不是冒号（决定键名还是值）。
    finished: Option<String>,
    /// 已确认的键名，等它的值。
    key: Option<String>,
}

impl SummaryExtractor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 灌入一段译文增量；任意切分都能续上（逐字节、跨转义、跨键名均可）。
    pub fn push(&mut self, chunk: &str) {
        for character in chunk.chars() {
            self.step(character);
        }
    }

    /// 展示用摘要（60 字符截断）；提取不到为空串。
    pub fn summary(&self) -> String {
        self.latest
            .as_ref()
            .map(StreamSummary::display)
            .unwrap_or_default()
    }

    fn step(&mut self, character: char) {
        if self.in_string {
            self.read_string(character);
            return;
        }
        if character.is_whitespace() {
            // 空白不终结正在等待判定的字符串（`"text" : "…"` 合法）。
            return;
        }
        if let Some(finished) = self.finished.take() {
            if character == ':' {
                self.key = Some(finished);
                return;
            }
            // 不是冒号：刚读完的字符串是某个值，按当前键名结算后继续处理本字符。
            self.apply_string_value(finished);
        }
        match character {
            '"' => {
                self.in_string = true;
                self.text.clear();
                self.overflow = false;
                self.escape = Escape::None;
            }
            // 结构符、数字与 true/false/null 等字面量都不是字符串值：清掉键名，
            // 避免把上一个键名误配给后面的值（冒号只可能在 finished 分支里消费）。
            _ => self.key = None,
        }
    }

    /// 字符串里的一个字符：还原转义，遇到收尾引号则结束这个字符串。
    fn read_string(&mut self, character: char) {
        match std::mem::take(&mut self.escape) {
            Escape::None => match character {
                '"' => self.finish_string(),
                '\\' => self.escape = Escape::Backslash,
                _ => self.accumulate(character),
            },
            Escape::Backslash => {
                self.escape = Escape::None;
                match character {
                    'u' => {
                        self.escape = Escape::Unicode {
                            digits: String::new(),
                        }
                    }
                    // 基本转义集；非法转义原样丢掉反斜杠，保留字符。
                    other => self.accumulate(match other {
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        _ => other,
                    }),
                }
            }
            Escape::Unicode { mut digits } => {
                digits.push(character);
                if digits.len() < 4 {
                    self.escape = Escape::Unicode { digits };
                    return;
                }
                let code = u16::from_str_radix(&digits, 16).unwrap_or(u16::MAX);
                self.escape = Escape::None;
                match self.surrogate_high.take() {
                    // 前一个转义是高代理项：这个转义必须是它的低代理项。
                    Some(high) => {
                        if matches!(code, 0xDC00..=0xDFFF) {
                            let combined =
                                0x1_0000 + ((high as u32 - 0xD800) << 10) + (code as u32 - 0xDC00);
                            if let Some(decoded) = char::from_u32(combined) {
                                self.accumulate(decoded);
                            }
                        } else if let Some(decoded) = char::from_u32(code as u32) {
                            // 没配上对：丢掉高代理项，按普通码点收下这个转义。
                            self.accumulate(decoded);
                        }
                    }
                    // 高代理项先存起来，等下一个 `\uXXXX`（代理对被增量切断也照样续上）。
                    None if matches!(code, 0xD800..=0xDBFF) => {
                        self.surrogate_high = Some(code);
                    }
                    None => {
                        if let Some(decoded) = char::from_u32(code as u32) {
                            self.accumulate(decoded);
                        }
                    }
                }
            }
        }
    }

    fn finish_string(&mut self) {
        self.in_string = false;
        self.escape = Escape::None;
        // 没收尾的字符串里残留的未配对代理项不再影响后续扫描。
        self.surrogate_high = None;
        let text = std::mem::take(&mut self.text);
        self.overflow = false;
        // 已确认这是 `text` 键的值：收尾引号出现即完整，立即入摘要
        // （流可能恰好结束在这里，等不到下一个字符）。
        if self.key.as_deref() == Some("text") {
            self.apply_string_value(text);
            return;
        }
        // 否则还要看下一个非空白字符是不是冒号，才能判定键名还是值。
        self.finished = Some(text);
    }

    /// 结算一个作为值的字符串：只有 `text` 键的值进摘要。
    fn apply_string_value(&mut self, value: String) {
        if self.key.take().as_deref() != Some("text") {
            return;
        }
        // 值完整即入摘要（流可能恰好结束在这里，等不到下一个字符）。
        self.latest = Some(StreamSummary { text: value });
    }

    /// 累积字符串内容；超过上限只保留前缀（展示只需要前 60 字符）。
    fn accumulate(&mut self, character: char) {
        // 普通字符出现即宣告之前的代理项没配上对，不再等待。
        self.surrogate_high = None;
        if self.overflow {
            return;
        }
        self.text.push(character);
        if self.text.len() > STRING_ACCUMULATE_LIMIT {
            self.text = self.text.chars().take(SUMMARY_CHAR_LIMIT).collect();
            self.overflow = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary_of(chunks: &[&str]) -> String {
        let mut extractor = SummaryExtractor::new();
        for chunk in chunks {
            extractor.push(chunk);
        }
        extractor.summary()
    }

    #[test]
    fn extracts_last_complete_text() {
        let json = r#"{"lines":[{"line":0,"text":"第一行。"},{"line":1,"text":"第二行。"}]}"#;
        assert_eq!(summary_of(&[json]), "第二行。");
    }

    #[test]
    fn survives_byte_by_byte_splits() {
        // 逐字符喂入：键名、字符串、转义、结构符全部跨增量。
        let json = "{\"lines\":[{\"line\":7,\"text\":\"他说\\\"走吧\\\"。\"}]}";
        let mut extractor = SummaryExtractor::new();
        for character in json.chars() {
            extractor.push(&character.to_string());
        }
        assert_eq!(extractor.summary(), "他说\"走吧\"。");
    }

    #[test]
    fn survives_chunk_boundaries_inside_escapes_and_keys() {
        // 任意两段切分都能续上：键名中间、`\uXXXX` 中间、收尾引号前后、多字节字符边界。
        let json = "{\"line\":12,\"text\":\"于是他低下头，避开那道追过来的视线\\u2026\"}";
        let expected = "于是他低下头，避开那道追过来的视线…";
        for (index, _) in json.char_indices() {
            let (head, tail) = json.split_at(index);
            assert_eq!(summary_of(&[head, tail]), expected, "切点 {index}");
        }
        // 代理对（emoji）跨块：高低代理各占一段。
        let emoji = "{\"text\":\"笑\\uD83D\\uDE00了\"}";
        assert_eq!(summary_of(&[&emoji[..20], &emoji[20..]]), "笑😀了");
    }

    #[test]
    fn decodes_escape_sequences() {
        let json = r#"{"text":"引号\" 反斜杠\\ 换行\n 制表\t 斜杠\/ 二\u4e8c"}"#;
        assert_eq!(
            summary_of(&[json]),
            "引号\" 反斜杠\\ 换行\n 制表\t 斜杠/ 二\u{4e8c}"
        );
    }

    #[test]
    fn empty_until_a_complete_value_arrives() {
        let mut extractor = SummaryExtractor::new();
        extractor.push("{\"lines\":[{\"line\":0,\"text\":\"半");
        assert_eq!(extractor.summary(), "", "字符串没收尾不算完整值");
        extractor.push("句话\"");
        assert_eq!(extractor.summary(), "半句话", "收尾引号出现即完整");
        // 只有键名、连冒号后的值都没有：同样不出摘要。
        let mut partial = SummaryExtractor::new();
        partial.push("{\"line\":3,\"te");
        assert_eq!(partial.summary(), "");
    }

    #[test]
    fn truncates_to_sixty_chars_on_char_boundaries() {
        let long = "长".repeat(200);
        let json = format!("{{\"line\":0,\"text\":\"{long}\"}}");
        let summary = summary_of(&[&json]);
        assert_eq!(summary, "长".repeat(60));
        assert_eq!(summary.chars().count(), 60, "正文按 60 字符截断");
        // 超长值也逐字节喂得动：扫描状态不错位。
        let mut extractor = SummaryExtractor::new();
        for character in json.chars() {
            extractor.push(&character.to_string());
        }
        assert_eq!(extractor.summary(), summary);
    }

    #[test]
    fn ignores_unknown_keys_and_non_json_streams() {
        // 设定集（Pass 1）形状：没有 text 键，恒空。
        let bible = r#"{"style":"克制","terms":[{"source":"アリス","translation":"爱丽丝"}]}"#;
        assert_eq!(summary_of(&[bible]), "");
        // 普通文本/保活内容：没有 JSON 结构，也空。
        assert_eq!(summary_of(&["keep-alive", ": ping", "[DONE]"]), "");
        // 键名像 text 但不是（值里的字符串、别的键）都不算。
        assert_eq!(
            summary_of(&[r#"{"note":"text","texts":"x","sub":{"text":1}}"#]),
            ""
        );
    }

    #[test]
    fn keeps_last_complete_value_when_trailing_text_is_incomplete() {
        let mut extractor = SummaryExtractor::new();
        extractor.push("{\"lines\":[{\"line\":0,\"text\":\"完整的一行\"},");
        assert_eq!(extractor.summary(), "完整的一行");
        extractor.push("{\"line\":1,\"text\":\"没收尾");
        assert_eq!(extractor.summary(), "完整的一行", "半行不覆盖完整行");
    }

    #[test]
    fn ignores_stray_braces_without_losing_extraction() {
        // 病态输入：大量不成对的 `{` `}` 只是结构符，不影响后续提取。
        let mut extractor = SummaryExtractor::new();
        extractor.push(&"{".repeat(500));
        extractor.push(&"}".repeat(500));
        assert_eq!(extractor.summary(), "");
        extractor.push("{\"text\":\"括号之后仍能提取\"}");
        assert_eq!(extractor.summary(), "括号之后仍能提取");
    }
}
