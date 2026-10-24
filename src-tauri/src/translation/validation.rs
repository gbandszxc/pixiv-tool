//! 译文结构与标记校验、页行合并（Pass 3 / 发布前确定性检查）。
//!
//! 这些检查不替代独立审查，但能挡住结构性错误：缺段、重复、顺序错乱、空译文、
//! `Markup` / `Chapter` / `Ruby` 标记被破坏、数字 / URL / 跳转标记数量变化，以及
//! 明显的目标语言残留。错误只含 ID、类型与计数，不含原文或译文正文。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;

use crate::translation::artifacts::{
    Draft, Issue, MergeReport, Severity, TranslatedLine, TranslatedSegment,
};
use crate::translation::chunking::Chunk;
use crate::translation::source::{SegmentKind, SourceSegment, parse_segment_id};

/// 单片段译文长度上限。
pub const MAX_SEGMENT_CHARS: usize = 100_000;

/// 日译中时允许的假名占比（百分比，保守阈值）。
const KANA_RATIO_LIMIT_PERCENT: usize = 15;
/// 中译外时允许保留的汉字占比（百分比）。
const HAN_RATIO_LIMIT_PERCENT: usize = 30;

/// 校验单块草稿；不指定目标语言时按原文 / 译文自动判断语言组合。
pub fn validate_draft(chunk: &Chunk, draft: &Draft) -> Result<(), Vec<Issue>> {
    validate_draft_with_language(chunk, draft, None)
}

/// 校验单块草稿，并显式指定目标语言（如 `zh-CN` / `en`）。
pub fn validate_draft_with_language(
    chunk: &Chunk,
    draft: &Draft,
    target_language: Option<&str>,
) -> Result<(), Vec<Issue>> {
    let mut issues = Vec::new();

    let expected: Vec<&str> = chunk
        .segments
        .iter()
        .map(|segment| segment.id.as_str())
        .collect();
    let actual: Vec<&str> = draft
        .segments
        .iter()
        .map(|segment| segment.id.as_str())
        .collect();
    let expected_set: BTreeSet<&str> = expected.iter().copied().collect();
    let actual_set: BTreeSet<&str> = actual.iter().copied().collect();

    // 缺失
    for id in &expected {
        if !actual_set.contains(id) {
            issues.push(issue(
                id,
                "missing_segment",
                Severity::Critical,
                "草稿缺少该片段",
            ));
        }
    }
    // 未知
    for id in &actual {
        if !expected_set.contains(id) {
            issues.push(issue(
                id,
                "unknown_segment",
                Severity::Critical,
                "草稿包含本块之外的片段",
            ));
        }
    }
    // 重复
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for id in &actual {
        *counts.entry(*id).or_insert(0) += 1;
    }
    for id in &actual {
        if counts.get(id).copied().unwrap_or(0) > 1
            && !issues
                .iter()
                .any(|existing| existing.segment_id == *id && existing.kind == "duplicate_segment")
        {
            issues.push(issue(
                id,
                "duplicate_segment",
                Severity::Critical,
                "草稿重复包含该片段",
            ));
        }
    }
    // 顺序：把实际 ID 去重后与「按预期顺序出现的已知 ID」比较。
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let actual_unique: Vec<&str> = actual
        .iter()
        .copied()
        .filter(|id| seen.insert(*id))
        .collect();
    let expected_present: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|id| actual_set.contains(id))
        .collect();
    if actual_unique != expected_present {
        let first = actual_unique
            .iter()
            .zip(expected_present.iter())
            .find(|(left, right)| left != right)
            .map(|(left, _)| *left)
            .or_else(|| actual_unique.last().copied())
            .unwrap_or("");
        issues.push(issue(
            first,
            "order_mismatch",
            Severity::Critical,
            "草稿片段顺序与本块不一致",
        ));
    }

    // 逐片段结构与保真。
    for source in &chunk.segments {
        let matches: Vec<&TranslatedSegment> = draft
            .segments
            .iter()
            .filter(|translated| translated.id == source.id)
            .collect();
        if matches.len() != 1 {
            continue;
        }
        let translated = matches[0];
        check_segment(source, translated, &mut issues);
    }

    check_language_purity(chunk, draft, target_language, &mut issues);

    // 确定性：同一组输入产生相同顺序的问题。
    issues.sort_by(|left, right| {
        left.segment_id
            .cmp(&right.segment_id)
            .then(left.kind.cmp(&right.kind))
            .then(left.detail.cmp(&right.detail))
    });

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn check_segment(source: &SourceSegment, translated: &TranslatedSegment, issues: &mut Vec<Issue>) {
    let raw = translated.text.as_str();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        issues.push(issue(
            &source.id,
            "empty_translation",
            Severity::Major,
            "译文为空",
        ));
        return;
    }
    if trimmed.chars().count() > MAX_SEGMENT_CHARS {
        issues.push(issue(
            &source.id,
            "translation_too_long",
            Severity::Major,
            "译文超过单片段长度上限",
        ));
    }

    match source.kind {
        SegmentKind::Markup => {
            if raw != source.text {
                issues.push(issue(
                    &source.id,
                    "markup_modified",
                    Severity::Major,
                    "纯标记行必须原样返回",
                ));
            }
            return;
        }
        SegmentKind::Chapter => {
            if chapter_title(trimmed).is_none() {
                issues.push(issue(
                    &source.id,
                    "chapter_markup",
                    Severity::Major,
                    "章节标记结构应保持为 [chapter:标题]",
                ));
            }
            return;
        }
        SegmentKind::Ruby => {
            let source_occurrences = source.text.matches("[rb:").count();
            let dest_occurrences = trimmed.matches("[rb:").count();
            let dest_valid = ruby_re().find_iter(trimmed).count();
            if source_occurrences != dest_occurrences || dest_valid != dest_occurrences {
                issues.push(issue(
                    &source.id,
                    "ruby_markup",
                    Severity::Major,
                    "注音标记结构应保持为 [rb:原文>注音]",
                ));
            }
        }
        SegmentKind::Text => {}
    }

    let source_markers = marker_count(&source.text);
    let dest_markers = marker_count(trimmed);
    if source_markers != dest_markers {
        issues.push(issue(
            &source.id,
            "markup_count_mismatch",
            Severity::Major,
            "图片 / 跳转标记数量与原文不一致",
        ));
    }

    let source_digits = digit_count(&source.text);
    let dest_digits = digit_count(trimmed);
    if source_digits != dest_digits {
        issues.push(issue(
            &source.id,
            "number_mismatch",
            Severity::Major,
            "数字数量与原文不一致",
        ));
    }

    let source_urls = url_count(&source.text);
    let dest_urls = url_count(trimmed);
    if source_urls != dest_urls {
        issues.push(issue(
            &source.id,
            "url_mismatch",
            Severity::Major,
            "URL 数量与原文不一致",
        ));
    }
}

fn check_language_purity(
    chunk: &Chunk,
    draft: &Draft,
    target_language: Option<&str>,
    issues: &mut Vec<Issue>,
) {
    let mut source_text = String::new();
    let mut translated_text = String::new();
    let mut anchor = String::new();
    for source in &chunk.segments {
        if source.kind == SegmentKind::Markup {
            continue;
        }
        if anchor.is_empty() {
            anchor = source.id.clone();
        }
        source_text.push_str(&source.text);
        if let Some(translated) = draft
            .segments
            .iter()
            .find(|translated| translated.id == source.id)
        {
            translated_text.push_str(&translated.text);
        }
    }

    let source_ns = non_whitespace(&source_text);
    let dest_ns = non_whitespace(&translated_text);
    if source_ns == 0 || dest_ns == 0 {
        return;
    }

    let source_kana = kana_count(&source_text);
    let source_han = han_count(&source_text);
    let dest_kana = kana_count(&translated_text);
    let dest_han = han_count(&translated_text);
    let dest_latin = latin_count(&translated_text);

    let source_is_japanese = source_kana * 100 >= source_ns * 5;
    let explicit_zh = target_language.is_some_and(|language| language.starts_with("zh"));
    let explicit_non_zh = target_language.is_some_and(|language| !language.starts_with("zh"));
    let target_is_zh = explicit_zh || (!explicit_non_zh && dest_han * 100 >= dest_ns * 30);

    if source_is_japanese && !explicit_non_zh && target_is_zh {
        // 15% 是保守阈值；短专名 / 拟声残留通常低于该比例，不误报整块。
        if dest_kana * 100 > dest_ns * KANA_RATIO_LIMIT_PERCENT {
            issues.push(issue(
                &anchor,
                "target_language_purity",
                Severity::Major,
                &format!(
                    "日译中假名残留比例偏高：{}%，上限 {}%",
                    dest_kana * 100 / dest_ns,
                    KANA_RATIO_LIMIT_PERCENT
                ),
            ));
        }
    }

    let source_is_chinese = source_han * 100 >= source_ns * 30 && source_kana * 100 < source_ns * 5;
    let explicit_en = target_language.is_some_and(|language| language.starts_with("en"));
    let target_is_en =
        explicit_en || (target_language.is_none() && dest_latin * 100 >= dest_ns * 50);
    if source_is_chinese && target_is_en && dest_han * 100 > dest_ns * HAN_RATIO_LIMIT_PERCENT {
        issues.push(issue(
            &anchor,
            "target_language_purity",
            Severity::Major,
            &format!(
                "中译外时汉字残留比例偏高：{}%，上限 {}%",
                dest_han * 100 / dest_ns,
                HAN_RATIO_LIMIT_PERCENT
            ),
        ));
    }
}

/// 合并校验：所有块都有通过结构校验的 draft、无重复片段、页行顺序单调、
/// 每页覆盖全部可译行。
pub fn validate_merge(
    chunks: &[Chunk],
    drafts: &[Draft],
    pages: &BTreeMap<u32, Vec<TranslatedLine>>,
) -> MergeReport {
    validate_merge_with_language(chunks, drafts, pages, None)
}

/// 合并校验，并显式指定目标语言（如 `zh-CN` / `en`）。
pub fn validate_merge_with_language(
    chunks: &[Chunk],
    drafts: &[Draft],
    pages: &BTreeMap<u32, Vec<TranslatedLine>>,
    target_language: Option<&str>,
) -> MergeReport {
    let mut missing_ids: BTreeSet<String> = BTreeSet::new();
    let mut duplicate_ids: BTreeSet<String> = BTreeSet::new();
    let mut markup_issues: Vec<Issue> = Vec::new();
    let mut coverage_ok = true;
    let mut order_ok = true;

    // 跨块重复：同一 segment 只能出现一次。
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for draft in drafts {
        for segment in &draft.segments {
            *seen.entry(segment.id.as_str()).or_insert(0) += 1;
        }
    }
    for (id, count) in seen {
        if count > 1 {
            duplicate_ids.insert(id.to_string());
        }
    }

    let draft_by_chunk: BTreeMap<&str, &Draft> = drafts
        .iter()
        .map(|draft| (draft.chunk_id.as_str(), draft))
        .collect();

    let mut expected_lines: BTreeSet<(u32, u32)> = BTreeSet::new();
    for chunk in chunks {
        for segment in &chunk.segments {
            if let Some((page, line, _)) = parse_segment_id(&segment.id) {
                expected_lines.insert((page, line));
            }
        }

        match draft_by_chunk.get(chunk.id.as_str()) {
            None => {
                coverage_ok = false;
                for segment in &chunk.segments {
                    missing_ids.insert(segment.id.clone());
                }
            }
            Some(draft) => {
                if let Err(issues) = validate_draft_with_language(chunk, draft, target_language) {
                    coverage_ok = false;
                    for issue in issues {
                        match issue.kind.as_str() {
                            "missing_segment" => {
                                missing_ids.insert(issue.segment_id.clone());
                            }
                            "duplicate_segment" => {
                                duplicate_ids.insert(issue.segment_id.clone());
                            }
                            "order_mismatch" => {
                                order_ok = false;
                            }
                            "markup_modified"
                            | "chapter_markup"
                            | "ruby_markup"
                            | "markup_count_mismatch" => {
                                markup_issues.push(issue);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // 页行顺序必须严格单调。
    for lines in pages.values() {
        if lines.windows(2).any(|pair| pair[0].line >= pair[1].line) {
            order_ok = false;
        }
    }

    // 页行覆盖：结果必须恰好覆盖全部可译行（缺行或多行都算不完整）。
    let result_lines: BTreeSet<(u32, u32)> = pages
        .iter()
        .flat_map(|(page, lines)| lines.iter().map(|line| (*page, line.line)))
        .collect();
    if result_lines != expected_lines {
        coverage_ok = false;
    }

    if !missing_ids.is_empty() || !duplicate_ids.is_empty() {
        coverage_ok = false;
    }

    markup_issues.sort_by(|left, right| {
        left.segment_id
            .cmp(&right.segment_id)
            .then(left.kind.cmp(&right.kind))
    });

    MergeReport {
        coverage_ok,
        order_ok,
        duplicate_ids: duplicate_ids.into_iter().collect(),
        missing_ids: missing_ids.into_iter().collect(),
        markup_issues,
    }
}

/// 按块顺序与片段序号把译文拼回页 / 行；同一行多个片段直接连接。
///
/// 缺失的片段跳过（由 [`validate_merge`] 报告），重复 ID 只取第一次出现。
pub fn merge_pages(chunks: &[Chunk], drafts: &[Draft]) -> BTreeMap<u32, Vec<TranslatedLine>> {
    let mut translated: BTreeMap<&str, &str> = BTreeMap::new();
    for draft in drafts {
        for segment in &draft.segments {
            translated
                .entry(segment.id.as_str())
                .or_insert(segment.text.as_str());
        }
    }

    let mut ordered: Vec<&Chunk> = chunks.iter().collect();
    ordered.sort_by(|left, right| left.order.cmp(&right.order).then(left.id.cmp(&right.id)));

    let mut pages: BTreeMap<u32, BTreeMap<u32, String>> = BTreeMap::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for chunk in ordered {
        for segment in &chunk.segments {
            if !seen.insert(segment.id.as_str()) {
                continue;
            }
            let Some(text) = translated.get(segment.id.as_str()) else {
                continue;
            };
            let Some((page, line, _)) = parse_segment_id(&segment.id) else {
                continue;
            };
            pages
                .entry(page)
                .or_default()
                .entry(line)
                .or_default()
                .push_str(text);
        }
    }

    pages
        .into_iter()
        .map(|(page, lines)| {
            let lines = lines
                .into_iter()
                .map(|(line, text)| TranslatedLine { line, text })
                .collect();
            (page, lines)
        })
        .collect()
}

fn issue(segment_id: &str, kind: &str, severity: Severity, detail: &str) -> Issue {
    Issue {
        segment_id: segment_id.to_string(),
        kind: kind.to_string(),
        severity,
        detail: detail.to_string(),
    }
}

fn chapter_title(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("[chapter:")?.strip_suffix(']')?;
    let title = rest.trim();
    if title.is_empty() { None } else { Some(title) }
}

fn ruby_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[rb:([^>\]]+)>([^\]]+)\]").expect("ruby 正则合法"))
}

fn marker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\[(?:pixivimage|uploadedimage|jump|jumpurl|jumpuri):[^\]]*\]")
            .expect("marker 正则合法")
    })
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"https?://[^\s\]\)]+").expect("url 正则合法"))
}

fn marker_count(text: &str) -> usize {
    marker_re().find_iter(text).count()
}

fn url_count(text: &str) -> usize {
    url_re().find_iter(text).count()
}

fn digit_count(text: &str) -> usize {
    text.chars().filter(char::is_ascii_digit).count()
}

fn non_whitespace(text: &str) -> usize {
    text.chars().filter(|ch| !ch.is_whitespace()).count()
}

fn kana_count(text: &str) -> usize {
    text.chars().filter(|ch| is_kana(*ch)).count()
}

fn han_count(text: &str) -> usize {
    text.chars().filter(|ch| is_han(*ch)).count()
}

fn latin_count(text: &str) -> usize {
    text.chars().filter(char::is_ascii_alphabetic).count()
}

fn is_kana(ch: char) -> bool {
    matches!(ch as u32,
        0x3040..=0x309F   // 平假名
        | 0x30A0..=0x30FF // 片假名（含 ー）
        | 0x31F0..=0x31FF // 片假名扩展
        | 0xFF66..=0xFF9D // 半角片假名
    )
}

fn is_han(ch: char) -> bool {
    matches!(ch as u32,
        0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF
        | 0x20000..=0x2FA1F
    )
}

// ----------------------------------------------------------------------
// 单元测试
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::artifacts::{TokenUsage, TranslatedSegment};
    use crate::translation::source::SourceSegment;

    fn segment(id: &str, text: &str, kind: SegmentKind) -> SourceSegment {
        SourceSegment {
            id: id.into(),
            text: text.into(),
            kind,
        }
    }

    fn chunk(segments: Vec<SourceSegment>) -> Chunk {
        Chunk {
            id: "c0001".into(),
            order: 1,
            segments,
            prev_context: String::new(),
            next_context: String::new(),
            chars: 0,
        }
    }

    fn draft(segments: Vec<(&str, &str)>) -> Draft {
        Draft {
            chunk_id: "c0001".into(),
            version: 1,
            segments: segments
                .into_iter()
                .map(|(id, text)| TranslatedSegment {
                    id: id.into(),
                    text: text.into(),
                })
                .collect(),
            usage: TokenUsage::default(),
            reviewed: false,
            needs_repair: false,
        }
    }

    #[test]
    fn accepts_complete_draft() {
        let chunk = chunk(vec![
            segment("p001-l000-s0", "正文一", SegmentKind::Text),
            segment("p001-l001-s0", "正文二", SegmentKind::Text),
        ]);
        let draft = draft(vec![("p001-l000-s0", "译文一"), ("p001-l001-s0", "译文二")]);
        assert!(validate_draft(&chunk, &draft).is_ok());
    }

    #[test]
    fn detects_missing_duplicate_unknown_and_order() {
        let chunk = chunk(vec![
            segment("p001-l000-s0", "甲", SegmentKind::Text),
            segment("p001-l001-s0", "乙", SegmentKind::Text),
        ]);

        let missing = draft(vec![("p001-l000-s0", "甲译")]);
        let issues = validate_draft(&chunk, &missing).unwrap_err();
        assert!(issues.iter().any(|issue| issue.kind == "missing_segment"));

        let duplicate = draft(vec![
            ("p001-l000-s0", "甲译"),
            ("p001-l000-s0", "甲译"),
            ("p001-l001-s0", "乙译"),
        ]);
        let issues = validate_draft(&chunk, &duplicate).unwrap_err();
        assert!(issues.iter().any(|issue| issue.kind == "duplicate_segment"));

        let unknown = draft(vec![
            ("p001-l000-s0", "甲译"),
            ("p001-l001-s0", "乙译"),
            ("p999-l999-s9", "多"),
        ]);
        let issues = validate_draft(&chunk, &unknown).unwrap_err();
        assert!(issues.iter().any(|issue| issue.kind == "unknown_segment"));

        let wrong_order = draft(vec![("p001-l001-s0", "乙译"), ("p001-l000-s0", "甲译")]);
        let issues = validate_draft(&chunk, &wrong_order).unwrap_err();
        assert!(issues.iter().any(|issue| issue.kind == "order_mismatch"));
    }

    #[test]
    fn detects_empty_and_oversized_translation() {
        let chunk = chunk(vec![segment("p001-l000-s0", "正文", SegmentKind::Text)]);
        let empty = draft(vec![("p001-l000-s0", "   ")]);
        let issues = validate_draft(&chunk, &empty).unwrap_err();
        assert!(issues.iter().any(|issue| issue.kind == "empty_translation"));

        let long = "字".repeat(MAX_SEGMENT_CHARS + 1);
        let oversized = draft(vec![("p001-l000-s0", &long)]);
        let issues = validate_draft(&chunk, &oversized).unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| issue.kind == "translation_too_long")
        );
    }

    #[test]
    fn markup_must_be_returned_verbatim() {
        let chunk = chunk(vec![segment(
            "p001-l000-s0",
            "[pixivimage:123]",
            SegmentKind::Markup,
        )]);
        let changed = draft(vec![("p001-l000-s0", "[pixivimage:456]")]);
        let issues = validate_draft(&chunk, &changed).unwrap_err();
        assert!(issues.iter().any(|issue| issue.kind == "markup_modified"));
    }

    #[test]
    fn chapter_structure_is_required() {
        let chunk = chunk(vec![segment(
            "p001-l000-s0",
            "[chapter:第一话]",
            SegmentKind::Chapter,
        )]);
        let broken = draft(vec![("p001-l000-s0", "第一话")]);
        let issues = validate_draft(&chunk, &broken).unwrap_err();
        assert!(issues.iter().any(|issue| issue.kind == "chapter_markup"));

        let good = draft(vec![("p001-l000-s0", "[chapter:第一章]")]);
        assert!(validate_draft(&chunk, &good).is_ok());
    }

    #[test]
    fn ruby_structure_must_survive() {
        let source = "[rb:漢字>かんじ]です";
        let chunk = chunk(vec![segment("p001-l000-s0", source, SegmentKind::Ruby)]);
        let broken = draft(vec![("p001-l000-s0", "汉字假名呀")]);
        let issues = validate_draft(&chunk, &broken).unwrap_err();
        assert!(issues.iter().any(|issue| issue.kind == "ruby_markup"));

        let good = draft(vec![("p001-l000-s0", "[rb:汉字>kanji]呀")]);
        assert!(validate_draft(&chunk, &good).is_ok());
    }

    #[test]
    fn numbers_urls_and_markers_must_match() {
        let chunk = chunk(vec![segment(
            "p001-l000-s0",
            "第123回，见 https://example.com 与[pixivimage:7]",
            SegmentKind::Text,
        )]);

        let lost_number = draft(vec![(
            "p001-l000-s0",
            "第回，见 https://example.com 与[pixivimage:7]",
        )]);
        let issues = validate_draft(&chunk, &lost_number).unwrap_err();
        assert!(issues.iter().any(|issue| issue.kind == "number_mismatch"));

        let lost_url = draft(vec![("p001-l000-s0", "第123回，见 与[pixivimage:7]")]);
        let issues = validate_draft(&chunk, &lost_url).unwrap_err();
        assert!(issues.iter().any(|issue| issue.kind == "url_mismatch"));

        let lost_marker = draft(vec![("p001-l000-s0", "第123回，见 https://example.com 与")]);
        let issues = validate_draft(&chunk, &lost_marker).unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| issue.kind == "markup_count_mismatch")
        );
    }

    #[test]
    fn japanese_residue_is_flagged_for_chinese_target() {
        let chunk = chunk(vec![segment(
            "p001-l000-s0",
            "これはテストです",
            SegmentKind::Text,
        )]);
        let residue = draft(vec![("p001-l000-s0", "これはテストです")]);
        let issues = validate_draft_with_language(&chunk, &residue, Some("zh-CN")).unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| issue.kind == "target_language_purity")
        );

        let natural = draft(vec![("p001-l000-s0", "这是测试")]);
        assert!(validate_draft_with_language(&chunk, &natural, Some("zh-CN")).is_ok());
    }

    #[test]
    fn chinese_residue_is_flagged_for_english_target() {
        let chunk = chunk(vec![segment(
            "p001-l000-s0",
            "这是一个中文句子",
            SegmentKind::Text,
        )]);
        let residue = draft(vec![("p001-l000-s0", "This is 一个中文句子")]);
        let issues = validate_draft_with_language(&chunk, &residue, Some("en")).unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| issue.kind == "target_language_purity")
        );

        let natural = draft(vec![("p001-l000-s0", "This is a Chinese sentence")]);
        assert!(validate_draft_with_language(&chunk, &natural, Some("en")).is_ok());
    }

    #[test]
    fn merge_pages_concatenates_same_line_segments() {
        let chunk = Chunk {
            id: "c0001".into(),
            order: 1,
            segments: vec![
                segment("p001-l000-s0", "前半", SegmentKind::Text),
                segment("p001-l000-s1", "后半", SegmentKind::Text),
                segment("p002-l000-s0", "第二页", SegmentKind::Text),
            ],
            prev_context: String::new(),
            next_context: String::new(),
            chars: 0,
        };
        let draft = draft(vec![
            ("p001-l000-s0", "前"),
            ("p001-l000-s1", "后"),
            ("p002-l000-s0", "次页"),
        ]);
        let pages = merge_pages(&[chunk], &[draft]);
        assert_eq!(
            pages[&1],
            vec![TranslatedLine {
                line: 0,
                text: "前后".into()
            }]
        );
        assert_eq!(
            pages[&2],
            vec![TranslatedLine {
                line: 0,
                text: "次页".into()
            }]
        );
    }

    #[test]
    fn explicit_target_language_affects_merge_coverage() {
        let chunks = vec![Chunk {
            id: "c0001".into(),
            order: 1,
            segments: vec![segment(
                "p001-l000-s0",
                "これはテストです",
                SegmentKind::Text,
            )],
            prev_context: String::new(),
            next_context: String::new(),
            chars: 8,
        }];
        let drafts = vec![draft(vec![("p001-l000-s0", "これはテストです")])];
        let pages = merge_pages(&chunks, &drafts);

        // 自动判断看到译文只有假名、没有汉字，不会当作中文目标。
        assert!(validate_merge(&chunks, &drafts, &pages).coverage_ok);
        // 显式中文目标时，假名残留使草稿校验不通过。
        assert!(!validate_merge_with_language(&chunks, &drafts, &pages, Some("zh-CN")).coverage_ok);
    }

    #[test]
    fn validate_merge_reports_complete_coverage() {
        let chunks = vec![Chunk {
            id: "c0001".into(),
            order: 1,
            segments: vec![
                segment("p001-l000-s0", "正文", SegmentKind::Text),
                segment("p001-l001-s0", "正文", SegmentKind::Text),
            ],
            prev_context: String::new(),
            next_context: String::new(),
            chars: 4,
        }];
        let drafts = vec![draft(vec![
            ("p001-l000-s0", "译文"),
            ("p001-l001-s0", "译文"),
        ])];
        let pages = merge_pages(&chunks, &drafts);
        let report = validate_merge(&chunks, &drafts, &pages);
        assert!(report.coverage_ok);
        assert!(report.order_ok);
        assert!(report.duplicate_ids.is_empty());
        assert!(report.missing_ids.is_empty());
        assert!(report.markup_issues.is_empty());

        // 结果缺一行 -> 覆盖不完整。
        let mut broken = pages.clone();
        broken.get_mut(&1).unwrap().pop();
        let report = validate_merge(&chunks, &drafts, &broken);
        assert!(!report.coverage_ok);
    }

    #[test]
    fn validate_merge_reports_missing_draft_and_markup_issues() {
        let chunks = vec![
            Chunk {
                id: "c0001".into(),
                order: 1,
                segments: vec![segment("p001-l000-s0", "正文", SegmentKind::Text)],
                prev_context: String::new(),
                next_context: String::new(),
                chars: 2,
            },
            Chunk {
                id: "c0002".into(),
                order: 2,
                segments: vec![segment(
                    "p001-l001-s0",
                    "[pixivimage:1]",
                    SegmentKind::Markup,
                )],
                prev_context: String::new(),
                next_context: String::new(),
                chars: 0,
            },
        ];
        let drafts = vec![
            draft(vec![("p001-l000-s0", "译文")]),
            Draft {
                chunk_id: "c0002".into(),
                version: 1,
                segments: vec![TranslatedSegment {
                    id: "p001-l001-s0".into(),
                    text: "[pixivimage:2]".into(),
                }],
                usage: TokenUsage::default(),
                reviewed: false,
                needs_repair: false,
            },
        ];
        let pages = merge_pages(&chunks, &drafts);
        let report = validate_merge(&chunks, &drafts, &pages);
        assert!(!report.coverage_ok);
        assert_eq!(report.markup_issues.len(), 1);
        assert_eq!(report.markup_issues[0].kind, "markup_modified");

        // 完全没有 draft 的块全部片段记为缺失。
        let report = validate_merge(&chunks, &drafts[..1], &pages);
        assert!(!report.coverage_ok);
        assert!(report.missing_ids.contains(&"p001-l001-s0".to_string()));
    }

    #[test]
    fn validate_merge_detects_duplicate_ids_across_drafts() {
        let chunks = vec![
            Chunk {
                id: "c0001".into(),
                order: 1,
                segments: vec![segment("p001-l000-s0", "甲", SegmentKind::Text)],
                prev_context: String::new(),
                next_context: String::new(),
                chars: 1,
            },
            Chunk {
                id: "c0002".into(),
                order: 2,
                segments: vec![segment("p001-l001-s0", "乙", SegmentKind::Text)],
                prev_context: String::new(),
                next_context: String::new(),
                chars: 1,
            },
        ];
        let drafts = vec![
            draft(vec![("p001-l000-s0", "甲译")]),
            Draft {
                chunk_id: "c0002".into(),
                version: 1,
                segments: vec![TranslatedSegment {
                    id: "p001-l000-s0".into(),
                    text: "重复".into(),
                }],
                usage: TokenUsage::default(),
                reviewed: false,
                needs_repair: false,
            },
        ];
        let pages = merge_pages(&chunks, &drafts);
        let report = validate_merge(&chunks, &drafts, &pages);
        assert!(report.duplicate_ids.contains(&"p001-l000-s0".to_string()));
    }
}
