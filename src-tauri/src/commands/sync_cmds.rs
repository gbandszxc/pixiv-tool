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
}

/// 测试 WebDAV 连通性：只建目录，不读不写同步包。
#[tauri::command]
pub async fn sync_test_connection(
    state: State<'_, AppState>,
    config: Option<SyncConfigOverride>,
) -> Result<SyncTestResult, SyncError> {
    let http_status =
        crate::sync::service::test_connection(&state, &config.unwrap_or_default()).await?;
    Ok(SyncTestResult {
        status: "ok",
        http_status,
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
