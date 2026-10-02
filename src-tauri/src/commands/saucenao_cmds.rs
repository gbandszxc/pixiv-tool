//! SauceNAO 以图识图命令（契约见 docs/SPEC.md §7 + docs/research/saucenao-api.md）。
//!
//! 形状约定与 browse_api_cmds 一致：`#[tauri::command]` 薄壳 +
//! `saucenao_search_impl(&AppState, ...)` 可离线调用，业务失败统一
//! `Err(中文文案)`。无需 pixiv 登录态，但要求设置里已配置 `saucenao_api_key`
//! （key 只在 saucenao 模块内存中拼 query，不写日志、不进报错原文）。

use serde_json::Value;
use tauri::State;

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
    serde_json::to_value(response).map_err(|err| format!("结果序列化失败: {err}"))
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
}
