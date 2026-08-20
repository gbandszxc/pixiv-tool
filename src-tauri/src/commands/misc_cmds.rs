//! 小说/插画记录管理命令。
//!
//! 返回体与旧 HTTP 后端一致（业务错误走 200 + {"error":...}，校验错误走 Err）：
//! - novel_delete：不存在 → {"error":"小说不存在"}；成功 → {"status":"success"}
//! - novels_batch_delete：空列表 → {"error":"novel_ids 不能为空"}；
//!   成功 → {"status":"success","deleted":n}
//! - novels_delete_all → {"status":"success","deleted":n}
//! - open_novel_file：不存在 → {"error":"小说不存在"}；无文件 → {"error":"文件不存在"}；
//!   成功 → {"status":"success"}（reveal_in_file_manager）
//! - illustration_*：同构（{"error":"插画记录不存在"}、{"error":"没有已保存的文件"}、
//!   {"error":"illustration_ids 不能为空"}）

use std::path::Path;

use serde_json::{Value, json};
use tauri::State;

use crate::platform::reveal_in_file_manager;
use crate::state::AppState;

/// 删除单条小说记录（可选删文件）。
#[tauri::command]
pub async fn novel_delete(
    state: State<'_, AppState>,
    novel_id: i64,
    delete_file: bool,
) -> Result<Value, String> {
    novel_delete_impl(&state, novel_id, delete_file)
}

/// 批量删除小说记录（可选删文件）。
#[tauri::command]
pub async fn novels_batch_delete(
    state: State<'_, AppState>,
    novel_ids: Vec<i64>,
    delete_file: bool,
) -> Result<Value, String> {
    novels_batch_delete_impl(&state, novel_ids, delete_file)
}

/// 清空全部小说记录（可选删文件）。
#[tauri::command]
pub async fn novels_delete_all(
    state: State<'_, AppState>,
    delete_file: bool,
) -> Result<Value, String> {
    let deleted = state.db.delete_all_novels(delete_file)?;
    Ok(json!({ "status": "success", "deleted": deleted }))
}

/// 删除单条插画记录（可选删 saved_paths 内全部文件）。
#[tauri::command]
pub async fn illustration_delete(
    state: State<'_, AppState>,
    artwork_id: i64,
    delete_file: bool,
) -> Result<Value, String> {
    illustration_delete_impl(&state, artwork_id, delete_file)
}

/// 批量删除插画记录（可选删文件）。
#[tauri::command]
pub async fn illustrations_batch_delete(
    state: State<'_, AppState>,
    artwork_ids: Vec<i64>,
    delete_file: bool,
) -> Result<Value, String> {
    illustrations_batch_delete_impl(&state, artwork_ids, delete_file)
}

/// 清空全部插画记录（可选删文件）。
#[tauri::command]
pub async fn illustrations_delete_all(
    state: State<'_, AppState>,
    delete_file: bool,
) -> Result<Value, String> {
    let deleted = state.db.delete_all_illustrations(delete_file)?;
    Ok(json!({ "status": "success", "deleted": deleted }))
}

/// 在系统文件管理器中定位小说文件（txt 优先 md 兜底）。
#[tauri::command]
pub async fn open_novel_file(state: State<'_, AppState>, novel_id: i64) -> Result<Value, String> {
    open_novel_file_impl(&state, novel_id)
}

/// 在系统文件管理器中打开插画所在目录（saved_paths[0]）。
#[tauri::command]
pub async fn open_illustration_folder(
    state: State<'_, AppState>,
    artwork_id: i64,
) -> Result<Value, String> {
    open_illustration_folder_impl(&state, artwork_id)
}

// 以下 _impl 函数不依赖 tauri::State，可离线单测。

fn novel_delete_impl(state: &AppState, novel_id: i64, delete_file: bool) -> Result<Value, String> {
    if state.db.get_novel(novel_id).is_none() {
        return Ok(json!({ "error": "小说不存在" }));
    }
    state.db.delete_novels(&[novel_id], delete_file)?;
    Ok(json!({ "status": "success" }))
}

fn novels_batch_delete_impl(
    state: &AppState,
    novel_ids: Vec<i64>,
    delete_file: bool,
) -> Result<Value, String> {
    if novel_ids.is_empty() {
        return Ok(json!({ "error": "novel_ids 不能为空" }));
    }
    let deleted = state.db.delete_novels(&novel_ids, delete_file)?;
    Ok(json!({ "status": "success", "deleted": deleted }))
}

fn illustration_delete_impl(
    state: &AppState,
    artwork_id: i64,
    delete_file: bool,
) -> Result<Value, String> {
    if state.db.get_illustration(artwork_id).is_none() {
        return Ok(json!({ "error": "插画记录不存在" }));
    }
    state.db.delete_illustrations(&[artwork_id], delete_file)?;
    Ok(json!({ "status": "success" }))
}

fn illustrations_batch_delete_impl(
    state: &AppState,
    artwork_ids: Vec<i64>,
    delete_file: bool,
) -> Result<Value, String> {
    if artwork_ids.is_empty() {
        return Ok(json!({ "error": "illustration_ids 不能为空" }));
    }
    let deleted = state.db.delete_illustrations(&artwork_ids, delete_file)?;
    Ok(json!({ "status": "success", "deleted": deleted }))
}

fn open_novel_file_impl(state: &AppState, novel_id: i64) -> Result<Value, String> {
    let Some(novel) = state.db.get_novel(novel_id) else {
        return Ok(json!({ "error": "小说不存在" }));
    };
    // txt 优先 md 兜底；空值或文件已不存在 → 200 + error（对齐旧端点）
    let Some(path) = novel
        .txt_path
        .as_deref()
        .filter(|p| !p.is_empty())
        .or_else(|| novel.md_path.as_deref().filter(|p| !p.is_empty()))
    else {
        return Ok(json!({ "error": "文件不存在" }));
    };
    if !Path::new(path).exists() {
        return Ok(json!({ "error": "文件不存在" }));
    }
    match reveal_in_file_manager(Path::new(path)) {
        Ok(()) => Ok(json!({ "status": "success" })),
        Err(msg) => Ok(json!({ "error": msg })),
    }
}

fn open_illustration_folder_impl(state: &AppState, artwork_id: i64) -> Result<Value, String> {
    let Some(illustration) = state.db.get_illustration(artwork_id) else {
        return Ok(json!({ "error": "插画记录不存在" }));
    };
    // 坏 JSON / 空数组都视为没有已保存文件
    let saved_paths: Vec<String> =
        serde_json::from_str(&illustration.saved_paths).unwrap_or_default();
    let Some(first) = saved_paths.first().filter(|p| !p.is_empty()) else {
        return Ok(json!({ "error": "没有已保存的文件" }));
    };
    match reveal_in_file_manager(Path::new(first)) {
        Ok(()) => Ok(json!({ "status": "success" })),
        Err(msg) => Ok(json!({ "error": msg })),
    }
}

// ----------------------------------------------------------------------
// 单元测试（真实临时库；reveal 只测失败分支——成功分支会打开真实文件管理器）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, IllustrationInsert, NovelInsert, now_iso};
    use crate::paths::AppPaths;
    use crate::settings::Settings;
    use crate::state::AppState;

    fn temp_state(tag: &str) -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "pixiv-tool-misccmd-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("app.db")).unwrap();
        let paths = AppPaths {
            data_dir: dir.clone(),
            config_dir: dir.join("config"),
            logs_dir: dir.join("logs"),
        };
        (AppState::new(paths, Settings::default(), db), dir)
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn novel_delete_missing_and_success() {
        let (state, dir) = temp_state("novel-del");
        let body = novel_delete_impl(&state, 999, false).unwrap();
        assert_eq!(body["error"], json!("小说不存在"));

        state
            .db
            .insert_novel(&NovelInsert {
                novel_id: 1,
                title: "t".into(),
                captured_at: now_iso(),
                ..Default::default()
            })
            .unwrap();
        let body = novel_delete_impl(&state, 1, false).unwrap();
        assert_eq!(body["status"], json!("success"));
        assert!(state.db.get_novel(1).is_none());
        cleanup(&dir);
    }

    #[tokio::test]
    async fn novels_batch_delete_empty_ids_error() {
        let (state, dir) = temp_state("novel-batch");
        let body = novels_batch_delete_impl(&state, Vec::new(), false).unwrap();
        assert_eq!(body["error"], json!("novel_ids 不能为空"));
        cleanup(&dir);
    }

    #[tokio::test]
    async fn novels_batch_delete_counts() {
        let (state, dir) = temp_state("novel-batch2");
        for id in [1, 2, 3] {
            state
                .db
                .insert_novel(&NovelInsert {
                    novel_id: id,
                    title: format!("t{id}"),
                    captured_at: now_iso(),
                    ..Default::default()
                })
                .unwrap();
        }
        let body = novels_batch_delete_impl(&state, vec![1, 2], false).unwrap();
        assert_eq!(body["status"], json!("success"));
        assert_eq!(body["deleted"], json!(2));
        assert!(state.db.get_novel(3).is_some(), "未指定的记录保留");
        cleanup(&dir);
    }

    #[tokio::test]
    async fn illustration_delete_missing_and_batch_empty() {
        let (state, dir) = temp_state("illust-del");
        let body = illustration_delete_impl(&state, 42, false).unwrap();
        assert_eq!(body["error"], json!("插画记录不存在"));
        let body = illustrations_batch_delete_impl(&state, Vec::new(), false).unwrap();
        assert_eq!(body["error"], json!("illustration_ids 不能为空"));
        cleanup(&dir);
    }

    #[test]
    fn open_novel_file_error_branches() {
        let (state, dir) = temp_state("open-novel");
        // 记录不存在
        let body = open_novel_file_impl(&state, 9).unwrap();
        assert_eq!(body["error"], json!("小说不存在"));

        // 记录存在但没有任何文件
        state
            .db
            .insert_novel(&NovelInsert {
                novel_id: 10,
                title: "t".into(),
                captured_at: now_iso(),
                ..Default::default()
            })
            .unwrap();
        let body = open_novel_file_impl(&state, 10).unwrap();
        assert_eq!(body["error"], json!("文件不存在"));

        // txt_path 指向不存在的文件（txt 优先于 md）
        state
            .db
            .update_novel_paths(10, Some("/nonexistent/path/x.txt"), None)
            .unwrap();
        let body = open_novel_file_impl(&state, 10).unwrap();
        assert_eq!(body["error"], json!("文件不存在"));
        cleanup(&dir);
    }

    #[test]
    fn open_illustration_folder_error_branches() {
        let (state, dir) = temp_state("open-illust");
        let body = open_illustration_folder_impl(&state, 7).unwrap();
        assert_eq!(body["error"], json!("插画记录不存在"));

        // saved_paths 空数组 → 没有已保存的文件
        state
            .db
            .insert_illustration(&IllustrationInsert {
                artwork_id: 7,
                title: "T".into(),
                captured_at: now_iso(),
                ..Default::default()
            })
            .unwrap();
        let body = open_illustration_folder_impl(&state, 7).unwrap();
        assert_eq!(body["error"], json!("没有已保存的文件"));

        // 指向不存在路径 → reveal 错误文案透传
        state
            .db
            .insert_illustration(&IllustrationInsert {
                artwork_id: 8,
                title: "T".into(),
                saved_paths: serde_json::to_string(&vec!["/nonexistent/dir/a.png"]).unwrap(),
                captured_at: now_iso(),
                ..Default::default()
            })
            .unwrap();
        let body = open_illustration_folder_impl(&state, 8).unwrap();
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|m| m.starts_with("路径不存在"))
        );
        cleanup(&dir);
    }
}
