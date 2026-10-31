//! 跨平台系统集成小工具（与旧 Python `platform.py` 行为一致）。

use std::path::Path;
#[cfg(test)]
pub static LAST_OPENED_PATH: std::sync::Mutex<Option<std::path::PathBuf>> =
    std::sync::Mutex::new(None);

/// 在系统文件管理器中直接打开目录（进入目录而非选中）。
pub fn open_dir_in_file_manager(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Err(format!("路径不存在: {}", dir.display()));
    }

    #[cfg(test)]
    {
        if let Ok(mut lock) = LAST_OPENED_PATH.lock() {
            *lock = Some(dir.to_path_buf());
        }
        return Ok(());
    }

    #[cfg(not(test))]
    {
        #[cfg(target_os = "windows")]
        let spawn_result = std::process::Command::new("explorer")
            .arg(dir)
            .spawn();
        #[cfg(target_os = "macos")]
        let spawn_result = std::process::Command::new("open")
            .arg(dir)
            .spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let spawn_result = std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn();

        spawn_result
            .map(|_| ())
            .map_err(|err| format!("打开文件管理器失败: {err}"))
    }
}

/// 在系统文件管理器中定位文件；目标为目录时直接打开；文件不存在时退化为打开所在目录；父目录也没有 → Err。
///
/// - Windows: 文件走 `explorer /select, <path>`；目录走 `explorer <dir>`
/// - macOS:   文件走 `open -R <path>`；目录走 `open <dir>`
/// - Linux:   `xdg-open <目录>`
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

    if target.is_dir() {
        return open_dir_in_file_manager(target);
    }

    #[cfg(test)]
    {
        if let Ok(mut lock) = LAST_OPENED_PATH.lock() {
            *lock = Some(target.to_path_buf());
        }
        return Ok(());
    }

    #[cfg(not(test))]
    {
        #[cfg(target_os = "windows")]
        let spawn_result = std::process::Command::new("explorer")
            .arg(format!("/select,{}", target.display()))
            .spawn();
        #[cfg(target_os = "macos")]
        let spawn_result = std::process::Command::new("open")
            .arg("-R")
            .arg(target)
            .spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let spawn_result = {
            let dir = target.parent().unwrap_or(target);
            std::process::Command::new("xdg-open").arg(dir).spawn()
        };

        spawn_result
            .map(|_| ())
            .map_err(|err| format!("打开文件管理器失败: {err}"))
    }
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
        let err_dir = open_dir_in_file_manager(&missing).unwrap_err();
        assert!(err_dir.starts_with("路径不存在:"), "got {err_dir}");
    }

    #[test]
    fn existing_path_records_last_opened() {
        let dir = std::env::temp_dir().join(format!("pixiv-tool-plat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("test.txt");
        std::fs::write(&file, "hello").unwrap();

        reveal_in_file_manager(&file).unwrap();
        assert_eq!(LAST_OPENED_PATH.lock().unwrap().clone(), Some(file.clone()));

        open_dir_in_file_manager(&dir).unwrap();
        assert_eq!(LAST_OPENED_PATH.lock().unwrap().clone(), Some(dir.clone()));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
