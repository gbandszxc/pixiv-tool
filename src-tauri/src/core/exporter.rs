//! 导出器：txt / markdown + 文件名净化。
//!
//! **桩（phase C 实现）**：本文件只定稿公共契约，函数体待填充。
#![allow(unused)]

use std::io;
use std::path::{Path, PathBuf};

/// 移除文件名非法字符：正则 `[<>:"/\\|?*\x00-\x1f]` 全部替换为 '_'，
/// 再 strip 两侧的 `'_. '`。
pub fn sanitize_filename(name: &str) -> String {
    todo!("phase C")
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
    todo!("phase C")
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
        todo!("phase C")
    }
}

/// Markdown 输出。行级转换规则（逐行处理，基于 strip 后的行首判断）：
/// - 行首 `[chapter:X]` → `"\n## X\n"`
/// - 整行 `[newpage]` → `"\n---\n"`
/// - `[jump:` 开头行 → 删除
/// - `[pixivimage:` / `[uploadedimage:` 开头行 → `"<!-- 图片占位 -->\n"`
/// - `[rb:A>B]` → `A(B)`（**修正**：Python 版此规则写了但作用在原 line 上、
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
        todo!("phase C")
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

/// 根据配置选择启用的导出器（"txt" / "markdown"；未知格式忽略，顺序跟随输入）。
pub fn create_exporters(formats: &[String]) -> Vec<Exporter> {
    todo!("phase C")
}
