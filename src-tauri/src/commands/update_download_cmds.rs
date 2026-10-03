//! 应用内更新：只从本仓库 Release 选择包，流式下载、校验后交给系统安装器。
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::ipc::Channel;
use tauri_plugin_opener::OpenerExt;
use tokio::{io::AsyncWriteExt, sync::watch};

use super::update_cmds::{is_stable_tag, shared_clients};

const DOWNLOAD_PREFIX: &str = "https://github.com/gbandszxc/pixiv-tool/releases/download/";
static ACTIVE: Mutex<Option<watch::Sender<bool>>> = Mutex::new(None);
static COMPLETED: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
    digest: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Clone, Serialize)]
pub struct UpdateProgress {
    phase: &'static str,
    file_name: String,
    downloaded: u64,
    total: u64,
    bytes_per_second: f64,
}

#[derive(Serialize)]
pub struct UpdateDownloadResult {
    pub path: String,
    pub installer_opened: bool,
    pub directory_opened: bool,
    pub package_type: String,
}

/// bundler 会在产物内写入实际包类型；未知类型不猜测 MSI/deb 等安装方式。
fn package_type() -> Result<String, String> {
    match tauri::utils::platform::bundle_type()
        .map(|kind| kind.to_string())
        .as_deref()
    {
        Some("app" | "dmg") => Ok("dmg".into()),
        Some(kind @ ("msi" | "nsis" | "deb" | "rpm" | "appimage")) => Ok(kind.into()),
        _ => Err("无法识别当前安装包类型，请从发布页选择安装包".into()),
    }
}

/// 严格对应 CI 的文件命名格式（GitHub 会将 productName 中的空格规范为点）。
fn asset_matches(name: &str, version: &str, os: &str, arch: &str, kind: &str) -> bool {
    let Some(suffix) = ["Pixiv.Tool_", "Pixiv Tool_", "pixiv-tool_"]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .and_then(|rest| rest.strip_prefix(&format!("{version}_")))
    else {
        return false;
    };
    match (os, arch, kind) {
        ("windows", "x86_64", "msi") => suffix == "x64_en-US.msi",
        ("windows", "aarch64", "msi") => suffix == "arm64_en-US.msi",
        ("windows", "x86_64", "nsis") => suffix == "x64-setup.exe",
        ("windows", "aarch64", "nsis") => suffix == "arm64-setup.exe",
        ("macos", "x86_64", "dmg") => suffix == "x64.dmg" || suffix == "universal.dmg",
        ("macos", "aarch64", "dmg") => suffix == "aarch64.dmg" || suffix == "universal.dmg",
        ("linux", "x86_64", "appimage") => suffix == "amd64.AppImage",
        ("linux", "aarch64", "appimage") => {
            suffix == "aarch64.AppImage" || suffix == "arm64.AppImage"
        }
        ("linux", "x86_64", "deb") => suffix == "amd64.deb",
        ("linux", "aarch64", "deb") => suffix == "arm64.deb",
        // RPM 的版本与架构使用连字符/点，单独在 select_asset 中处理。
        _ => false,
    }
}

fn select_asset(
    assets: &[Asset],
    version: &str,
    os: &str,
    arch: &str,
    kind: &str,
    universal: bool,
) -> Result<Asset, String> {
    if !matches!(arch, "x86_64" | "aarch64") {
        return Err("当前架构暂无更新包，请打开发布页".into());
    }
    let rpm_arch = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => "unsupported",
    };
    let mut matching: Vec<_> = assets
        .iter()
        .filter(|asset| {
            let name = &asset.name;
            let matches = if os == "linux" && kind == "rpm" {
                name == &format!("pixiv-tool-{version}-1.{rpm_arch}.rpm")
                    || name == &format!("Pixiv.Tool-{version}-1.{rpm_arch}.rpm")
                    || name == &format!("Pixiv Tool-{version}-1.{rpm_arch}.rpm")
            } else {
                asset_matches(name, version, os, arch, kind)
            };
            matches && (!universal || name.ends_with("_universal.dmg"))
        })
        .collect();
    // 原生架构优先，Intel 无 x64 DMG 时才取通用包；通用运行包保持通用。
    matching.sort_by_key(|asset| asset.name.ends_with("_universal.dmg"));
    let asset = matching
        .first()
        .ok_or("最新版本没有与当前平台、架构及包类型匹配的安装包")?;
    if matching.len() > 1
        && matching[0].name.ends_with("_universal.dmg")
            == matching[1].name.ends_with("_universal.dmg")
    {
        return Err("发布包匹配不唯一，请从发布页选择安装包".into());
    }
    validate_asset(asset, version)?;
    Ok((*asset).clone())
}

fn validate_asset(asset: &Asset, version: &str) -> Result<(), String> {
    // 来源、文件名、大小、摘要都是边界：不接受前端指定 URL 或任意路径。
    let rest = asset
        .browser_download_url
        .strip_prefix(DOWNLOAD_PREFIX)
        .ok_or("更新包来源无效")?;
    let (tag, file) = rest.split_once('/').ok_or("更新包地址无效")?;
    if tag.strip_prefix('v').unwrap_or(tag) != version
        || !is_stable_tag(tag)
        || file != asset.name
        || asset.size == 0
        || asset.name.contains(['/', '\\', ':'])
        || asset.name.starts_with('.')
    {
        return Err("更新包元数据无效".into());
    }
    if let Some(digest) = &asset.digest {
        let hash = digest
            .strip_prefix("sha256:")
            .ok_or("不支持发布包的校验算法")?;
        if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("更新包校验摘要无效".into());
        }
    }
    Ok(())
}

async fn request(url: &str, download: bool) -> Result<wreq::Response, String> {
    let (system, direct) = shared_clients()?;
    for client in [system, direct] {
        // 下载不使用元数据的 15s 总超时；分块读取另外设 30s 无数据超时。
        let timeout = if download { 3600 } else { 15 };
        let headers_timeout = if download { 30 } else { 15 };
        if let Ok(Ok(response)) = tokio::time::timeout(
            Duration::from_secs(headers_timeout),
            client.get(url).timeout(Duration::from_secs(timeout)).send(),
        )
        .await
        {
            if response.status().is_success() {
                return Ok(response);
            }
        }
    }
    Err("无法下载更新，请检查网络或稍后重试".into())
}

async fn release_asset(version: &str, kind: &str) -> Result<Asset, String> {
    // 检查页支持裸版本 tag；两个精确 tag 均验证返回内容，不能混用其他版本。
    for tag in [format!("v{version}"), version.to_string()] {
        let url = format!("https://api.github.com/repos/gbandszxc/pixiv-tool/releases/tags/{tag}");
        let Ok(response) = request(&url, false).await else {
            continue;
        };
        let body = response.text().await.map_err(|_| "读取发布信息失败")?;
        let release: Release = serde_json::from_str(&body).map_err(|_| "发布信息格式无效")?;
        if release.draft || release.prerelease || release.tag_name != tag {
            return Err("发布版本无效".into());
        }
        let mut universal = false;
        if cfg!(target_os = "macos") {
            use std::io::Read;
            if let Ok(exe) = std::env::current_exe() {
                if let Ok(mut file) = std::fs::File::open(exe) {
                    let mut magic = [0; 4];
                    if file.read_exact(&mut magic).is_ok() {
                        universal = matches!(
                            magic,
                            [0xca, 0xfe, 0xba, 0xbe]
                                | [0xbe, 0xba, 0xfe, 0xca]
                                | [0xca, 0xfe, 0xba, 0xbf]
                                | [0xbf, 0xba, 0xfe, 0xca]
                        );
                    }
                }
            }
        }
        return select_asset(
            &release.assets,
            version,
            std::env::consts::OS,
            std::env::consts::ARCH,
            kind,
            universal,
        );
    }
    Err("无法读取该版本的发布包，请重试或打开发布页".into())
}

async fn transfer(
    asset: &Asset,
    part: &std::path::Path,
    progress: &Channel<UpdateProgress>,
) -> Result<(), String> {
    let response = request(&asset.browser_download_url, true).await?;
    if response
        .content_length()
        .is_some_and(|size| size != asset.size)
    {
        return Err("更新包大小与发布信息不一致".into());
    }
    let mut stream = std::pin::pin!(response.bytes_stream());
    let mut file = tokio::fs::File::create(part)
        .await
        .map_err(|_| "无法创建更新临时文件")?;
    let mut hash = Sha256::new();
    let mut downloaded = 0u64;
    let mut samples = VecDeque::from([(Instant::now(), 0u64)]);
    let mut last_emit = Instant::now();
    let emit = |phase, downloaded, speed| {
        let _ = progress.send(UpdateProgress {
            phase,
            file_name: asset.name.clone(),
            downloaded,
            total: asset.size,
            bytes_per_second: speed,
        });
    };
    emit("downloading", 0, 0.0);
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(30), stream.next())
        .await
        .map_err(|_| "下载超时，请重试")?
    {
        let chunk = chunk.map_err(|_| "更新下载中断，请重试")?;
        downloaded += chunk.len() as u64;
        if downloaded > asset.size {
            return Err("更新包超出预期大小".into());
        }
        file.write_all(&chunk)
            .await
            .map_err(|_| "写入更新包失败，请检查磁盘空间")?;
        hash.update(&chunk);
        let now = Instant::now();
        if now.duration_since(last_emit) >= Duration::from_millis(200) {
            samples.push_back((now, downloaded));
            while samples.len() > 2 && now.duration_since(samples[1].0) > Duration::from_secs(2) {
                samples.pop_front();
            }
            let (start, bytes) = samples[0];
            emit(
                "downloading",
                downloaded,
                (downloaded - bytes) as f64 / now.duration_since(start).as_secs_f64().max(0.001),
            );
            last_emit = now;
        }
    }
    if downloaded != asset.size {
        return Err("更新包下载不完整，请重新下载".into());
    }
    if let Some(expected) = &asset.digest {
        if !format!("sha256:{:x}", hash.finalize()).eq_ignore_ascii_case(expected) {
            return Err("更新包 SHA256 校验失败，请重新下载".into());
        }
    }
    file.flush().await.map_err(|_| "保存更新包失败")?;
    file.sync_all().await.map_err(|_| "保存更新包失败")?;
    emit("opening", downloaded, 0.0);
    Ok(())
}

struct ActiveGuard;
impl Drop for ActiveGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock() {
            *active = None;
        }
    }
}

#[tauri::command]
pub async fn download_app_update(
    app: tauri::AppHandle,
    version: String,
    progress: Channel<UpdateProgress>,
) -> Result<UpdateDownloadResult, String> {
    if !is_stable_tag(&version) || version.starts_with('v') {
        return Err("更新版本无效".into());
    }
    let current = app.package_info().version.clone();
    let requested: Vec<u64> = version
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|_| "更新版本无效")?;
    if requested.as_slice() <= [current.major, current.minor, current.patch].as_slice() {
        return Err("更新版本必须高于当前版本".into());
    }
    let (cancel, mut receiver) = watch::channel(false);
    {
        let mut active = ACTIVE.lock().map_err(|_| "更新状态不可用")?;
        if active.is_some() {
            return Err("已有更新下载正在进行".into());
        }
        *active = Some(cancel);
    }
    let _guard = ActiveGuard;
    let kind = package_type()?;
    let dir = std::env::temp_dir()
        .join("pixiv-tool-update")
        .join(&version)
        .join(uuid::Uuid::new_v4().to_string());
    let download = async {
        let asset = release_asset(&version, &kind).await?;
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|_| "无法创建更新临时目录")?;
        let part = dir.join(format!("{}.part", asset.name));
        transfer(&asset, &part, &progress).await?;
        let path = dir.join(&asset.name);
        tokio::fs::rename(part, &path)
            .await
            .map_err(|_| "无法保存更新包")?;
        Ok::<_, String>(path)
    };
    let result = tokio::select! { biased;
        _ = receiver.changed() => Err("更新下载已取消".into()),
        result = download => result,
    };
    let path = match result {
        Ok(path) => path,
        Err(error) => {
            let _ = tokio::fs::remove_dir_all(&dir).await;
            return Err(error);
        }
    };
    *COMPLETED.lock().map_err(|_| "更新状态不可用")? = Some(path.clone());
    let installer_opened = system_open(&app, &path).await;
    let directory_opened = !installer_opened && reveal(&app, &path).await;
    Ok(UpdateDownloadResult {
        path: path.to_string_lossy().into_owned(),
        installer_opened,
        directory_opened,
        package_type: kind,
    })
}

async fn system_open(app: &tauri::AppHandle, path: &std::path::Path) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let _ = app;
        // detached opener 只能判断是否成功派生进程，会漏掉 xdg-open 无关联的退出错误。
        let program = if cfg!(target_os = "macos") {
            "/usr/bin/open"
        } else {
            "xdg-open"
        };
        let mut command = tokio::process::Command::new(program);
        command
            .arg(path)
            .kill_on_drop(true)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        matches!(tokio::time::timeout(Duration::from_secs(15), command.status()).await, Ok(Ok(status)) if status.success())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let handle = app.clone();
        let path = path.to_string_lossy().into_owned();
        tokio::task::spawn_blocking(move || handle.opener().open_path(path, None::<&str>).is_ok())
            .await
            .unwrap_or(false)
    }
}

async fn reveal(app: &tauri::AppHandle, path: &std::path::Path) -> bool {
    let handle = app.clone();
    let file = path.to_path_buf();
    if tokio::task::spawn_blocking(move || handle.opener().reveal_item_in_dir(file).is_ok())
        .await
        .unwrap_or(false)
    {
        return true;
    }
    if let Some(dir) = path.parent() {
        system_open(app, dir).await
    } else {
        false
    }
}

#[tauri::command]
pub fn cancel_app_update() -> Result<(), String> {
    if let Some(sender) = ACTIVE.lock().map_err(|_| "更新状态不可用")?.as_ref() {
        let _ = sender.send(true);
    }
    Ok(())
}

#[tauri::command]
pub async fn open_update_directory(app: tauri::AppHandle) -> Result<(), String> {
    let path = COMPLETED
        .lock()
        .map_err(|_| "更新状态不可用")?
        .clone()
        .ok_or("尚无已下载的更新包")?;
    if reveal(&app, &path).await {
        Ok(())
    } else {
        Err("无法打开下载目录，请按显示的路径手动打开".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn asset(name: &str) -> Asset {
        Asset {
            name: name.into(),
            size: 10,
            browser_download_url: format!("{DOWNLOAD_PREFIX}v1.3.0/{name}"),
            digest: None,
        }
    }
    #[test]
    fn packages_match_platform_architecture_and_format() {
        let assets = vec![
            asset("Pixiv.Tool_1.3.0_x64_en-US.msi"),
            asset("Pixiv.Tool_1.3.0_arm64_en-US.msi"),
            asset("Pixiv.Tool_1.3.0_universal.dmg"),
            asset("Pixiv.Tool_1.3.0_aarch64.dmg"),
            asset("Pixiv.Tool_1.3.0_amd64.AppImage"),
            asset("Pixiv.Tool_1.3.0_arm64.deb"),
            asset("Pixiv.Tool-1.3.0-1.x86_64.rpm"),
        ];
        assert!(
            select_asset(&assets, "1.3.0", "windows", "x86_64", "msi", false)
                .unwrap()
                .name
                .contains("x64_")
        );
        assert!(
            select_asset(&assets, "1.3.0", "linux", "aarch64", "deb", false)
                .unwrap()
                .name
                .ends_with("arm64.deb")
        );
        assert!(select_asset(&assets, "1.3.0", "linux", "aarch64", "appimage", false).is_err());
        assert!(select_asset(&assets, "1.3.0", "linux", "x86_64", "rpm", false).is_ok());
        assert!(
            select_asset(&assets, "1.3.0", "macos", "aarch64", "dmg", false)
                .unwrap()
                .name
                .ends_with("aarch64.dmg")
        );
        assert!(
            select_asset(&assets, "1.3.0", "macos", "aarch64", "dmg", true)
                .unwrap()
                .name
                .ends_with("universal.dmg")
        );
        assert!(select_asset(&assets, "1.2.0", "windows", "x86_64", "msi", false).is_err());
    }
    #[test]
    fn reject_ambiguous_and_untrusted_assets() {
        let mut valid = asset("Pixiv.Tool_1.3.0_x64_en-US.msi");
        assert!(
            select_asset(
                &[valid.clone(), valid.clone()],
                "1.3.0",
                "windows",
                "x86_64",
                "msi",
                false
            )
            .is_err()
        );
        valid.browser_download_url = "https://evil.example/installer.msi".into();
        assert!(validate_asset(&valid, "1.3.0").is_err());
        valid = asset("../installer.msi");
        assert!(validate_asset(&valid, "1.3.0").is_err());
        valid = asset("Pixiv.Tool_1.3.0_x64_en-US.msi");
        valid.digest = Some("sha256:bad".into());
        assert!(validate_asset(&valid, "1.3.0").is_err());
    }

    #[tokio::test]
    async fn stream_download_checks_size_digest_and_progress() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // 仅测试使用临时回环端口，不访问 GitHub，也不启动安装器。
        let dir = std::env::temp_dir().join(format!("pixiv-update-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir(&dir).await.unwrap();
        for (expected_size, digest, succeeds) in [
            (
                6,
                Some(format!("sha256:{:x}", Sha256::digest(b"abcdef"))),
                true,
            ),
            (7, None, false),
            (6, Some(format!("sha256:{}", "0".repeat(64))), false),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = [0; 4096];
                socket.read(&mut buffer).await.unwrap();
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n").await.unwrap();
                tokio::time::sleep(Duration::from_millis(250)).await;
                socket.write_all(b"3\r\ndef\r\n0\r\n\r\n").await.unwrap();
            });
            let messages = std::sync::Arc::new(Mutex::new(Vec::new()));
            let captured = messages.clone();
            let channel = Channel::new(move |message| {
                if let tauri::ipc::InvokeResponseBody::Json(json) = message {
                    captured.lock().unwrap().push(json);
                }
                Ok(())
            });
            let file = dir.join("installer.part");
            let mut sample = asset("test.msi");
            sample.browser_download_url = format!("http://{address}/installer");
            sample.size = expected_size;
            sample.digest = digest;
            assert_eq!(transfer(&sample, &file, &channel).await.is_ok(), succeeds);
            server.await.unwrap();
            let events = messages.lock().unwrap();
            assert!(events.iter().any(|json| json.contains("downloading")));
            assert_eq!(events.iter().any(|json| json.contains("opening")), succeeds);
            if succeeds {
                assert_eq!(tokio::fs::read(&file).await.unwrap(), b"abcdef");
            }
        }
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}
