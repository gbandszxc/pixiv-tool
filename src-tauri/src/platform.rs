//! 跨平台系统集成小工具（与旧 Python `platform.py` 行为一致）。

use std::path::Path;

/// 在系统文件管理器中定位文件；文件不存在时打开其所在目录；父目录也没有 → Err。
///
/// - Windows: `explorer /select, <path>`（打开所在目录并选中文件）
/// - macOS:   `open -R <path>`（在 Finder 中显示文件）
/// - Linux:   `xdg-open <父目录或目录>`（无"选中文件"语义，直接打开所在目录）
///
/// std::process::Command::spawn（不等待退出）。
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    let target = if path.exists() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    if !target.exists() {
        return Err(format!("路径不存在: {}", target.display()));
    }

    #[cfg(target_os = "windows")]
    let spawn_result = std::process::Command::new("explorer")
        .arg("/select,")
        .arg(target)
        .spawn();
    #[cfg(target_os = "macos")]
    let spawn_result = std::process::Command::new("open")
        .arg("-R")
        .arg(target)
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let spawn_result = {
        // xdg-open 只能打开目录；目标是文件时打开其父目录
        let dir = if target.is_file() {
            target.parent().unwrap_or(target)
        } else {
            target
        };
        std::process::Command::new("xdg-open").arg(dir).spawn()
    };

    spawn_result
        .map(|_| ())
        .map_err(|err| format!("打开文件管理器失败: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_path_is_error() {
        // 父目录也不存在，才能走到"路径不存在"分支（父目录存在时会打开父目录）
        let missing = std::env::temp_dir()
            .join(format!("pixiv-tool-nope-{}", uuid::Uuid::new_v4()))
            .join("missing-child");
        let err = reveal_in_file_manager(&missing).unwrap_err();
        assert!(err.starts_with("路径不存在:"), "got {err}");
    }
}
