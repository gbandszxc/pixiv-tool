//! 原文解析与稳定的页 / 行 / 片段映射（Pass 0 上半段）。
//!
//! 与模型、Tauri、文件系统无关：输入是一篇 Pixiv 小说的原始正文，输出是可被
//! 切块、翻译、审查与合并复用的 [`SourceDoc`]。
//!
//! 关键规则：
//! - 只按「整行 trim 后完全等于 `[newpage]`」切页（大小写敏感），行内出现不切；
//! - 页码从 1 起，行号是页内 0 起，与阅读器 `NovelContent.vue` 的页 / 行契约一致；
//! - 空行不产生行；纯图片 / 跳转行产生 `Markup` 片段，由模型原样回填；
//! - `[chapter:X]` 产生 `Chapter` 片段；含 `[rb:A>B]` 的行产生 `Ruby` 片段；
//! - 单行超过 [`LONG_LINE_LIMIT`] 字符时按句末标点 / 空白切成多个片段，
//!   切点不落在 `[...]` / `[rb:...]` 标记内部；
//! - 片段 ID 为 `p<页三位>-l<行三位>-s<序号>`，由本模块生成，模型不得自造。

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 单行超过该字符数时拆成多个片段。
pub const LONG_LINE_LIMIT: usize = 1_500;

/// 片段语义类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentKind {
    /// 普通可译文本（可能内联图片 / 跳转等标记）。
    Text,
    /// 整行章节标记 `[chapter:X]`。
    Chapter,
    /// 含 `[rb:A>B]` 注音标记的可译文本。
    Ruby,
    /// 纯标记行（图片 / 跳转），不送翻译，译文原样回填。
    Markup,
}

/// 一个最小的翻译与校验单元。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSegment {
    /// 稳定 ID：`p003-l012-s0`。
    pub id: String,
    /// 片段原文（保留 `[rb:...]` 等标记）。
    pub text: String,
    pub kind: SegmentKind,
}

/// 页内一行：行号 0 起，与阅读器页内行号一致。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLine {
    pub line: u32,
    /// 该行原始文本（trim 后，保留标记）。
    pub text: String,
    pub segments: Vec<SourceSegment>,
}

/// 一个阅读页：页码 1 起。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePage {
    pub page: u32,
    /// 该页原始文本（已统一换行并 trim 两端，供页内行号映射）。
    pub text: String,
    pub lines: Vec<SourceLine>,
}

/// 整篇原文的稳定映射。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDoc {
    pub pages: Vec<SourcePage>,
    /// 对 `pages` 的 page/text 稳定序列化后的 SHA-256；不含目标语言。
    pub input_hash: String,
    pub target_language: String,
}

impl SourceDoc {
    /// 全部片段数量（含 Markup；调用方需要真实可译数时可自行过滤）。
    pub fn segment_count(&self) -> usize {
        self.pages
            .iter()
            .flat_map(|page| page.lines.iter())
            .map(|line| line.segments.len())
            .sum()
    }

    /// 页数（空原文也视为 1 个空页，与阅读器切页一致）。
    pub fn page_count(&self) -> u32 {
        self.pages.len() as u32
    }
}

/// 生成片段 ID：`p003-l012-s0`。
pub fn segment_id(page: u32, line: u32, seq: u32) -> String {
    format!("p{page:03}-l{line:03}-s{seq}")
}

/// 解析片段 ID；格式不合法返回 `None`。
pub fn parse_segment_id(id: &str) -> Option<(u32, u32, u32)> {
    let rest = id.strip_prefix('p')?;
    let (page, rest) = rest.split_once("-l")?;
    let (line, seq) = rest.split_once("-s")?;
    if page.is_empty() || line.is_empty() || seq.is_empty() {
        return None;
    }
    if !page.chars().all(|c| c.is_ascii_digit())
        || !line.chars().all(|c| c.is_ascii_digit())
        || !seq.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some((page.parse().ok()?, line.parse().ok()?, seq.parse().ok()?))
}

/// 解析整篇原文，建立页 / 行 / 片段映射。
///
/// 错误只包含安全摘要，不包含原文正文。
pub fn parse_source(content: &str, target_language: &str) -> Result<SourceDoc, String> {
    if content.len() > 8_000_000 {
        return Err("原文超过 8MB，无法建立翻译映射".into());
    }

    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut raw_pages: Vec<Vec<String>> = vec![Vec::new()];
    for line in normalized.lines() {
        if line.trim() == "[newpage]" {
            raw_pages.push(Vec::new());
        } else if let Some(current) = raw_pages.last_mut() {
            current.push(line.to_string());
        }
    }

    let mut pages = Vec::with_capacity(raw_pages.len());
    for (page_index, raw_lines) in raw_pages.into_iter().enumerate() {
        let page_number = (page_index + 1) as u32;
        // 与阅读器一致：整页先 trim 两端，再按 \n 枚举页内行号。
        let text = raw_lines.join("\n").trim().to_string();
        let mut lines = Vec::new();
        if !text.is_empty() {
            for (line_index, raw) in text.split('\n').enumerate() {
                if let Some(line) = build_line(page_number, line_index as u32, raw) {
                    lines.push(line);
                }
            }
        }
        pages.push(SourcePage {
            page: page_number,
            text,
            lines,
        });
    }

    let input_hash = source_hash(&pages);
    Ok(SourceDoc {
        pages,
        input_hash,
        target_language: target_language.to_string(),
    })
}

fn build_line(page: u32, line: u32, raw: &str) -> Option<SourceLine> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }

    if is_chapter_line(text) {
        return Some(SourceLine {
            line,
            text: text.to_string(),
            segments: vec![SourceSegment {
                id: segment_id(page, line, 0),
                text: text.to_string(),
                kind: SegmentKind::Chapter,
            }],
        });
    }

    // 纯标记行：去掉图片 / 跳转标记后没有可见文本。
    if markup_re().replace_all(text, "").trim().is_empty() {
        return Some(SourceLine {
            line,
            text: text.to_string(),
            segments: vec![SourceSegment {
                id: segment_id(page, line, 0),
                text: text.to_string(),
                kind: SegmentKind::Markup,
            }],
        });
    }

    let kind = if ruby_re().is_match(text) {
        SegmentKind::Ruby
    } else {
        SegmentKind::Text
    };
    let parts = split_long_line(text);
    let segments = parts
        .into_iter()
        .enumerate()
        .map(|(seq, part)| {
            let part_kind = match kind {
                SegmentKind::Ruby => {
                    if ruby_re().is_match(&part) {
                        SegmentKind::Ruby
                    } else {
                        SegmentKind::Text
                    }
                }
                other => other,
            };
            SourceSegment {
                id: segment_id(page, line, seq as u32),
                text: part,
                kind: part_kind,
            }
        })
        .collect();

    Some(SourceLine {
        line,
        text: text.to_string(),
        segments,
    })
}

fn is_chapter_line(text: &str) -> bool {
    text.starts_with("[chapter:") && text.ends_with(']')
}

fn markup_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\[(?:pixivimage|uploadedimage|jump|jumpurl):[^\]]*\]")
            .expect("markup 正则合法")
    })
}

fn ruby_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[rb:([^>\]]+)>([^\]]+)\]").expect("ruby 正则合法"))
}

/// 句末标点与空白：超长行的候选切点（切在字符之后，字符仍留在前一片段）。
fn is_split_char(ch: char) -> bool {
    matches!(
        ch,
        '。' | '！' | '？' | '!' | '?' | '…' | '；' | ';' | '、' | '，' | ','
    ) || ch.is_whitespace()
}

/// 超长单行拆分：按句末标点 / 空白切成不超过 [`LONG_LINE_LIMIT`] 的片段。
///
/// 切点只落在 `[...]` 深度为 0 的位置，因此不会切断 `[rb:A>B]` 或其他标记。
/// 不丢弃任何字符，片段按顺序直接连接即可还原原文。
fn split_long_line(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= LONG_LINE_LIMIT {
        return vec![text.to_string()];
    }

    let n = chars.len();

    // depth[k] = 处理完前 k 个字符后的方括号深度。
    let mut depth = vec![0i32; n + 1];
    let mut current = 0i32;
    for (index, ch) in chars.iter().enumerate() {
        match ch {
            '[' => current += 1,
            ']' => current = (current - 1).max(0),
            _ => {}
        }
        depth[index + 1] = current;
    }

    let is_safe_cut = |k: usize| {
        k > 0 && k <= n && depth[k] == 0 && chars.get(k - 1).copied().is_some_and(is_split_char)
    };

    let mut parts = Vec::new();
    let mut start = 0usize;
    while n - start > LONG_LINE_LIMIT {
        let hard = start + LONG_LINE_LIMIT;

        // 优先在硬上限之前、最近的句末标点 / 空白处切。
        let mut cut = (start + 1..=hard).rev().find(|&k| is_safe_cut(k));
        if cut.is_none() {
            // 否则向后找最近的合法边界（一个超长句子）。
            cut = (hard + 1..=(start + 2 * LONG_LINE_LIMIT).min(n)).find(|&k| is_safe_cut(k));
        }
        let cut = cut.unwrap_or_else(|| {
            // 最后兜底：硬切，但向前挪到方括号之外。
            let mut k = hard;
            while k < n && depth[k] != 0 {
                k += 1;
            }
            k
        });
        let cut = cut.max(start + 1).min(n);
        parts.push(chars[start..cut].iter().collect());
        start = cut;
    }
    if start < n {
        parts.push(chars[start..].iter().collect());
    }
    parts
}

/// 对 pages 的 page/text 做稳定序列化后取 SHA-256（不含 target_language）。
fn source_hash(pages: &[SourcePage]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"pixiv-tool/translation-source/v2\0");
    for page in pages {
        hasher.update(page.page.to_le_bytes());
        hasher.update((page.text.len() as u64).to_le_bytes());
        hasher.update(page.text.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

// ----------------------------------------------------------------------
// 单元测试
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn all_lines(doc: &SourceDoc) -> Vec<&SourceLine> {
        doc.pages
            .iter()
            .flat_map(|page| page.lines.iter())
            .collect()
    }

    #[test]
    fn multi_page_line_mapping() {
        let doc = parse_source("第一行\n第二行\n[newpage]\n第三行", "zh-CN").unwrap();
        assert_eq!(doc.pages.len(), 2);
        assert_eq!(doc.pages[0].page, 1);
        assert_eq!(doc.pages[1].page, 2);
        assert_eq!(doc.pages[0].lines.len(), 2);
        assert_eq!(doc.pages[0].lines[0].line, 0);
        assert_eq!(doc.pages[0].lines[1].line, 1);
        assert_eq!(doc.pages[1].lines[0].line, 0);
        assert_eq!(doc.pages[0].lines[0].segments[0].id, "p001-l000-s0");
        assert_eq!(doc.pages[1].lines[0].segments[0].id, "p002-l000-s0");
        assert_eq!(doc.target_language, "zh-CN");
    }

    #[test]
    fn blank_lines_are_skipped_but_line_numbers_preserved() {
        let doc = parse_source("a\n\n \n b", "zh-CN").unwrap();
        let lines = &doc.pages[0].lines;
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line, 0);
        assert_eq!(
            lines[1].line, 3,
            "空行不产生 SourceLine，但行号保持原始位置"
        );
    }

    #[test]
    fn pure_image_and_jump_lines_become_markup() {
        let doc = parse_source(
            "[pixivimage:12345]\n[uploadedimage:9]\n[jump:1]\n[jumpurl:https://x]\n正文",
            "zh-CN",
        )
        .unwrap();
        let lines = &doc.pages[0].lines;
        assert_eq!(lines.len(), 5);
        for line in &lines[..4] {
            assert_eq!(line.segments.len(), 1);
            assert_eq!(line.segments[0].kind, SegmentKind::Markup);
            assert_eq!(line.segments[0].text, line.text);
        }
        assert_eq!(lines[4].segments[0].kind, SegmentKind::Text);
    }

    #[test]
    fn chapter_and_ruby_kinds() {
        let doc = parse_source("[chapter:第一话]\n[rb:漢字>かんじ]です", "zh-CN").unwrap();
        let lines = &doc.pages[0].lines;
        assert_eq!(lines[0].segments[0].kind, SegmentKind::Chapter);
        assert_eq!(lines[0].segments[0].text, "[chapter:第一话]");
        assert_eq!(lines[1].segments[0].kind, SegmentKind::Ruby);
        assert!(lines[1].segments[0].text.contains("[rb:漢字>かんじ]"));
    }

    #[test]
    fn mixed_markup_line_stays_text() {
        let doc = parse_source("前文[pixivimage:1]后文", "zh-CN").unwrap();
        let line = &doc.pages[0].lines[0];
        assert_eq!(line.segments[0].kind, SegmentKind::Text);
        assert_eq!(line.segments[0].text, "前文[pixivimage:1]后文");
    }

    #[test]
    fn long_line_is_split_without_losing_characters() {
        let sentence = "这是一个很长的句子，用来触发超长单行的片段切分。";
        let mut source = String::new();
        while source.chars().count() < LONG_LINE_LIMIT + 300 {
            source.push_str(sentence);
        }
        let doc = parse_source(&source, "zh-CN").unwrap();
        let line = &doc.pages[0].lines[0];
        assert!(line.segments.len() > 1, "超长单行应拆成多个片段");
        let rejoined: String = line
            .segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(rejoined, line.text);
        for (index, segment) in line.segments.iter().enumerate() {
            assert_eq!(segment.id, format!("p001-l000-s{index}"));
            assert!(
                segment.text.chars().count() <= LONG_LINE_LIMIT + 100,
                "片段不应无限长"
            );
        }
    }

    #[test]
    fn long_line_never_cuts_inside_markup() {
        let mut source = String::new();
        while source.chars().count() < LONG_LINE_LIMIT + 100 {
            source.push_str("文字文字文字文字文字文字文字文字[rb:漢字>かんじ]。");
        }
        let doc = parse_source(&source, "zh-CN").unwrap();
        let line = &doc.pages[0].lines[0];
        for segment in &line.segments {
            assert!(
                !segment.text.contains("[rb:") || segment.text.contains("]"),
                "不得在 rb 标记内部切断"
            );
            let open = segment.text.matches('[').count();
            let close = segment.text.matches(']').count();
            assert_eq!(open, close, "片段内方括号应成对");
        }
    }

    #[test]
    fn unicode_and_combining_characters_roundtrip() {
        let source = "👨‍👩‍👧‍👦 e\u{301} 日本語のテスト\n[newpage]\n😀";
        let doc = parse_source(source, "zh-CN").unwrap();
        let joined: String = doc
            .pages
            .iter()
            .flat_map(|page| page.lines.iter())
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(joined, "👨‍👩‍👧‍👦 e\u{301} 日本語のテスト\n😀");
    }

    #[test]
    fn crlf_is_normalized() {
        let doc = parse_source("a\r\nb\r\n[newpage]\r\nc", "zh-CN").unwrap();
        assert_eq!(doc.pages.len(), 2);
        assert_eq!(doc.pages[0].text, "a\nb");
        assert_eq!(doc.pages[1].text, "c");
    }

    #[test]
    fn trailing_newline_does_not_add_line() {
        let doc = parse_source("a\n", "zh-CN").unwrap();
        assert_eq!(doc.pages[0].lines.len(), 1);
        assert_eq!(doc.pages[0].text, "a");
    }

    #[test]
    fn consecutive_newpage_keeps_empty_pages() {
        let doc = parse_source("a\n[newpage]\n[newpage]\nb", "zh-CN").unwrap();
        assert_eq!(doc.pages.len(), 3);
        assert!(doc.pages[1].lines.is_empty());
        assert_eq!(doc.pages[2].lines[0].segments[0].id, "p003-l000-s0");
    }

    #[test]
    fn newpage_inside_line_is_not_a_separator() {
        let doc = parse_source("前[newpage]后", "zh-CN").unwrap();
        assert_eq!(doc.pages.len(), 1);
        assert_eq!(doc.pages[0].text, "前[newpage]后");
    }

    #[test]
    fn segments_concat_to_line_text_for_every_line() {
        let doc = parse_source(
            "[chapter:序]\n普通行\n\n[rb:漢字>かんじ]と本文\n[newpage]\n[pixivimage:1]",
            "zh-CN",
        )
        .unwrap();
        for line in all_lines(&doc) {
            let joined: String = line
                .segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect();
            assert_eq!(joined, line.text);
        }
    }

    #[test]
    fn hash_ignores_target_language_and_changes_with_content() {
        let a = parse_source("正文", "zh-CN").unwrap();
        let b = parse_source("正文", "en").unwrap();
        let c = parse_source("正文2", "zh-CN").unwrap();
        assert_eq!(a.input_hash, b.input_hash);
        assert_ne!(a.input_hash, c.input_hash);
        assert_eq!(a.input_hash.len(), 64);
    }

    #[test]
    fn segment_id_roundtrip() {
        assert_eq!(segment_id(3, 12, 0), "p003-l012-s0");
        assert_eq!(parse_segment_id("p003-l012-s0"), Some((3, 12, 0)));
        assert_eq!(parse_segment_id("p3-l12-s0"), Some((3, 12, 0)));
        assert_eq!(parse_segment_id("c0001"), None);
        assert_eq!(parse_segment_id("p00x-l001-s0"), None);
    }
}
