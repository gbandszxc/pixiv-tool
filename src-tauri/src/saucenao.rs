//! SauceNAO 以图识图客户端（`https://saucenao.com/search.php`，output_type=2 JSON）。
//!
//! 契约事实源：`docs/SPEC.md` §7 命令表；接口行为 / 错误形态 / fixture 见
//! `docs/research/saucenao-api.md`（本模块解析逻辑与其附录 A1 / A3 逐条对应）。
//!
//! 与 pixiv 客户端的差异：**不带任何 cookie**（SauceNAO 鉴权只看 query 上的
//! `api_key`），同款 Chrome147 指纹（wreq emulation + UA）。单请求 30s 超时、
//! 无重试、无限速队列（用户手动触发；免费账号 4 次/30s 由服务端配额约束）。
//!
//! 日志纪律：任何日志与错误文案不得出现 api_key 值与完整带 key 的 URL——
//! wreq 网络错误的 Display 可能内嵌完整请求 URL，统一经
//! [`sanitize_error_text`] 剥掉查询串后再进错误文案。
//!
//! multipart：wreq 6 的 `multipart` feature 本仓库未启用（Cargo.toml 锁定
//! 依赖集），file 通道手动构造 multipart/form-data 字节流（随机 boundary）。

use std::sync::OnceLock;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use wreq::header::{HeaderMap, HeaderValue};
use wreq_util::Emulation;

use crate::pixiv::client::{ACCEPT_LANGUAGE, USER_AGENT};
use crate::state::AppState;

/// 搜索端点（GET / POST 共用；api_key 等参数全放 URL query）。
pub const SEARCH_URL: &str = "https://saucenao.com/search.php";
/// 单请求超时（秒）。
pub const REQUEST_TIMEOUT_SECS: u64 = 30;
/// 本地文件大小上限（字节）：10MB（保守预检值，服务端真实上限未公开）。
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
/// 扩展名白名单（webp 未验证但放行，服务端报错走错误分类）。
pub const ALLOWED_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
/// numres 默认与上限（SauceNAO 取值域 1–40，上限为社区文档记载）。
pub const NUMRES_DEFAULT: i64 = 16;
pub const NUMRES_MAX: i64 = 40;

/// 命中库判定：5 = pixiv（现行）、6 = pixivhistorical（旧图）。
fn is_pixiv_index(index_id: i64) -> bool {
    matches!(index_id, 5 | 6)
}

/// 扩展名 → multipart Content-Type。
fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

// ----------------------------------------------------------------------
// 响应模型（serde 序列化为 snake_case，即 IPC 返回体形状）
// ----------------------------------------------------------------------

/// 单条匹配结果。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SaucenaoResult {
    /// 命中库编号（pixiv=5 / pixivhistorical=6）。
    pub index_id: i64,
    /// 库名原样（形如 `Index #5: Pixiv Images - 4933944_s.jpg`，展示层剥离前缀）。
    pub index_name: String,
    /// 相似度（SauceNAO 给字符串，parseFloat 失败置 0.0）。
    pub similarity: f64,
    /// 缩略图（saucenao 签名临时 URL，会过期；前端直连 + 失败占位块）。
    pub thumbnail: String,
    /// index_id ∈ {5, 6}。
    pub is_pixiv: bool,
    /// pixiv 作品 id：`data.pixiv_id` → ext_urls 回退链；拿不到为 null。
    pub pixiv_id: Option<i64>,
    /// 作者 uid：`data.member_id` → ext_urls 回退链；拿不到为 null。
    pub member_id: Option<i64>,
    pub member_name: Option<String>,
    pub title: Option<String>,
    /// 外链（ext_urls[0]）。
    pub ext_url: Option<String>,
}

/// 双窗口配额（short = 每 30 秒 / long = 每 24 小时；剩余量缺省 null）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SaucenaoQuota {
    pub short_limit: String,
    pub long_limit: String,
    pub short_remaining: Option<i64>,
    pub long_remaining: Option<i64>,
}

/// 搜索返回体（`saucenao_search` 的 Ok 载荷）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SaucenaoSearchResponse {
    pub results: Vec<SaucenaoResult>,
    /// 无顶层 header 时为 null。
    pub quota: Option<SaucenaoQuota>,
}

// ----------------------------------------------------------------------
// 入口：搜索（file / url 两通道）
// ----------------------------------------------------------------------

/// 以图识图搜索。参数粗校验在命令层（`saucenao_cmds`），此处只做 key 检查、
/// 文件预检与网络调用；解析在 [`parse_response`]（独立纯函数，离线可测）。
pub async fn search_impl(
    state: &AppState,
    source_type: &str,
    source: &str,
    numres: Option<i64>,
) -> Result<SaucenaoSearchResponse, String> {
    // key 属用户凭据：只在此处读入内存拼 query，不写日志、不进报错原文
    let api_key = state
        .settings_snapshot()
        .saucenao_api_key
        .trim()
        .to_string();
    if api_key.is_empty() {
        return Err("未配置 SauceNAO API Key，请在设置中填写".to_string());
    }
    // 命令层已白名单校验，clamp 只作直接调用方的防御兜底
    let numres = numres.unwrap_or(NUMRES_DEFAULT).clamp(1, NUMRES_MAX);

    let client = build_client().map_err(|err| format!("创建 HTTP 客户端失败: {err}"))?;
    let key_enc = percent_encode(&api_key);
    let (http_status, body) = match source_type {
        // url 通道：公网图片直链，GET `url=`（percent-encoded）
        "url" => {
            let url = format!(
                "{SEARCH_URL}?output_type=2&db=999&numres={numres}&api_key={key_enc}&url={}",
                percent_encode(source)
            );
            let resp = client.get(&url).send().await.map_err(|err| {
                format!(
                    "SauceNAO 请求失败: {}",
                    sanitize_error_text(&err.to_string())
                )
            })?;
            let status = resp.status().as_u16();
            // 只记状态码，不记 URL（query 含 api_key）
            log::info!("SauceNAO search(url) → {status}");
            let body = resp.text().await.map_err(|err| {
                format!(
                    "SauceNAO 请求失败: {}",
                    sanitize_error_text(&err.to_string())
                )
            })?;
            (status, body)
        }
        // file 通道：本地文件字节，POST multipart `file` 字段（api_key 等在 query）
        "file" => {
            let ext = precheck_file(source)?;
            let bytes = std::fs::read(source).map_err(|err| format!("读取图片文件失败: {err}"))?;
            let boundary = new_boundary();
            let body_bytes = build_multipart_body(
                &format!("upload.{ext}"),
                mime_for_ext(&ext),
                &bytes,
                &boundary,
            );
            let url =
                format!("{SEARCH_URL}?output_type=2&db=999&numres={numres}&api_key={key_enc}");
            let resp = client
                .post(&url)
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(body_bytes)
                .send()
                .await
                .map_err(|err| {
                    format!(
                        "SauceNAO 请求失败: {}",
                        sanitize_error_text(&err.to_string())
                    )
                })?;
            let status = resp.status().as_u16();
            log::info!("SauceNAO search(file, {} bytes) → {status}", bytes.len());
            let body = resp.text().await.map_err(|err| {
                format!(
                    "SauceNAO 请求失败: {}",
                    sanitize_error_text(&err.to_string())
                )
            })?;
            (status, body)
        }
        other => return Err(format!("不支持的来源类型: {other}（仅 file|url）")),
    };
    parse_response(http_status, &body)
}

/// 构造无 cookie 的 Chrome147 指纹客户端（与 pixiv/client.rs 同款 emulation + UA；
/// 不带 Referer——saucenao 无防盗链要求）。
fn build_client() -> anyhow::Result<wreq::Client> {
    let mut default_headers = HeaderMap::new();
    default_headers.insert("user-agent", HeaderValue::from_static(USER_AGENT));
    default_headers.insert("accept-language", HeaderValue::from_static(ACCEPT_LANGUAGE));
    wreq::Client::builder()
        .emulation(Emulation::Chrome147)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .default_headers(default_headers)
        .build()
        .map_err(Into::into)
}

/// 文件预检：存在 + 扩展名白名单 + 大小上限，返回小写扩展名。
fn precheck_file(source: &str) -> Result<String, String> {
    let path = std::path::Path::new(source);
    if !path.is_file() {
        return Err(format!("图片文件不存在: {source}"));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !ALLOWED_EXTS.contains(&ext.as_str()) {
        let shown = if ext.is_empty() {
            "<无扩展名>".to_string()
        } else {
            ext
        };
        return Err(format!(
            "不支持的图片格式: {shown}（仅 png/jpg/jpeg/gif/webp）"
        ));
    }
    let meta = std::fs::metadata(path).map_err(|err| format!("读取图片文件信息失败: {err}"))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err("图片文件超过 10MB 上限".to_string());
    }
    Ok(ext)
}

// ----------------------------------------------------------------------
// 响应解析（独立纯函数；解析顺序红线：HTTP 状态 → header.status → results）
// ----------------------------------------------------------------------

/// 按 HTTP 状态码 + body 形态归类错误，成功（200 且 header.status==0）解析结果。
pub fn parse_response(http_status: u16, body: &str) -> Result<SaucenaoSearchResponse, String> {
    if http_status != 200 {
        // 错误体也可能是 JSON（403 鉴权错误），先试解再归类
        let as_json: Option<Value> = serde_json::from_str(body).ok();
        return match http_status {
            429 => Err("SauceNAO 请求过于频繁，请稍后再试".to_string()),
            403 => {
                let is_auth = as_json
                    .as_ref()
                    .and_then(|v| v.get("header"))
                    .and_then(|h| h.get("status"))
                    .and_then(|s| s.as_i64())
                    == Some(-1);
                if is_auth {
                    Err("SauceNAO API Key 缺失或无效，请在设置中检查".to_string())
                } else {
                    // 403 非 JSON = Cloudflare 挑战页；其余 403 形态同归防护拦截
                    Err("请求被 SauceNAO 的防护拦截，请稍后重试".to_string())
                }
            }
            other => Err(format!("SauceNAO 返回异常状态 {other}")),
        };
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|_| "请求被 SauceNAO 的防护拦截，请稍后重试".to_string())?;
    parse_success(&value)
}

/// 200 响应解析：header.status != 0 → 业务错误；否则遍历 results。
fn parse_success(value: &Value) -> Result<SaucenaoSearchResponse, String> {
    let header = value.get("header");
    if let Some(status) = header
        .and_then(|h| h.get("status"))
        .and_then(|s| s.as_i64())
    {
        if status != 0 {
            let message = header
                .and_then(|h| h.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("未知错误");
            return Err(format!("SauceNAO 返回错误：{message}"));
        }
    }
    let quota = header.map(|h| SaucenaoQuota {
        short_limit: loose_str(h.get("short_limit")).unwrap_or_default(),
        long_limit: loose_str(h.get("long_limit")).unwrap_or_default(),
        short_remaining: h.get("short_remaining").and_then(|v| v.as_i64()),
        long_remaining: h.get("long_remaining").and_then(|v| v.as_i64()),
    });
    let results = value
        .get("results")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().map(parse_result).collect())
        .unwrap_or_default();
    Ok(SaucenaoSearchResponse { results, quota })
}

/// 单条结果解析（宽容：字段缺失不致命，未知字段忽略）。
fn parse_result(item: &Value) -> SaucenaoResult {
    let header = item.get("header");
    let data = item.get("data");
    let index_id = header
        .and_then(|h| h.get("index_id"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let ext_urls: Vec<&str> = data
        .and_then(|d| d.get("ext_urls"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|u| u.as_str()).collect())
        .unwrap_or_default();
    SaucenaoResult {
        index_id,
        index_name: header
            .and_then(|h| h.get("index_name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        similarity: header.map(parse_similarity).unwrap_or(0.0),
        thumbnail: header
            .and_then(|h| h.get("thumbnail"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        is_pixiv: is_pixiv_index(index_id),
        pixiv_id: data
            .and_then(|d| d.get("pixiv_id"))
            .and_then(|v| v.as_i64())
            .or_else(|| extract_pixiv_id(&ext_urls)),
        member_id: data
            .and_then(|d| d.get("member_id"))
            .and_then(|v| v.as_i64())
            .or_else(|| extract_member_id(&ext_urls)),
        member_name: loose_str(data.and_then(|d| d.get("member_name"))),
        title: loose_str(data.and_then(|d| d.get("title"))),
        ext_url: ext_urls.first().map(|s| (*s).to_string()),
    }
}

/// similarity 解析：字符串 parseFloat 失败置 0.0；数字原样；其他 0.0。
fn parse_similarity(header: &Value) -> f64 {
    match header.get("similarity") {
        Some(Value::String(s)) => s.trim().parse::<f64>().unwrap_or(0.0),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// 宽容字符串化：字符串原样、数字转字符串、缺失/其他 None（quota limit 字段
/// 契约是字符串，防御服务端字段类型漂移）。
fn loose_str(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

// ----------------------------------------------------------------------
// ext_urls 回退链（data.pixiv_id / member_id 缺失时）
// ----------------------------------------------------------------------

fn re_illust_id() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"illust_id=(\d+)").unwrap())
}

fn re_artworks() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"/artworks/(\d+)").unwrap())
}

fn re_member() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"member\.php\?id=(\d+)").unwrap())
}

/// pixiv_id 回退链：旧式 `illust_id=(\d+)` 优先 → 新式 `/artworks/(\d+)`。
fn extract_pixiv_id(ext_urls: &[&str]) -> Option<i64> {
    for re in [re_illust_id(), re_artworks()] {
        for url in ext_urls {
            if let Some(caps) = re.captures(url) {
                if let Ok(id) = caps[1].parse::<i64>() {
                    return Some(id);
                }
            }
        }
    }
    None
}

/// member_id 回退：`member.php?id=(\d+)`。
fn extract_member_id(ext_urls: &[&str]) -> Option<i64> {
    for url in ext_urls {
        if let Some(caps) = re_member().captures(url) {
            if let Ok(id) = caps[1].parse::<i64>() {
                return Some(id);
            }
        }
    }
    None
}

// ----------------------------------------------------------------------
// 请求构造工具（percent-encode / multipart / 脱敏）
// ----------------------------------------------------------------------

/// RFC 3986 query 值百分号编码：unreserved 字符（字母数字 `-_.~`）保留，
/// 其余按 UTF-8 字节转 `%XX`（无 url crate 依赖，手写覆盖 api_key 与图片 URL）。
fn percent_encode(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0x0f) as usize] as char);
            }
        }
    }
    out
}

/// 随机 multipart boundary（128-bit hex；不出现在内容中即可安全分段）。
fn new_boundary() -> String {
    format!(
        "pixivtool{:016x}{:016x}",
        rand::random::<u64>(),
        rand::random::<u64>()
    )
}

/// 手动构造 multipart/form-data：单 `file` 字段。filename 用固定 ASCII 名
/// （原文件名可能含非 ASCII / 引号，规避 header 转义问题）。
fn build_multipart_body(filename: &str, mime: &str, bytes: &[u8], boundary: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(bytes.len() + 256);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// 错误文本脱敏：wreq 错误可能内嵌完整请求 URL（query 含 api_key），
/// 剥掉首个 `?` 之后的内容再进错误文案（安全边界：报错不出现凭据原文）。
fn sanitize_error_text(text: &str) -> String {
    text.split('?')
        .next()
        .unwrap_or(text)
        .trim_end()
        .to_string()
}

// ----------------------------------------------------------------------
// 单元测试（全部离线，不发真实网络；fixture = 调研文档附录 A1 / A3）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    /// 附录 A3：2020 年真实成功响应片段（nomnoms12 测试套件记录），仅作形状参考。
    const FIXTURE_OK: &str = r#"{
      "header": {
        "user_id": 0,
        "account_type": 0,
        "short_limit": "4",
        "long_limit": "100",
        "long_remaining": 79,
        "short_remaining": 2,
        "status": 0,
        "results_requested": 6,
        "search_depth": "128",
        "minimum_similarity": 24.6,
        "query_image_display": "userdata/O3bQNenXC.gif.png",
        "query_image": "O3bQNenXC.gif",
        "results_returned": 6
      },
      "results": [
        {
          "header": {
            "similarity": "23.50",
            "thumbnail": "https://img1.saucenao.com/res/pixiv/493/4933944_s.jpg?auth=kwJTn57-P4LeASMM5JTIeQ&exp=1596483386",
            "index_id": 5,
            "index_name": "Index #5: Pixiv Images - 4933944_s.jpg"
          },
          "data": {
            "ext_urls": ["https://www.pixiv.net/member_illust.php?mode=medium&illust_id=4933944"],
            "title": "妖キャラをカリスマ化してみた。",
            "pixiv_id": 4933944,
            "member_name": "佳虫",
            "member_id": 724886
          }
        },
        {
          "header": {
            "similarity": "21.48",
            "thumbnail": "https://img1.saucenao.com/res/pixiv_historical/383/3836606_s.jpg?auth=yA_uYFsJWaH0QTpyFFWXig&exp=1596483386",
            "index_id": 6,
            "index_name": "Index #6: Pixiv Historical - 3836606_s.jpg"
          },
          "data": {
            "ext_urls": ["https://www.pixiv.net/member_illust.php?mode=medium&illust_id=3836606"]
          }
        },
        {
          "header": {
            "similarity": "23.60",
            "thumbnail": "https://img3.saucenao.com/dA/51571/515715132.jpg",
            "index_id": 34,
            "index_name": "Index #34: deviantArt - 515715132.jpg"
          },
          "data": {
            "ext_urls": ["https://deviantart.com/view/515715132"],
            "title": "Koshitantan + video link+stagedl",
            "da_id": 515715132,
            "author_name": "SliverRose0916",
            "author_url": "http://sliverrose0916.deviantart.com"
          }
        }
      ]
    }"#;

    #[test]
    fn parse_success_fixture_maps_all_fields() {
        let resp = parse_response(200, FIXTURE_OK).unwrap();
        assert_eq!(resp.results.len(), 3);

        // quota：limit 字符串原样 + 剩余量数值
        let quota = resp
            .quota
            .as_ref()
            .expect("顶层 header 存在 → quota 非 null");
        assert_eq!(quota.short_limit, "4");
        assert_eq!(quota.long_limit, "100");
        assert_eq!(quota.short_remaining, Some(2));
        assert_eq!(quota.long_remaining, Some(79));

        // 条目 1（index 5，现行 pixiv 库）：data 字段全命中
        let first = &resp.results[0];
        assert_eq!(first.index_id, 5);
        assert!(first.is_pixiv);
        assert_eq!(first.similarity, 23.50, "similarity 字符串转 f64");
        assert_eq!(first.pixiv_id, Some(4933944));
        assert_eq!(first.member_id, Some(724886));
        assert_eq!(first.member_name.as_deref(), Some("佳虫"));
        assert_eq!(
            first.title.as_deref(),
            Some("妖キャラをカリスマ化してみた。")
        );
        assert_eq!(
            first.ext_url.as_deref(),
            Some("https://www.pixiv.net/member_illust.php?mode=medium&illust_id=4933944")
        );
        assert!(first.thumbnail.contains("img1.saucenao.com"));

        // 条目 2（index 6，历史库）：data.pixiv_id 缺失 → ext_urls 旧式 URL 回退；
        // member_id 缺失且 ext_urls 无 member.php → None
        let second = &resp.results[1];
        assert_eq!(second.index_id, 6);
        assert!(second.is_pixiv, "index 6 = pixivhistorical 仍判 pixiv");
        assert_eq!(second.pixiv_id, Some(3836606));
        assert_eq!(second.member_id, None);
        assert_eq!(second.member_name, None);

        // 条目 3（index 34，deviantArt）：非 pixiv，pid/member 提取不到 → null
        let third = &resp.results[2];
        assert_eq!(third.index_id, 34);
        assert!(!third.is_pixiv);
        assert_eq!(third.pixiv_id, None);
        assert_eq!(third.member_id, None);
        assert_eq!(
            third.ext_url.as_deref(),
            Some("https://deviantart.com/view/515715132")
        );
    }

    #[test]
    fn parse_403_anonymous_error_maps_to_key_message() {
        // 附录 A1 实测错误体（匿名 / 无 key / 无效 key 同形态）
        let body = r#"{"header":{"status":-1,"message":"The anonymous account type does not permit API usage."}}"#;
        assert_eq!(
            parse_response(403, body).unwrap_err(),
            "SauceNAO API Key 缺失或无效，请在设置中检查"
        );
    }

    #[test]
    fn parse_403_non_json_maps_to_protection_message() {
        // Cloudflare 挑战页（HTML）
        assert_eq!(
            parse_response(403, "<html>Just a moment...</html>").unwrap_err(),
            "请求被 SauceNAO 的防护拦截，请稍后重试"
        );
    }

    #[test]
    fn parse_429_and_unexpected_status() {
        assert_eq!(
            parse_response(429, "rate limited").unwrap_err(),
            "SauceNAO 请求过于频繁，请稍后再试"
        );
        assert_eq!(
            parse_response(502, "bad gateway").unwrap_err(),
            "SauceNAO 返回异常状态 502"
        );
        assert_eq!(
            parse_response(500, "").unwrap_err(),
            "SauceNAO 返回异常状态 500"
        );
    }

    #[test]
    fn parse_non_json_body_on_200_maps_to_protection_message() {
        // 实测：output_type=1 返回 200 纯文本 —— 非 JSON body 一律按防护归类
        assert_eq!(
            parse_response(200, "XML not yet implemented.").unwrap_err(),
            "请求被 SauceNAO 的防护拦截，请稍后重试"
        );
    }

    #[test]
    fn parse_header_status_not_zero_passes_message_through() {
        let body =
            r#"{"header":{"status":-3,"message":"Search failed: image too small"},"results":[]}"#;
        assert_eq!(
            parse_response(200, body).unwrap_err(),
            "SauceNAO 返回错误：Search failed: image too small"
        );
        // message 缺失 → 兜底文案
        let body = r#"{"header":{"status":-2},"results":[]}"#;
        assert_eq!(
            parse_response(200, body).unwrap_err(),
            "SauceNAO 返回错误：未知错误"
        );
    }

    #[test]
    fn parse_missing_header_yields_null_quota_and_empty_results() {
        let resp = parse_response(200, r#"{"results":[]}"#).unwrap();
        assert!(resp.results.is_empty());
        assert_eq!(resp.quota, None, "无顶层 header → quota=null");
    }

    #[test]
    fn ext_url_fallback_chain_rules() {
        // 旧式 member_illust.php（2026 实测主形态）
        assert_eq!(
            extract_pixiv_id(&[
                "https://www.pixiv.net/member_illust.php?mode=medium&illust_id=76100412"
            ]),
            Some(76100412)
        );
        // 新式 /artworks/ 兜底（含语言前缀变体）
        assert_eq!(
            extract_pixiv_id(&["https://www.pixiv.net/en/artworks/8868617"]),
            Some(8868617)
        );
        // 旧式优先于新式（正则顺序 = 回退链顺序）
        assert_eq!(
            extract_pixiv_id(&[
                "https://www.pixiv.net/artworks/111",
                "https://www.pixiv.net/member_illust.php?illust_id=222",
            ]),
            Some(222)
        );
        // 非 pixiv URL 提取不到
        assert_eq!(
            extract_pixiv_id(&["https://deviantart.com/view/515715132"]),
            None
        );
        assert_eq!(extract_pixiv_id(&[]), None);
        // member_id 回退
        assert_eq!(
            extract_member_id(&["https://www.pixiv.net/member.php?id=17473379"]),
            Some(17473379)
        );
        assert_eq!(
            extract_member_id(&["https://www.pixiv.net/artworks/1"]),
            None
        );
    }

    #[test]
    fn similarity_parses_string_and_falls_back_to_zero() {
        let h = serde_json::json!({"similarity": "93.5"});
        assert_eq!(parse_similarity(&h), 93.5);
        // 非数字字符串 → 0.0
        let h = serde_json::json!({"similarity": "n/a"});
        assert_eq!(parse_similarity(&h), 0.0);
        // 数字字段（防御类型漂移）→ 原值
        let h = serde_json::json!({"similarity": 77.2});
        assert_eq!(parse_similarity(&h), 77.2);
        // 缺失 → 0.0
        assert_eq!(parse_similarity(&serde_json::json!({})), 0.0);
    }

    #[test]
    fn multipart_body_shape() {
        let body = build_multipart_body("upload.png", "image/png", b"PNGDATA", "b1");
        let text = String::from_utf8(body).unwrap();
        assert!(text.starts_with("--b1\r\n"), "开头 = boundary 分隔行");
        assert!(
            text.contains("Content-Disposition: form-data; name=\"file\"; filename=\"upload.png\""),
            "字段名恒为 file: {text}"
        );
        assert!(text.contains("Content-Type: image/png"));
        assert!(text.contains("PNGDATA"));
        assert!(text.ends_with("\r\n--b1--\r\n"), "结尾 = 结束分隔行");
    }

    #[test]
    fn precheck_file_rules() {
        let dir = std::env::temp_dir().join(format!(
            "pixiv-tool-saucenao-precheck-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        // 合法 png
        let png = dir.join("a.PNG");
        std::fs::write(&png, b"x").unwrap();
        assert_eq!(
            precheck_file(png.to_str().unwrap()).unwrap(),
            "png",
            "扩展名小写化"
        );

        // 不存在
        let err = precheck_file(dir.join("missing.png").to_str().unwrap()).unwrap_err();
        assert!(err.contains("不存在"), "got {err}");

        // 扩展名白名单外
        let txt = dir.join("b.txt");
        std::fs::write(&txt, b"x").unwrap();
        let err = precheck_file(txt.to_str().unwrap()).unwrap_err();
        assert!(err.contains("不支持的图片格式"), "got {err}");
        // 无扩展名
        let noext = dir.join("c");
        std::fs::write(&noext, b"x").unwrap();
        let err = precheck_file(noext.to_str().unwrap()).unwrap_err();
        assert!(err.contains("不支持的图片格式"), "got {err}");

        // 超过 10MB（稀疏文件 set_len，不实际写盘）
        let big = dir.join("d.png");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_FILE_BYTES + 1).unwrap();
        drop(f);
        let err = precheck_file(big.to_str().unwrap()).unwrap_err();
        assert!(err.contains("10MB"), "got {err}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn percent_encode_rfc3986_unreserved() {
        assert_eq!(percent_encode("abc-_.~123"), "abc-_.~123");
        assert_eq!(
            percent_encode("https://example.com/a.jpg?a=1&b=2"),
            "https%3A%2F%2Fexample.com%2Fa.jpg%3Fa%3D1%26b%3D2"
        );
        // 非 ASCII 按 UTF-8 字节编码
        assert_eq!(percent_encode("图"), "%E5%9B%BE");
    }

    #[test]
    fn sanitize_error_text_strips_query() {
        // 模拟 wreq 错误内嵌带 api_key 的 URL：剥掉 '?' 之后
        let raw = "error sending request for url (https://saucenao.com/search.php?api_key=SECRET)";
        assert_eq!(
            sanitize_error_text(raw),
            "error sending request for url (https://saucenao.com/search.php"
        );
        assert!(!sanitize_error_text(raw).contains("SECRET"));
        // 无 query 的错误原样
        assert_eq!(
            sanitize_error_text("operation timed out"),
            "operation timed out"
        );
    }

    #[test]
    fn boundary_is_random_and_unique() {
        let a = new_boundary();
        let b = new_boundary();
        assert_ne!(a, b, "随机 boundary 不应重复");
        assert!(a.starts_with("pixivtool"));
        assert!(a.len() <= 70, "RFC 2046 boundary 上限 70 字符");
    }
}
