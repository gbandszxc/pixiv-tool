//! SauceNAO 以图识图命令（契约见 docs/SPEC.md §7 + docs/research/saucenao-api.md）。
//!
//! 形状约定与 browse_api_cmds 一致：`#[tauri::command]` 薄壳 +
//! `*_impl(&AppState, ...)` 可离线调用，业务失败统一 `Err(中文文案)`。
//! 无需 pixiv 登录态，但要求设置里已配置 `saucenao_api_key`
//! （key 只在 saucenao 模块内存中拼 query，不写日志、不进报错原文）。
//!
//! 搜索历史：`saucenao_search` 成功路径内自动记录（后端权威，前端不补记；
//! 同 source 刷新时间与结果数，保留最近 50 条），`saucenao_history_*` 三个
//! 命令负责列表 / 单条删除 / 清空。

use serde_json::{Value, json};
use tauri::State;

use crate::db::{SAUCENAO_SEARCH_HISTORY_KEEP, SaucenaoSearchHistoryEntry, SaucenaoSearchHistoryRow};
use crate::saucenao;
use crate::state::AppState;

/// 来源类型粗校验（file=本地图片路径 / url=公网图片地址）。
fn validate_source_type(source_type: &str) -> Result<(), String> {
    if !matches!(source_type, "file" | "url") {
        return Err(format!("不支持的来源类型: {source_type}（仅 file|url）"));
    }
    Ok(())
}

/// numres 区间粗校验（SauceNAO 取值域 1–40）。
fn validate_numres(numres: Option<i64>) -> Result<(), String> {
    if numres.is_some_and(|n| !(1..=saucenao::NUMRES_MAX).contains(&n)) {
        return Err("numres 必须在 1~40 之间".to_string());
    }
    Ok(())
}

/// 历史条目展示标签：file 取文件名（含扩展名，取不到回退原路径），url 原样。
fn history_label(source_type: &str, source: &str) -> String {
    match source_type {
        "file" => std::path::Path::new(source)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(source)
            .to_string(),
        _ => source.to_string(),
    }
}

/// 以图识图搜索：file=本地图片路径（POST multipart）/ url=公网图片（GET）。
#[tauri::command]
pub async fn saucenao_search(
    state: State<'_, AppState>,
    source_type: String,
    source: String,
    numres: Option<i64>,
) -> Result<Value, String> {
    saucenao_search_impl(&state, &source_type, &source, numres).await
}

pub async fn saucenao_search_impl(
    state: &AppState,
    source_type: &str,
    source: &str,
    numres: Option<i64>,
) -> Result<Value, String> {
    // 参数粗校验在前：未配置 key 也先暴露参数错误（与 browse 层顺序约定一致）
    validate_source_type(source_type)?;
    if source.trim().is_empty() {
        return Err("图片来源不能为空".to_string());
    }
    validate_numres(numres)?;
    let response = saucenao::search_impl(state, source_type, source, numres).await?;
    let value = serde_json::to_value(&response).map_err(|err| format!("结果序列化失败: {err}"))?;
    // 成功路径自动记录搜索历史（同 source 刷新时间与结果数，裁剪保留最近 50 条）；
    // 记录失败只降级打日志，不阻断已成功的搜索结果
    let entry = SaucenaoSearchHistoryEntry {
        source_type: source_type.to_string(),
        source: source.to_string(),
        label: history_label(source_type, source),
        result_count: response.results.len() as i64,
        searched_at: crate::db::now_iso(),
    };
    if let Err(err) = state.db.record_saucenao_search_history(&entry) {
        log::warn!("记录以图识图搜索历史失败: {err}");
    }
    Ok(value)
}

// ----------------------------------------------------------------------
// 搜索历史（列表 / 单条删除 / 清空）
// ----------------------------------------------------------------------

/// 以图识图搜索历史列表（最新在前；limit 缺省 50，即表内全量）。
#[tauri::command]
pub async fn saucenao_history_list(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<SaucenaoSearchHistoryRow>, String> {
    saucenao_history_list_impl(&state, limit)
}

/// 命令实现（不依赖 tauri::State，可离线调用）。
pub fn saucenao_history_list_impl(
    state: &AppState,
    limit: Option<u32>,
) -> Result<Vec<SaucenaoSearchHistoryRow>, String> {
    let limit = limit
        .map(i64::from)
        .unwrap_or(SAUCENAO_SEARCH_HISTORY_KEEP);
    state.db.list_saucenao_search_history(limit)
}

/// 删除单条搜索历史（id 不存在幂等，deleted=0）。
#[tauri::command]
pub async fn saucenao_history_delete(state: State<'_, AppState>, id: i64) -> Result<Value, String> {
    saucenao_history_delete_impl(&state, id)
}

/// 命令实现（离线可直调）。
pub fn saucenao_history_delete_impl(state: &AppState, id: i64) -> Result<Value, String> {
    let deleted = state.db.delete_saucenao_search_history(id)?;
    Ok(json!({ "status": "success", "deleted": deleted }))
}

/// 清空以图识图搜索历史，返回清除条数。
#[tauri::command]
pub async fn saucenao_history_clear(state: State<'_, AppState>) -> Result<Value, String> {
    saucenao_history_clear_impl(&state)
}

/// 命令实现（离线可直调）。
pub fn saucenao_history_clear_impl(state: &AppState) -> Result<Value, String> {
    let deleted = state.db.clear_saucenao_search_history()?;
    Ok(json!({ "status": "success", "deleted": deleted }))
}

// ----------------------------------------------------------------------
// 单元测试（纯函数，全部离线）
// ----------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_source_type_whitelist() {
        assert!(validate_source_type("file").is_ok());
        assert!(validate_source_type("url").is_ok());
        assert_eq!(
            validate_source_type("web").unwrap_err(),
            "不支持的来源类型: web（仅 file|url）"
        );
        assert_eq!(
            validate_source_type("").unwrap_err(),
            "不支持的来源类型: （仅 file|url）"
        );
    }

    #[test]
    fn validate_numres_range() {
        assert!(validate_numres(None).is_ok(), "缺省合法（后端兜底 16）");
        assert!(validate_numres(Some(1)).is_ok());
        assert!(validate_numres(Some(40)).is_ok());
        assert_eq!(
            validate_numres(Some(0)).unwrap_err(),
            "numres 必须在 1~40 之间"
        );
        assert_eq!(
            validate_numres(Some(41)).unwrap_err(),
            "numres 必须在 1~40 之间"
        );
        assert_eq!(
            validate_numres(Some(-3)).unwrap_err(),
            "numres 必须在 1~40 之间"
        );
    }

    #[test]
    fn history_label_rules() {
        // file 取文件名（含扩展名，跨平台分隔符都认）
        assert_eq!(history_label("file", r"C:\pic\Some Image.PNG"), "Some Image.PNG");
        assert_eq!(history_label("file", "/home/u/a.jpg"), "a.jpg");
        // 取不到文件名回退原路径
        assert_eq!(history_label("file", "/"), "/");
        // url 原样
        assert_eq!(
            history_label("url", "https://example.com/a.jpg?x=1"),
            "https://example.com/a.jpg?x=1"
        );
    }
}
