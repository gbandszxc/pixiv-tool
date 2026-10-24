//! 整篇小说翻译 Job 的 IPC 命令。
//!
//! 命令只做参数校验与状态转发，业务真相源在 [`crate::translation::service`]：
//! - `novel_translation_start` 立即返回 Job 快照，后台 tokio 任务继续执行；
//! - 其余命令按 `job_id` 操作，状态与进度通过 `translation://job` 事件推送。
//!
//! v1 的 `novel_translation_get` / `translation_models` / `translation_test` 仍由
//! `crate::translation` 再导出保留；`novel_translate_page` 已从 invoke_handler 移除
//! （函数保留以便回退）。

use serde_json::{Value, json};
use tauri::State;

use crate::{state::AppState, translation::NovelInput};

/// 整篇翻译的启动结果（浮层据此区分「已创建 / 原文已是目标语言 / 加入既有任务」）。
#[derive(serde::Serialize)]
pub struct StartResponse {
    pub job: crate::translation::service::TranslationJobSnapshot,
    /// 目标语言与原文一致：未创建 Job。
    pub already_target_language: bool,
}

/// 启动（或加入）整篇翻译。
#[tauri::command]
pub async fn novel_translation_start(
    state: State<'_, AppState>,
    novel: NovelInput,
    force: bool,
) -> Result<StartResponse, String> {
    let service = state.translation_service.clone();
    let snapshot = service.start(novel, force).await?;
    Ok(StartResponse {
        already_target_language: snapshot.already_target_language,
        job: snapshot,
    })
}

/// 任务列表（内存索引 + 磁盘扫描，含重启后的 interrupted）。
#[tauri::command]
pub async fn novel_translation_jobs(
    state: State<'_, AppState>,
) -> Result<crate::translation::service::JobList, String> {
    let service = state.translation_service.clone();
    Ok(service.jobs().await)
}

/// 单个任务状态快照。
#[tauri::command]
pub async fn novel_translation_status(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<crate::translation::service::TranslationJobSnapshot, String> {
    let service = state.translation_service.clone();
    service.snapshot(&job_id).await
}

/// 阅读器结果：优先已完成 v2 Job，其次 v1 只读缓存（partial）。
#[tauri::command]
pub async fn novel_translation_result(
    state: State<'_, AppState>,
    novel_id: i64,
) -> Result<Option<crate::translation::artifacts::TranslationResult>, String> {
    let service = state.translation_service.clone();
    service.result(novel_id).await
}

/// 确认术语与风格（确认模式）。
#[tauri::command]
pub async fn novel_translation_confirm(
    state: State<'_, AppState>,
    job_id: String,
    terms: Vec<crate::translation::service::ConstraintTermEdit>,
    style: String,
) -> Result<(), String> {
    let service = state.translation_service.clone();
    service
        .confirm(
            &job_id,
            crate::translation::service::ConstraintEdit { terms, style },
        )
        .await
}

/// 暂停派发（停在最近检查点）。
#[tauri::command]
pub async fn novel_translation_pause(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    let service = state.translation_service.clone();
    service.pause(&job_id).await
}

/// 从检查点恢复。
#[tauri::command]
pub async fn novel_translation_resume(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    let service = state.translation_service.clone();
    service.resume(&job_id).await
}

/// 立即取消（保留检查点与已提交产物）。
#[tauri::command]
pub async fn novel_translation_cancel(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    let service = state.translation_service.clone();
    service.cancel(&job_id).await
}

/// 删除整个任务目录（含全部中间产物）。
#[tauri::command]
pub async fn novel_translation_delete(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    let service = state.translation_service.clone();
    service.delete(&job_id).await
}

/// 浮层预览：原文 / 译文对照。
#[tauri::command]
pub async fn novel_translation_preview(
    state: State<'_, AppState>,
    job_id: String,
    chunk_id: String,
) -> Result<crate::translation::service::ChunkPreview, String> {
    let service = state.translation_service.clone();
    service.preview(&job_id, &chunk_id).await
}

/// 约束浮层只读查询：版本、术语、风格、冲突与未解决项。
///
/// 只读 `preparation/constraints.json` 与 manifest，不产生副作用，可与运行中的
/// Job 并发调用。约束尚未生成时返回明确错误。
#[tauri::command]
pub async fn novel_translation_constraints(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<crate::translation::service::ConstraintsView, String> {
    let service = state.translation_service.clone();
    service.constraints(&job_id).await
}

/// 审查问题浮层只读查询：汇总全部 `reviews/*.json` 的问题与受影响块。
///
/// 只返回问题定位与计数，不含原文 / 译文；不产生副作用，可与运行中的 Job 并发调用。
#[tauri::command]
pub async fn novel_translation_issues(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<crate::translation::service::IssuesView, String> {
    let service = state.translation_service.clone();
    service.issues(&job_id).await
}

/// 会话级详细诊断开关（仅本次运行有效，重启复位为关闭）。
///
/// 开启后日志级别切到 DEBUG，输出工具 / 请求 / 子任务的起止与长度；
/// 不输出原文、译文、提示词、Key 或 Cookie。返回生效后的状态。
#[tauri::command]
pub async fn translation_set_diagnostic(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    Ok(state.translation_service.set_diagnostic(enabled))
}

/// 当前诊断开关状态（供设置页回显）。
#[tauri::command]
pub async fn translation_diagnostic_status(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.translation_service.diagnostic())
}

/// 维护页「翻译」分类状态：v1 + v2 任务计数（诊断用，不含正文）。
#[tauri::command]
pub async fn novel_translation_diagnostics(
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let service = state.translation_service.clone();
    let list = service.jobs().await;
    let mut by_state = std::collections::BTreeMap::<String, u32>::new();
    for job in &list.jobs {
        *by_state.entry(job.state.as_str().to_string()).or_insert(0) += 1;
    }
    Ok(json!({
        "generation": service.generation().get(),
        "active": service.active_jobs(),
        "jobs": list.jobs.len(),
        "by_state": by_state,
    }))
}
