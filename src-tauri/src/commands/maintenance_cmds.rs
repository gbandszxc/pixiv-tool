//! 本机缓存与日志维护。路径只从 AppPaths 派生，不接受前端提供的文件路径。
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tauri::State;

use crate::paths::AppPaths;
use crate::state::AppState;

fn cache_path(paths: &AppPaths, kind: &str) -> Result<PathBuf, String> {
    match kind {
        "translation" => Ok(paths.data_dir.join("translations")),
        "images" => Ok(paths.data_dir.join("cache").join("img")),
        _ => Err("缓存类型无效".into()),
    }
}

// 不跟随符号链接 / Windows junction，清理边界固定为当前应用的数据目录。
fn check_path(paths: &AppPaths, path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let base = paths
        .data_dir
        .canonicalize()
        .map_err(|e| format!("无法读取数据目录：{e}"))?;
    let resolved = path
        .canonicalize()
        .map_err(|e| format!("无法读取维护目录：{e}"))?;
    if resolved == base
        || !resolved.starts_with(&base)
        || fs::symlink_metadata(path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
    {
        return Err("维护目录不是应用内的普通目录，拒绝操作".into());
    }
    Ok(())
}

fn usage(path: &Path) -> Result<(u64, u64), String> {
    if !path.exists() {
        return Ok((0, 0));
    }
    let (mut bytes, mut files) = (0, 0);
    for entry in fs::read_dir(path).map_err(|e| format!("无法统计目录：{e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let (size, count) = usage(&entry.path())?;
            bytes += size;
            files += count;
        } else if metadata.is_file() {
            bytes += metadata.len();
            files += 1;
        }
    }
    Ok((bytes, files))
}

fn log_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if !path.exists() {
        return Ok(vec![]);
    }
    fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| {
            let result = entry.map_err(|e| e.to_string()).and_then(|entry| {
                let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
                Ok((metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && entry.path().extension().is_some_and(|ext| ext == "log"))
                .then(|| entry.path()))
            });
            match result {
                Ok(None) => None,
                Ok(Some(path)) => Some(Ok(path)),
                Err(error) => Some(Err(error)),
            }
        })
        .collect()
}

fn info(paths: &AppPaths) -> Result<Value, String> {
    let mut result = json!({});
    for kind in ["translation", "images"] {
        let path = cache_path(paths, kind)?;
        check_path(paths, &path)?;
        let (bytes, files) = usage(&path)?;
        result[kind] = json!({"path":path, "bytes":bytes, "files":files});
    }
    check_path(paths, &paths.logs_dir)?;
    let files = log_files(&paths.logs_dir)?;
    let mut bytes = 0;
    for file in &files {
        bytes += fs::metadata(file).map_err(|e| e.to_string())?.len();
    }
    result["logs"] = json!({"path":paths.logs_dir,"bytes":bytes,"files":files.len()});
    Ok(result)
}

fn clear_directory(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("缓存包含链接，拒绝清理链接目标".into());
        }
        if metadata.is_dir() {
            clear_directory(&path)?;
            fs::remove_dir(&path).map_err(|e| format!("清理缓存失败：{e}"))?;
        } else if metadata.is_file() {
            fs::remove_file(&path).map_err(|e| format!("清理缓存失败：{e}"))?;
        }
    }
    Ok(())
}

pub(crate) fn clear_log_files(paths: &AppPaths) -> Result<(), String> {
    check_path(paths, &paths.logs_dir)?;
    log::logger().flush();
    for path in log_files(&paths.logs_dir)? {
        // 保留文件和活动 logger 的句柄，后续日志仍能追加。
        OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(|e| format!("清理日志失败：{e}"))?;
    }
    Ok(())
}

fn log_tail(paths: &AppPaths) -> Result<String, String> {
    check_path(paths, &paths.logs_dir)?;
    let path = paths.logs_dir.join("app.log");
    if !path.exists() {
        return Ok(String::new());
    }
    check_path(paths, &path)?;
    log::logger().flush();
    let mut file = File::open(path).map_err(|e| format!("读取日志失败：{e}"))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    // 固定读取末尾 64KiB / 最多 200 行，不随日志增长无限读盘。
    let start = size.saturating_sub(64 * 1024);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    file.take(64 * 1024)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let text = if start > 0 {
        text.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    } else {
        &text
    };
    let lines: Vec<_> = text.lines().rev().take(200).collect();
    Ok(lines.into_iter().rev().collect::<Vec<_>>().join("\n"))
}

#[tauri::command]
pub async fn maintenance_info(state: State<'_, AppState>) -> Result<Value, String> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || info(&paths))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_logs(state: State<'_, AppState>) -> Result<String, String> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || log_tail(&paths))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clear_cache(state: State<'_, AppState>, kind: String) -> Result<Value, String> {
    let path = cache_path(&state.paths, &kind)?;
    let _guard = if kind == "translation" {
        Some(
            state
                .translation_lock
                .try_lock()
                .map_err(|_| "小说翻译正在进行，请完成后再清理")?,
        )
    } else {
        None
    };
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || {
        check_path(&paths, &path)?;
        clear_directory(&path)?;
        Ok(json!({"status":"success"}))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn maintenance_counts_tails_and_clears_only_managed_files() {
        let dir = std::env::temp_dir().join(format!("pixiv-maintenance-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths {
            data_dir: dir.clone(),
            config_dir: dir.join("config"),
            logs_dir: dir.join("logs"),
        };
        let translation = cache_path(&paths, "translation").unwrap();
        fs::create_dir_all(translation.join("42")).unwrap();
        fs::create_dir_all(&paths.logs_dir).unwrap();
        fs::write(translation.join("42/book.json"), b"12345").unwrap();
        fs::write(dir.join("app.db"), b"keep").unwrap();
        let log = (0..500).map(|i| format!("第 {i} 行\n")).collect::<String>();
        fs::write(paths.logs_dir.join("app.log"), &log).unwrap();
        fs::write(paths.logs_dir.join("app_old.log"), b"old").unwrap();
        let result = info(&paths).unwrap();
        assert_eq!(result["translation"]["bytes"], 5);
        assert_eq!(result["images"]["bytes"], 0);
        assert_eq!(result["logs"]["bytes"], log.len() + 3);
        assert!(cache_path(&paths, "../").is_err());
        assert!(check_path(&paths, &dir).is_err());
        assert_eq!(log_tail(&paths).unwrap().lines().count(), 200);
        assert!(log_tail(&paths).unwrap().starts_with("第 300 行"));
        clear_directory(&translation).unwrap();
        assert_eq!(info(&paths).unwrap()["translation"]["bytes"], 0);
        assert!(dir.join("app.db").exists());
        let mut active = OpenOptions::new()
            .append(true)
            .open(paths.logs_dir.join("app.log"))
            .unwrap();
        clear_log_files(&paths).unwrap();
        assert_eq!(info(&paths).unwrap()["logs"]["bytes"], 0);
        active.write_all(b"new").unwrap();
        active.flush().unwrap();
        assert_eq!(fs::read(paths.logs_dir.join("app.log")).unwrap(), b"new");
        assert_eq!(info(&paths).unwrap()["logs"]["bytes"], 3);
        drop(active);
        fs::remove_dir_all(dir).unwrap();
    }
}
