//! `pixiv-img` 自定义协议：pximg CDN 图片代理 + 磁盘缓存。
//!
//! 前端用法：`convertFileSrc(encodeURIComponent(pximgUrl), "pixiv-img")`。
//! - Windows（WebView2）：`http://pixiv-img.localhost/https%3A%2F%2Fi.pximg.net%2F...`
//! - macOS / Linux：`pixiv-img://localhost/https://i.pximg.net/...`
//!
//! 协议处理器（lib.rs 注册）把请求转交 [`handle_image_request`]：
//! 路径 percent-decode（兼容编码与未编码两种形态）→ pximg 白名单校验 →
//! 磁盘缓存（`<data>/cache/img/`）命中直接回读；未命中则经 PixivClient
//! 无 cookie 代下（`send_download` 自带 Referer 过防盗链）→ 落盘缓存 →
//! 回传字节（Content-Type 按扩展名，Cache-Control 一天）。
//!
//! 设计约束：
//! - 本模块不依赖 AppState：缓存目录由调用方传入，纯逻辑均可离线单测；
//! - CDN 下载不受 ajax 限速器约束（每次请求各建 client，互不排队），
//!   用静态 Semaphore 把全局并发放宽到 6；
//! - 日志纪律：不打印任何 cookie；pximg URL 只在 debug 级出现，
//!   warn/info 只记状态、计数与缓存路径。

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use tauri::http::{Request, Response};

use crate::pixiv::client::{PixivClient, PixivError, mask_url};

/// CDN 图片并发上限（独立于 ajax 限速，browse 页面网格加载的合理水位）。
const CDN_MAX_CONCURRENT_DOWNLOADS: usize = 6;
/// 单张图片下载整体超时（秒），覆盖 client 内部的重试与退避。
const DOWNLOAD_TIMEOUT_SECS: u64 = 15;
/// 图片磁盘缓存总大小上限（字节）：1GB，超出按 mtime 从旧到新清理。
const IMAGE_CACHE_MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// 全局 CDN 下载闸门（进程级，与具体 client 无关）。
static CDN_GATE: LazyLock<tokio::sync::Semaphore> =
    LazyLock::new(|| tokio::sync::Semaphore::new(CDN_MAX_CONCURRENT_DOWNLOADS));

// ----------------------------------------------------------------------
// SHA-256（FIPS 180-4）
// ----------------------------------------------------------------------
// 依赖清单锁定期内不引入 sha2 crate，本地实现并用官方测试向量锁定行为，
// 仅用于缓存键（输入为短 URL），不承载安全敏感场景。
mod sha256 {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    const H0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    /// 摘要的小写 hex（64 字符）。
    pub fn hex(data: &[u8]) -> String {
        let mut h = H0;
        let bit_len = (data.len() as u64) * 8;
        let mut msg = Vec::with_capacity(data.len() + 72);
        msg.extend_from_slice(data);
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bit_len.to_be_bytes());

        let mut w = [0u32; 64];
        for block in msg.chunks_exact(64) {
            for (i, word) in w.iter_mut().take(16).enumerate() {
                *word = u32::from_be_bytes([
                    block[4 * i],
                    block[4 * i + 1],
                    block[4 * i + 2],
                    block[4 * i + 3],
                ]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }

            let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h7] = h;
            for (i, k) in K.iter().enumerate() {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ ((!e) & g);
                let t1 = h7
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(*k)
                    .wrapping_add(w[i]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let t2 = s0.wrapping_add(maj);
                h7 = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }
            for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, h7]) {
                *slot = slot.wrapping_add(value);
            }
        }

        let mut out = String::with_capacity(64);
        for word in h {
            out.push_str(&format!("{word:08x}"));
        }
        out
    }
}

// ----------------------------------------------------------------------
// 纯函数：路径解析 / 白名单 / 缓存键
// ----------------------------------------------------------------------

/// percent-decode（`%XX` → 字节；不做表单 `+`→空格转换）。
/// 非法转义（截断或非 hex）或结果非 UTF-8 → None（pximg URL 恒为 ASCII）。
fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3)?;
                let hi = (hex[0] as char).to_digit(16)?;
                let lo = (hex[1] as char).to_digit(16)?;
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// pximg 白名单：仅允许 `https` 且 host 以 `.pximg.net` 结尾
/// （i.pximg.net、s.pximg.net 等子域），其余一律拒绝（403 语义）。
///
/// 手写解析只取 host 做判断，足以挡住：非 https、裸 pximg.net、
/// `*.pximg.net.evil.com`、userinfo 诱导（`i.pximg.net@evil.com`）、
/// 反斜杠/空白/控制字符等形态。
fn is_allowed_pximg_url(url: &str) -> bool {
    // 字符集闸门：仅 ASCII 可打印且不含反斜杠（URL 解析分歧面收窄）
    if !url.is_ascii() || url.bytes().any(|b| b < 0x21 || b == 0x7f || b == b'\\') {
        return false;
    }
    if url.len() < 8 || !url[..8].eq_ignore_ascii_case("https://") {
        return false;
    }
    let rest = &url[8..];
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // userinfo 诱导：真实 host 在最后一个 @ 之后
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    // 剥端口（IPv6 字面量含 `:` 必然匹配不上白名单，无需特判）
    let host = host_port
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    host.ends_with(".pximg.net")
}

/// 协议请求路径 → 目标 pximg URL。兼容两种形态：
/// - Windows 编码态：`/https%3A%2F%2Fi.pximg.net%2F...`（decode 后命中）
/// - macOS/Linux 未编码态：`/https://i.pximg.net/...`（原样命中）
///
/// 解析失败或白名单外 → None（调用方回 403）。
fn path_to_pximg_url(path: &str) -> Option<String> {
    let raw = path.trim_start_matches('/');
    if raw.is_empty() {
        return None;
    }
    if let Some(decoded) = percent_decode(raw) {
        if is_allowed_pximg_url(&decoded) {
            return Some(decoded);
        }
    }
    if is_allowed_pximg_url(raw) {
        return Some(raw.to_string());
    }
    None
}

/// 缓存文件扩展名：取 URL 末段最后一个 `.` 之后（查询串/锚点剥离），
/// 1..=8 位字母数字才可信（小写化），否则默认 jpg。zip 亦在规则内
/// （为 ugoira 预留）。
fn url_extension(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last_segment = path.rsplit('/').next().unwrap_or_default();
    match last_segment.rsplit_once('.') {
        Some((_, ext))
            if !ext.is_empty()
                && ext.len() <= 8
                && ext.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            ext.to_ascii_lowercase()
        }
        _ => "jpg".to_string(),
    }
}

/// 缓存键：完整 URL 的 SHA-256 hex + 原扩展名（`<64hex>.<ext>`）。
fn cache_key(url: &str) -> String {
    format!("{}.{}", sha256::hex(url.as_bytes()), url_extension(url))
}

/// 扩展名 → Content-Type。
fn mime_for_extension(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

fn mime_for_filename(filename: &str) -> &'static str {
    mime_for_extension(filename.rsplit('.').next().unwrap_or_default())
}

// ----------------------------------------------------------------------
// 磁盘缓存清理
// ----------------------------------------------------------------------

struct CacheEntry {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

fn collect_cache_entries(dir: &Path) -> std::io::Result<Vec<CacheEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        entries.push(CacheEntry {
            path: entry.path(),
            size: metadata.len(),
            // mtime 缺失按"最新"处理（宁可少删）
            modified: metadata.modified().unwrap_or_else(|_| SystemTime::now()),
        });
    }
    Ok(entries)
}

/// 缓存总量超上限时按 mtime 从旧到新删除直至低于上限；
/// `keep`（本次刚写的文件）永不被删。失败只记日志，不向上传播。
pub fn trim_cache(dir: &Path, keep: &Path, max_bytes: u64) {
    let entries = match collect_cache_entries(dir) {
        Ok(entries) => entries,
        Err(err) => {
            log::debug!("图片缓存清理跳过（目录不可读）: {err}");
            return;
        }
    };
    let mut total: u64 = entries.iter().map(|e| e.size).sum();
    if total <= max_bytes {
        return;
    }
    let mut victims: Vec<&CacheEntry> = entries.iter().filter(|e| e.path != keep).collect();
    victims.sort_by_key(|e| e.modified);
    let mut removed = 0usize;
    for victim in victims {
        if total <= max_bytes {
            break;
        }
        match std::fs::remove_file(&victim.path) {
            Ok(()) => {
                total = total.saturating_sub(victim.size);
                removed += 1;
            }
            Err(err) => log::debug!("缓存清理删除失败 {}: {err}", victim.path.display()),
        }
    }
    log::info!("图片缓存清理：删除 {removed} 个文件，剩余 {total} / 上限 {max_bytes} 字节");
}

// ----------------------------------------------------------------------
// 下载与请求处理
// ----------------------------------------------------------------------

/// 无 cookie 构造 PixivClient 走 CDN 下载（`send_download` 自带
/// `Referer: https://www.pixiv.net/` 过 pximg 防盗链）。每次调用各建
/// client：互不共享 ajax 限速闸门，并发由静态 CDN_GATE 收敛到 6。
/// CDN 偶发抖动实测约 5%（列表并发加载时），失败自动重试一次。
async fn download_via_cdn(url: &str) -> Result<Vec<u8>, PixivError> {
    let client = PixivClient::new(&HashMap::new())
        .map_err(|err| PixivError::Network(format!("构造 HTTP 客户端失败: {err}")))?;
    let _permit = CDN_GATE
        .acquire()
        .await
        .map_err(|_| PixivError::Network("CDN 并发闸门已关闭".into()))?;
    let attempt = || async {
        tokio::time::timeout(
            Duration::from_secs(DOWNLOAD_TIMEOUT_SECS),
            client.download_bytes(url),
        )
        .await
        .unwrap_or_else(|_| Err(PixivError::Network("图片下载超时".into())))
    };
    let first = attempt().await;
    if first.is_ok() {
        return first;
    }
    log::debug!("图片下载失败将重试一次: {}", mask_url(url));
    attempt().await
}

async fn write_cache_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, bytes).await
}

/// 协议请求主入口：解析 → 校验 → 缓存回读 / 代下落盘 → 响应。
/// 状态码语义：403 白名单外；404 CDN 无此文件；502 下载失败。
pub async fn handle_image_request(
    request: Request<Vec<u8>>,
    cache_dir: &Path,
) -> Response<Cow<'static, [u8]>> {
    let Some(url) = path_to_pximg_url(request.uri().path()) else {
        log::debug!("pixiv-img 拒绝路径: {}", request.uri().path());
        log::warn!("pixiv-img 403：目标不在 pximg 白名单");
        return error_response(403);
    };
    log::debug!("pixiv-img 目标: {url}");

    let key = cache_key(&url);
    let cache_path = cache_dir.join(&key);
    match tokio::fs::read(&cache_path).await {
        Ok(bytes) => {
            log::info!("图片缓存命中: {}", cache_path.display());
            return image_response(&key, bytes);
        }
        // 未命中 → 回源；其余读取失败也回源（自愈，不阻塞用户请求）
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            log::warn!("图片缓存读取失败（回源）: {err}");
        }
        Err(_) => {}
    }

    match download_via_cdn(&url).await {
        Ok(bytes) => {
            if let Err(err) = write_cache_file(&cache_path, &bytes).await {
                log::warn!("图片缓存写入失败（仍返回字节）: {err}");
            } else {
                trim_cache(cache_dir, &cache_path, IMAGE_CACHE_MAX_BYTES);
            }
            log::info!("图片已缓存: {}", cache_path.display());
            image_response(&key, bytes)
        }
        Err(err @ PixivError::NotFound) => {
            log::debug!("pixiv-img 404 详情: {err}");
            log::warn!("pixiv-img 404：CDN 无此文件");
            error_response(404)
        }
        Err(err) => {
            log::debug!("pixiv-img 下载失败详情: {err}");
            log::warn!("pixiv-img 502：图片下载失败");
            error_response(502)
        }
    }
}

fn image_response(filename: &str, bytes: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(200)
        .header("Content-Type", mime_for_filename(filename))
        .header("Cache-Control", "public, max-age=86400")
        .body(Cow::Owned(bytes))
        .expect("静态成功响应构造不会失败")
}

fn error_response(status: u16) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .header("Cache-Control", "no-store")
        .body(Cow::Borrowed(&[][..]))
        .expect("静态错误响应构造不会失败")
}

// ----------------------------------------------------------------------
// 单元测试（全部离线，不发真实网络）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pixiv-img-test-{tag}-{}", uuid::Uuid::new_v4()))
    }

    fn request_for(path: &str) -> Request<Vec<u8>> {
        Request::builder()
            .uri(format!("http://pixiv-img.localhost{path}"))
            .body(Vec::new())
            .expect("测试请求构造不会失败")
    }

    fn write_with_mtime(path: &Path, size: usize, secs_ago: u64) {
        std::fs::write(path, vec![0u8; size]).expect("测试文件写入不应失败");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("测试文件重开不应失败");
        file.set_modified(SystemTime::now() - Duration::from_secs(secs_ago))
            .expect("设置 mtime 不应失败");
    }

    #[test]
    fn sha256_matches_official_vectors() {
        // FIPS 180-4 / NIST 标准向量（覆盖空输入、单块、55/56 字节填充边界）
        assert_eq!(
            sha256::hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256::hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256::hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            sha256::hex(
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
            ),
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
        );
    }

    #[test]
    fn percent_decode_rules() {
        assert_eq!(
            percent_decode("https%3A%2F%2Fi.pximg.net%2Fa%2Fb.jpg").as_deref(),
            Some("https://i.pximg.net/a/b.jpg")
        );
        // 无转义原样通过
        assert_eq!(
            percent_decode("https://i.pximg.net/a.jpg").as_deref(),
            Some("https://i.pximg.net/a.jpg")
        );
        // 非法转义与截断
        assert_eq!(percent_decode("%ZZ"), None);
        assert_eq!(percent_decode("%2"), None);
        assert_eq!(percent_decode("abc%"), None);
    }

    #[test]
    fn whitelist_accepts_pximg_subdomains() {
        for url in [
            "https://i.pximg.net/img-master/img/2024/01/01/00/00/00/1_p0.jpg",
            "https://s.pximg.net/common/images/limit_unknown_360.png",
            "https://embed.pximg.net/a.zip",
            "https://I.PXIMG.NET/a.jpg",     // host 大小写不敏感
            "https://i.pximg.net:443/a.jpg", // 剥端口
        ] {
            assert!(is_allowed_pximg_url(url), "应放行 {url}");
        }
    }

    #[test]
    fn whitelist_rejects_everything_else() {
        for url in [
            "http://i.pximg.net/a.jpg",             // 非 https
            "https://pximg.net/a.jpg",              // 裸域不放行
            "https://i.pximg.net.evil.com/a.jpg",   // 后缀伪装
            "https://evil.com/i.pximg.net/a.jpg",   // 路径伪装
            "https://i.pximg.net@evil.com/a.jpg",   // userinfo 诱导
            "https://i.pximg.net\\@evil.com/a.jpg", // 反斜杠歧义
            "ftp://i.pximg.net/a.jpg",              // 其他 scheme
            "https://i.pximg.net/a b.jpg",          // 空白
            "",                                     // 空
            "https://i.pximg.net/a\x01.jpg",        // 控制字符
        ] {
            assert!(!is_allowed_pximg_url(url), "应拒绝 {url:?}");
        }
    }

    #[test]
    fn path_to_pximg_url_handles_encoded_and_raw() {
        // Windows 编码态
        assert_eq!(
            path_to_pximg_url("/https%3A%2F%2Fi.pximg.net%2Fimg%2Fa.jpg").as_deref(),
            Some("https://i.pximg.net/img/a.jpg")
        );
        // macOS/Linux 未编码态
        assert_eq!(
            path_to_pximg_url("/https://i.pximg.net/img/a.jpg").as_deref(),
            Some("https://i.pximg.net/img/a.jpg")
        );
        // 白名单外 / 空路径 / 解析不出 URL
        assert_eq!(
            path_to_pximg_url("/https%3A%2F%2Fevil.example.com%2Fa.jpg"),
            None
        );
        assert_eq!(path_to_pximg_url("/"), None);
        assert_eq!(path_to_pximg_url(""), None);
    }

    #[test]
    fn url_extension_rules() {
        assert_eq!(url_extension("https://i.pximg.net/a/b_c_50.jpg"), "jpg");
        assert_eq!(url_extension("https://i.pximg.net/a/b.JPG"), "jpg"); // 小写化
        assert_eq!(url_extension("https://i.pximg.net/a/ugoira.zip"), "zip");
        assert_eq!(url_extension("https://i.pximg.net/a/b.webp?x=1"), "webp"); // 剥查询串
        assert_eq!(url_extension("https://i.pximg.net/a/noext"), "jpg"); // 取不到默认
        assert_eq!(url_extension("https://i.pximg.net/a/"), "jpg"); // 空末段
    }

    #[test]
    fn cache_key_is_hex_plus_extension() {
        let key = cache_key("https://i.pximg.net/a/b.jpg");
        let (hex, ext) = key.rsplit_once('.').expect("键必含扩展名");
        assert_eq!(ext, "jpg");
        assert_eq!(hex.len(), 64);
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
        // URL 变化 → 键变化；同 URL → 键稳定
        assert_ne!(key, cache_key("https://i.pximg.net/a/b.png"));
        assert_eq!(key, cache_key("https://i.pximg.net/a/b.jpg"));
        // zip 预留
        assert!(cache_key("https://i.pximg.net/a.zip").ends_with(".zip"));
    }

    #[test]
    fn mime_table_covers_common_types() {
        assert_eq!(mime_for_filename("abc.jpg"), "image/jpeg");
        assert_eq!(mime_for_filename("abc.jpeg"), "image/jpeg");
        assert_eq!(mime_for_filename("abc.png"), "image/png");
        assert_eq!(mime_for_filename("abc.webp"), "image/webp");
        assert_eq!(mime_for_filename("abc.gif"), "image/gif");
        assert_eq!(mime_for_filename("abc.zip"), "application/zip");
        assert_eq!(mime_for_filename("abc.weird"), "application/octet-stream");
    }

    #[test]
    fn trim_cache_deletes_oldest_first_and_skips_keep() {
        let dir = temp_dir("trim");
        std::fs::create_dir_all(&dir).expect("临时目录创建不应失败");
        let keep = dir.join("keep.jpg");
        let old = dir.join("old.jpg");
        let mid = dir.join("mid.jpg");
        write_with_mtime(&keep, 900, 300); // 最旧，但必须保留
        write_with_mtime(&old, 200, 200);
        write_with_mtime(&mid, 300, 100);

        // 总量 1400 > 1000：keep 被跳过，先删 old(200)→1200 仍超，再删 mid(300)→900
        trim_cache(&dir, &keep, 1000);
        assert!(keep.exists(), "本次刚写的文件不应被删");
        assert!(!old.exists());
        assert!(!mid.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn trim_cache_noop_under_limit() {
        let dir = temp_dir("trim-noop");
        std::fs::create_dir_all(&dir).expect("临时目录创建不应失败");
        let a = dir.join("a.jpg");
        let b = dir.join("b.jpg");
        write_with_mtime(&a, 100, 300);
        write_with_mtime(&b, 100, 100);
        trim_cache(&dir, &b, 1000);
        assert!(a.exists() && b.exists(), "未超上限不应删除任何文件");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn handle_rejects_non_whitelist_with_403() {
        let response = handle_image_request(
            request_for("/https%3A%2F%2Fevil.example.com%2Fa.jpg"),
            Path::new("unused"),
        )
        .await;
        assert_eq!(response.status(), 403);
        assert_eq!(response.headers()["Cache-Control"], "no-store");
    }

    #[tokio::test]
    async fn handle_serves_from_cache_without_network() {
        let url = "https://i.pximg.net/img/a.jpg";
        let dir = temp_dir("hit");
        std::fs::create_dir_all(&dir).expect("临时目录创建不应失败");
        std::fs::write(dir.join(cache_key(url)), b"JPEGBYTES").expect("缓存桩文件写入不应失败");

        let response = handle_image_request(
            request_for("/https%3A%2F%2Fi.pximg.net%2Fimg%2Fa.jpg"),
            &dir,
        )
        .await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["Content-Type"], "image/jpeg");
        assert_eq!(response.headers()["Cache-Control"], "public, max-age=86400");
        assert_eq!(response.body().as_ref(), b"JPEGBYTES".as_slice());
        std::fs::remove_dir_all(&dir).ok();
    }
}
