//! 导出器：txt / markdown + 文件名净化。
//!
//! 语义对齐 Python `core/exporter.py`，其中 markdown 的 `[rb:A>B] → A(B)`
//! 规则在 Python 版写在了 stripped 变量上、追加的是原始 line（未生效），
//! Rust 版**修正**为对输出行实际应用替换。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

/// 文件名非法字符（对齐 Python `re.sub(r'[<>:"/\\|?*\x00-\x1f]', '_', name)`）。
fn is_illegal_filename_char(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (c as u32) <= 0x1f
}

/// 移除文件名非法字符：`[<>:"/\\|?*\x00-\x1f]` 全部替换为 '_'，
/// 再 strip 两侧的 `'_. '`。
pub fn sanitize_filename(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| if is_illegal_filename_char(c) { '_' } else { c })
        .collect();
    replaced
        .trim_matches(|c: char| c == '_' || c == '.' || c == ' ')
        .to_string()
}

/// `[rb:base>ruby]` → `base(ruby)`（对齐 Python 正则 `\[rb:([^>]+)>([^\]]+)\]`）。
fn apply_rb(line: &str) -> String {
    static RB: OnceLock<Regex> = OnceLock::new();
    let re = RB.get_or_init(|| Regex::new(r"\[rb:([^>]+)>([^\]]+)\]").expect("rb 正则合法"));
    re.replace_all(line, "${1}(${2})").to_string()
}

/// pixiv 小说标记 → Markdown（行级转换；strip 后判断、处理后追加）。
pub(crate) fn render_markdown(content: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in content.split('\n') {
        let stripped = line.trim();
        if stripped.starts_with("[chapter:") && stripped.ends_with(']') {
            let title = &stripped["[chapter:".len()..stripped.len() - 1];
            out.push(format!("\n## {title}\n"));
        } else if stripped == "[newpage]" {
            out.push("\n---\n".to_string());
        } else if stripped.starts_with("[jump:") {
            // 跳转标记：整行删除
            continue;
        } else if stripped.starts_with("[pixivimage:") || stripped.starts_with("[uploadedimage:") {
            out.push("<!-- 图片占位 -->\n".to_string());
        } else {
            // 其余原样（行内 rb 注音替换生效——修正 Python 未生效的 bug）
            out.push(apply_rb(line));
        }
    }
    out.join("\n")
}

/// 导出用元数据（文件命名需要）。
#[derive(Debug, Clone)]
pub struct ExportMeta {
    pub novel_id: i64,
    pub title: String,
    pub page_count: i64,
    pub series_order: Option<i64>,
}

/// 生成输出文件名（SPEC §4.4）：
/// - 有 series_order → `{zfill(order, page_count<=99?2:3)}_{safe_title}.{ext}`
/// - 无 → `{safe_title}_{novel_id}.{ext}`
pub fn make_filename(meta: &ExportMeta, ext: &str) -> String {
    let safe_title = sanitize_filename(&meta.title);
    match meta.series_order {
        Some(order) => {
            let width = if meta.page_count <= 99 { 2 } else { 3 };
            format!("{:0width$}_{safe_title}.{ext}", order, width = width)
        }
        None => format!("{safe_title}_{novel_id}.{ext}", novel_id = meta.novel_id),
    }
}

/// 写单文件：目标目录不存在则创建，UTF-8 输出，返回生成路径。
fn write_export(
    content: &str,
    meta: &ExportMeta,
    ext: &str,
    target_dir: &Path,
) -> io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(target_dir)?;
    let filepath = target_dir.join(make_filename(meta, ext));
    std::fs::write(&filepath, content)?;
    Ok(vec![filepath])
}

/// 纯文本输出（多页用空行分隔）。
pub struct TxtExporter;

impl TxtExporter {
    /// 导出并返回生成的文件路径列表（target_dir 不存在则创建）。
    pub fn export(
        &self,
        content: &str,
        meta: &ExportMeta,
        target_dir: &Path,
    ) -> io::Result<Vec<PathBuf>> {
        write_export(content, meta, "txt", target_dir)
    }
}

/// Markdown 输出。行级转换规则（逐行处理，基于 strip 后的行首判断）：
/// - 行首 `[chapter:X]` 且整行以 `]` 结尾 → `"\n## X\n"`
/// - 整行 `[newpage]` → `"\n---\n"`
/// - `[jump:` 开头行 → 删除
/// - `[pixivimage:` / `[uploadedimage:` 开头行 → `"<!-- 图片占位 -->\n"`
/// - 行内 `[rb:A>B]` → `A(B)`（**修正**：Python 版此规则写了但作用在原 line 上、
///   未生效；Rust 版必须实现该转换）
pub struct MarkdownExporter;

impl MarkdownExporter {
    /// 导出并返回生成的文件路径列表（target_dir 不存在则创建）。
    pub fn export(
        &self,
        content: &str,
        meta: &ExportMeta,
        target_dir: &Path,
    ) -> io::Result<Vec<PathBuf>> {
        write_export(&render_markdown(content), meta, "md", target_dir)
    }
}

/// 按配置选择的导出器。
pub enum Exporter {
    Txt(TxtExporter),
    Markdown(MarkdownExporter),
}

impl Exporter {
    pub fn export(
        &self,
        content: &str,
        meta: &ExportMeta,
        target_dir: &Path,
    ) -> io::Result<Vec<PathBuf>> {
        match self {
            Exporter::Txt(e) => e.export(content, meta, target_dir),
            Exporter::Markdown(e) => e.export(content, meta, target_dir),
        }
    }
}

/// 根据配置选择启用的导出器（"txt" / "markdown"；未知格式忽略，顺序跟随输入；
/// 空输入 → 空列表，由调用方决定是否回退默认格式）。
pub fn create_exporters(formats: &[String]) -> Vec<Exporter> {
    formats
        .iter()
        .filter_map(|f| match f.as_str() {
            "txt" => Some(Exporter::Txt(TxtExporter)),
            "markdown" => Some(Exporter::Markdown(MarkdownExporter)),
            _ => None,
        })
        .collect()
}

// ----------------------------------------------------------------------
// 单元测试
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn meta(title: &str, page_count: i64, series_order: Option<i64>) -> ExportMeta {
        ExportMeta {
            novel_id: 12345,
            title: title.into(),
            page_count,
            series_order,
        }
    }

    #[test]
    fn sanitize_replaces_every_illegal_char() {
        assert_eq!(
            sanitize_filename(r#"a<b>c:d"e/f\g|h?i*j"#),
            "a_b_c_d_e_f_g_h_i_j"
        );
        // 控制字符 \x00-\x1f
        assert_eq!(sanitize_filename("a\u{0}b\u{1f}c"), "a_b_c");
        assert_eq!(sanitize_filename("a\u{1}b"), "a_b");
    }

    #[test]
    fn sanitize_strips_edge_chars() {
        assert_eq!(sanitize_filename("_. 标题 ._"), "标题");
        assert_eq!(sanitize_filename("  名字  "), "名字");
        assert_eq!(sanitize_filename("___"), "", "全非法字符 → 空");
        // 中间的不动
        assert_eq!(sanitize_filename("a.b c_d"), "a.b c_d");
    }

    #[test]
    fn make_filename_single_uses_id_suffix() {
        assert_eq!(
            make_filename(&meta("小说", 1, None), "txt"),
            "小说_12345.txt"
        );
        // 标题净化后为空 → 前缀只剩 "_"
        assert_eq!(make_filename(&meta("___", 1, None), "md"), "_12345.md");
    }

    #[test]
    fn make_filename_series_padding_2_or_3() {
        // page_count <= 99 → 2 位 zfill
        assert_eq!(make_filename(&meta("话", 10, Some(5)), "txt"), "05_话.txt");
        assert_eq!(make_filename(&meta("话", 99, Some(12)), "txt"), "12_话.txt");
        // page_count >= 100 → 3 位 zfill
        assert_eq!(
            make_filename(&meta("话", 100, Some(5)), "txt"),
            "005_话.txt"
        );
        assert_eq!(
            make_filename(&meta("话", 250, Some(120)), "txt"),
            "120_话.txt"
        );
        // zfill 不截断：order 超过宽度时原样
        assert_eq!(
            make_filename(&meta("话", 5, Some(123)), "txt"),
            "123_话.txt"
        );
    }

    #[test]
    fn markdown_chapter_newpage_jump_placeholder() {
        let content =
            "[chapter:第一话]\n正文A\n[newpage]\n正文B\n[jump:123]\n[pixivimage:99]\n结尾";
        let md = render_markdown(content);
        assert_eq!(
            md,
            "\n## 第一话\n\n正文A\n\n---\n\n正文B\n<!-- 图片占位 -->\n\n结尾"
        );
    }

    #[test]
    fn markdown_strip_before_match() {
        // 行首空白仍按标记处理（strip 后判断）
        let md = render_markdown("   [chapter:X]\n  [newpage]  \n\t[jump:9]");
        assert_eq!(md, "\n## X\n\n\n---\n");
    }

    #[test]
    fn markdown_chapter_requires_closing_bracket() {
        // 不以 ] 结尾的 [chapter: 行按普通文本处理（Python 盲取 [9:-1]，Rust 更严格）
        let md = render_markdown("[chapter:abc");
        assert_eq!(md, "[chapter:abc");
    }

    #[test]
    fn markdown_rb_conversion_applies() {
        // 修正 Python 未生效的 bug：行内 [rb:base>ruby] 必须转换为 base(ruby)
        assert_eq!(
            render_markdown("漢字に[rb:漢字>かんじ]ルビ"),
            "漢字に漢字(かんじ)ルビ"
        );
        assert_eq!(render_markdown("[rb:戦>いく]もの"), "戦(いく)もの");
        // 一行多个 rb 标记全部转换
        assert_eq!(
            render_markdown("[rb:青>あお]と[rb:赤>あか]"),
            "青(あお)と赤(あか)"
        );
        // 不完整的 rb 标记保持原样
        assert_eq!(render_markdown("a[rb:x"), "a[rb:x");
        assert_eq!(render_markdown("a[rb:x>]b"), "a[rb:x>]b", "空 ruby 不匹配");
    }

    #[test]
    fn markdown_mixed_multiline() {
        let content = "[chapter:序]\n[rb:導入>どうにゅう]しました\n\n[newpage]\n[jump:1]\n[uploadedimage:7]\n尾巴";
        assert_eq!(
            render_markdown(content),
            "\n## 序\n\n導入(どうにゅう)しました\n\n\n---\n\n<!-- 图片占位 -->\n\n尾巴"
        );
    }

    #[test]
    fn txt_export_creates_dir_and_writes_utf8() {
        let dir =
            std::env::temp_dir().join(format!("pixiv-tool-export-test-{}", uuid::Uuid::new_v4()));
        let sub = dir.join("deep/nested"); // 不存在，应自动创建
        let paths = TxtExporter
            .export("中文内容", &meta("标题", 1, None), &sub)
            .unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], sub.join("标题_12345.txt"));
        assert_eq!(std::fs::read_to_string(&paths[0]).unwrap(), "中文内容");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn markdown_export_uses_rendered_content() {
        let dir =
            std::env::temp_dir().join(format!("pixiv-tool-export-test-{}", uuid::Uuid::new_v4()));
        let paths = MarkdownExporter
            .export("[chapter:T]\n[rb:a>b]", &meta("M", 2, Some(1)), &dir)
            .unwrap();
        assert_eq!(paths[0], dir.join("01_M.md"));
        assert_eq!(
            std::fs::read_to_string(&paths[0]).unwrap(),
            "\n## T\n\na(b)"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_exporters_filters_unknown_and_keeps_order() {
        let formats: Vec<String> =
            vec!["epub".into(), "markdown".into(), "txt".into(), "txt".into()];
        let exporters = create_exporters(&formats);
        assert_eq!(exporters.len(), 3, "未知格式跳过，重复保留");
        // 顺序跟随输入（markdown 在前）
        assert!(matches!(exporters[0], Exporter::Markdown(_)));
        assert!(matches!(exporters[1], Exporter::Txt(_)));
        assert!(matches!(exporters[2], Exporter::Txt(_)));
        // 空输入 → 空
        assert!(create_exporters(&[]).is_empty());
    }
}
