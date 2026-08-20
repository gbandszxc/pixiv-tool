//! 用户数据目录锚定（分平台）。与旧 Python `storage/paths.py` 行为一致。
//!
//! dev（debug 编译）模式（所有平台）：锚定 `<repo_root>/data`、`<repo_root>/config`，
//! 数据落在仓库内方便开发。repo root 取编译期 `CARGO_MANIFEST_DIR`
//! （即 `<repo>/src-tauri`）的上一级。
//!
//! release 模式分平台：
//! - Windows: 优先 `<exe_dir>/data`、`<exe_dir>/config`（portable，zip 解压
//!   即用）；若 exe 目录不可写（MSI/NSIS 装进 Program Files），回退
//!   `%LOCALAPPDATA%/pixiv-tool/{data,config}`。升级不删用户数据即保留
//!   （V2-03 产品决策，核心需求）。
//! - macOS: `~/Library/Application Support/pixiv-tool/{data,config}`。
//!   不用 `<exe_dir>`：.app bundle 内部代码签名/公证后只读，写入必失败。
//! - Linux: 遵循 XDG 规范，data 和 config 分开：
//!   `$XDG_DATA_HOME/pixiv-tool/data`（默认 `~/.local/share/pixiv-tool/data`）、
//!   `$XDG_CONFIG_HOME/pixiv-tool/config`（默认 `~/.config/pixiv-tool/config`）。
//!
//! 只用标准库（std::env / std::fs），不引入 dirs / platformdirs 等第三方依赖。

use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "pixiv-tool";

/// 应用数据/配置/日志目录聚合。
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    /// 始终是 `data_dir/logs`。
    pub logs_dir: PathBuf,
}

/// 用户 home 目录（Windows: USERPROFILE，兼容 HOMEDRIVE+HOMEPATH；其它: HOME）。
fn home_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(home) = std::env::var_os("USERPROFILE") {
            return PathBuf::from(home);
        }
        let drive = std::env::var("HOMEDRIVE").unwrap_or_default();
        let path = std::env::var("HOMEPATH").unwrap_or_default();
        if !drive.is_empty() && !path.is_empty() {
            return PathBuf::from(format!("{drive}{path}"));
        }
    }
    #[cfg(not(target_os = "windows"))]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home);
    }
    PathBuf::from(".")
}

/// dev 模式的仓库根：`CARGO_MANIFEST_DIR`（= `<repo>/src-tauri`）的上一级。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// release 可执行文件所在目录（Windows portable 用，仅 Windows 编译）。
#[cfg(target_os = "windows")]
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 计算应用目录并确保三个目录都存在。
///
/// 目录创建失败只 eprintln 不中断——后续真正读写（DB 打开 / 配置保存）会
/// 再次失败并把错误暴露给调用方。
pub fn app_paths(dev: bool) -> AppPaths {
    let (data_dir, config_dir): (PathBuf, PathBuf);
    if dev {
        let root = repo_root();
        data_dir = root.join("data");
        config_dir = root.join("config");
    } else {
        #[cfg(target_os = "windows")]
        {
            // 优先 exe 同级（portable，zip 解压即用）；MSI/NSIS 装进
            // Program Files 后普通用户无写权限，回退 %LOCALAPPDATA%。
            let portable = exe_dir();
            let portable_ok = [portable.join("data"), portable.join("config")]
                .iter()
                .all(|d| std::fs::create_dir_all(d).is_ok());
            let base = if portable_ok {
                portable
            } else {
                std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(home_dir)
                    .join(APP_NAME)
            };
            data_dir = base.join("data");
            config_dir = base.join("config");
        }
        #[cfg(target_os = "macos")]
        {
            let root = home_dir()
                .join("Library")
                .join("Application Support")
                .join(APP_NAME);
            data_dir = root.join("data");
            config_dir = root.join("config");
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let data_root = match std::env::var_os("XDG_DATA_HOME") {
                Some(v) if !v.is_empty() => PathBuf::from(v).join(APP_NAME),
                _ => home_dir().join(".local").join("share").join(APP_NAME),
            };
            let config_root = match std::env::var_os("XDG_CONFIG_HOME") {
                Some(v) if !v.is_empty() => PathBuf::from(v).join(APP_NAME),
                _ => home_dir().join(".config").join(APP_NAME),
            };
            data_dir = data_root.join("data");
            config_dir = config_root.join("config");
        }
    }
    let logs_dir = data_dir.join("logs");
    for dir in [&data_dir, &config_dir, &logs_dir] {
        if let Err(err) = std::fs::create_dir_all(dir) {
            eprintln!("创建目录失败 {}: {err}", dir.display());
        }
    }
    AppPaths {
        data_dir,
        config_dir,
        logs_dir,
    }
}

/// 默认输出目录：`~/Downloads/pixiv-tool`（跨平台统一；用户可在设置页自改）。
pub fn default_output_dir() -> PathBuf {
    home_dir().join("Downloads").join(APP_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_output_dir_is_downloads_subdir() {
        let dir = default_output_dir();
        let s = dir.to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("Downloads/pixiv-tool"), "got {s}");
    }

    #[test]
    fn app_paths_creates_directories() {
        let paths = app_paths(true);
        assert!(paths.data_dir.is_dir());
        assert!(paths.config_dir.is_dir());
        assert!(paths.logs_dir.is_dir());
        assert!(paths.logs_dir.ends_with("logs"));
    }
}
