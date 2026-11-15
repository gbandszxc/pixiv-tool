//! 小说/插画记录管理命令。
//!
//! 返回体与旧 HTTP 后端一致（业务错误走 200 + {"error":...}，校验错误走 Err）：
//! - novel_delete：不存在 → {"error":"小说不存在"}；成功 → {"status":"success"}
//! - novels_batch_delete：空列表 → {"error":"novel_ids 不能为空"}；
//!   成功 → {"status":"success","deleted":n}
//! - novels_delete_all → {"status":"success","deleted":n}
//! - open_novel_file：不存在 → {"error":"小说不存在"}；无文件 → {"error":"文件不存在"}；
//!   成功 → {"status":"success"}（reveal_in_file_manager）；kind ∈ {txt,md,epub} 可选
//!   （缺省 txt→md 兜底链；epub 仅读 novels.epub_path，缺失 → {"error":"文件不存在"}）
//! - illustration_*：同构（{"error":"插画记录不存在"}、{"error":"没有已保存的文件"}、
//!   {"error":"illustration_ids 不能为空"}）
//! - illust_saved_pages → {"pages":[0,2],"ugoira":false}（0 起升序；无记录 → 空数组）

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tauri::State;

use crate::paths::anchor_output_dir;
use crate::core::illust_crawler::UGOIRA_TYPE;
use crate::platform::{open_dir_in_file_manager, reveal_in_file_manager};
use crate::settings::Settings;
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

/// 在系统文件管理器中定位小说文件。`kind ∈ {txt, md, epub}` 可选：
/// 缺省 / txt / md 走「txt 优先 md 兜底」链（epub 不插队）；epub 仅读
/// novels.epub_path，缺失或文件不存在 → {"error":"文件不存在"}。
#[tauri::command]
pub async fn open_novel_file(
    state: State<'_, AppState>,
    novel_id: i64,
    kind: Option<String>,
) -> Result<Value, String> {
    open_novel_file_impl(&state, novel_id, kind.as_deref())
}

/// 在系统文件管理器中打开插画所在目录（saved_paths[0]）。
#[tauri::command]
pub async fn open_illustration_folder(
    state: State<'_, AppState>,
    artwork_id: i64,
) -> Result<Value, String> {
    open_illustration_folder_impl(&state, artwork_id)
}

/// 查询作品已保存的页码（0 起，升序）与是否 ugoira。
/// 无记录 → {"pages": [], "ugoira": false}；ugoira 有记录 → {"pages":[0],"ugoira":true}。
#[tauri::command]
pub async fn illust_saved_pages(
    state: State<'_, AppState>,
    artwork_id: i64,
) -> Result<Value, String> {
    illust_saved_pages_impl(&state, artwork_id)
}

// 以下 _impl 函数不依赖 tauri::State，可离线单测/集成测试。

pub fn novel_delete_impl(
    state: &AppState,
    novel_id: i64,
    delete_file: bool,
) -> Result<Value, String> {
    if state.db.get_novel(novel_id).is_none() {
        return Ok(json!({ "error": "小说不存在" }));
    }
    state.db.delete_novels(&[novel_id], delete_file)?;
    Ok(json!({ "status": "success" }))
}

pub fn novels_batch_delete_impl(
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

pub fn illustration_delete_impl(
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

pub fn illustrations_batch_delete_impl(
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

pub fn open_novel_file_impl(state: &AppState, novel_id: i64, kind: Option<&str>) -> Result<Value, String> {
    let Some(novel) = state.db.get_novel(novel_id) else {
        return Ok(json!({ "error": "小说不存在" }));
    };

    let settings = Settings::load_or_init(&state.paths.config_dir);
    let base_output_dir = anchor_output_dir(&settings.output_dir, &state.paths.data_dir);
    let novel_dir = base_output_dir.join("novel");

    // 显式 kind = epub：只认 novels.epub_path（含 novel 目录下同名文件兜底），
    // 缺失/文件不存在 → 「文件不存在」，不开下载目录兜底
    if kind == Some("epub") {
        let recorded = novel.epub_path.as_deref().filter(|p| !p.is_empty());
        let candidate: Option<PathBuf> = match recorded {
            Some(p) if Path::new(p).exists() => Some(PathBuf::from(p)),
            // 记录路径被移走时，按当前 novel 目录下的同名文件兜底
            Some(p) => match Path::new(p).file_name() {
                Some(name) => {
                    let fallback = novel_dir.join(name);
                    fallback.exists().then_some(fallback)
                }
                None => None,
            },
            None => None,
        };
        let Some(path) = candidate else {
            return Ok(json!({ "error": "文件不存在" }));
        };
        return match reveal_in_file_manager(&path) {
            Ok(()) => Ok(json!({ "status": "success" })),
            Err(msg) => Ok(json!({ "error": msg })),
        };
    }

    // 缺省 / txt / md（未知 kind 宽容回落默认链）：txt 优先 md 兜底
    let candidate_paths = [
        novel.txt_path.as_deref(),
        novel.md_path.as_deref(),
    ];

    // 1. 优先尝试定位已有文件
    for path_opt in candidate_paths {
        if let Some(path) = path_opt.filter(|p| !p.is_empty() && Path::new(p).exists()) {
            if reveal_in_file_manager(Path::new(path)).is_ok() {
                return Ok(json!({ "status": "success" }));
            }
        }
    }

    // 2. 检查当前 novel 目录下是否存在同名文件
    for path_opt in candidate_paths {
        if let Some(path) = path_opt.filter(|p| !p.is_empty()) {
            if let Some(file_name) = Path::new(path).file_name() {
                let candidate = novel_dir.join(file_name);
                if candidate.exists() && reveal_in_file_manager(&candidate).is_ok() {
                    return Ok(json!({ "status": "success" }));
                }
            }
        }
    }

    // 3. 文件不存在（被移动走或删除）：始终打开用户配置的下载目录
    let target_dir = if novel_dir.exists() {
        novel_dir
    } else {
        let _ = std::fs::create_dir_all(&novel_dir);
        if novel_dir.exists() {
            novel_dir
        } else {
            let _ = std::fs::create_dir_all(&base_output_dir);
            base_output_dir
        }
    };

    match open_dir_in_file_manager(&target_dir) {
        Ok(()) => Ok(json!({ "status": "success" })),
        Err(msg) => Ok(json!({ "error": msg })),
    }
}


pub fn illust_saved_pages_impl(state: &AppState, artwork_id: i64) -> Result<Value, String> {
    let Some(illustration) = state.db.get_illustration(artwork_id) else {
        // 无记录 → 什么都没存过
        return Ok(json!({ "pages": [], "ugoira": false }));
    };
    let ugoira = illustration.illust_type == UGOIRA_TYPE;
    // saved_pages 已由 Db 读取出口归一；坏 JSON 兜底为空数组
    let pages: Vec<i64> = serde_json::from_str(&illustration.saved_pages).unwrap_or_default();
    Ok(json!({ "pages": pages, "ugoira": ugoira }))
}

pub fn open_illustration_folder_impl(state: &AppState, artwork_id: i64) -> Result<Value, String> {
    let Some(illustration) = state.db.get_illustration(artwork_id) else {
        return Ok(json!({ "error": "插画记录不存在" }));
    };

    let settings = Settings::load_or_init(&state.paths.config_dir);
    let base_output_dir = anchor_output_dir(&settings.output_dir, &state.paths.data_dir);
    let illust_dir = base_output_dir.join("pic");

    // 坏 JSON / 空数组都视为没有已保存文件
    let saved_paths: Vec<String> =
        serde_json::from_str(&illustration.saved_paths).unwrap_or_default();

    // 1. 优先尝试在 saved_paths 中定位实际存在的文件
    for p in &saved_paths {
        if !p.is_empty() && Path::new(p).exists() {
            if reveal_in_file_manager(Path::new(p)).is_ok() {
                return Ok(json!({ "status": "success" }));
            }
        }
    }

    // 2. 检查当前设置的输出目录 pic 下是否存在同名文件
    for p in &saved_paths {
        if !p.is_empty() {
            if let Some(file_name) = Path::new(p).file_name() {
                let candidate = illust_dir.join(file_name);
                if candidate.exists() && reveal_in_file_manager(&candidate).is_ok() {
                    return Ok(json!({ "status": "success" }));
                }
            }
        }
    }

    // 3. 文件不存在（已被用户手动移动走或删除）：始终打开用户配置的下载目录
    let target_dir = if illust_dir.exists() {
        illust_dir
    } else {
        let _ = std::fs::create_dir_all(&illust_dir);
        if illust_dir.exists() {
            illust_dir
        } else {
            let _ = std::fs::create_dir_all(&base_output_dir);
            base_output_dir
        }
    };

    match open_dir_in_file_manager(&target_dir) {
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
        let body = open_novel_file_impl(&state, 9, None).unwrap();
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
        let body = open_novel_file_impl(&state, 10, None).unwrap();
        assert_eq!(body["status"], json!("success"));

        // txt_path 指向不存在的文件（用户移走）→ 同样成功打开配置目录
        state
            .db
            .update_novel_paths(10, Some("/nonexistent/path/x.txt"), None)
            .unwrap();
        let body = open_novel_file_impl(&state, 10, None).unwrap();
        assert_eq!(body["status"], json!("success"));
        cleanup(&dir);
    }

    #[test]
    fn open_novel_file_epub_kind_reads_epub_path_only() {
        let (state, dir) = temp_state("open-novel-epub");
        // epub 缺失（无记录 / 无 epub_path / 文件不存在）→ 「文件不存在」
        let body = open_novel_file_impl(&state, 1, Some("epub")).unwrap();
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
        let body = open_novel_file_impl(&state, 1, Some("epub")).unwrap();
        assert_eq!(body["error"], json!("文件不存在"), "epub_path 为空");
        state
            .db
            .update_novel_epub_path(1, "/nonexistent/path/x.epub")
            .unwrap();
        let body = open_novel_file_impl(&state, 1, Some("epub")).unwrap();
        assert_eq!(body["error"], json!("文件不存在"), "epub 文件不存在");

        // epub_path 指向真实文件 → success（reveal 走平台层，测试环境路径存在即可命中）
        let real = dir.join("novel").join("book.epub");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, b"PK").unwrap();
        state.db.update_novel_epub_path(1, real.to_string_lossy().as_ref()).unwrap();
        let body = open_novel_file_impl(&state, 1, Some("epub")).unwrap();
        assert_eq!(body["status"], json!("success"));

        // kind 缺省走 txt→md 兜底链，epub_path 不参与
        let body = open_novel_file_impl(&state, 1, None).unwrap();
        assert_eq!(body["status"], json!("success"));
        cleanup(&dir);
    }

    #[test]
    fn illust_saved_pages_contract() {
        let (state, dir) = temp_state("saved-pages");
        // 无记录 → 空数组 + ugoira false
        let body = illust_saved_pages_impl(&state, 100).unwrap();
        assert_eq!(body["pages"], json!([]));
        assert_eq!(body["ugoira"], json!(false));

        // 普通多页作品：部分页已存
        state
            .db
            .insert_illustration(&IllustrationInsert {
                artwork_id: 101,
                title: "T".into(),
                saved_paths: r#"["/a/p0.png","/a/p2.png"]"#.into(),
                saved_pages: "[0,2]".into(),
                captured_at: now_iso(),
                ..Default::default()
            })
            .unwrap();
        let body = illust_saved_pages_impl(&state, 101).unwrap();
        assert_eq!(body["pages"], json!([0, 2]));
        assert_eq!(body["ugoira"], json!(false));

        // ugoira 有记录 → [0] + ugoira true
        state
            .db
            .insert_illustration(&IllustrationInsert {
                artwork_id: 102,
                title: "U".into(),
                illust_type: 2,
                saved_paths: r#"["/a/u.zip"]"#.into(),
                captured_at: now_iso(),
                ..Default::default()
            })
            .unwrap();
        let body = illust_saved_pages_impl(&state, 102).unwrap();
        assert_eq!(body["pages"], json!([0]));
        assert_eq!(body["ugoira"], json!(true));
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
        assert_eq!(body["status"], json!("success"));

        // 指向不存在路径（文件被移走）→ 回退打开配置的下载目录
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
        assert_eq!(body["status"], json!("success"));
        cleanup(&dir);
    }
}
