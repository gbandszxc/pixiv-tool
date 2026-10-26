//! ugoira（动图）帧管线：元数据解析 → zip 下载解包 → 自定义协议 URL。
//!
//! 数据流：
//! 1. `/ajax/illust/{id}/ugoira_meta`（需登录态）返回**平铺**结构：
//!    `src`（600×600 预览 zip）、`originalSrc`（1920×1080 zip）、
//!    `frames: [{file, delay}]`（delay 为毫秒）；
//! 2. 取 zip URL（`src` 优先，体积远小于 originalSrc；缺失回退
//!    `originalSrc`，再回退 `zip_urls.original`）+ 帧列表；
//! 3. 缓存目录 `<cache_img_root>/ugoira/<id>/`：命中（manifest 的 zip URL 一致
//!    且帧文件齐全）直接复用，否则下载 zip、按 meta 白名单解包、原子写 manifest；
//! 4. 返回 `UgoiraFrames`，`src` 为 `pixiv-ugoira` 自定义协议 URL，前端按顺序
//!    轮播并按 `delay` 计时。
//!
//! 设计约束：
//! - 本模块不依赖 AppState：缓存根目录由调用方传入，纯逻辑（解析 / 帧名净化 /
//!   URL 构造 / 解包）全部可离线单测；
//! - 解包只写 meta frames 白名单里的条目，且条目名必须是**单段文件名**
//!   （拒绝 `../x`、`a/b`、盘符、控制字符），杜绝路径穿越写出目录外；
//! - 目录维护只在新解包后触发（刚写入的目录 mtime 最新，不会被自己清掉）；
//!   失败只 `log::warn`，不影响本次播放；
//! - 日志纪律：不打印 cookie / token；zip URL 可能带签名参数，只在 debug 级出现。

use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pixiv::api::PixivApi;

/// 自定义协议名（`lib.rs` 注册；Windows 侧实际 URL 形态为
/// `http://pixiv-ugoira.localhost/...`，见 [`frame_scheme_url`]）。
pub const FRAME_SCHEME: &str = "pixiv-ugoira";
/// frames 条目缺 `delay`（或非法）时的兜底帧间隔（毫秒）。
const DEFAULT_FRAME_DELAY_MS: i64 = 100;
/// `<cache_img_root>/ugoira/` 下最多保留的作品目录数（按 mtime 最新保留）。
const MAX_CACHED_ARTWORKS: usize = 12;
/// 缓存清单文件名（相对单个作品目录）。
const MANIFEST_FILE: &str = "manifest.json";
/// 清单临时文件名（写完后 rename 覆盖正式文件，避免半截 JSON）。
const MANIFEST_TMP_FILE: &str = "manifest.json.tmp";

/// 单帧：`src` 为可直接给 `<img src>` 的协议 URL，`delay` 为停留毫秒数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UgoiraFrame {
    pub src: String,
    pub delay: i64,
}

/// 一次播放所需的完整帧序列。
///
/// `width` / `height` 恒为 `None`：zip 内帧图尺寸由前端按 `naturalWidth`
/// 读取，后端不做图片解码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UgoiraFrames {
    pub frames: Vec<UgoiraFrame>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

/// 缓存清单条目（与 meta frames 一一对应）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestFrame {
    file: String,
    delay: i64,
}

/// 缓存清单：`{"zip": url, "frames": [{file, delay}]}`。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    zip: String,
    frames: Vec<ManifestFrame>,
}

// ----------------------------------------------------------------------
// 纯函数：解析 / 帧名净化 / URL 构造 / 目录
// ----------------------------------------------------------------------

/// `/ajax/illust/{id}/ugoira_meta` body → (zip URL, frames)。
///
/// URL 优先级：`src` → `originalSrc` → `zip_urls.original`（空串视同缺失）。
/// frames 逐项取 `file` + `delay`（`delay` 缺失/非正数按 100ms 兜底）；
/// `frames` 不是数组则返回空表。无可用 zip URL → None。
///
/// 注意：本函数不做帧名净化（净化在 [`ensure_frames`] 写入前统一做），
/// 因此空 `file` 也原样返回，由调用方跳过。
pub(crate) fn parse_ugoira_playback(body: &Value) -> Option<(String, Vec<(String, i64)>)> {
    let pick = |value: Option<&Value>| -> Option<String> {
        value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let zip_url = pick(body.get("src"))
        .or_else(|| pick(body.get("originalSrc")))
        .or_else(|| pick(body.pointer("/zip_urls/original")))?;

    let frames = body
        .get("frames")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let file = item.get("file").and_then(Value::as_str)?.to_string();
                    let delay = item
                        .get("delay")
                        .and_then(Value::as_i64)
                        .filter(|delay| *delay > 0)
                        .unwrap_or(DEFAULT_FRAME_DELAY_MS);
                    Some((file, delay))
                })
                .collect::<Vec<(String, i64)>>()
        })
        .unwrap_or_default();

    Some((zip_url, frames))
}

/// 帧文件名净化：只接受**单段文件名**，否则 None。
///
/// 拒绝：空串、首尾空白、含 `..`、含路径分隔符或 Windows 非法字符
/// （`/ \ : * ? " < > |`）、控制字符，以及任何非 Normal 的路径组件
/// （盘符前缀、根、`.`/`..`）。pixiv 帧名形如 `000000.jpg`，正常用例全部通过。
pub(crate) fn sanitize_frame_name(name: &str) -> Option<String> {
    if name.is_empty() || name.trim() != name {
        return None;
    }
    if name.contains("..") {
        return None;
    }
    if name.chars().any(|c| {
        c.is_control()
            || matches!(
                c,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            )
    }) {
        return None;
    }
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Some(name.to_string()),
        _ => None,
    }
}

/// 帧图的 `pixiv-ugoira` 协议 URL（形态对齐头像协议 `pixiv-avatar`）。
///
/// Windows（WebView2）自定义协议走 `http://<scheme>.localhost/` 前缀拦截；
/// 其它平台直接注册 scheme。
pub(crate) fn frame_scheme_url(artwork_id: i64, file: &str) -> String {
    if cfg!(windows) {
        format!("http://{FRAME_SCHEME}.localhost/{artwork_id}/{file}")
    } else {
        format!("{FRAME_SCHEME}://localhost/{artwork_id}/{file}")
    }
}

/// 单个作品的帧缓存目录：`<cache_img_root>/ugoira/<id>`。
pub(crate) fn frames_dir(cache_img_root: &Path, artwork_id: i64) -> PathBuf {
    cache_img_root.join("ugoira").join(artwork_id.to_string())
}

// ----------------------------------------------------------------------
// 解包（同步，离线可测；调用方放 spawn_blocking）
// ----------------------------------------------------------------------

/// 把 zip 里的白名单帧写到 `dir`。
///
/// 约束：
/// - 先清空 `dir`（旧 zip 的残留帧不能与新 meta 混用），再建目录；
/// - 只写出 `wanted` 里出现的文件名，且 zip 条目名必须本身是合法单段名
///   （`sub/x.jpg`、`../x.jpg` 一律跳过，记 warn）——即使 basename 命中白名单；
/// - `wanted` 里任何一个文件在 zip 内缺失 → Err（前端会拿到中文错误，而不是
///   一半帧的破图）。
pub(crate) fn extract_frames(
    bytes: &[u8],
    dir: &Path,
    wanted: &[(String, i64)],
) -> Result<(), String> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(format!("清理动图缓存目录失败: {err}")),
    }
    std::fs::create_dir_all(dir).map_err(|err| format!("创建动图缓存目录失败: {err}"))?;

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|err| format!("动图 zip 解包失败: {err}"))?;
    let allowed: HashSet<&str> = wanted.iter().map(|(file, _)| file.as_str()).collect();
    let mut written: HashSet<String> = HashSet::with_capacity(allowed.len());

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("读取动图 zip 条目失败: {err}"))?;
        if entry.is_dir() {
            continue;
        }
        let raw = entry.name().to_string();
        let Some(name) = sanitize_frame_name(&raw) else {
            // 用 Debug 形态记日志：zip 条目名不可信，避免控制字符污染终端。
            log::warn!("动图 zip 跳过可疑条目（非单段文件名）: {raw:?}");
            continue;
        };
        if !allowed.contains(name.as_str()) {
            continue;
        }
        // 不信任 zip 声明的解压后大小（恶意/损坏 zip 可借此撑爆内存）：
        // 预分配上限 8 MiB，超出部分按需增长。
        let mut buf = Vec::with_capacity((entry.size() as usize).min(8 * 1024 * 1024));
        entry
            .read_to_end(&mut buf)
            .map_err(|err| format!("读取动图帧数据失败（{name}）: {err}"))?;
        std::fs::write(dir.join(&name), &buf)
            .map_err(|err| format!("写入动图帧失败（{name}）: {err}"))?;
        written.insert(name);
    }

    let missing: Vec<&str> = wanted
        .iter()
        .map(|(file, _)| file.as_str())
        .filter(|file| !written.contains(*file))
        .collect();
    if !missing.is_empty() {
        return Err(format!("动图 zip 缺少帧文件: {}", missing.join(", ")));
    }
    Ok(())
}

// ----------------------------------------------------------------------
// 缓存清单 / 目录维护
// ----------------------------------------------------------------------

/// 缓存是否可复用：manifest 的 zip URL 与本次一致，且 `wanted` 里每个帧文件
/// 都已在磁盘上（manifest 自身记录也需覆盖这些文件）。
async fn manifest_matches(dir: &Path, zip_url: &str, wanted: &[(String, i64)]) -> bool {
    let Ok(bytes) = tokio::fs::read(dir.join(MANIFEST_FILE)).await else {
        return false;
    };
    let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes) else {
        log::warn!("动图缓存清单损坏，将重新解包: {}", dir.display());
        return false;
    };
    if manifest.zip != zip_url {
        return false;
    }
    let recorded: HashSet<&str> = manifest.frames.iter().map(|f| f.file.as_str()).collect();
    for (file, _) in wanted {
        if !recorded.contains(file.as_str()) {
            return false;
        }
        if !tokio::fs::try_exists(dir.join(file)).await.unwrap_or(false) {
            return false;
        }
    }
    true
}

/// 原子写清单：先写 `.tmp` 再 rename（rename 覆盖同目录文件，Windows 上
/// 目标已存在时 `std::fs::rename` 亦可覆盖）。
async fn write_manifest(dir: &Path, zip_url: &str, frames: &[(String, i64)]) -> Result<(), String> {
    let manifest = Manifest {
        zip: zip_url.to_string(),
        frames: frames
            .iter()
            .map(|(file, delay)| ManifestFrame {
                file: file.clone(),
                delay: *delay,
            })
            .collect(),
    };
    let json =
        serde_json::to_vec(&manifest).map_err(|err| format!("动图缓存清单序列化失败: {err}"))?;
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|err| format!("创建动图缓存目录失败: {err}"))?;
    let tmp = dir.join(MANIFEST_TMP_FILE);
    tokio::fs::write(&tmp, &json)
        .await
        .map_err(|err| format!("写入动图缓存清单失败: {err}"))?;
    tokio::fs::rename(&tmp, dir.join(MANIFEST_FILE))
        .await
        .map_err(|err| format!("提交动图缓存清单失败: {err}"))
}

/// 目录维护：`<cache_img_root>/ugoira/` 下最多保留 12 个作品目录（按目录
/// mtime 从新到旧），其余 `remove_dir_all`。任何失败只 warn。
///
/// 只在新解包成功后调用：刚写入的目录 mtime 最新，不会被自己清掉；缓存命中
/// 时不调用，避免把正在播放的旧目录按 mtime 误删。
async fn prune_cached_artworks(cache_img_root: &Path) {
    let root = cache_img_root.join("ugoira");
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(entries) => entries,
        Err(err) => {
            log::warn!("动图缓存目录扫描失败 {}: {err}", root.display());
            return;
        }
    };
    let mut dirs: Vec<(PathBuf, SystemTime)> = Vec::new();
    loop {
        match entries.next_entry().await {
            Ok(Some(entry)) => {
                let path = entry.path();
                let Ok(meta) = entry.metadata().await else {
                    continue;
                };
                if !meta.is_dir() {
                    continue;
                }
                dirs.push((path, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
            }
            Ok(None) => break,
            Err(err) => {
                log::warn!("动图缓存目录扫描失败 {}: {err}", root.display());
                return;
            }
        }
    }
    if dirs.len() <= MAX_CACHED_ARTWORKS {
        return;
    }
    dirs.sort_by(|a, b| b.1.cmp(&a.1));
    for (path, _) in dirs.into_iter().skip(MAX_CACHED_ARTWORKS) {
        if let Err(err) = tokio::fs::remove_dir_all(&path).await {
            log::warn!("动图缓存清理失败 {}: {err}", path.display());
        }
    }
}

// ----------------------------------------------------------------------
// 对外主入口
// ----------------------------------------------------------------------

/// 确保作品 `artwork_id` 的帧已落在本地缓存，并返回可播放的帧序列。
///
/// 失败一律返回中文可读文案（元数据不可用 / 下载失败 / 解包失败 / 缺帧）。
pub async fn ensure_frames(
    api: &PixivApi,
    cache_img_root: &Path,
    artwork_id: i64,
) -> Result<UgoiraFrames, String> {
    let body = api
        .client()
        .get_json(&format!("/ajax/illust/{artwork_id}/ugoira_meta"))
        .await
        .map_err(|err| format!("动图元数据获取失败: {err}"))?;
    let Some((zip_url, raw_frames)) = parse_ugoira_playback(&body) else {
        return Err("动图元数据不可用（作品可能不是动图，或登录态已失效）".to_string());
    };

    let mut wanted: Vec<(String, i64)> = Vec::with_capacity(raw_frames.len());
    for (file, delay) in raw_frames {
        match sanitize_frame_name(&file) {
            Some(name) => wanted.push((name, delay)),
            None => log::warn!("动图 {artwork_id} 帧名非法，已跳过: {file:?}"),
        }
    }
    if wanted.is_empty() {
        return Err("动图元数据不可用：帧列表为空".to_string());
    }

    let dir = frames_dir(cache_img_root, artwork_id);
    if !manifest_matches(&dir, &zip_url, &wanted).await {
        log::debug!("动图 {artwork_id} 缓存未命中，开始下载 zip");
        let bytes = api
            .client()
            .download_bytes(&zip_url)
            .await
            .map_err(|err| format!("动图 zip 下载失败: {err}"))?;
        let extract_dir = dir.clone();
        let extract_wanted = wanted.clone();
        tokio::task::spawn_blocking(move || {
            extract_frames(&bytes, &extract_dir, &extract_wanted)
        })
        .await
        .map_err(|err| format!("动图解包任务失败: {err}"))??;
        write_manifest(&dir, &zip_url, &wanted).await?;
        prune_cached_artworks(cache_img_root).await;
    }

    Ok(UgoiraFrames {
        frames: wanted
            .iter()
            .map(|(file, delay)| UgoiraFrame {
                src: frame_scheme_url(artwork_id, file),
                delay: *delay,
            })
            .collect(),
        // 帧图尺寸由前端按 naturalWidth/naturalHeight 读取，后端不解码图片。
        width: None,
        height: None,
    })
}

// ----------------------------------------------------------------------
// 单元测试（全离线：内存造 zip，不碰网络）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    /// 内存构造 zip（Stored，无需压缩 feature 之外的任何东西）。
    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            writer.start_file(*name, options).expect("start_file");
            writer.write_all(data).expect("write frame");
        }
        writer.finish().expect("finish zip").into_inner()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pixiv-ugoira-{tag}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn parse_playback_url_priority() {
        // src（预览 zip）优先
        let body = json!({
            "src": "https://x/src.zip",
            "originalSrc": "https://x/orig.zip",
            "zip_urls": { "original": "https://x/fallback.zip" },
            "frames": [{ "file": "000.jpg", "delay": 40 }]
        });
        let (url, frames) = parse_ugoira_playback(&body).unwrap();
        assert_eq!(url, "https://x/src.zip");
        assert_eq!(frames, vec![("000.jpg".to_string(), 40)]);

        // src 缺失 → originalSrc
        let body = json!({
            "originalSrc": "https://x/orig.zip",
            "zip_urls": { "original": "https://x/fallback.zip" },
            "frames": []
        });
        assert_eq!(
            parse_ugoira_playback(&body).unwrap().0,
            "https://x/orig.zip"
        );

        // 前两者缺失 → zip_urls.original
        let body = json!({ "zip_urls": { "original": "https://x/fallback.zip" } });
        let (url, frames) = parse_ugoira_playback(&body).unwrap();
        assert_eq!(url, "https://x/fallback.zip");
        assert!(frames.is_empty(), "frames 缺失应为空表");

        // 空串视同缺失；全无 → None
        let body = json!({ "src": "", "originalSrc": "  " });
        assert!(parse_ugoira_playback(&body).is_none());
        assert!(parse_ugoira_playback(&json!({})).is_none());
    }

    #[test]
    fn parse_playback_frame_delay_fallback() {
        let body = json!({
            "src": "https://x/src.zip",
            "frames": [
                { "file": "000.jpg", "delay": 70 },
                { "file": "001.jpg" },
                { "file": "002.jpg", "delay": 0 },
                { "file": "003.jpg", "delay": -5 }
            ]
        });
        let (_, frames) = parse_ugoira_playback(&body).unwrap();
        assert_eq!(
            frames,
            vec![
                ("000.jpg".to_string(), 70),
                ("001.jpg".to_string(), 100),
                ("002.jpg".to_string(), 100),
                ("003.jpg".to_string(), 100),
            ]
        );

        // frames 不是数组 → 空表（URL 仍可解析）
        let body = json!({ "src": "https://x/src.zip", "frames": "oops" });
        let (url, frames) = parse_ugoira_playback(&body).unwrap();
        assert_eq!(url, "https://x/src.zip");
        assert!(frames.is_empty());
    }

    #[test]
    fn sanitize_frame_name_rejects_traversal_and_junk() {
        // 正常帧名
        assert_eq!(
            sanitize_frame_name("000000.jpg"),
            Some("000000.jpg".to_string())
        );
        assert_eq!(
            sanitize_frame_name("frame-01_2.webp"),
            Some("frame-01_2.webp".to_string())
        );
        // 路径穿越 / 多段 / 空 / 控制字符 / Windows 非法字符 / 盘符前缀
        for bad in [
            "../x",
            "a/b",
            "a\\b",
            "",
            "  ",
            "a\u{7}b.png",
            "a\u{0}.png",
            "C:x.png",
            "x:y.png",
            "a*b.png",
            "x|y.png",
            " lead.png",
            "trail.png ",
            ".",
            "..",
            "a..b.png",
        ] {
            assert_eq!(sanitize_frame_name(bad), None, "应拒绝: {bad:?}");
        }
    }

    #[test]
    fn frame_scheme_url_prefix_matches_platform() {
        let url = frame_scheme_url(1234, "000.jpg");
        if cfg!(windows) {
            assert_eq!(url, "http://pixiv-ugoira.localhost/1234/000.jpg");
        } else {
            assert_eq!(url, "pixiv-ugoira://localhost/1234/000.jpg");
        }
    }

    #[test]
    fn frames_dir_layout() {
        let dir = frames_dir(Path::new("data/cache/img"), 987);
        assert_eq!(dir, Path::new("data/cache/img").join("ugoira").join("987"));
    }

    #[test]
    fn extract_frames_writes_only_whitelisted_single_segment_entries() {
        let dir = temp_dir("extract");
        let bytes = build_zip(&[
            ("000.jpg", b"frame-0"),
            ("001.jpg", b"frame-1"),
            ("../escaped.jpg", b"evil"),
            ("sub/002.jpg", b"evil-2"),
            ("003.jpg", b"not-wanted"),
        ]);
        let wanted = vec![
            ("000.jpg".to_string(), 100),
            ("001.jpg".to_string(), 80),
        ];
        extract_frames(&bytes, &dir, &wanted).expect("解包应成功");

        assert_eq!(std::fs::read(dir.join("000.jpg")).unwrap(), b"frame-0");
        assert_eq!(std::fs::read(dir.join("001.jpg")).unwrap(), b"frame-1");
        // 白名单外 / 多段 / 穿越条目一律不落盘
        assert!(!dir.join("003.jpg").exists());
        assert!(!dir.join("sub").exists());
        assert!(!dir.join("escaped.jpg").exists());
        assert!(!dir.parent().unwrap().join("escaped.jpg").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_frames_errors_when_wanted_frame_missing() {
        let dir = temp_dir("missing");
        let bytes = build_zip(&[("000.jpg", b"frame-0")]);
        let wanted = vec![
            ("000.jpg".to_string(), 100),
            ("001.jpg".to_string(), 100),
        ];
        let err = extract_frames(&bytes, &dir, &wanted).unwrap_err();
        assert!(err.contains("缺少帧文件"), "实际错误: {err}");
        assert!(err.contains("001.jpg"), "实际错误: {err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_frames_clears_stale_files_from_previous_zip() {
        let dir = temp_dir("stale");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("old.jpg"), b"stale").unwrap();

        let bytes = build_zip(&[("000.jpg", b"frame-0")]);
        extract_frames(&bytes, &dir, &[("000.jpg".to_string(), 100)]).expect("解包应成功");
        assert!(!dir.join("old.jpg").exists(), "旧 zip 残留帧必须清理");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
