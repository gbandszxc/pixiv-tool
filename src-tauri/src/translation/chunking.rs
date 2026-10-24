//! 语义切块（Pass 0 下半段）。
//!
//! 输入是 [`SourceDoc`]，输出是按原文顺序排列的 [`Chunk`]。切块只做确定性操作：
//! - 不拆分任何 `segment`，保证译文可以按 ID 原样合并回页 / 行；
//! - 语义边界优先级：章节 > 空行段落 > 行；页面起点与章节同等对待；
//! - 块目标不超过 `target_chars`，硬上限 `max_chars`；
//! - 单个 segment 本身超过 `max_chars` 时独占一块并通过 [`ChunkWarning`] 显式报告，
//!   不静默截断；
//! - 相邻块的 `prev_context` / `next_context` 各取 `context_chars` 个字符；
//! - 同一输入多次调用输出完全一致（无 HashMap 迭代序）。

use serde::{Deserialize, Serialize};

use crate::translation::source::{SegmentKind, SourceDoc, SourceSegment};

/// 切块参数（由设置提供，经 U4 传入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkConfig {
    /// 单块目标字符数。
    pub target_chars: usize,
    /// 单块硬上限（单个超长 segment 可突破，但会报告警告）。
    pub max_chars: usize,
    /// 相邻块前后参考窗口字符数。
    pub context_chars: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            target_chars: 3_000,
            max_chars: 6_000,
            context_chars: 600,
        }
    }
}

/// 一个处理单元：连续原文片段 + 前后文参考窗口。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    /// `c0001` 起。
    pub id: String,
    /// 全局顺序，1 起。
    pub order: u32,
    pub segments: Vec<SourceSegment>,
    pub prev_context: String,
    pub next_context: String,
    /// 本块片段字符数（Unicode scalar 计数）。
    pub chars: u32,
}

/// 单块超限报告：segment 本身大于 `max_chars`，已独占一块。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkWarning {
    pub chunk_id: String,
    pub segment_id: String,
    pub chars: u32,
    pub max_chars: u32,
}

/// 切块；超限信息可用 [`chunk_source_with_warnings`] 取得。
pub fn chunk_source(doc: &SourceDoc, config: &ChunkConfig) -> Vec<Chunk> {
    chunk_source_with_warnings(doc, config).0
}

/// 切块并返回超限警告。
pub fn chunk_source_with_warnings(
    doc: &SourceDoc,
    config: &ChunkConfig,
) -> (Vec<Chunk>, Vec<ChunkWarning>) {
    let target = config.target_chars.max(1);
    let max_chars = config.max_chars.max(target);

    let atoms = collect_atoms(doc);
    if atoms.is_empty() {
        return (Vec::new(), Vec::new());
    }

    // prefix[i] = 前 i 个 atom 的字符数之和。
    let mut prefix = Vec::with_capacity(atoms.len() + 1);
    prefix.push(0usize);
    for atom in &atoms {
        let next = prefix.last().copied().unwrap_or(0) + atom.segment.text.chars().count();
        prefix.push(next);
    }

    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    while start < atoms.len() {
        let mut end = start;
        let mut chars = 0usize;
        while end < atoms.len() {
            let segment_chars = prefix[end + 1] - prefix[end];
            if end > start && chars + segment_chars > target {
                break;
            }
            chars += segment_chars;
            end += 1;
            // 单个 segment 已超过硬上限：不再拼接后续内容。
            if end - start == 1 && chars > max_chars {
                break;
            }
        }

        let cutoff = choose_cutoff(&atoms, &prefix, start, end, target);
        let cutoff = cutoff.max(start + 1).min(end);
        ranges.push((start, cutoff));
        start = cutoff;
    }

    let mut texts = Vec::with_capacity(ranges.len());
    for &(start, end) in &ranges {
        texts.push(chunk_text(&atoms[start..end]));
    }

    let mut chunks = Vec::with_capacity(ranges.len());
    let mut warnings = Vec::new();
    for (index, &(start, end)) in ranges.iter().enumerate() {
        let order = (index + 1) as u32;
        let id = format!("c{order:04}");
        let segments: Vec<SourceSegment> = atoms[start..end]
            .iter()
            .map(|atom| atom.segment.clone())
            .collect();
        let chars = prefix[end] - prefix[start];
        let prev_context = index
            .checked_sub(1)
            .and_then(|previous| texts.get(previous))
            .map(|text| tail_chars(text, config.context_chars))
            .unwrap_or_default();
        let next_context = texts
            .get(index + 1)
            .map(|text| head_chars(text, config.context_chars))
            .unwrap_or_default();

        for segment in &segments {
            let segment_chars = segment.text.chars().count();
            if segment_chars > max_chars {
                warnings.push(ChunkWarning {
                    chunk_id: id.clone(),
                    segment_id: segment.id.clone(),
                    chars: saturating_u32(segment_chars),
                    max_chars: saturating_u32(max_chars),
                });
            }
        }

        chunks.push(Chunk {
            id,
            order,
            segments,
            prev_context,
            next_context,
            chars: saturating_u32(chars),
        });
    }

    (chunks, warnings)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Boundary {
    /// 页面起点或章节标记：必须另起一块。
    Chapter,
    /// 空行分隔的段落起点。
    Paragraph,
    /// 普通行起点。
    Line,
    /// 同一行的后续片段。
    None,
}

struct Atom {
    segment: SourceSegment,
    boundary: Boundary,
}

fn collect_atoms(doc: &SourceDoc) -> Vec<Atom> {
    let mut atoms = Vec::new();
    for page in &doc.pages {
        for (index, line) in page.lines.iter().enumerate() {
            let line_boundary = if index == 0 {
                Boundary::Chapter
            } else {
                let previous = &page.lines[index - 1];
                if line.line > previous.line + 1 {
                    Boundary::Paragraph
                } else {
                    Boundary::Line
                }
            };
            let first_is_chapter = line.segments.first().is_some_and(|segment| {
                segment.kind == SegmentKind::Chapter || segment.text.starts_with("[chapter:")
            });

            for (segment_index, segment) in line.segments.iter().enumerate() {
                let boundary = if segment_index == 0 {
                    if first_is_chapter {
                        Boundary::Chapter
                    } else {
                        line_boundary
                    }
                } else {
                    Boundary::None
                };
                atoms.push(Atom {
                    segment: segment.clone(),
                    boundary,
                });
            }
        }
    }
    atoms
}

/// 在 `(start, end]` 内选择切点：章节边界强制生效；段落 / 行边界在块已填充
/// 至少一半目标时优先采用较晚者。
fn choose_cutoff(
    atoms: &[Atom],
    prefix: &[usize],
    start: usize,
    end: usize,
    target: usize,
) -> usize {
    let boundary_at = |index: usize| atoms.get(index).map(|atom| atom.boundary);

    // 章节 / 页面起点：最早出现者即切点，让章节标题开启新块。
    if let Some(cut) =
        (start + 1..=end).find(|&index| boundary_at(index) == Some(Boundary::Chapter))
    {
        return cut;
    }

    let min_fill = target.div_ceil(2);
    let fill = |index: usize| prefix[index].saturating_sub(prefix[start]);

    if let Some(cut) = (start + 1..=end)
        .rev()
        .find(|&index| boundary_at(index) == Some(Boundary::Paragraph) && fill(index) >= min_fill)
    {
        return cut;
    }

    // 若本次填充恰好停在行中间，优先回退到最近的行首；文档末尾直接收尾。
    if end < atoms.len()
        && atoms
            .get(end)
            .is_some_and(|atom| atom.boundary == Boundary::None)
    {
        if let Some(cut) = (start + 1..=end)
            .rev()
            .find(|&index| boundary_at(index) == Some(Boundary::Line) && fill(index) >= min_fill)
        {
            return cut;
        }
    }

    end
}

/// 块内文本：不同行之间补 `\n`，同一行片段直接连接。
fn chunk_text(atoms: &[Atom]) -> String {
    let mut out = String::new();
    let mut previous_line: Option<&str> = None;
    for atom in atoms {
        let line_key = atom
            .segment
            .id
            .rsplit_once("-s")
            .map(|(key, _)| key)
            .unwrap_or(atom.segment.id.as_str());
        if let Some(previous) = previous_line {
            if previous != line_key {
                out.push('\n');
            }
        }
        out.push_str(&atom.segment.text);
        previous_line = Some(line_key);
    }
    out
}

fn head_chars(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn tail_chars(text: &str, count: usize) -> String {
    let total = text.chars().count();
    if total <= count {
        text.to_string()
    } else {
        text.chars().skip(total - count).collect()
    }
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

// ----------------------------------------------------------------------
// 单元测试
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::source::{SourceLine, SourcePage, segment_id};

    fn doc_from_pages(pages: Vec<Vec<(u32, &str)>>) -> SourceDoc {
        let mut out = Vec::new();
        for (page_index, lines) in pages.into_iter().enumerate() {
            let page = (page_index + 1) as u32;
            let text = lines
                .iter()
                .map(|(_, text)| *text)
                .collect::<Vec<_>>()
                .join("\n");
            let source_lines = lines
                .into_iter()
                .map(|(line, text)| SourceLine {
                    line,
                    text: text.to_string(),
                    segments: vec![SourceSegment {
                        id: segment_id(page, line, 0),
                        text: text.to_string(),
                        kind: SegmentKind::Text,
                    }],
                })
                .collect();
            out.push(SourcePage {
                page,
                text,
                lines: source_lines,
            });
        }
        SourceDoc {
            pages: out,
            input_hash: "test".into(),
            target_language: "zh-CN".into(),
        }
    }

    fn doc_with_many_lines(count: u32) -> SourceDoc {
        let page = SourcePage {
            page: 1,
            text: String::new(),
            lines: (0..count)
                .map(|line| SourceLine {
                    line,
                    text: "abcdefghij".into(),
                    segments: vec![SourceSegment {
                        id: segment_id(1, line, 0),
                        text: "abcdefghij".into(),
                        kind: SegmentKind::Text,
                    }],
                })
                .collect(),
        };
        SourceDoc {
            pages: vec![page],
            input_hash: "test".into(),
            target_language: "zh-CN".into(),
        }
    }

    #[test]
    fn respects_target_and_hard_limit() {
        let doc = doc_with_many_lines(40);
        let config = ChunkConfig {
            target_chars: 50,
            max_chars: 200,
            context_chars: 10,
        };
        let chunks = chunk_source(&doc, &config);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.chars as usize <= config.max_chars);
            if chunk.segments.len() > 1 {
                assert!(chunk.chars as usize <= config.target_chars);
            }
        }
    }

    #[test]
    fn never_splits_segments_and_preserves_order() {
        let doc = doc_with_many_lines(17);
        let config = ChunkConfig {
            target_chars: 35,
            max_chars: 100,
            context_chars: 5,
        };
        let chunks = chunk_source(&doc, &config);
        let expected: Vec<String> = doc
            .pages
            .iter()
            .flat_map(|page| page.lines.iter())
            .flat_map(|line| line.segments.iter())
            .map(|segment| segment.id.clone())
            .collect();
        let actual: Vec<String> = chunks
            .iter()
            .flat_map(|chunk| chunk.segments.iter())
            .map(|segment| segment.id.clone())
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn chapter_boundary_starts_new_chunk() {
        let doc = doc_from_pages(vec![vec![
            (0, "前文一"),
            (1, "前文二"),
            (2, "[chapter:第一章]"),
            (3, "正文"),
        ]]);
        let config = ChunkConfig {
            target_chars: 1_000,
            max_chars: 1_000,
            context_chars: 10,
        };
        let chunks = chunk_source(&doc, &config);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].segments[0].text, "[chapter:第一章]");
    }

    #[test]
    fn paragraph_boundary_preferred_near_target() {
        let doc = doc_from_pages(vec![vec![
            (0, "aaaaaaaaaa"),
            (1, "bbbbbbbbbb"),
            // 空行（行号跳跃）后是新段落
            (3, "cccccccccc"),
            (4, "dddddddddd"),
        ]]);
        let config = ChunkConfig {
            target_chars: 20,
            max_chars: 100,
            context_chars: 5,
        };
        let chunks = chunk_source(&doc, &config);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].segments.len(), 2);
        assert_eq!(chunks[1].segments.len(), 2);
    }

    #[test]
    fn context_windows_follow_adjacent_chunks() {
        let doc = doc_with_many_lines(6);
        let config = ChunkConfig {
            target_chars: 20,
            max_chars: 100,
            context_chars: 4,
        };
        let chunks = chunk_source(&doc, &config);
        assert!(chunks.len() >= 3);
        assert!(chunks[0].prev_context.is_empty());
        assert!(chunks.last().unwrap().next_context.is_empty());
        // 第二块的前文来自第一块末尾。
        assert_eq!(chunks[1].prev_context, "ghij");
        assert_eq!(chunks[0].next_context, "abcd");
        assert_eq!(chunks[2].prev_context, "ghij", "第三块前文来自第二块末尾");
    }

    #[test]
    fn output_is_stable_across_calls() {
        let doc = doc_with_many_lines(23);
        let config = ChunkConfig::default();
        let first = chunk_source(&doc, &config);
        let second = chunk_source(&doc, &config);
        assert_eq!(first, second);
    }

    #[test]
    fn oversize_segment_gets_own_chunk_and_warning() {
        let long = "字".repeat(7_000);
        let page = SourcePage {
            page: 1,
            text: long.clone(),
            lines: vec![SourceLine {
                line: 0,
                text: long.clone(),
                segments: vec![SourceSegment {
                    id: segment_id(1, 0, 0),
                    text: long.clone(),
                    kind: SegmentKind::Text,
                }],
            }],
        };
        let doc = SourceDoc {
            pages: vec![page],
            input_hash: "test".into(),
            target_language: "zh-CN".into(),
        };
        let config = ChunkConfig {
            target_chars: 100,
            max_chars: 200,
            context_chars: 10,
        };
        let (chunks, warnings) = chunk_source_with_warnings(&doc, &config);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].segments.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].chunk_id, "c0001");
        assert_eq!(warnings[0].segment_id, "p001-l000-s0");
        assert_eq!(warnings[0].chars, 7_000);
        assert_eq!(warnings[0].max_chars, 200);
    }

    #[test]
    fn empty_document_yields_no_chunks() {
        let doc = doc_from_pages(vec![Vec::new()]);
        let chunks = chunk_source(&doc, &ChunkConfig::default());
        assert!(chunks.is_empty());
    }

    #[test]
    fn chunk_ids_and_order_are_sequential() {
        let doc = doc_with_many_lines(20);
        let config = ChunkConfig {
            target_chars: 20,
            max_chars: 100,
            context_chars: 0,
        };
        let chunks = chunk_source(&doc, &config);
        for (index, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.order, (index + 1) as u32);
            assert_eq!(chunk.id, format!("c{:04}", index + 1));
        }
    }
}
