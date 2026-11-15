//! EPUB 导出引擎（纯库：无网络请求、无 IPC、无数据库访问）。
//!
//! 同一文件按三个模块区组织：
//!
//! 1. 【标记渲染】pixiv 小说标记 → XHTML body 片段（[`render_xhtml_body`]）。
//!    转换口径对齐前端 `NovelContent.vue` 的只读渲染：
//!    整行 `[chapter:X]` → `<h2>`；整行 `[newpage]` → 分页 div；
//!    `[rb:显示>注音]`（容忍 `/` 旧分隔）→ `<ruby>`；`[[jumpuri:文字 > URL]]` → `<a>`；
//!    `[jump:*]` / `[jumpurl:*]` → 删除；`[uploadedimage:id]` / `[pixivimage:id(-N)]` →
//!    图片（整行块级 / 行内混排内联）或占位块；其余逐行 `<p>`；
//!    空行折叠为段距（不输出空 `<p>`）；全部文本经 HTML 实体转义。
//! 2. 【EPUB 组装】EPUB 3.0 zip 容器（[`EpubBook::to_bytes`] / [`EpubBook::write_to`]）：
//!    `mimetype`（Stored 无压缩、首个条目）→ `META-INF/container.xml` →
//!    `OEBPS/content.opf` → `nav.xhtml`（EPUB3 目录）→ `toc.ncx`（EPUB2 兼容）→
//!    `style.css` → `cover.xhtml`（仅存在位图封面时）→ `images/*` → `chapter_{n:04}.xhtml`。
//! 3. 【占位封面】按 `seed_id` 取五色板绘制 1000×1414 PNG
//!    （[`render_placeholder_cover`]）：顶部「小说」印章式细线框、标题自动缩字号
//!    最多 3 行、白色半透明分割线、作者名；系统字体候选全部缺失 → 无文字版封面。
//!
//! 图片解析由调用方注入：[`ImageResolver`] 把标记内的 id（如 `12345`、`12345-2`）
//! 映射为 `OEBPS/images/` 下的条目文件名（如 `12345.png`）；返回 `None` 渲染占位。

use std::borrow::Cow;
use std::collections::HashSet;
use std::io::{Cursor, Write as _};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use thiserror::Error;

// Font 提供 as_scaled（Font → PxScaleFont 包装）；
// ScaleFont 提供缩放层方法（ascent / glyph_id / h_advance / kern / scaled_glyph / outline_glyph）。
use ab_glyph::{Font, ScaleFont};

// ======================================================================
// 模块区 0：公共数据结构与错误
// ======================================================================

/// EPUB 组装 / 写盘错误。
#[derive(Debug, Error)]
pub enum EpubError {
    #[error("EPUB zip 写入失败: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("EPUB 文件写入失败: {0}")]
    Io(#[from] std::io::Error),
    /// 清洗后图片 id 重复，或与封面保留名 `cover` 冲突（调用方 bug，尽早失败）。
    #[error("内嵌图片 id 冲突: {0}")]
    DuplicateImage(String),
}

/// 一本待导出的 EPUB 书稿（章节 body 需已经 [`render_xhtml_body`] 渲染）。
#[derive(Debug, Clone)]
pub struct EpubBook {
    pub title: String,
    pub author: String,
    /// BCP-47 语言标签（写入 `dc:language` 与各 XHTML `xml:lang`）。
    pub language: String,
    /// 书籍 UUID（`dc:identifier` / `dtb:uid`，不含 `urn:uuid:` 前缀）。
    pub book_uuid: String,
    /// 章节有序列表（每章一个 XHTML 文件；单篇 = 1 章，系列 = 每话 1 章）。
    pub chapters: Vec<EpubChapter>,
    /// 显式封面（pixiv 封面下载成功时提供；优先于占位封面）。
    pub cover: Option<CoverImage>,
    /// 正文内嵌图片清单（渲染时经 [`ImageResolver`] 按 id 引用）。
    pub images: Vec<EmbeddedImage>,
    /// 占位封面参数（无显式封面时由本库绘制 PNG）。
    pub placeholder_cover: Option<PlaceholderCoverSpec>,
}

/// 单个章节。
#[derive(Debug, Clone)]
pub struct EpubChapter {
    pub title: String,
    /// XHTML body 片段（不含 `<html>` 外壳，组装时包壳）。
    pub xhtml_body: String,
}

/// 封面位图（pixiv 封面原字节或占位封面 PNG）。
#[derive(Debug, Clone)]
pub struct CoverImage {
    pub bytes: Vec<u8>,
    /// 不带点的扩展名（`jpg` / `png` / ...），决定条目文件名与 media-type。
    pub ext: String,
}

/// 正文内嵌图片资源。
#[derive(Debug, Clone)]
pub struct EmbeddedImage {
    /// 标记中的原始 id（调用方保证可区分；写入 zip 前会清洗为安全文件名字符）。
    pub id: String,
    pub bytes: Vec<u8>,
    pub ext: String,
}

/// 占位封面绘制参数。
#[derive(Debug, Clone)]
pub struct PlaceholderCoverSpec {
    pub title: String,
    pub author: String,
    /// 主题取模种子（novel_id 或 series_id，与前端 HistoryView `BOOK_THEMES` 同口径）。
    pub seed_id: i64,
}

/// 图片解析回调：标记 id → `OEBPS/images/` 下的条目文件名（含扩展名）。
/// 返回 `None` 表示该图不可解析，渲染为占位。
pub type ImageResolver<'a> = dyn Fn(&str) -> Option<String> + 'a;

impl EpubBook {
    /// 组装为 EPUB 字节缓冲（zip 容器）。
    pub fn to_bytes(&self) -> Result<Vec<u8>, EpubError> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        // 小闭包统一「开条目 + 写内容」，减少重复样板。
        let write_entry = |w: &mut zip::ZipWriter<Cursor<Vec<u8>>>,
                           name: &str,
                           opts: zip::write::SimpleFileOptions,
                           data: &[u8]|
         -> Result<(), EpubError> {
            w.start_file(name, opts)?;
            w.write_all(data)?;
            Ok(())
        };

        // (1) mimetype：EPUB 规范要求其为第一个 zip 条目且不压缩。
        write_entry(&mut writer, "mimetype", stored, b"application/epub+zip")?;

        // 封面解析：显式封面优先，缺失回落占位封面（本库绘制 PNG）。
        let cover = self.resolved_cover()?;

        // (2) 容器声明 → (3)~(6) OPF / nav / ncx / css。
        write_entry(
            &mut writer,
            "META-INF/container.xml",
            deflated,
            container_xml().as_bytes(),
        )?;
        write_entry(
            &mut writer,
            "OEBPS/content.opf",
            deflated,
            self.content_opf(cover.as_ref().map(|(_, ext)| ext.as_str())).as_bytes(),
        )?;
        write_entry(&mut writer, "OEBPS/nav.xhtml", deflated, self.nav_xhtml().as_bytes())?;
        write_entry(&mut writer, "OEBPS/toc.ncx", deflated, self.toc_ncx().as_bytes())?;
        write_entry(&mut writer, "OEBPS/style.css", deflated, STYLE_CSS.as_bytes())?;

        // (7) 封面：cover.xhtml + 封面图（无任何封面时不生成，spine 亦不含）。
        if let Some((bytes, ext)) = &cover {
            let cover_href = format!("images/cover.{ext}");
            write_entry(
                &mut writer,
                "OEBPS/cover.xhtml",
                deflated,
                self.cover_xhtml(&cover_href).as_bytes(),
            )?;
            write_entry(&mut writer, &format!("OEBPS/{cover_href}"), deflated, bytes)?;
        }

        // (8) 内嵌图片（清洗 id + 冲突检查；条目名与渲染器 `images/...` 引用一致）。
        let stems = self.image_entry_stems()?;
        for (img, stem) in self.images.iter().zip(&stems) {
            let name = format!("OEBPS/images/{stem}.{}", img.ext);
            write_entry(&mut writer, &name, deflated, &img.bytes)?;
        }

        // (9) 章节正文。
        for (i, chapter) in self.chapters.iter().enumerate() {
            let name = format!("OEBPS/chapter_{:04}.xhtml", i + 1);
            write_entry(&mut writer, &name, deflated, self.chapter_xhtml(chapter).as_bytes())?;
        }

        Ok(writer.finish()?.into_inner())
    }

    /// 组装并写入指定路径。
    pub fn write_to(&self, path: impl AsRef<Path>) -> Result<(), EpubError> {
        let bytes = self.to_bytes()?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// 封面位图解析：显式封面借用；否则按占位参数绘制（生成 PNG，无错误路径）。
    fn resolved_cover(&self) -> Result<Option<(Cow<'_, [u8]>, String)>, EpubError> {
        if let Some(c) = &self.cover {
            return Ok(Some((Cow::Borrowed(&c.bytes), c.ext.clone())));
        }
        if let Some(spec) = &self.placeholder_cover {
            let png = render_placeholder_cover(spec);
            return Ok(Some((Cow::Owned(png), "png".to_string())));
        }
        Ok(None)
    }

    /// 内嵌图片条目名（不含扩展名）：清洗非法字符 + 查重 + 封面保留名检查。
    fn image_entry_stems(&self) -> Result<Vec<String>, EpubError> {
        let mut seen = HashSet::new();
        self.images
            .iter()
            .map(|img| {
                let stem = sanitize_image_id(&img.id);
                if stem == "cover" || !seen.insert(stem.clone()) {
                    return Err(EpubError::DuplicateImage(stem));
                }
                Ok(stem)
            })
            .collect()
    }

    // ---- OPF / nav / ncx / 单页 XHTML 模板 ----

    /// `OEBPS/content.opf`（EPUB 3.0 包描述）。
    fn content_opf(&self, cover_ext: Option<&str>) -> String {
        let title = escape_xml(&self.title);
        let mut manifest = String::new();
        if let Some(ext) = cover_ext {
            let mt = image_media_type(ext);
            manifest.push_str(&format!(
                "    <item id=\"cover-image\" href=\"images/cover.{ext}\" media-type=\"{mt}\" properties=\"cover-image\"/>\n"
            ));
            manifest.push_str(
                "    <item id=\"cover-page\" href=\"cover.xhtml\" media-type=\"application/xhtml+xml\"/>\n",
            );
        }
        manifest.push_str(
            "    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n",
        );
        manifest.push_str(
            "    <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n",
        );
        manifest.push_str("    <item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>\n");

        // 内嵌图片条目（与 image_entry_stems 同序，此处清洗结果已通过冲突检查）。
        for img in &self.images {
            let stem = sanitize_image_id(&img.id);
            manifest.push_str(&format!(
                "    <item id=\"img-{stem}\" href=\"images/{stem}.{}\" media-type=\"{}\"/>\n",
                img.ext,
                image_media_type(&img.ext)
            ));
        }
        for i in 1..=self.chapters.len() {
            manifest.push_str(&format!(
                "    <item id=\"chapter-{i}\" href=\"chapter_{i:04}.xhtml\" media-type=\"application/xhtml+xml\"/>\n"
            ));
        }

        let mut spine = String::new();
        if cover_ext.is_some() {
            spine.push_str("    <itemref idref=\"cover-page\"/>\n");
        }
        for i in 1..=self.chapters.len() {
            spine.push_str(&format!("    <itemref idref=\"chapter-{i}\"/>\n"));
        }

        let creator = (!self.author.trim().is_empty())
            .then(|| format!("    <dc:creator>{}</dc:creator>\n", escape_xml(&self.author)))
            .unwrap_or_default();
        let modified = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");

        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="book-id">urn:uuid:{uuid}</dc:identifier>
    <dc:title>{title}</dc:title>
{creator}    <dc:language>{lang}</dc:language>
    <meta property="dcterms:modified">{modified}</meta>
  </metadata>
  <manifest>
{manifest}  </manifest>
  <spine toc="ncx">
{spine}  </spine>
</package>
"#,
            uuid = escape_xml(&self.book_uuid),
            lang = escape_xml(&self.language),
        )
    }

    /// `OEBPS/nav.xhtml`（EPUB 3 目录）。
    fn nav_xhtml(&self) -> String {
        let title = escape_xml(&self.title);
        let lang = escape_xml(&self.language);
        let mut items = String::new();
        for (i, chapter) in self.chapters.iter().enumerate() {
            let name = format!("chapter_{:04}.xhtml", i + 1);
            items.push_str(&format!(
                "      <li><a href=\"{name}\">{}</a></li>\n",
                escape_xml(chapter.title.trim())
            ));
        }
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="{lang}" lang="{lang}">
  <head>
    <title>{title}</title>
    <meta charset="utf-8"/>
  </head>
  <body>
    <nav epub:type="toc" id="toc">
      <h1>{title}</h1>
      <ol>
{items}      </ol>
    </nav>
  </body>
</html>
"#
        )
    }

    /// `OEBPS/toc.ncx`（EPUB 2 兼容目录，旧阅读器用）。
    fn toc_ncx(&self) -> String {
        let title = escape_xml(&self.title);
        let mut points = String::new();
        for (i, chapter) in self.chapters.iter().enumerate() {
            let n = i + 1;
            points.push_str(&format!(
                r#"    <navPoint id="np-{n}" playOrder="{n}">
      <navLabel><text>{}</text></navLabel>
      <content src="chapter_{n:04}.xhtml"/>
    </navPoint>
"#,
                escape_xml(chapter.title.trim())
            ));
        }
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head>
    <meta name="dtb:uid" content="urn:uuid:{uuid}"/>
    <meta name="dtb:depth" content="1"/>
    <meta name="dtb:totalPageCount" content="0"/>
    <meta name="dtb:maxPageNumber" content="0"/>
  </head>
  <docTitle><text>{title}</text></docTitle>
  <navMap>
{points}  </navMap>
</ncx>
"#,
            uuid = escape_xml(&self.book_uuid),
        )
    }

    /// `OEBPS/cover.xhtml`（全屏封面页；仅存在位图封面时生成）。
    fn cover_xhtml(&self, cover_href: &str) -> String {
        let title = escape_xml(&self.title);
        let lang = escape_xml(&self.language);
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="{lang}" lang="{lang}">
  <head>
    <title>{title}</title>
    <meta charset="utf-8"/>
    <link rel="stylesheet" type="text/css" href="style.css"/>
  </head>
  <body class="cover-page">
    <div class="cover-wrap"><img src="{cover_href}" alt="{title}"/></div>
  </body>
</html>
"#
        )
    }

    /// 章节 XHTML（包壳：head 引 style.css，body 嵌渲染好的片段）。
    fn chapter_xhtml(&self, chapter: &EpubChapter) -> String {
        let title = escape_xml(chapter.title.trim());
        let lang = escape_xml(&self.language);
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="{lang}" lang="{lang}">
  <head>
    <title>{title}</title>
    <meta charset="utf-8"/>
    <link rel="stylesheet" type="text/css" href="style.css"/>
  </head>
  <body>
{}
  </body>
</html>
"#,
            chapter.xhtml_body
        )
    }
}

/// `META-INF/container.xml`（指向 OPF，固定模板）。
fn container_xml() -> &'static str {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#
}

/// `OEBPS/style.css`：正文排版（导出产物内嵌样式，独立于前端 token 体系）。
const STYLE_CSS: &str = r#"body { margin: 0 6%; line-height: 1.8; }
p { margin: 0 0 1em; }
h2 { font-size: 1.15em; font-weight: 700; line-height: 1.5; margin: 1.6em 0 0.8em; }
ruby rt { font-size: 0.55em; }
.page-break { page-break-before: always; break-before: page; }
.epub-image { margin: 1.5em 0; text-align: center; }
.epub-image img { max-width: 100%; height: auto; }
.epub-image-inline { max-height: 1.6em; vertical-align: middle; margin: 0 0.25em; }
.epub-placeholder { margin: 1.5em auto; padding: 1em 1.5em; border: 1px dashed #999; color: #666; font-size: 0.9em; text-align: center; }
.cover-page { margin: 0; text-align: center; }
.cover-page img { max-width: 100%; height: auto; }
"#;

/// 扩展名 → 图片 media-type。
fn image_media_type(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// 图片 id 清洗为 zip 条目安全字符：`[A-Za-z0-9_-]` 之外全部替换 `_`。
fn sanitize_image_id(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if cleaned.is_empty() { "_".to_string() } else { cleaned }
}

/// XML / HTML 实体转义（文本与属性通用；先转 `&` 防二次转义）。
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

// ======================================================================
// 模块区 1：pixiv 标记 → XHTML 渲染
// ======================================================================

/// 图片不可解析时的占位文案（与契约「〔图片占位〕」一致）。
pub const IMAGE_PLACEHOLDER_TEXT: &str = "〔图片占位〕";

/// 行内标记合并正则，一次扫描，顺序即优先级（对齐 `NovelContent.vue` 的 INLINE_RE）：
/// 1. 注音 `[rb:显示>注音]`（容忍旧格式 `/` 分隔）；
/// 2. 图片 `[uploadedimage:id]` / `[pixivimage:id]` / `[pixivimage:id-N]`；
/// 3. 链接 `[[jumpuri:文字 > URL]]`（双层方括号，容忍单层旧式）；
/// 4. 跳转 `[jump:N]` / `[jumpurl:...]`（匹配后直接丢弃）。
const INLINE_RE_SRC: &str = concat!(
    r"\[rb:(?P<rb_inner>[^>\]]+>[^\]]+|[^/\]]+/[^\]]+)\]",
    r"|\[(?:pixivimage|uploadedimage):(?P<img_id>[^\]]*)\]",
    r"|\[\[?jumpuri:(?P<j_label>[^>\]]*?)\s*>\s*(?P<j_url>[^\]]+?)\s*\]\]?",
    r"|\[(?:jump|jumpurl):[^\]]*\]",
);

fn inline_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(INLINE_RE_SRC).expect("行内标记正则合法"))
}

/// 整行章节标题：`[chapter:X]`。
fn chapter_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\[chapter:(.*)\]$").expect("章节正则合法"))
}

/// 整行跳转标记：`[jump:N]` / `[jumpurl:...]` → 删行。
fn jump_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\[(?:jump|jumpurl):[^\]]*\]$").expect("跳转正则合法"))
}

/// 行内片段（`parse_inline` 输出）。
#[derive(Debug, PartialEq)]
enum Seg {
    /// 普通文本（存原文，渲染时统一转义）。
    Text(String),
    /// 注音 `<ruby>`。
    Ruby { base: String, ruby: String },
    /// 图片；`None` = 解析失败（调用方映射为 `None`）→ 占位。
    Image { file: Option<String> },
    /// 外链 `<a>`。
    Link { text: String, url: String },
}

/// pixiv 小说原文 → XHTML body 片段（不含 `<html>` 外壳，组装器包壳）。
///
/// `resolve_image`：标记 id → `OEBPS/images/` 条目文件名（如 `12345.png`）。
pub fn render_xhtml_body(content: &str, resolve_image: &ImageResolver<'_>) -> String {
    // 行结尾归一（pixiv 原文可能混用 \r\n / \r）。
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut out: Vec<String> = Vec::new();

    for raw_line in normalized.split('\n') {
        let line = raw_line.trim();
        if line.is_empty() {
            // 连续空行折叠为段距：不输出任何空块。
            continue;
        }

        // 整行 [chapter:X] → <h2>（标题 trim 后为空则整行丢弃，对齐前端）。
        if let Some(caps) = chapter_line_re().captures(line) {
            let title = caps[1].trim();
            if !title.is_empty() {
                out.push(format!("<h2>{}</h2>", escape_xml(title)));
            }
            continue;
        }
        // 整行 [newpage] → 分页块；行内出现的 [newpage] 不匹配任何行内正则，
        // 按普通文本转义输出（与契约一致）。
        if line == "[newpage]" {
            out.push(r#"<div class="page-break"></div>"#.to_string());
            continue;
        }
        // 整行跳转标记 → 删行。
        if jump_line_re().is_match(line) {
            continue;
        }

        let segs = parse_inline(line, resolve_image);
        if segs.is_empty() {
            // 纯标记行（如只含 [jump:N]）→ 无可见内容，整行丢弃。
            continue;
        }
        if segs.iter().all(|s| matches!(s, Seg::Image { .. })) {
            // 整行只有图片标记 → 每图一个块级 div（对齐前端逐图成块）。
            for seg in segs {
                out.push(render_image_block(&seg));
            }
        } else {
            let inner = render_segments(&segs);
            // 纯标记行剔除标记后可能只剩空白 → 不输出空段。
            if !inner.trim().is_empty() {
                out.push(format!("<p>{inner}</p>"));
            }
        }
    }

    out.join("\n")
}

/// 行内一次扫描：rb → image → jumpuri → jump（丢弃），其余文本保持原样。
fn parse_inline(line: &str, resolve_image: &ImageResolver<'_>) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut plain = String::new();
    let mut last = 0usize;

    for caps in inline_re().captures_iter(line) {
        let m = caps.get(0).expect("整体匹配存在");
        if m.start() > last {
            plain.push_str(&line[last..m.start()]);
        }
        last = m.end();
        // 标记前的累积文本先落为一个 Text 段，保持「文本 / 标记」交替的输出顺序。
        if !plain.is_empty() {
            segs.push(Seg::Text(std::mem::take(&mut plain)));
        }

        if let Some(inner) = caps.name("rb_inner") {
            // 注音：优先 `>` 分隔（pixiv 现行），否则 `/` 旧分隔。
            let (base, ruby) = if let Some((b, r)) = inner.as_str().split_once('>') {
                (b, r)
            } else if let Some((b, r)) = inner.as_str().split_once('/') {
                (b, r)
            } else {
                unreachable!("正则保证内文必含 > 或 /");
            };
            segs.push(Seg::Ruby {
                base: base.to_string(),
                ruby: ruby.to_string(),
            });
        } else if let Some(id) = caps.name("img_id") {
            // 图片：id 原文（含 pixivimage 的 `-N` 页码）交给调用方映射。
            let mark_id = id.as_str().trim();
            segs.push(Seg::Image { file: resolve_image(mark_id) });
        } else if caps.name("j_url").is_some() {
            // 链接：锚文本为空时回落 URL 本身（对齐前端 `label || url`）。
            let label = caps["j_label"].trim();
            let url = caps["j_url"].trim();
            let text = if label.is_empty() { url } else { label };
            segs.push(Seg::Link {
                text: text.to_string(),
                url: url.to_string(),
            });
        }
        // jump 分支：无命名捕获，直接丢弃（不产生任何段）。
    }
    if last < line.len() {
        plain.push_str(&line[last..]);
    }
    if !plain.is_empty() {
        segs.push(Seg::Text(plain));
    }
    segs
}

/// 块级图片：解析成功 → 居中图；失败 → 占位块。
fn render_image_block(seg: &Seg) -> String {
    match seg {
        Seg::Image { file: Some(file) } => format!(
            r#"<div class="epub-image"><img src="images/{}" alt=""/></div>"#,
            escape_xml(file)
        ),
        _ => format!(r#"<div class="epub-placeholder">{IMAGE_PLACEHOLDER_TEXT}</div>"#),
    }
}

/// 段落级片段渲染（全部经实体转义）。
fn render_segments(segs: &[Seg]) -> String {
    let mut out = String::new();
    for seg in segs {
        match seg {
            Seg::Text(text) => out.push_str(&escape_xml(text)),
            Seg::Ruby { base, ruby } => {
                out.push_str("<ruby>");
                out.push_str(&escape_xml(base));
                out.push_str("<rt>");
                out.push_str(&escape_xml(ruby));
                out.push_str("</rt></ruby>");
            }
            Seg::Image { file: Some(file) } => {
                out.push_str(&format!(
                    r#"<img class="epub-image-inline" src="images/{}" alt=""/>"#,
                    escape_xml(file)
                ));
            }
            // 混排段落里解析失败的图：不占块，退化为占位文本（不打断行文）。
            Seg::Image { file: None } => out.push_str(IMAGE_PLACEHOLDER_TEXT),
            Seg::Link { text, url } => {
                out.push_str(&format!(
                    r#"<a href="{}">{}</a>"#,
                    escape_xml(url),
                    escape_xml(text)
                ));
            }
        }
    }
    out
}

// ======================================================================
// 模块区 2：占位封面 PNG 绘制
// ======================================================================

/// 单个像素颜色（RGBA）。
#[derive(Debug, Clone, Copy)]
struct Rgba {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

/// 封面画布：RGBA 线性缓冲（自绘矩形 / 线条 / 文字光栅化混合）。
struct Canvas {
    w: u32,
    h: u32,
    buf: Vec<u8>,
}

impl Canvas {
    /// 以纯色底初始化画布。
    fn new_bg(w: u32, h: u32, bg: Rgba) -> Self {
        let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
        for px in buf.chunks_exact_mut(4) {
            px[0] = bg.r;
            px[1] = bg.g;
            px[2] = bg.b;
            px[3] = 255;
        }
        Self { w, h, buf }
    }

    /// 按 coverage 混合单个像素（越界忽略；coverage 0.0~1.0）。
    fn blend_pixel(&mut self, x: i32, y: i32, c: Rgba, coverage: f32) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 || coverage <= 0.0 {
            return;
        }
        let idx = ((y as u32 * self.w + x as u32) * 4) as usize;
        let alpha = (c.a as f32 / 255.0) * coverage.clamp(0.0, 1.0);
        let inv = 1.0 - alpha;
        self.buf[idx] = (c.r as f32 * alpha + self.buf[idx] as f32 * inv).round() as u8;
        self.buf[idx + 1] = (c.g as f32 * alpha + self.buf[idx + 1] as f32 * inv).round() as u8;
        self.buf[idx + 2] = (c.b as f32 * alpha + self.buf[idx + 2] as f32 * inv).round() as u8;
        self.buf[idx + 3] = 255;
    }

    /// 水平线（含两端）。
    fn hline(&mut self, x0: i32, x1: i32, y: i32, c: Rgba) {
        for x in x0.min(x1)..=x0.max(x1) {
            self.blend_pixel(x, y, c, 1.0);
        }
    }

    /// 竖直线（含两端）。
    fn vline(&mut self, x: i32, y0: i32, y1: i32, c: Rgba) {
        for y in y0.min(y1)..=y0.max(y1) {
            self.blend_pixel(x, y, c, 1.0);
        }
    }

    /// 1px 矩形描边。
    fn stroke_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        self.hline(x0, x1, y0, c);
        self.hline(x0, x1, y1, c);
        self.vline(x0, y0, y1, c);
        self.vline(x1, y0, y1, c);
    }
}

/// 前端 `HistoryView.vue` BOOK_THEMES 五色板（bg, text），移植后端供占位封面取色。
const BOOK_THEMES: [(Rgba, Rgba); 5] = [
    // 藏青
    (Rgba { r: 0x1e, g: 0x3a, b: 0x5f, a: 255 }, Rgba { r: 0xf0, g: 0xf4, b: 0xf8, a: 255 }),
    // 黛绿
    (Rgba { r: 0x2d, g: 0x4a, b: 0x3e, a: 255 }, Rgba { r: 0xf2, g: 0xf7, b: 0xf4, a: 255 }),
    // 绯木
    (Rgba { r: 0x52, g: 0x32, b: 0x32, a: 255 }, Rgba { r: 0xfd, g: 0xf5, b: 0xf5, a: 255 }),
    // 玄黑
    (Rgba { r: 0x36, g: 0x36, b: 0x36, a: 255 }, Rgba { r: 0xf5, g: 0xf5, b: 0xf5, a: 255 }),
    // 琥珀
    (Rgba { r: 0x4a, g: 0x3c, b: 0x28, a: 255 }, Rgba { r: 0xfa, g: 0xf6, b: 0xed, a: 255 }),
];

/// 封面尺寸 1000×1414（1:1.414 书籍比例）。
const COVER_W: u32 = 1000;
const COVER_H: u32 = 1414;

// 印章（顶部「小说」细线框）。
const SEAL_SIZE: f32 = 36.0;
const SEAL_BOX_W: i32 = 124;
const SEAL_BOX_H: i32 = 64;
/// 白 50%（印章框与印章文字）。
const SEAL_COLOR: Rgba = Rgba { r: 255, g: 255, b: 255, a: 128 };

// 标题：自动缩字号、最多 3 行、行高 1.4。
const TITLE_MAX_SIZE: f32 = 72.0;
const TITLE_MIN_SIZE: f32 = 36.0;
const TITLE_SIZE_STEP: f32 = 8.0;
const TITLE_MAX_LINES: usize = 3;
const TITLE_LINE_HEIGHT: f32 = 1.4;

/// 作者名基础字号；超宽时缩到的下限。
const AUTHOR_SIZE: f32 = 28.0;
const AUTHOR_MIN_SIZE: f32 = 20.0;

/// 内容区左右边距。
const CONTENT_MARGIN: f32 = 90.0;

// 布局纵向锚点。
const SEAL_CENTER_Y_RATIO: f32 = 0.12;
const TITLE_TOP_RATIO: f32 = 0.35;
const DIVIDER_GAP: f32 = 56.0;
const AUTHOR_GAP: f32 = 40.0;
/// 白 20% 分割线。
const DIVIDER_COLOR: Rgba = Rgba { r: 255, g: 255, b: 255, a: 51 };
/// 分割线半宽（居中）。
const DIVIDER_HALF_W: f32 = 200.0;

/// 封面配色（底色 + 文字色）。
#[derive(Debug, Clone, Copy)]
struct CoverTheme {
    bg: Rgba,
    text: Rgba,
}

/// 按 seed_id 绝对值取模取主题（对齐前端 `Math.abs(id) % 5`）。
fn cover_theme(seed_id: i64) -> CoverTheme {
    let (bg, text) = BOOK_THEMES[(seed_id.unsigned_abs() % BOOK_THEMES.len() as u64) as usize];
    CoverTheme { bg, text }
}

/// 系统字体候选路径（按平台按序取第一个存在的；ttc 取 index 0）。
pub fn system_font_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if cfg!(windows) {
        for name in ["msyh.ttc", "simhei.ttf", "simsun.ttc"] {
            paths.push(PathBuf::from(r"C:\Windows\Fonts").join(name));
        }
    } else if cfg!(target_os = "macos") {
        paths.push(PathBuf::from("/System/Library/Fonts/PingFang.ttc"));
        paths.push(PathBuf::from("/Library/Fonts/Arial Unicode.ttf"));
    } else {
        for path in [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        ] {
            paths.push(PathBuf::from(path));
        }
    }
    paths
}

/// 占位封面默认入口：使用系统字体候选列表。
pub fn render_placeholder_cover(spec: &PlaceholderCoverSpec) -> Vec<u8> {
    render_placeholder_cover_with_fonts(spec, &system_font_candidates())
}

/// 绘制占位封面 PNG（1000×1414 RGBA）。
///
/// 布局：顶部 12% 处「小说」印章式细线框 → 中部标题（自动缩字号最多 3 行）→
/// 白色半透明分割线 → 作者名。字体候选全部不可用 → 退化为底色 + 印章框
///（无文字、无分割线），不报错。
pub fn render_placeholder_cover_with_fonts(spec: &PlaceholderCoverSpec, font_candidates: &[PathBuf]) -> Vec<u8> {
    let theme = cover_theme(spec.seed_id);
    let mut canvas = Canvas::new_bg(COVER_W, COVER_H, theme.bg);
    // 有字体才走完整排版链路；无字体只画底色与印章框。
    let font = load_font_from_candidates(font_candidates);

    // --- 印章框（恒画，居中于顶部 12% 高度处）---
    let seal_cx = (COVER_W as f32 / 2.0).round() as i32;
    let seal_cy = (COVER_H as f32 * SEAL_CENTER_Y_RATIO).round() as i32;
    let (bx0, by0) = (seal_cx - SEAL_BOX_W / 2, seal_cy - SEAL_BOX_H / 2);
    let (bx1, by1) = (seal_cx + SEAL_BOX_W / 2, seal_cy + SEAL_BOX_H / 2);
    canvas.stroke_rect(bx0, by0, bx1, by1, SEAL_COLOR);

    if let Some(font) = &font {
        // --- 印章文字「小说」（垂直居中于框内）---
        draw_centered_text(&mut canvas, font, SEAL_SIZE, "小说", seal_cx as f32, seal_cy as f32, SEAL_COLOR);

        let avail_width = COVER_W as f32 - CONTENT_MARGIN * 2.0;

        // --- 标题：从 72px 逐档缩到 36px，取第一个 ≤3 行的字号；仍超则按 36px 硬截 3 行 ---
        let mut title_size = TITLE_MAX_SIZE;
        let mut lines = wrap_text_by_width(font, title_size, &spec.title, avail_width);
        while lines.len() > TITLE_MAX_LINES && title_size > TITLE_MIN_SIZE {
            title_size = (title_size - TITLE_SIZE_STEP).max(TITLE_MIN_SIZE);
            lines = wrap_text_by_width(font, title_size, &spec.title, avail_width);
        }
        if lines.len() > TITLE_MAX_LINES {
            lines.truncate(TITLE_MAX_LINES);
        }

        let title_top = COVER_H as f32 * TITLE_TOP_RATIO;
        let line_h = title_size * TITLE_LINE_HEIGHT;
        for (i, line) in lines.iter().enumerate() {
            let line_top = title_top + i as f32 * line_h;
            draw_text_line(
                &mut canvas,
                font,
                title_size,
                line,
                (COVER_W as f32 - text_width(font, title_size, line)) / 2.0,
                line_top + baseline_offset(font, title_size, line_h),
                theme.text,
            );
        }

        // --- 分割线（白色 20%，1px，居中，位于标题块下方）---
        let divider_y = (title_top + lines.len() as f32 * line_h + DIVIDER_GAP).round() as i32;
        let cx0 = ((COVER_W as f32 - DIVIDER_HALF_W * 2.0) / 2.0).round() as i32;
        let cx1 = cx0 + (DIVIDER_HALF_W * 2.0).round() as i32;
        canvas.hline(cx0, cx1, divider_y, DIVIDER_COLOR);

        // --- 作者名（分割线下方；超宽自动缩字号到下限，仍超则画布裁切）---
        let mut author_size = AUTHOR_SIZE;
        while author_size > AUTHOR_MIN_SIZE && text_width(font, author_size, &spec.author) > avail_width {
            author_size -= 4.0;
        }
        let author_top = divider_y as f32 + AUTHOR_GAP;
        draw_centered_text(
            &mut canvas,
            font,
            author_size,
            &spec.author,
            COVER_W as f32 / 2.0,
            author_top,
            theme.text,
        );
    }

    encode_png(&canvas.buf, canvas.w, canvas.h)
}

/// 按候选路径顺序加载第一个可用字体（ttc 取 index 0；失败继续下一个）。
fn load_font_from_candidates(paths: &[PathBuf]) -> Option<ab_glyph::FontVec> {
    for path in paths {
        if let Ok(data) = std::fs::read(path) {
            if let Ok(font) = ab_glyph::FontVec::try_from_vec_and_index(data, 0) {
                return Some(font);
            }
        }
    }
    None
}

/// 文本像素宽（含 kerning）。
fn text_width(font: &ab_glyph::FontVec, size: f32, text: &str) -> f32 {
    let scaled = font.as_scaled(ab_glyph::PxScale::from(size));
    let mut width = 0.0;
    let mut prev: Option<ab_glyph::GlyphId> = None;
    for ch in text.chars() {
        let gid = scaled.glyph_id(ch);
        if let Some(p) = prev {
            width += scaled.kern(p, gid);
        }
        width += scaled.h_advance(gid);
        prev = Some(gid);
    }
    width
}

/// 贪心逐字符折行（CJK 场景按字符断行即可；返回行数可能超过 `max_lines`，截断由调用方决定）。
fn wrap_text_by_width(font: &ab_glyph::FontVec, size: f32, text: &str, max_width: f32) -> Vec<String> {
    let scaled = font.as_scaled(ab_glyph::PxScale::from(size));
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0.0;
    let mut prev: Option<ab_glyph::GlyphId> = None;
    for ch in text.chars() {
        let gid = scaled.glyph_id(ch);
        // 换行判断先于 kern 累计：新行首字不携带上一行末字的 kern。
        if !cur.is_empty() && cur_w + scaled.h_advance(gid) > max_width {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
            prev = None;
        }
        let adv = scaled.h_advance(gid) + prev.map(|p| scaled.kern(p, gid)).unwrap_or(0.0);
        cur.push(ch);
        cur_w += adv;
        prev = Some(gid);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// 指定行框高度内基线距行顶的偏移（按字体 ascent/descent 垂直居中）。
fn baseline_offset(font: &ab_glyph::FontVec, size: f32, line_height: f32) -> f32 {
    let scaled = font.as_scaled(ab_glyph::PxScale::from(size));
    (line_height - (scaled.ascent() - scaled.descent())) / 2.0 + scaled.ascent()
}

/// 以 left 为起点画一行文字（字形轮廓经内置光栅化逐像素混合）。
fn draw_text_line(
    canvas: &mut Canvas,
    font: &ab_glyph::FontVec,
    size: f32,
    text: &str,
    left_x: f32,
    baseline_y: f32,
    color: Rgba,
) {
    let scaled = font.as_scaled(ab_glyph::PxScale::from(size));
    let mut pen_x = left_x;
    let mut prev: Option<ab_glyph::GlyphId> = None;
    for ch in text.chars() {
        let gid = scaled.glyph_id(ch);
        if let Some(p) = prev {
            pen_x += scaled.kern(p, gid);
        }
        let mut glyph = scaled.scaled_glyph(ch);
        glyph.position = ab_glyph::point(pen_x, baseline_y);
        if let Some(outlined) = scaled.outline_glyph(glyph) {
            // draw 回调的 (x, y) 相对 px_bounds.min，换算为画布坐标后逐像素混合。
            let bounds_min = outlined.px_bounds().min;
            let (bx, by) = (bounds_min.x.floor() as i32, bounds_min.y.floor() as i32);
            outlined.draw(|px, py, coverage| {
                canvas.blend_pixel(bx + px as i32, by + py as i32, color, coverage);
            });
        }
        pen_x += scaled.h_advance(gid);
        prev = Some(gid);
    }
}

/// 水平居中画一行文字（给定基线 y；`center_y` 语义见印章调用处——按字高中轴对齐）。
fn draw_centered_text(
    canvas: &mut Canvas,
    font: &ab_glyph::FontVec,
    size: f32,
    text: &str,
    center_x: f32,
    center_y: f32,
    color: Rgba,
) {
    let left = center_x - text_width(font, size, text) / 2.0;
    let scaled = font.as_scaled(ab_glyph::PxScale::from(size));
    // 以字形中轴（(ascent - descent) / 2）为基线偏移，视觉上垂直居中于 center_y。
    let baseline = center_y + (scaled.ascent() - scaled.descent()) / 2.0;
    draw_text_line(canvas, font, size, text, left, baseline, color);
}

/// RGBA 缓冲编码为 PNG（内存写入不产生 IO 错误，失败视为不可达）。
fn encode_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .expect("PNG 头写入（内存流）不会失败");
    writer
        .write_image_data(rgba)
        .expect("PNG 数据写入（内存流）不会失败");
    writer.finish().expect("PNG 收尾（内存流）不会失败");
    out.into_inner()
}

// ======================================================================
// 模块区 3：内嵌单元测试（全离线，不碰网络 / 文件系统除字体读取）
// ======================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// 恒定占位的 resolver：所有图都解析失败。
    fn no_images(_: &str) -> Option<String> {
        None
    }

    // ---- 标记渲染 ----

    #[test]
    fn render_chapter_line_becomes_h2() {
        let body = render_xhtml_body("[chapter:雨夜序章]\n正文第一行", &no_images);
        assert!(body.contains("<h2>雨夜序章</h2>"));
        assert!(body.contains("<p>正文第一行</p>"));
        // 空 / 空白标题整行丢弃
        let body = render_xhtml_body("[chapter:  ]\n下一行", &no_images);
        assert!(!body.contains("<h2>"));
        assert!(body.contains("<p>下一行</p>"));
    }

    #[test]
    fn render_chapter_inline_is_plain_text() {
        let body = render_xhtml_body("行内 [chapter:X] 保持文本", &no_images);
        assert!(body.contains("<p>行内 [chapter:X] 保持文本</p>"));
    }

    #[test]
    fn render_newpage_line_and_inline() {
        let body = render_xhtml_body("前页\n[newpage]\n后页", &no_images);
        assert!(body.contains(r#"<div class="page-break"></div>"#));
        assert!(body.contains("<p>前页</p>"));
        assert!(body.contains("<p>后页</p>"));
        // 行内 [newpage] 按普通文本转义（[ 无需转义，原样保留）
        let body = render_xhtml_body("行内 [newpage] 文本", &no_images);
        assert!(!body.contains("page-break"));
        assert!(body.contains("<p>行内 [newpage] 文本</p>"));
    }

    #[test]
    fn render_ruby_both_separators() {
        // pixiv 现行 `>` 分隔
        let body = render_xhtml_body("[rb:漢字>かんじ]の読み", &no_images);
        assert!(body.contains("<ruby>漢字<rt>かんじ</rt></ruby>の読み"));
        // 旧格式 `/` 分隔
        let body = render_xhtml_body("[rb:漢字/かんじ]の読み", &no_images);
        assert!(body.contains("<ruby>漢字<rt>かんじ</rt></ruby>の読み"));
        // 显示文本含 `/` 时 `>` 分隔优先
        let body = render_xhtml_body("[rb:日/本>にほん]", &no_images);
        assert!(body.contains("<ruby>日/本<rt>にほん</rt></ruby>"));
    }

    #[test]
    fn render_jumpuri_double_brackets() {
        let body = render_xhtml_body("详见 [[jumpuri:pixiv 官网 > https://www.pixiv.net/]] 即可", &no_images);
        assert!(body.contains(r#"<a href="https://www.pixiv.net/">pixiv 官网</a>"#));
        // 单层旧式容忍
        let body = render_xhtml_body("[jumpuri:链接 > https://example.com/a?b=1]", &no_images);
        assert!(body.contains(r#"<a href="https://example.com/a?b=1">链接</a>"#));
        // URL 中的 & 转义为 &amp;
        let body = render_xhtml_body("[[jumpuri:t > https://x.com/?a=1&b=2]]", &no_images);
        assert!(body.contains(r#"href="https://x.com/?a=1&amp;b=2""#));
    }

    #[test]
    fn render_jump_removed_line_and_inline() {
        // 整行 → 删行
        let body = render_xhtml_body("[jump:3]\n正文段", &no_images);
        assert!(!body.contains("[jump:3]"));
        assert!(body.contains("<p>正文段</p>"));
        let body = render_xhtml_body("[jumpurl:https://x.com]\n正文段", &no_images);
        assert!(!body.contains("[jumpurl:"));
        // 行内 → 剔除
        let body = render_xhtml_body("前 [jump:3] 后", &no_images);
        assert!(!body.contains("[jump:3]"));
        assert!(body.contains("前"));
        assert!(body.contains("后"));
        // 纯标记行（jump + 空白）整行丢弃
        let body = render_xhtml_body("[jump:1]  [jump:2]\n正文", &no_images);
        assert_eq!(body.matches("<p>").count(), 1);
    }

    #[test]
    fn render_image_block_and_placeholder() {
        let resolve = |id: &str| (id == "100").then(|| "100.png".to_string());
        // 整行图片 + 映射命中 → 块级 img
        let body = render_xhtml_body("[uploadedimage:100]", &resolve);
        assert_eq!(body, r#"<div class="epub-image"><img src="images/100.png" alt=""/></div>"#);
        // 整行图片 + 映射未命中 → 占位块
        let body = render_xhtml_body("[pixivimage:200-1]", &resolve);
        assert_eq!(body, format!(r#"<div class="epub-placeholder">{IMAGE_PLACEHOLDER_TEXT}</div>"#));
    }

    #[test]
    fn render_image_inline_mixed() {
        let resolve = |id: &str| (id == "100").then(|| "100.png".to_string());
        // 混排 + 命中 → 行内 img
        let body = render_xhtml_body("图 [uploadedimage:100] 混排", &resolve);
        assert!(body.contains(r#"<img class="epub-image-inline" src="images/100.png" alt=""/>"#));
        // 混排 + 未命中 → 占位文本
        let body = render_xhtml_body("图 [pixivimage:200] 混排", &resolve);
        assert!(body.contains(format!("<p>图 {IMAGE_PLACEHOLDER_TEXT} 混排</p>").as_str()));
        // 一行多图（全图段）→ 逐图块级
        let body = render_xhtml_body("[uploadedimage:100][uploadedimage:100]", &resolve);
        assert_eq!(body.matches(r#"<div class="epub-image">"#).count(), 2);
    }

    #[test]
    fn render_html_escape() {
        let body = render_xhtml_body("a<b>&\"c\"'d'", &no_images);
        assert_eq!(body, "<p>a&lt;b&gt;&amp;&quot;c&quot;&#39;d&#39;</p>");
        // 章节标题同样转义
        let body = render_xhtml_body("[chapter:<X&Y>]", &no_images);
        assert!(body.contains("<h2>&lt;X&amp;Y&gt;</h2>"));
    }

    #[test]
    fn render_crlf_normalized_and_blank_lines_folded() {
        let body = render_xhtml_body("一\r\n\r\n\r\n二\r\n三\r\n", &no_images);
        assert_eq!(body.matches("<p>").count(), 3);
        assert!(!body.contains("<p></p>"));
        // \r 单独出现也归一
        let body = render_xhtml_body("甲\r乙", &no_images);
        assert!(body.contains("<p>甲</p>"));
        assert!(body.contains("<p>乙</p>"));
    }

    // ---- EPUB 组装回读 ----

    /// 测试用 2×2 红绿蓝白 PNG。
    fn tiny_png() -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut encoder = png::Encoder::new(&mut out, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("header");
        let pixels: [u8; 16] = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        writer.write_image_data(&pixels).expect("data");
        writer.finish().expect("finish");
        out.into_inner()
    }

    fn sample_book() -> EpubBook {
        EpubBook {
            title: "雨夜的故事".to_string(),
            author: "作者名".to_string(),
            language: "zh-CN".to_string(),
            book_uuid: "3fa85f64-5717-4562-b3fc-2c963f66afa6".to_string(),
            chapters: vec![
                EpubChapter { title: "第一章".to_string(), xhtml_body: "<p>第一段</p>".to_string() },
                EpubChapter { title: "第二章".to_string(), xhtml_body: "<h2>节</h2>".to_string() },
            ],
            cover: Some(CoverImage { bytes: vec![0xFF, 0xD8, 0xFF, 0xE0], ext: "jpg".to_string() }),
            images: vec![
                EmbeddedImage { id: "9001".to_string(), bytes: tiny_png(), ext: "png".to_string() },
                EmbeddedImage { id: "9002".to_string(), bytes: tiny_png(), ext: "png".to_string() },
            ],
            placeholder_cover: None,
        }
    }

    #[test]
    fn epub_assembly_roundtrip() {
        let book = sample_book();
        let bytes = book.to_bytes().expect("组装成功");

        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes[..])).expect("zip 合法");

        // 条目顺序：mimetype 必须第一个且 Stored 无压缩。
        assert_eq!(archive.by_index(0).expect("首条目").name(), "mimetype");
        assert_eq!(
            archive.by_name("mimetype").expect("mimetype 存在").compression(),
            zip::CompressionMethod::Stored
        );
        assert_eq!(read_entry_text(&mut archive, "mimetype"), "application/epub+zip");

        // 结构条目齐全。
        for name in
            ["META-INF/container.xml", "OEBPS/content.opf", "OEBPS/nav.xhtml", "OEBPS/toc.ncx", "OEBPS/style.css", "OEBPS/cover.xhtml"]
        {
            assert!(archive.by_name(name).is_ok(), "缺少条目 {name}");
        }

        // 章节与图片条目齐全。
        assert!(archive.by_name("OEBPS/chapter_0001.xhtml").is_ok());
        assert!(archive.by_name("OEBPS/chapter_0002.xhtml").is_ok());
        assert!(archive.by_name("OEBPS/images/cover.jpg").is_ok());
        assert!(archive.by_name("OEBPS/images/9001.png").is_ok());
        assert!(archive.by_name("OEBPS/images/9002.png").is_ok());

        // 章节内容包含渲染好的 body。
        let ch1 = read_entry_text(&mut archive, "OEBPS/chapter_0001.xhtml");
        assert!(ch1.contains("<p>第一段</p>"));
        assert!(ch1.contains("style.css"));

        // OPF 引用与实际条目一致：提取全部 href 属性逐一回查 zip 条目。
        let opf = read_entry_text(&mut archive, "OEBPS/content.opf");
        let href_re = Regex::new(r#"href="([^"]+)""#).expect("href 正则合法");
        for caps in href_re.captures_iter(&opf) {
            let href = caps[1].to_string();
            let zip_path = format!("OEBPS/{href}");
            assert!(
                archive.by_name(&zip_path).is_ok(),
                "OPF 引用 {href} 在 zip 中不存在（应为 {zip_path}）"
            );
        }
        // OPF 元数据。
        assert!(opf.contains("<dc:title>雨夜的故事</dc:title>"));
        assert!(opf.contains("<dc:creator>作者名</dc:creator>"));
        assert!(opf.contains("<dc:language>zh-CN</dc:language>"));
        assert!(opf.contains("urn:uuid:3fa85f64-5717-4562-b3fc-2c963f66afa6"));
        assert!(opf.contains("properties=\"cover-image\""));
        assert!(opf.contains("properties=\"nav\""));
        assert!(opf.contains("dcterms:modified"));
        // spine：封面页在首。
        let spine_start = opf.find("<spine").expect("spine 存在");
        let spine = &opf[spine_start..];
        assert!(spine.find("cover-page").expect("封面在 spine") < spine.find("chapter-1").expect("章节在 spine"));

        // nav.xhtml 收录全部章节链接。
        let nav = read_entry_text(&mut archive, "OEBPS/nav.xhtml");
        assert!(nav.contains(r#"href="chapter_0001.xhtml">第一章</a>"#));
        assert!(nav.contains(r#"href="chapter_0002.xhtml">第二章</a>"#));
        // toc.ncx 收录全部 navPoint。
        let ncx = read_entry_text(&mut archive, "OEBPS/toc.ncx");
        assert_eq!(ncx.matches("<navPoint").count(), 2);
    }

    #[test]
    fn epub_without_cover_has_no_cover_page() {
        let mut book = sample_book();
        book.cover = None;
        book.placeholder_cover = None;
        let bytes = book.to_bytes().expect("组装成功");
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes[..])).expect("zip 合法");
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).expect("条目").name().to_string())
            .collect();
        assert!(!names.iter().any(|n| n.contains("cover")), "无封面时不得出现 cover 条目: {names:?}");
        let opf = read_entry_text(&mut archive, "OEBPS/content.opf");
        assert!(!opf.contains("cover-image"));
        // spine 首项为第一章。
        let spine_start = opf.find("<spine").expect("spine 存在");
        assert!(opf[spine_start..].contains("chapter-1"));
    }

    #[test]
    fn epub_placeholder_cover_generated_when_no_explicit_cover() {
        let mut book = sample_book();
        book.cover = None;
        book.placeholder_cover = Some(PlaceholderCoverSpec {
            title: "占位封面书".to_string(),
            author: "作者".to_string(),
            seed_id: 1,
        });
        let bytes = book.to_bytes().expect("组装成功");
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes[..])).expect("zip 合法");
        assert!(archive.by_name("OEBPS/cover.xhtml").is_ok());
        let cover_png = read_entry_bytes(&mut archive, "OEBPS/images/cover.png");
        assert_eq!(&cover_png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn epub_duplicate_image_id_rejected() {
        let mut book = sample_book();
        book.images.push(EmbeddedImage { id: "9001".to_string(), bytes: tiny_png(), ext: "png".to_string() });
        assert!(matches!(book.to_bytes(), Err(EpubError::DuplicateImage(_))));
    }

    // ---- 占位封面 ----

    /// 解码 PNG 回 RGBA（测试辅助）。
    fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(Cursor::new(bytes));
        let mut reader = decoder.read_info().expect("PNG 合法");
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).expect("帧解码");
        (info.width, info.height, buf)
    }

    /// 读 zip 文本条目（每次调用独立借用，规避 ZipFile 与 archive 的多重借用）。
    fn read_entry_text<R: Read + std::io::Seek>(
        archive: &mut zip::ZipArchive<R>,
        name: &str,
    ) -> String {
        let mut text = String::new();
        archive
            .by_name(name)
            .unwrap_or_else(|e| panic!("条目 {name} 读取失败: {e}"))
            .read_to_string(&mut text)
            .expect("条目为合法 UTF-8");
        text
    }

    /// 读 zip 二进制条目。
    fn read_entry_bytes<R: Read + std::io::Seek>(
        archive: &mut zip::ZipArchive<R>,
        name: &str,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        archive
            .by_name(name)
            .unwrap_or_else(|e| panic!("条目 {name} 读取失败: {e}"))
            .read_to_end(&mut data)
            .expect("二进制可读");
        data
    }

    fn sample_cover_spec() -> PlaceholderCoverSpec {
        PlaceholderCoverSpec {
            title: "很长很长的小说标题必须自动缩小字号来适应画布宽度限制".to_string(),
            author: "作者名".to_string(),
            seed_id: -3, // 负数种子 → |−3| % 5 = 3 → 玄黑
        }
    }

    #[test]
    fn placeholder_cover_png_dimensions() {
        // 系统候选（本机存在则走有字路径）；两条路径都必须产出合法 PNG。
        let png = render_placeholder_cover(&sample_cover_spec());
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let (w, h, _) = decode_png(&png);
        assert_eq!((w, h), (1000, 1414));
    }

    #[test]
    fn placeholder_cover_without_font_is_plain_with_seal_box() {
        let missing = vec![PathBuf::from("Z:/__definitely_missing_font__.ttc")];
        let png = render_placeholder_cover_with_fonts(&sample_cover_spec(), &missing);
        let (w, h, rgba) = decode_png(&png);
        assert_eq!((w, h), (1000, 1414));

        // 底色 = seed -3 → 主题 3（玄黑 #363636）。
        let bg = cover_theme(-3).bg;
        let center = ((700 * w + 500) * 4) as usize; // 标题区中心（y=700）
        assert_eq!(rgba[center], bg.r);
        assert_eq!(rgba[center + 1], bg.g);
        assert_eq!(rgba[center + 2], bg.b);

        // 标题区（避开印章与分割线）不得出现文字。
        let mut non_bg = 0usize;
        for y in (500..800).step_by(7) {
            for x in (250..750).step_by(7) {
                let idx = ((y * w + x) * 4) as usize;
                if rgba[idx] != bg.r || rgba[idx + 1] != bg.g || rgba[idx + 2] != bg.b {
                    non_bg += 1;
                }
            }
        }
        assert_eq!(non_bg, 0, "无字体路径标题区不应有任何文字像素");
        // 印章框仍存在（框线附近有非底色像素）。
        let seal_y = (COVER_H as f32 * SEAL_CENTER_Y_RATIO).round() as i32;
        let frame_x = (COVER_W as i32 / 2 - SEAL_BOX_W / 2) as usize;
        let idx = ((seal_y as u32 * w + frame_x as u32) * 4) as usize;
        assert!(rgba[idx] != bg.r || rgba[idx + 1] != bg.g, "印章框线应覆盖底色");
    }

    #[test]
    fn placeholder_cover_with_font_paints_title() {
        // 仅当本机存在候选字体时才有意义（Windows msyh / macOS PingFang / Linux Noto）。
        let candidates = system_font_candidates();
        let font = load_font_from_candidates(&candidates);
        let png = render_placeholder_cover_with_fonts(&sample_cover_spec(), &candidates);
        let (w, _h, rgba) = decode_png(&png);
        let bg = cover_theme(-3).bg;

        let title_area_has_ink = |rgba: &[u8]| -> bool {
            for y in (500..800).step_by(5) {
                for x in (250..750).step_by(5) {
                    let idx = ((y * w as usize + x) * 4) as usize;
                    if rgba[idx] != bg.r || rgba[idx + 1] != bg.g || rgba[idx + 2] != bg.b {
                        return true;
                    }
                }
            }
            false
        };

        if font.is_some() {
            assert!(
                title_area_has_ink(&rgba),
                "有字体路径应在标题区绘制文字（候选: {:?}）",
                candidates
            );
        }
        // 无断言失败时也保证结构合法（dimension 断言由上一测试覆盖）。
    }

    // ---- 样张（集中验收用，默认跳过）----

    #[test]
    #[ignore = "样张验证：主会话集中验收时以 --ignored 执行，生成含 ruby/章节/分页/图片/占位封面的 EPUB"]
    fn sample_epub_to_temp() {
        let content = concat!(
            "[chapter:第一章 雨夜]\n",
            "风住了，[rb:雨>あめ]还没有停。\n",
            "[uploadedimage:9001]\n",
            "这是行内图 [pixivimage:612345-2] 的混排段落。\n",
            "[jump:1]\n",
            "详见 [[jumpuri:pixiv 官网 > https://www.pixiv.net/]]。\n",
            "\n",
            "第二段：a<b>&\"引号\"'单引号'\n",
            "[newpage]\n",
            "[chapter:第二章 晴天]\n",
            "[rb:晴/せい]れた空。\n",
            "[uploadedimage:404]\n",
            "结束。\n",
        );
        let resolve = |id: &str| (id == "9001").then(|| "9001.png".to_string());
        let body = render_xhtml_body(content, &resolve);

        let book = EpubBook {
            title: "样张：EPUB 导出验证".to_string(),
            author: "样张作者".to_string(),
            language: "ja".to_string(),
            book_uuid: "11111111-2222-3333-4444-555555555555".to_string(),
            chapters: vec![EpubChapter { title: "样张章节".to_string(), xhtml_body: body }],
            cover: None,
            images: vec![EmbeddedImage { id: "9001".to_string(), bytes: tiny_png(), ext: "png".to_string() }],
            placeholder_cover: Some(PlaceholderCoverSpec {
                title: "样张：EPUB 导出验证".to_string(),
                author: "样张作者".to_string(),
                seed_id: 12345,
            }),
        };
        let path = std::env::temp_dir().join("pixiv-epub-sample.epub");
        book.write_to(&path).expect("样张写盘");
        println!("样张 EPUB 已写入: {}", path.display());
        println!("包含：ruby 注音 / 章节标题 / 分页 / 命中与占位图片 / 跳转删除 / jumpuri 链接 / HTML 转义 / 占位封面");
    }
}
