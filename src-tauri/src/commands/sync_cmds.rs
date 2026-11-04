//! WebDAV 同步命令（3 个）：测试连接 / 上传加密包 / 下载并恢复。
//!
//! 返回体见各 `Summary`；失败一律 `SyncError`（`{code, field, message}`，
//! 形状与 ADR 0044 一致），前端按 `code` 取 i18n 文案。
//! `config` 是设置弹窗草稿覆盖（全部字段可选），`None` = 全部用已保存值。

use serde::Serialize;
use tauri::State;

use crate::state::AppState;
use crate::sync::{DownloadSummary, SyncConfigOverride, SyncError, UploadSummary};

/// `sync_test_connection` 返回体。
#[derive(Debug, Clone, Serialize)]
pub struct SyncTestResult {
    /// 固定 `"ok"`（失败走 Err）。
    pub status: &'static str,
    /// MKCOL 实际返回的 HTTP 状态码（已存在 / 不支持 MKCOL 也算连接成功）。
    pub http_status: u16,
    /// 云端是否已有同步包（`GET 404` = 无备份；探测失败走 Err）。
    pub has_backup: bool,
    /// 备份时间：远端 `Last-Modified` 原文（HTTP 日期，前端可用 `Date` 解析）。
    pub backup_time: Option<String>,
}

/// 测试 WebDAV 连通性：建目录 + 探测远端同步包，不读不写包内容。
#[tauri::command]
pub async fn sync_test_connection(
    state: State<'_, AppState>,
    config: Option<SyncConfigOverride>,
) -> Result<SyncTestResult, SyncError> {
    let outcome =
        crate::sync::service::test_connection(&state, &config.unwrap_or_default()).await?;
    Ok(SyncTestResult {
        status: "ok",
        http_status: outcome.http_status,
        has_backup: outcome.backup_time.is_some(),
        backup_time: outcome.backup_time,
    })
}

/// 导出所选范围 → 加密 → 上传。返回包大小与导出条数。
#[tauri::command]
pub async fn sync_upload(
    state: State<'_, AppState>,
    scopes: Vec<String>,
    config: Option<SyncConfigOverride>,
) -> Result<UploadSummary, SyncError> {
    crate::sync::service::upload(&state, &config.unwrap_or_default(), &scopes).await
}

/// 下载 → 解密 → 按所选范围合并写回。
#[tauri::command]
pub async fn sync_download(
    state: State<'_, AppState>,
    scopes: Vec<String>,
    config: Option<SyncConfigOverride>,
) -> Result<DownloadSummary, SyncError> {
    crate::sync::service::download(&state, &config.unwrap_or_default(), &scopes).await
}
