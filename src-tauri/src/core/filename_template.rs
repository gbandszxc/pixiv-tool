//! 下载文件名模板：占位符解析 / 校验 / 渲染（纯函数，无 IO）。
//!
//! 模板由「字面段 + `{占位符}`」组成，占位符白名单按用途划分：
//! - 插画：`{user}` `{uid}` `{title}` `{pid}` `{page}` `{page1}`
//! - 小说 / 系列：`{user}` `{uid}` `{title}` `{nid}` `{order}` `{sid}`
//!
//! 渲染按序执行：
//! 1. 模板解析为字面段 / 占位符 token 序列；
//! 2. 动图渲染（[`render_illust`] 的 `ugoira=true`）：删除所有 `{page}` /
//!    `{page1}`，并从紧邻前导字面段末尾删除分隔符（`_p` / `_` / `-`），
//!    拼接结果净化后追加 `_ugoira` 后缀（`.zip` 扩展名由调用方拼接）；
//! 3. 值为空的占位符（非系列的 `{order}` / `{sid}`）删除该 token，并从
//!    紧邻前导字面段末尾吸收 1 个分隔字符（`_` / `-`），避免悬挂分隔符；
//! 4. 拼接结果整体过 [`sanitize_filename`] 净化；
//! 5. 净化后为空 → 返回 `None`（调用方回落现行默认命名）。
//!
//! 默认模板与现行命名 byte 级一致（`illust_crawler::page_filename` /
//! `ugoira_filename`、`exporter::make_filename`，测试内直接对照）；除规则
//! 2/3 的局部吸收外不做全局 strip 或连续 `_` 收敛——标题尾部合法 `_`
//! 在拼接后非末尾时保留。
//!
//! 模板应先经 [`validate_template`] 校验；未校验的模板渲染不会 panic：
//! 未配对花括号按字面处理，白名单外的 `{xxx}` 原样保留（再经整体净化）。

use crate::core::exporter::sanitize_filename;

/// 插画（多页）文件名默认模板；标题可净化为非空时与现行
/// `illust_crawler::page_filename` 输出 byte 级一致。
pub const ILLUST_FILENAME_TEMPLATE_DEFAULT: &str = "{title}_{pid}_p{page}";

/// 小说（单篇）文件名默认模板；标题可净化为非空时与现行
/// `exporter::make_filename` 单篇输出 byte 级一致（空标题按规格回退
/// `{nid}`，exporter 无此兜底，属模板路径有意的行为变更）。
pub const NOVEL_FILENAME_TEMPLATE_DEFAULT: &str = "{title}_{nid}";

/// 系列各话文件名默认模板；标题可净化为非空时与现行
/// `exporter::make_filename` 系列输出 byte 级一致。
pub const NOVEL_SERIES_FILENAME_TEMPLATE_DEFAULT: &str = "{order}_{title}";

/// 动图词干后缀：对齐现行 `ugoira_filename` 的 `{safe}_{id}_ugoira.zip`，
/// 本模块产出不含 `.zip` 扩展名的完整词干，`.zip` 由调用方拼接。
const UGOIRA_SUFFIX: &str = "_ugoira";

/// 模板用途：决定占位符白名单与必含项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateKind {
    /// 插画（多页）：必须含 `{page}` 或 `{page1}`，防多页同名覆盖
    Illust,
    /// 小说单篇
    Novel,
    /// 系列各话：必须含 `{order}`，防各话同名覆盖
    NovelSeries,
}

impl TemplateKind {
    fn label(self) -> &'static str {
        match self {
            TemplateKind::Illust => "插画",
            TemplateKind::Novel => "小说",
            TemplateKind::NovelSeries => "小说系列",
        }
    }
}

/// 占位符（全局已知名字；是否可用于某用途由 [`TemplateKind`] 白名单定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placeholder {
    User,
    Uid,
    Title,
    Pid,
    Page,
    Page1,
    Nid,
    Order,
    Sid,
}

impl Placeholder {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "user" => Placeholder::User,
            "uid" => Placeholder::Uid,
            "title" => Placeholder::Title,
            "pid" => Placeholder::Pid,
            "page" => Placeholder::Page,
            "page1" => Placeholder::Page1,
            "nid" => Placeholder::Nid,
            "order" => Placeholder::Order,
            "sid" => Placeholder::Sid,
            _ => return None,
        })
    }

    /// 该用途允许的占位符集合。
    fn allowed_for(kind: TemplateKind) -> &'static [Placeholder] {
        match kind {
            TemplateKind::Illust => &[
                Placeholder::User,
                Placeholder::Uid,
                Placeholder::Title,
                Placeholder::Pid,
                Placeholder::Page,
                Placeholder::Page1,
            ],
            TemplateKind::Novel | TemplateKind::NovelSeries => &[
                Placeholder::User,
                Placeholder::Uid,
                Placeholder::Title,
                Placeholder::Nid,
                Placeholder::Order,
                Placeholder::Sid,
            ],
        }
    }

    /// 错误提示里的写法（`{xxx}`）。
    fn display(self) -> &'static str {
        match self {
            Placeholder::User => "{user}",
            Placeholder::Uid => "{uid}",
            Placeholder::Title => "{title}",
            Placeholder::Pid => "{pid}",
            Placeholder::Page => "{page}",
            Placeholder::Page1 => "{page1}",
            Placeholder::Nid => "{nid}",
            Placeholder::Order => "{order}",
            Placeholder::Sid => "{sid}",
        }
    }
}

/// 模板 token。
#[derive(Debug, Clone)]
enum Token {
    /// 字面段
    Literal(String),
    /// 白名单内占位符
    Placeholder(Placeholder),
    /// 白名单外的 `{xxx}`：渲染时原样保留，校验时拒绝
    Unknown(String),
}

/// 词法扫描：拆成 token 序列；第二返回值为「花括号不配对」标记
/// （缺失 `}` 或多余 `}`）。渲染路径忽略该标记按字面处理，校验路径拒绝。
fn scan(template: &str) -> (Vec<Token>, bool) {
    let mut tokens: Vec<Token> = Vec::new();
    let mut literal = String::new();
    let mut name = String::new();
    let mut in_brace = false;
    let mut unpaired = false;
    for c in template.chars() {
        match c {
            '{' if !in_brace => {
                if !literal.is_empty() {
                    tokens.push(Token::Literal(std::mem::take(&mut literal)));
                }
                in_brace = true;
                name.clear();
            }
            '}' => {
                if in_brace {
                    tokens.push(match Placeholder::from_name(&name) {
                        Some(ph) => Token::Placeholder(ph),
                        None => Token::Unknown(std::mem::take(&mut name)),
                    });
                    in_brace = false;
                } else {
                    unpaired = true;
                    literal.push(c);
                }
            }
            _ if in_brace => name.push(c),
            _ => literal.push(c),
        }
    }
    if in_brace {
        // 缺右括号：整段回落为字面
        unpaired = true;
        literal.push('{');
        literal.push_str(&name);
    }
    if !literal.is_empty() {
        tokens.push(Token::Literal(literal));
    }
    (tokens, unpaired)
}

/// 校验模板（错误信息中文，可直接展示给用户）：
/// - 非空；长度 ≤ 200（Unicode 字符数）；
/// - 禁含 `< > : " / \ | ? *` 与 ASCII 控制字符；
/// - `{` / `}` 必须配对，`{...}` 内名字必须在对应用途的白名单内；
/// - 插画模板必须含 `{page}` / `{page1}`（防多页同名覆盖）；
///   系列模板必须含 `{order}`（防各话同名覆盖）。
pub fn validate_template(template: &str, kind: TemplateKind) -> Result<(), String> {
    if template.is_empty() {
        return Err("模板不能为空".into());
    }
    let len = template.chars().count();
    if len > 200 {
        return Err(format!("模板长度 {len} 超过上限 200 个字符"));
    }
    for c in template.chars() {
        if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            return Err(format!("模板包含不允许的字符：{c}"));
        }
        if c.is_ascii_control() {
            return Err("模板包含不允许的控制字符".into());
        }
    }
    let (tokens, unpaired) = scan(template);
    if unpaired {
        return Err("花括号不配对：请检查占位符的 { 与 } 是否成对".into());
    }
    let allowed = Placeholder::allowed_for(kind);
    let allowed_list = |phs: &[Placeholder]| {
        phs.iter()
            .map(|p| p.display())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut present: Vec<Placeholder> = Vec::new();
    for token in &tokens {
        match token {
            Token::Unknown(name) => {
                return Err(format!(
                    "未知占位符：{{{name}}}（{}模板可用：{}）",
                    kind.label(),
                    allowed_list(allowed)
                ));
            }
            Token::Placeholder(ph) => {
                if !allowed.contains(ph) {
                    return Err(format!(
                        "占位符 {} 不适用于{}模板（可用：{}）",
                        ph.display(),
                        kind.label(),
                        allowed_list(allowed)
                    ));
                }
                present.push(*ph);
            }
            Token::Literal(_) => {}
        }
    }
    match kind {
        TemplateKind::Illust
            if !present.contains(&Placeholder::Page)
                && !present.contains(&Placeholder::Page1) =>
        {
            return Err("插画模板必须包含 {page} 或 {page1}，避免多页文件同名覆盖".into());
        }
        TemplateKind::NovelSeries if !present.contains(&Placeholder::Order) => {
            return Err("系列模板必须包含 {order}，避免各话文件同名覆盖".into());
        }
        _ => {}
    }
    Ok(())
}

/// 插画命名信息（[`render_illust`] 入参）。
#[derive(Debug, Clone)]
pub struct IllustNameInfo {
    /// 作者名；为空时 `{user}` 回落 `user_{uid}`
    pub user_name: String,
    /// 作者 ID（`{uid}`）
    pub user_id: i64,
    /// 作品名（原样传入，渲染流程统一净化）；净化后为空回退 `{pid}`
    pub title: String,
    /// 插画 ID（`{pid}`）
    pub id: i64,
}

/// 小说命名信息（[`render_novel`] 入参）。
#[derive(Debug, Clone)]
pub struct NovelNameInfo {
    /// 作者名；为空时 `{user}` 回落 `user_{uid}`
    pub user_name: String,
    /// 作者 ID（`{uid}`）
    pub user_id: i64,
    /// 作品名（原样传入，渲染流程统一净化）；净化后为空回退 `{nid}`
    pub title: String,
    /// 小说 ID（`{nid}`）
    pub id: i64,
    /// 系列 ID（`{sid}`）；非系列为 `None`，占位符按空值删除并吸收分隔符
    pub series_id: Option<i64>,
    /// 系列话序（`{order}`）；非系列为 `None`，同上
    pub series_order: Option<i64>,
    /// 系列总话数：决定 `{order}` 零填充宽度（≤99 → 2 位，否则 3 位，
    /// 沿用 `exporter::make_filename` 现行规则）
    pub page_count: i64,
}

/// 渲染插画文件名词干（不含扩展名）。
///
/// `ugoira=true` 时先删除所有 `{page}` / `{page1}` 及其前导分隔符，结果
/// 净化后追加 `_ugoira` 后缀（调用方拼接 `.zip` 即得与现行
/// `ugoira_filename` 一致的完整文件名）。返回 `None` 表示渲染结果为空，
/// 调用方应回落现行默认命名。
pub fn render_illust(
    info: &IllustNameInfo,
    template: &str,
    page: usize,
    ugoira: bool,
) -> Option<String> {
    let (mut tokens, _) = scan(template);
    if ugoira {
        strip_page_tokens(&mut tokens);
    }
    let rendered = fill_tokens(&mut tokens, |ph| match ph {
        Placeholder::User => Some(user_value(&info.user_name, info.user_id)),
        Placeholder::Uid => Some(info.user_id.to_string()),
        Placeholder::Title => Some(title_value(&info.title, info.id)),
        Placeholder::Pid => Some(info.id.to_string()),
        Placeholder::Page => Some(page.to_string()),
        Placeholder::Page1 => Some((page + 1).to_string()),
        // 非插画占位符按空值处理（删除 + 吸收分隔符）
        _ => None,
    });
    finish(rendered, ugoira)
}

/// 渲染小说文件名词干（不含扩展名）。系列模板按 [`NovelNameInfo`] 是否
/// 携带 `series_order` / `series_id` 取值。返回 `None` 表示渲染结果为空，
/// 调用方应回落现行默认命名。
pub fn render_novel(info: &NovelNameInfo, template: &str) -> Option<String> {
    let (mut tokens, _) = scan(template);
    let rendered = fill_tokens(&mut tokens, |ph| match ph {
        Placeholder::User => Some(user_value(&info.user_name, info.user_id)),
        Placeholder::Uid => Some(info.user_id.to_string()),
        Placeholder::Title => Some(title_value(&info.title, info.id)),
        Placeholder::Nid => Some(info.id.to_string()),
        Placeholder::Order => info.series_order.map(|order| {
            let width = if info.page_count <= 99 { 2 } else { 3 };
            format!("{:0width$}", order, width = width)
        }),
        Placeholder::Sid => info.series_id.map(|sid| sid.to_string()),
        // 非小说占位符按空值处理（删除 + 吸收分隔符）
        _ => None,
    });
    finish(rendered, false)
}

/// `{user}` 取值：作者名，拿不到 / 为空回落 `user_{uid}`。
fn user_value(user_name: &str, user_id: i64) -> String {
    if user_name.is_empty() {
        format!("user_{user_id}")
    } else {
        user_name.to_string()
    }
}

/// `{title}` 取值：净化后为空 → 用作品 ID 兜底（插画 pid / 小说 nid）。
fn title_value(title: &str, id: i64) -> String {
    if sanitize_filename(title).is_empty() {
        id.to_string()
    } else {
        title.to_string()
    }
}

/// 动图预处理：删除所有 `{page}` / `{page1}` token；若其紧邻前导是字面段，
/// 删除末尾分隔符（`_p` 整体删除；`_` / `-` 各删 1 个字符），避免残留
/// `_p0` 之类的悬挂页码。例：`{title}_{pid}_p{page}` → `{title}_{pid}`。
fn strip_page_tokens(tokens: &mut Vec<Token>) {
    let mut i = 0;
    while i < tokens.len() {
        if matches!(
            tokens[i],
            Token::Placeholder(Placeholder::Page | Placeholder::Page1)
        ) {
            if i > 0 {
                if let Token::Literal(lit) = &mut tokens[i - 1] {
                    strip_separator_suffix(lit);
                }
            }
            tokens.remove(i);
        } else {
            i += 1;
        }
    }
}

/// 删除字面段末尾分隔符：`_p` → 空、`_` / `-` → 空；其余不动。
fn strip_separator_suffix(lit: &mut String) {
    for suffix in ["_p", "_", "-"] {
        if lit.ends_with(suffix) {
            lit.truncate(lit.len() - suffix.len());
            return;
        }
    }
}

/// 值填充 + 空值吸收：值为 `None` 的占位符删除该 token，并从紧邻前导
/// 字面段末尾吸收 1 个分隔字符（仅当该字面段以 `_` / `-` 结尾）。
/// 例：`{title}_{sid}_{nid}` 中 sid 为空 → 等价 `{title}_{nid}`。
fn fill_tokens(tokens: &mut [Token], value_of: impl Fn(Placeholder) -> Option<String>) -> String {
    let values: Vec<Option<String>> = tokens
        .iter()
        .map(|token| match token {
            Token::Placeholder(ph) => value_of(*ph),
            _ => None,
        })
        .collect();
    for i in 0..tokens.len() {
        if values[i].is_none() {
            if let Token::Placeholder(_) = tokens[i] {
                if i > 0 {
                    if let Token::Literal(lit) = &mut tokens[i - 1] {
                        absorb_separator_char(lit);
                    }
                }
            }
        }
    }
    let mut out = String::new();
    for (i, token) in tokens.iter().enumerate() {
        match token {
            Token::Literal(lit) => out.push_str(lit),
            Token::Placeholder(_) => {
                if let Some(value) = &values[i] {
                    out.push_str(value);
                }
            }
            Token::Unknown(name) => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
    }
    out
}

/// 从字面段末尾吸收 1 个分隔字符（`_` / `-`）。
fn absorb_separator_char(lit: &mut String) {
    if lit.ends_with('_') || lit.ends_with('-') {
        lit.pop();
    }
}

/// 收尾：整体净化 → 为空返回 `None` → 动图追加 `_ugoira` 后缀。
fn finish(rendered: String, ugoira: bool) -> Option<String> {
    let sanitized = sanitize_filename(&rendered);
    if sanitized.is_empty() {
        return None;
    }
    if ugoira {
        return Some(format!("{sanitized}{UGOIRA_SUFFIX}"));
    }
    Some(sanitized)
}

// ----------------------------------------------------------------------
// 单元测试（全部离线；现行命名函数直接对照，保证默认模板 byte 级兼容）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::exporter::{ExportMeta, make_filename};
    use crate::core::illust_crawler::{page_filename, safe_title_or_id, ugoira_filename};

    fn illust_info(title: &str) -> IllustNameInfo {
        IllustNameInfo {
            user_name: "作者名".into(),
            user_id: 11,
            title: title.into(),
            id: 2001,
        }
    }

    fn novel_info(
        title: &str,
        series_id: Option<i64>,
        series_order: Option<i64>,
        page_count: i64,
    ) -> NovelNameInfo {
        NovelNameInfo {
            user_name: "作者名".into(),
            user_id: 11,
            title: title.into(),
            id: 12345,
            series_id,
            series_order,
            page_count,
        }
    }

    fn exporter_meta(title: &str, page_count: i64, series_order: Option<i64>) -> ExportMeta {
        ExportMeta {
            novel_id: 12345,
            title: title.into(),
            page_count,
            series_order,
        }
    }

    // ------------------------------------------------------------------
    // byte 级兼容对照：默认模板 == 现行命名函数
    // ------------------------------------------------------------------

    #[test]
    fn illust_default_template_matches_current_naming() {
        let url = "https://i.pximg.net/img-original/img/2026/01/01/x_p0.jpg?x=1";
        // 中文标题 / 非法字符标题 / 空标题兜底，三种典型输入逐一对照
        for title in ["東方作品名", "作<品>名:测试", "___"] {
            let info = illust_info(title);
            let rendered = render_illust(&info, ILLUST_FILENAME_TEMPLATE_DEFAULT, 3, false)
                .expect("默认模板 + 兜底 id 不应为空");
            let current = page_filename(&safe_title_or_id(title, 2001), 2001, 3, url);
            assert_eq!(format!("{rendered}.jpg"), current, "title={title:?}");
        }
    }

    #[test]
    fn ugoira_default_template_matches_current_naming() {
        for title in ["東方作品名", "___"] {
            let info = illust_info(title);
            let rendered = render_illust(&info, ILLUST_FILENAME_TEMPLATE_DEFAULT, 0, true)
                .expect("默认模板 + 兜底 id 不应为空");
            let current = ugoira_filename(&safe_title_or_id(title, 2001), 2001);
            assert_eq!(format!("{rendered}.zip"), current, "title={title:?}");
        }
        // `{page1}` 同样被删（含前导 `_p` 分隔符）
        let rendered = render_illust(&illust_info("t"), "{title}_{pid}_p{page1}", 5, true)
            .expect("不应为空");
        assert_eq!(
            format!("{rendered}.zip"),
            ugoira_filename(&safe_title_or_id("t", 2001), 2001)
        );
        // 模板中段的 page：`_p` 分隔符整体删除、`-` 删 1 字符
        assert_eq!(
            render_illust(&illust_info("t"), "{title}_p{page}_{pid}", 5, true).unwrap(),
            "t_2001_ugoira"
        );
        assert_eq!(
            render_illust(&illust_info("t"), "{title}-{page}", 0, true).unwrap(),
            "t_ugoira"
        );
    }

    #[test]
    fn ugoira_without_page_token_renders_normally() {
        // 模板不含 page：删除规则为 no-op，仅追加 `_ugoira` 后缀
        let normal = render_illust(&illust_info("t"), "{title}_{pid}", 0, false).unwrap();
        let ugoira = render_illust(&illust_info("t"), "{title}_{pid}", 0, true).unwrap();
        assert_eq!(ugoira, format!("{normal}_ugoira"));
        // page 参数不影响动图渲染（page token 已删）
        assert_eq!(
            render_illust(&illust_info("t"), ILLUST_FILENAME_TEMPLATE_DEFAULT, 9, true).unwrap(),
            render_illust(&illust_info("t"), ILLUST_FILENAME_TEMPLATE_DEFAULT, 0, true).unwrap()
        );
    }

    #[test]
    fn novel_default_template_matches_current_naming() {
        // 单篇：中文标题 / 非法字符标题
        for title in ["小说标题", "作<品>名:测试"] {
            let info = novel_info(title, None, None, 1);
            let rendered = render_novel(&info, NOVEL_FILENAME_TEMPLATE_DEFAULT)
                .expect("默认模板 + 兜底 id 不应为空");
            let current = make_filename(&exporter_meta(title, 1, None), "txt");
            assert_eq!(format!("{rendered}.txt"), current, "title={title:?}");
        }
        // 系列：零填充宽度沿用现行规则（page_count ≤99 → 2 位，否则 3 位）
        let info = novel_info("话名", Some(999), Some(5), 10);
        assert_eq!(
            render_novel(&info, NOVEL_SERIES_FILENAME_TEMPLATE_DEFAULT).unwrap(),
            "05_话名"
        );
        assert_eq!(
            make_filename(&exporter_meta("话名", 10, Some(5)), "txt"),
            "05_话名.txt"
        );
        let info = novel_info("话名", Some(999), Some(5), 100);
        assert_eq!(
            render_novel(&info, NOVEL_SERIES_FILENAME_TEMPLATE_DEFAULT).unwrap(),
            "005_话名"
        );
        assert_eq!(
            make_filename(&exporter_meta("话名", 100, Some(5)), "txt"),
            "005_话名.txt"
        );
        // zfill 不截断：order 超过宽度时原样
        let info = novel_info("话名", Some(999), Some(123), 5);
        assert_eq!(
            render_novel(&info, NOVEL_SERIES_FILENAME_TEMPLATE_DEFAULT).unwrap(),
            "123_话名"
        );
    }

    #[test]
    fn novel_empty_title_falls_back_to_nid() {
        // 规格定稿：标题净化后为空 → 小说用 nid 兜底。现行 exporter::make_filename
        // 无兜底（产 "_12345"），模板路径有意与之不同。
        let info = novel_info("___", None, None, 1);
        assert_eq!(
            render_novel(&info, NOVEL_FILENAME_TEMPLATE_DEFAULT).unwrap(),
            "12345_12345"
        );
    }

    // ------------------------------------------------------------------
    // 空值吸收（非系列 sid / order）
    // ------------------------------------------------------------------

    #[test]
    fn empty_placeholders_absorb_separator() {
        let info = novel_info("小说标题", None, None, 1);
        assert_eq!(
            render_novel(&info, "{title}_{sid}_{nid}").unwrap(),
            "小说标题_12345",
            "sid 为空 → 吸收前导 `_`，无悬挂分隔符"
        );
        assert_eq!(
            render_novel(&info, "{title}-{order}-{nid}").unwrap(),
            "小说标题-12345"
        );
        assert_eq!(
            render_novel(&info, "{title}_{sid}_{order}_{nid}").unwrap(),
            "小说标题_12345",
            "连续空占位符逐个吸收"
        );
        // 行首空占位符无可吸收：悬挂 `_` 由整体净化兜底剥离
        assert_eq!(render_novel(&info, "{sid}_{nid}").unwrap(), "12345");
        // 系列值非空时正常填充
        let series = novel_info("小说标题", Some(999), Some(2), 10);
        assert_eq!(
            render_novel(&series, "{title}_{sid}_{order}").unwrap(),
            "小说标题_999_02"
        );
    }

    // ------------------------------------------------------------------
    // 自定义模板组合
    // ------------------------------------------------------------------

    #[test]
    fn custom_templates_render() {
        let info = illust_info("作品");
        assert_eq!(
            render_illust(&info, "{user}_{title}_{pid}_p{page1}", 0, false).unwrap(),
            "作者名_作品_2001_p1"
        );
        assert_eq!(render_illust(&info, "{page1}_{title}", 2, false).unwrap(), "3_作品");
        let novel = novel_info("小说标题", Some(999), Some(5), 10);
        assert_eq!(
            render_novel(&novel, "{user}_{uid}_{order}_{title}_{sid}").unwrap(),
            "作者名_11_05_小说标题_999"
        );
        // `{uid}` 总有值；未知占位符（未校验模板）按字面保留，不 panic
        assert_eq!(render_illust(&info, "{uid}_{pid}", 0, false).unwrap(), "11_2001");
        assert_eq!(render_illust(&info, "{foo}_{pid}", 0, false).unwrap(), "{foo}_2001");
        // 含 `/` 的模板是校验拒绝的范畴；渲染路径将其净化为 `_`
        assert_eq!(render_illust(&info, "{title}/{pid}", 0, false).unwrap(), "作品_2001");
    }

    // ------------------------------------------------------------------
    // 校验
    // ------------------------------------------------------------------

    #[test]
    fn validate_accepts_defaults_and_valid_templates() {
        assert_eq!(
            validate_template(ILLUST_FILENAME_TEMPLATE_DEFAULT, TemplateKind::Illust),
            Ok(())
        );
        assert_eq!(
            validate_template(NOVEL_FILENAME_TEMPLATE_DEFAULT, TemplateKind::Novel),
            Ok(())
        );
        assert_eq!(
            validate_template(
                NOVEL_SERIES_FILENAME_TEMPLATE_DEFAULT,
                TemplateKind::NovelSeries
            ),
            Ok(())
        );
        // order / sid 在小说单篇模板中同样合法
        assert!(validate_template("{title}_{nid}_{sid}", TemplateKind::Novel).is_ok());
        // 恰好 200 字符（上限内）
        let ok200 = format!("{{title}}_{{pid}}_p{{page}}{}", "x".repeat(179));
        assert_eq!(ok200.chars().count(), 200);
        assert!(validate_template(&ok200, TemplateKind::Illust).is_ok());
    }

    #[test]
    fn validate_rejects_bad_templates() {
        // 空 / 超长（201 字符）
        assert!(validate_template("", TemplateKind::Illust).is_err(), "空模板");
        assert!(
            validate_template(&"x".repeat(201), TemplateKind::Illust).is_err(),
            "超长"
        );
        // 非法字符与 ASCII 控制字符
        for c in ['<', '>', ':', '"', '/', '\\', '|', '?', '*'] {
            let t = format!("a{c}b_{{pid}}_p{{page}}");
            assert!(
                validate_template(&t, TemplateKind::Illust).is_err(),
                "非法字符 {c}"
            );
        }
        assert!(
            validate_template("a\u{7}b_{{pid}}_p{{page}}", TemplateKind::Illust).is_err(),
            "控制字符"
        );
        // 未知 / 空占位符
        assert!(
            validate_template("{foo}_{pid}_p{page}", TemplateKind::Illust).is_err(),
            "未知占位符"
        );
        assert!(validate_template("{}", TemplateKind::Illust).is_err(), "空占位符");
        // 花括号不配对（缺 `}` / 多余 `}`）
        assert!(validate_template("{title", TemplateKind::Illust).is_err(), "缺右括号");
        assert!(
            validate_template("ti}tle_{{pid}}_p{{page}}", TemplateKind::Illust).is_err(),
            "多余右括号"
        );
        // 必含项：插画缺 page、系列缺 order
        assert!(
            validate_template("{title}_{pid}", TemplateKind::Illust).is_err(),
            "插画缺 page"
        );
        assert!(
            validate_template("{title}_{nid}", TemplateKind::NovelSeries).is_err(),
            "系列缺 order"
        );
        // 白名单按用途区分：插画与小说占位符互不可用
        assert!(validate_template("{pid}", TemplateKind::Novel).is_err());
        assert!(validate_template("{nid}", TemplateKind::Illust).is_err());
        assert!(validate_template("{page}", TemplateKind::Novel).is_err());
    }

    // ------------------------------------------------------------------
    // 空结果回落信号
    // ------------------------------------------------------------------

    #[test]
    fn empty_render_returns_none() {
        // 纯非法字面模板 → 净化后为空（动图判空先于 `_ugoira` 后缀）
        assert!(render_illust(&illust_info("t"), "___", 0, false).is_none());
        assert!(render_illust(&illust_info("t"), "___", 0, true).is_none());
        // 仅剩空值占位符（非系列 sid / order）
        assert!(render_novel(&novel_info("t", None, None, 1), "{sid}").is_none());
        // 空作者名回落 user_{uid}，不会为空
        let info = illust_info("t");
        assert_eq!(
            render_illust(
                &IllustNameInfo {
                    user_name: String::new(),
                    ..info.clone()
                },
                "{user}",
                0,
                false
            )
            .unwrap(),
            "user_11"
        );
    }
}
