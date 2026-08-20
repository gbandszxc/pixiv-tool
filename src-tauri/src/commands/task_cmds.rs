//! 任务命令。
//!
//! 返回体与旧 HTTP 后端一致：
//! - tasks_list → {"items":[TaskRow...]}；未知分类 → Err("未知任务分类: {c}")
//! - task_create → {"task_id":"...","status":"pending"}（爬取后台执行，
//!   进度经 "task://progress" / "task://done" 事件推送）；
//!   未登录 → {"error":"未登录，请先登录"}；来源/分类非法 → {"error":"未知..."}
//! - task_pause / task_resume / task_cancel → {"status":"paused"|"running"|"canceled"}；
//!   任务不存在 → {"error":"任务不存在"}
//! - task_retry_failed → {"task_id":新id,"status":"pending"}；
//!   任务不存在 → {"error":"任务不存在"}；没有失败项 → {"error":"没有失败项"}
//! - task_delete → {"deleted":1}；不存在 → Err("任务不存在")
//! - tasks_delete → {"deleted":n}；空列表 → Err("task_ids 不能为空")；
//!   有不存在 → Err("任务不存在")
//! - tasks_delete_completed → {"deleted":n}
//!
//! 删除进行中任务前先 cancel（设取消标志让后台循环退出）再删记录。

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, State};

use crate::core::crawler::EventSink;
use crate::db::TERMINAL_TASK_STATUSES;
use crate::state::AppState;

/// 任务列表，可按 category（novel/illustration）过滤。
#[tauri::command]
pub async fn tasks_list(
    state: State<'_, AppState>,
    category: Option<String>,
) -> Result<Value, String> {
    if let Some(cat) = category.as_deref() {
        if !matches!(cat, "novel" | "illustration") {
            return Err(format!("未知任务分类: {cat}"));
        }
    }
    let items = state.tasks.list_tasks(category.as_deref())?;
    Ok(json!({ "items": items }))
}

/// 构造事件 sink：爬虫后台任务经它把 "task://" 事件转发到前端窗口。
fn forward_events(app: &AppHandle) -> EventSink {
    // EventSink 要求 'static：克隆句柄而非借用
    let app = app.clone();
    Arc::new(move |name: &str, payload: &Value| {
        // 前端未监听 / 窗口已关闭不算错误，静默丢弃
        let _ = app.emit(name, payload.clone());
    })
}

/// 读取登录 cookie；未登录/平台不支持 → None（调用方返回 200+error）。
fn login_cookies(state: &AppState) -> Option<HashMap<String, String>> {
    match state.cookies.load() {
        Ok(Some(cookies)) if !cookies.is_empty() => Some(cookies),
        Ok(_) => None,
        Err(err) => {
            log::warn!("读取登录态失败: {err}");
            None
        }
    }
}

/// 创建抓取任务。category 缺省 "novel"；formats 缺省 ["txt","markdown"]。
#[tauri::command]
pub async fn task_create(
    state: State<'_, AppState>,
    app: AppHandle,
    source_type: String,
    source_id: String,
    formats: Vec<String>,
    category: Option<String>,
) -> Result<Value, String> {
    let category = category.unwrap_or_else(|| "novel".to_string());
    // formats 空数组由 crawler 层兜底为 ["txt","markdown"]，命令层透传
    let Some(cookies) = login_cookies(&state) else {
        return Ok(json!({ "error": "未登录，请先登录" }));
    };
    let settings = state.settings_snapshot();
    let sink = forward_events(&app);

    match state.tasks.create_task(
        &source_type,
        &source_id,
        formats,
        &category,
        cookies,
        settings,
        sink,
    ) {
        Ok(task_id) => Ok(json!({ "task_id": task_id, "status": "pending" })),
        // 分类/来源/登录校验失败：业务错误走 200 + error（对齐旧端点）
        Err(msg) => Ok(json!({ "error": msg })),
    }
}

/// 暂停任务。
#[tauri::command]
pub async fn task_pause(state: State<'_, AppState>, task_id: String) -> Result<Value, String> {
    control_task(&state, task_id, "paused", TaskManagerOp::Pause)
}

/// 继续任务。
#[tauri::command]
pub async fn task_resume(state: State<'_, AppState>, task_id: String) -> Result<Value, String> {
    control_task(&state, task_id, "running", TaskManagerOp::Resume)
}

/// 取消任务。
#[tauri::command]
pub async fn task_cancel(state: State<'_, AppState>, task_id: String) -> Result<Value, String> {
    control_task(&state, task_id, "canceled", TaskManagerOp::Cancel)
}

/// pause/resume/cancel 的公共形状：成功 → {"status":...}；
/// "任务不存在" → 200 + error（对齐旧 404 detail 转 error 风格）；其余错误 reject。
enum TaskManagerOp {
    Pause,
    Resume,
    Cancel,
}

fn control_task(
    state: &AppState,
    task_id: String,
    ok_status: &str,
    op: TaskManagerOp,
) -> Result<Value, String> {
    let result = match op {
        TaskManagerOp::Pause => state.tasks.pause(&task_id),
        TaskManagerOp::Resume => state.tasks.resume(&task_id),
        TaskManagerOp::Cancel => state.tasks.cancel(&task_id),
    };
    match result {
        Ok(()) => Ok(json!({ "status": ok_status })),
        Err(msg) if msg == "任务不存在" => Ok(json!({ "error": msg })),
        Err(msg) => Err(msg),
    }
}

/// 重试任务失败项（返回新任务 id）。
#[tauri::command]
pub async fn task_retry_failed(
    state: State<'_, AppState>,
    app: AppHandle,
    task_id: String,
) -> Result<Value, String> {
    let Some(cookies) = login_cookies(&state) else {
        return Ok(json!({ "error": "未登录，请先登录" }));
    };
    let settings = state.settings_snapshot();
    let sink = forward_events(&app);

    match state.tasks.retry_failed(&task_id, cookies, settings, sink) {
        Ok(new_task_id) => Ok(json!({ "task_id": new_task_id, "status": "pending" })),
        Err(msg) => Ok(json!({ "error": msg })),
    }
}

/// 删除单个任务记录（含进行中任务，先 cancel）。
#[tauri::command]
pub async fn task_delete(state: State<'_, AppState>, task_id: String) -> Result<Value, String> {
    delete_tasks_impl(&state, vec![task_id])
}

/// 批量删除任务记录（含进行中任务，先 cancel）。
#[tauri::command]
pub async fn tasks_delete(
    state: State<'_, AppState>,
    task_ids: Vec<String>,
) -> Result<Value, String> {
    // 元素类型由 Vec<String> 签名保证：非字符串数组在参数反序列化时即 reject
    delete_tasks_impl(&state, task_ids)
}

/// 清除全部已完成任务记录。
#[tauri::command]
pub async fn tasks_delete_completed(state: State<'_, AppState>) -> Result<Value, String> {
    let deleted = state.db.delete_completed_tasks()?;
    Ok(json!({ "deleted": deleted }))
}

/// 批量删除公共实现（单个删除也走这里，与旧 _delete_task_ids 一致）：
/// 1. 非终态任务先 cancel（设取消标志，正在下载的当前项下完再退出）
/// 2. delete_tasks 整批不删语义：有 missing → Err("任务不存在")
fn delete_tasks_impl(state: &AppState, task_ids: Vec<String>) -> Result<Value, String> {
    if task_ids.is_empty() {
        return Err("task_ids 不能为空".to_string());
    }
    for task_id in &task_ids {
        // 查询失败仅记日志（删除本身会再暴露底层错误）
        if let Ok(Some(task)) = state.tasks.get_task(task_id) {
            if !TERMINAL_TASK_STATUSES.contains(&task.status.as_str()) {
                let _ = state.tasks.cancel(task_id);
            }
        }
    }
    match state.db.delete_tasks(&task_ids)? {
        (deleted, missing) if missing.is_empty() => Ok(json!({ "deleted": deleted })),
        _ => Err("任务不存在".to_string()),
    }
}

// ----------------------------------------------------------------------
// 单元测试（delete_tasks_impl 走真实临时库；网络/浏览器路径不在单测范围）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, TaskInsert, now_iso};
    use crate::paths::AppPaths;
    use crate::settings::Settings;

    fn temp_state(tag: &str) -> (AppState, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "pixiv-tool-taskcmd-test-{tag}-{}",
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

    fn insert_task(state: &AppState, task_id: &str, status: &str) {
        let now = now_iso();
        state
            .db
            .insert_task(&TaskInsert {
                task_id: task_id.to_string(),
                source_type: "single".into(),
                source_id: "1".into(),
                status: status.into(),
                created_at: now.clone(),
                updated_at: now,
                ..Default::default()
            })
            .unwrap();
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn delete_empty_ids_rejects() {
        let (state, dir) = temp_state("empty");
        assert_eq!(
            delete_tasks_impl(&state, Vec::new()).unwrap_err(),
            "task_ids 不能为空"
        );
        cleanup(&dir);
    }

    #[test]
    fn delete_missing_task_rejects_without_side_effect() {
        let (state, dir) = temp_state("missing");
        insert_task(&state, "keep", "done");
        assert_eq!(
            delete_tasks_impl(&state, vec!["nope".into()]).unwrap_err(),
            "任务不存在"
        );
        // 整批不删语义：存在的记录原样保留
        assert!(state.db.get_task("keep").unwrap().is_some());
        cleanup(&dir);
    }

    #[test]
    fn delete_terminal_task_skips_cancel_and_deletes() {
        // 终态任务无运行句柄：不应尝试 cancel（否则 cancel 报错被忽略也可过，
        // 但状态被写成 canceled 是错的），直接删记录
        let (state, dir) = temp_state("terminal");
        insert_task(&state, "t1", "done");
        let result = delete_tasks_impl(&state, vec!["t1".into()]).unwrap();
        assert_eq!(result["deleted"], json!(1));
        assert!(state.db.get_task("t1").unwrap().is_none());
        cleanup(&dir);
    }

    #[test]
    fn delete_running_task_cancels_then_deletes() {
        // running 任务无运行句柄（进程重启场景）：cancel 失败仅忽略，删除继续
        let (state, dir) = temp_state("running");
        insert_task(&state, "r1", "running");
        let result = delete_tasks_impl(&state, vec!["r1".into()]).unwrap();
        assert_eq!(result["deleted"], json!(1));
        assert!(state.db.get_task("r1").unwrap().is_none());
        cleanup(&dir);
    }

    #[test]
    fn delete_batch_mixed_missing_rejects_all() {
        let (state, dir) = temp_state("mixed");
        insert_task(&state, "a", "failed");
        assert_eq!(
            delete_tasks_impl(&state, vec!["a".into(), "ghost".into()]).unwrap_err(),
            "任务不存在"
        );
        assert!(state.db.get_task("a").unwrap().is_some());
        cleanup(&dir);
    }

    #[test]
    fn delete_completed_only_removes_done_rows() {
        let (state, dir) = temp_state("completed");
        insert_task(&state, "d1", "done");
        insert_task(&state, "f1", "failed");
        let deleted = state.db.delete_completed_tasks().unwrap();
        assert_eq!(deleted, 1);
        assert!(state.db.get_task("d1").unwrap().is_none());
        assert!(state.db.get_task("f1").unwrap().is_some());
        cleanup(&dir);
    }
}
