//! 发布投影：Markdown 合并稿、发布状态与早发布页选择。
//!
//! Markdown 规则沿用 `core::exporter`：
//! - 整行 `[chapter:X]` → `## X`；
//! - 页间（原 `[newpage]`）→ `---`；
//! - `[jump:]` 整行删除；
//! - 整行图片标记 → `<!-- 图片占位 -->`；
//! - 行内 `[rb:A>B]` → `A(B)`。
//!
//! 发布状态是确定性判断，不替代审查：只有每个块都有通过结构校验且版本匹配的
//! 审查、整体复核 `passed=true` 时才为 [`PublishStatus::Complete`]。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::translation::artifacts::{
    Draft, GlobalReview, Issue, PublishStatus, Review, ReviewVerdict, Severity, TranslatedLine,
};
use crate::translation::chunking::Chunk;
use crate::translation::source::{SegmentKind, SourceDoc, parse_segment_id};
use crate::translation::validation::validate_draft_with_language;

/// 已审查且可提前投影的页集合。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewedPages {
    /// 全部块已审查通过、无 critical / major 问题，可投影到正文的页。
    pub ready: BTreeSet<u32>,
    /// 仍待审查或存在阻断问题的页。
    pub pending: BTreeSet<u32>,
}

/// 渲染 Markdown 合并稿；缺失译文的行回退原文，保证合并稿可读。
pub fn render_markdown(doc: &SourceDoc, pages: &BTreeMap<u32, Vec<TranslatedLine>>) -> String {
    let mut out: Vec<String> = Vec::new();
    for (index, page) in doc.pages.iter().enumerate() {
        if index > 0 {
            out.push("\n---\n".to_string());
        }
        let translated: BTreeMap<u32, &str> = pages
            .get(&page.page)
            .map(|lines| {
                lines
                    .iter()
                    .map(|line| (line.line, line.text.as_str()))
                    .collect()
            })
            .unwrap_or_default();

        for line in &page.lines {
            let source = line.text.as_str();
            let text = translated.get(&line.line).copied().unwrap_or(source);
            let is_chapter = line
                .segments
                .first()
                .is_some_and(|segment| segment.kind == SegmentKind::Chapter);

            if is_chapter {
                let title = chapter_title(text)
                    .or_else(|| chapter_title(source))
                    .unwrap_or_default();
                out.push(format!("\n## {title}\n"));
            } else if source.starts_with("[jump:") {
                continue;
            } else if source.starts_with("[pixivimage:") || source.starts_with("[uploadedimage:") {
                out.push("<!-- 图片占位 -->\n".to_string());
            } else if is_pure_markup(line) {
                // 其它纯标记行（如 [jumpurl:...]）按原文保留，不当作正文翻译。
                out.push(source.to_string());
            } else {
                out.push(apply_rb(text));
            }
        }
    }
    out.join("\n")
}

/// 发布状态判定（目标语言按原文 / 译文自动判断）。
pub fn publish_status(
    chunks: &[Chunk],
    drafts: &[Draft],
    reviews: &[Review],
    global_review: Option<&GlobalReview>,
) -> PublishStatus {
    publish_status_with_language(chunks, drafts, reviews, global_review, None)
}

/// 发布状态判定，并显式指定目标语言（如 `zh-CN` / `en`）。
pub fn publish_status_with_language(
    chunks: &[Chunk],
    drafts: &[Draft],
    reviews: &[Review],
    global_review: Option<&GlobalReview>,
    target_language: Option<&str>,
) -> PublishStatus {
    if chunks.is_empty() {
        return PublishStatus::NeedsAttention;
    }

    let draft_by_chunk: BTreeMap<&str, &Draft> = drafts
        .iter()
        .map(|draft| (draft.chunk_id.as_str(), draft))
        .collect();
    let review_by_chunk: BTreeMap<&str, &Review> = reviews
        .iter()
        .map(|review| (review.chunk_id.as_str(), review))
        .collect();

    let mut complete = true;
    for chunk in chunks {
        let Some(draft) = draft_by_chunk.get(chunk.id.as_str()) else {
            // 尚未译出：属于「部分完成」，不是需要处理的错误。
            complete = false;
            continue;
        };
        if draft.needs_repair {
            return PublishStatus::NeedsAttention;
        }
        if validate_draft_with_language(chunk, draft, target_language).is_err() {
            return PublishStatus::NeedsAttention;
        }
        let Some(review) = review_by_chunk.get(chunk.id.as_str()) else {
            complete = false;
            continue;
        };
        if review.draft_version != draft.version {
            complete = false;
            continue;
        }
        match review.verdict {
            ReviewVerdict::Pass => {
                // 通过结论可以携带 minor 建议（展示 / 可选修订），只有
                // critical / major 阻断发布。submit_review 已在入口拒绝
                // pass + blocking，这里再做一次确定性兜底。
                if review.issues.iter().any(is_blocking) {
                    return PublishStatus::NeedsAttention;
                }
            }
            ReviewVerdict::NeedsFix => {
                if review.issues.iter().any(is_blocking) {
                    return PublishStatus::NeedsAttention;
                }
                complete = false;
            }
        }
    }

    match global_review {
        None => complete = false,
        Some(review) if !review.passed => return PublishStatus::NeedsAttention,
        Some(_) => {}
    }

    if complete {
        PublishStatus::Complete
    } else {
        PublishStatus::Partial
    }
}

/// 早发布投影：只返回全部块已审查通过、且无 critical / major 问题的页。
pub fn reviewed_pages(chunks: &[Chunk], drafts: &[Draft], reviews: &[Review]) -> ReviewedPages {
    reviewed_pages_with_language(chunks, drafts, reviews, None)
}

/// 早发布投影，并显式指定目标语言。
pub fn reviewed_pages_with_language(
    chunks: &[Chunk],
    drafts: &[Draft],
    reviews: &[Review],
    target_language: Option<&str>,
) -> ReviewedPages {
    let draft_by_chunk: BTreeMap<&str, &Draft> = drafts
        .iter()
        .map(|draft| (draft.chunk_id.as_str(), draft))
        .collect();
    let review_by_chunk: BTreeMap<&str, &Review> = reviews
        .iter()
        .map(|review| (review.chunk_id.as_str(), review))
        .collect();

    let mut page_chunks: BTreeMap<u32, BTreeSet<&str>> = BTreeMap::new();
    for chunk in chunks {
        for segment in &chunk.segments {
            if let Some((page, _, _)) = parse_segment_id(&segment.id) {
                page_chunks
                    .entry(page)
                    .or_default()
                    .insert(chunk.id.as_str());
            }
        }
    }

    let mut result = ReviewedPages::default();
    for (page, chunk_ids) in page_chunks {
        let ready = chunk_ids.iter().all(|chunk_id| {
            let Some(chunk) = chunks.iter().find(|chunk| chunk.id == *chunk_id) else {
                return false;
            };
            let Some(draft) = draft_by_chunk.get(*chunk_id) else {
                return false;
            };
            if !draft.reviewed || draft.needs_repair {
                return false;
            }
            if validate_draft_with_language(chunk, draft, target_language).is_err() {
                return false;
            }
            let Some(review) = review_by_chunk.get(*chunk_id) else {
                return false;
            };
            review.draft_version == draft.version
                && review.verdict == ReviewVerdict::Pass
                && !review.issues.iter().any(is_blocking)
        });
        if ready {
            result.ready.insert(page);
        } else {
            result.pending.insert(page);
        }
    }
    result
}

fn is_blocking(issue: &Issue) -> bool {
    matches!(issue.severity, Severity::Critical | Severity::Major)
}

fn is_pure_markup(line: &crate::translation::source::SourceLine) -> bool {
    line.segments
        .iter()
        .all(|segment| segment.kind == SegmentKind::Markup)
}

fn chapter_title(text: &str) -> Option<&str> {
    let rest = text.trim().strip_prefix("[chapter:")?.strip_suffix(']')?;
    let title = rest.trim();
    if title.is_empty() { None } else { Some(title) }
}

fn apply_rb(line: &str) -> String {
    static RB: OnceLock<Regex> = OnceLock::new();
    let re = RB.get_or_init(|| Regex::new(r"\[rb:([^>]+)>([^\]]+)\]").expect("rb 正则合法"));
    re.replace_all(line, "${1}(${2})").to_string()
}

// ----------------------------------------------------------------------
// 单元测试
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::artifacts::{TokenUsage, TranslatedSegment};
    use crate::translation::source::{SourceLine, SourcePage, SourceSegment, segment_id};

    fn line(page: u32, number: u32, text: &str, kind: SegmentKind) -> SourceLine {
        SourceLine {
            line: number,
            text: text.into(),
            segments: vec![SourceSegment {
                id: segment_id(page, number, 0),
                text: text.into(),
                kind,
            }],
        }
    }

    fn doc() -> SourceDoc {
        SourceDoc {
            pages: vec![
                SourcePage {
                    page: 1,
                    text: String::new(),
                    lines: vec![
                        line(1, 0, "[chapter:第一话]", SegmentKind::Chapter),
                        line(1, 1, "[rb:漢字>かんじ]です", SegmentKind::Ruby),
                        line(1, 2, "[jump:1]", SegmentKind::Markup),
                        line(1, 3, "[pixivimage:9]", SegmentKind::Markup),
                    ],
                },
                SourcePage {
                    page: 2,
                    text: String::new(),
                    lines: vec![line(2, 0, "尾巴", SegmentKind::Text)],
                },
            ],
            input_hash: "hash".into(),
            target_language: "zh-CN".into(),
        }
    }

    fn translations() -> BTreeMap<u32, Vec<TranslatedLine>> {
        BTreeMap::from([
            (
                1,
                vec![
                    TranslatedLine {
                        line: 0,
                        text: "[chapter:第一章]".into(),
                    },
                    TranslatedLine {
                        line: 1,
                        text: "[rb:汉字>kanji]呀".into(),
                    },
                ],
            ),
            (
                2,
                vec![TranslatedLine {
                    line: 0,
                    text: "尾巴译文".into(),
                }],
            ),
        ])
    }

    #[test]
    fn markdown_follows_exporter_rules() {
        let markdown = render_markdown(&doc(), &translations());
        assert!(markdown.contains("## 第一章"));
        assert!(markdown.contains("汉字(kanji)呀"));
        assert!(markdown.contains("<!-- 图片占位 -->"));
        assert!(!markdown.contains("[jump:1]"));
        assert!(markdown.contains("\n---\n"));
        assert!(markdown.contains("尾巴译文"));
    }

    #[test]
    fn markdown_falls_back_to_source_when_translation_missing() {
        let markdown = render_markdown(&doc(), &BTreeMap::new());
        assert!(markdown.contains("## 第一话"));
        assert!(markdown.contains("漢字(かんじ)です"));
        assert!(markdown.contains("尾巴"));
    }

    fn chunk(id: &str, order: u32, segment: &str, text: &str) -> Chunk {
        Chunk {
            id: id.into(),
            order,
            segments: vec![SourceSegment {
                id: segment.into(),
                text: text.into(),
                kind: SegmentKind::Text,
            }],
            prev_context: String::new(),
            next_context: String::new(),
            chars: text.chars().count() as u32,
        }
    }

    fn valid_draft(chunk_id: &str, segment: &str, text: &str) -> Draft {
        Draft {
            chunk_id: chunk_id.into(),
            version: 1,
            segments: vec![TranslatedSegment {
                id: segment.into(),
                text: text.into(),
            }],
            usage: TokenUsage::default(),
            reviewed: true,
            needs_repair: false,
        }
    }

    fn pass_review(chunk_id: &str) -> Review {
        Review {
            chunk_id: chunk_id.into(),
            draft_version: 1,
            verdict: ReviewVerdict::Pass,
            issues: vec![],
            notes: String::new(),
        }
    }

    #[test]
    fn complete_status_requires_every_review_and_global_pass() {
        let chunks = vec![chunk("c0001", 1, "p001-l000-s0", "正文")];
        let drafts = vec![valid_draft("c0001", "p001-l000-s0", "译文")];
        let reviews = vec![pass_review("c0001")];
        let global = GlobalReview {
            passed: true,
            sample_chunk_ids: vec!["c0001".into()],
            notes: String::new(),
        };

        assert_eq!(
            publish_status(&chunks, &drafts, &reviews, Some(&global)),
            PublishStatus::Complete
        );
        assert_eq!(
            publish_status(&chunks, &drafts, &reviews, None),
            PublishStatus::Partial
        );
        assert_eq!(
            publish_status(
                &chunks,
                &drafts,
                &reviews,
                Some(&GlobalReview {
                    passed: false,
                    ..global
                })
            ),
            PublishStatus::NeedsAttention
        );
    }

    #[test]
    fn critical_review_issue_needs_attention() {
        let chunks = vec![chunk("c0001", 1, "p001-l000-s0", "正文")];
        let drafts = vec![valid_draft("c0001", "p001-l000-s0", "译文")];
        let reviews = vec![Review {
            chunk_id: "c0001".into(),
            draft_version: 1,
            verdict: ReviewVerdict::NeedsFix,
            issues: vec![Issue {
                segment_id: "p001-l000-s0".into(),
                kind: "mistranslation".into(),
                severity: Severity::Critical,
                detail: "关键情节错误".into(),
            }],
            notes: String::new(),
        }];
        assert_eq!(
            publish_status(&chunks, &drafts, &reviews, None),
            PublishStatus::NeedsAttention
        );
    }

    #[test]
    fn pass_with_only_minor_issues_publishes() {
        let chunks = vec![chunk("c0001", 1, "p001-l000-s0", "正文")];
        let drafts = vec![valid_draft("c0001", "p001-l000-s0", "译文")];
        let mut review = pass_review("c0001");
        review.issues = vec![Issue {
            segment_id: "p001-l000-s0".into(),
            kind: "style".into(),
            severity: Severity::Minor,
            detail: "语序可更自然".into(),
        }];
        let global = GlobalReview {
            passed: true,
            sample_chunk_ids: vec!["c0001".into()],
            notes: String::new(),
        };

        // pass + minor 是允许的通过结论：minor 只展示，不阻塞发布。
        assert_eq!(
            publish_status(&chunks, &drafts, &[review.clone()], Some(&global)),
            PublishStatus::Complete
        );
        // 早发布投影同样放行。
        let pages = reviewed_pages(&chunks, &drafts, &[review]);
        assert_eq!(pages.ready, BTreeSet::from([1]));
    }

    #[test]
    fn pass_with_blocking_issue_needs_attention() {
        // submit_review 已在入口拒绝 pass + blocking；这里验证发布层的确定性兜底。
        for severity in [Severity::Major, Severity::Critical] {
            let chunks = vec![chunk("c0001", 1, "p001-l000-s0", "正文")];
            let drafts = vec![valid_draft("c0001", "p001-l000-s0", "译文")];
            let mut review = pass_review("c0001");
            review.issues = vec![Issue {
                segment_id: "p001-l000-s0".into(),
                kind: "mistranslation".into(),
                severity,
                detail: "情节错误".into(),
            }];
            let global = GlobalReview {
                passed: true,
                sample_chunk_ids: vec!["c0001".into()],
                notes: String::new(),
            };
            assert_eq!(
                publish_status(&chunks, &drafts, &[review], Some(&global)),
                PublishStatus::NeedsAttention,
                "{severity:?} 必须阻断发布"
            );
        }
    }

    #[test]
    fn needs_repair_or_structure_failure_needs_attention() {
        let chunks = vec![chunk("c0001", 1, "p001-l000-s0", "正文")];
        let mut draft = valid_draft("c0001", "p001-l000-s0", "译文");
        draft.needs_repair = true;
        assert_eq!(
            publish_status(&chunks, &[draft], &[], None),
            PublishStatus::NeedsAttention
        );

        let broken = Draft {
            chunk_id: "c0001".into(),
            version: 1,
            segments: vec![],
            usage: TokenUsage::default(),
            reviewed: false,
            needs_repair: false,
        };
        assert_eq!(
            publish_status(&chunks, &[broken], &[], None),
            PublishStatus::NeedsAttention
        );
    }

    #[test]
    fn explicit_target_language_can_block_publish() {
        let chunks = vec![chunk("c0001", 1, "p001-l000-s0", "これはテストです")];
        let drafts = vec![valid_draft("c0001", "p001-l000-s0", "これはテストです")];
        let reviews = vec![pass_review("c0001")];
        let global = GlobalReview {
            passed: true,
            sample_chunk_ids: vec!["c0001".into()],
            notes: String::new(),
        };

        // 自动判断不会把纯假名译文当作中文残留。
        assert_eq!(
            publish_status(&chunks, &drafts, &reviews, Some(&global)),
            PublishStatus::Complete
        );
        // 显式中文目标时，假名残留使草稿校验失败。
        assert_eq!(
            publish_status_with_language(&chunks, &drafts, &reviews, Some(&global), Some("zh-CN")),
            PublishStatus::NeedsAttention
        );
    }

    #[test]
    fn early_publish_pages_require_reviewed_clean_chunks() {
        let chunks = vec![
            chunk("c0001", 1, "p001-l000-s0", "正文一"),
            chunk("c0002", 2, "p002-l000-s0", "正文二"),
        ];
        let drafts = vec![
            valid_draft("c0001", "p001-l000-s0", "译文一"),
            valid_draft("c0002", "p002-l000-s0", "译文二"),
        ];
        // 只有第一块有审查结论。
        let reviews = vec![pass_review("c0001")];
        let pages = reviewed_pages(&chunks, &drafts, &reviews);
        assert_eq!(pages.ready, BTreeSet::from([1]));
        assert_eq!(pages.pending, BTreeSet::from([2]));

        // 第二块补齐后两页都就绪。
        let reviews = vec![pass_review("c0001"), pass_review("c0002")];
        let pages = reviewed_pages(&chunks, &drafts, &reviews);
        assert_eq!(pages.ready, BTreeSet::from([1, 2]));
        assert!(pages.pending.is_empty());
    }

    #[test]
    fn unreviewed_draft_is_not_ready() {
        let chunks = vec![chunk("c0001", 1, "p001-l000-s0", "正文")];
        let mut draft = valid_draft("c0001", "p001-l000-s0", "译文");
        draft.reviewed = false;
        let pages = reviewed_pages(&chunks, &[draft], &[pass_review("c0001")]);
        assert!(pages.ready.is_empty());
        assert_eq!(pages.pending, BTreeSet::from([1]));
    }

    #[test]
    fn stale_review_does_not_complete() {
        let chunks = vec![chunk("c0001", 1, "p001-l000-s0", "正文")];
        let drafts = vec![valid_draft("c0001", "p001-l000-s0", "译文")];
        let mut review = pass_review("c0001");
        review.draft_version = 0;
        assert_eq!(
            publish_status(&chunks, &drafts, &[review], None),
            PublishStatus::Partial
        );
    }
}
