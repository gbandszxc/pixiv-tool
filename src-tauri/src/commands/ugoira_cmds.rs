//! ugoira（动图）帧读取命令层（IPC 契约：`browse_ugoira_frames`）。
//!
//! 形状约定与 browse_api_cmds 一致：`#[tauri::command]` 薄壳 +
//! `*_impl(&AppState, ...)` 可离线调用，业务失败统一 `Err(中文文案)`。
//!
//! 调用顺序：
//! 1. 参数粗校验（`artwork_id <= 0` → 可读中文 Err），放在登录守卫之前，
//!    未登录也能先暴露参数错误；
//! 2. [`build_browse_api`] 登录守卫（PHPSESSID 为空即 `NOT_LOGGED_IN`，
//!    前端以「登录」关键字识别并弹登录窗，文案逐字沿用）；
//! 3. [`crate::ugoira::ensure_frames`]：元数据 → zip 下载解包 → 协议 URL。
//!
//! 返回体：`{"frames":[{"src":"…","delay":40}, …],"width":null,"height":null}`
//! （帧图尺寸由前端按 naturalWidth 读取，后端不解码图片）。
//!
//! 日志纪律：本层不打印 cookie/token。

use serde_json::{Value, json};
use tauri::State;

use crate::commands::browse_api_cmds::build_browse_api;
use crate::state::AppState;
use crate::ugoira::ensure_frames;

/// 读取动图帧序列（缓存优先，未命中则下载 zip 解包）。
#[tauri::command]
pub async fn browse_ugoira_frames(
    state: State<'_, AppState>,
    artwork_id: i64,
) -> Result<Value, String> {
    browse_ugoira_frames_impl(&state, artwork_id).await
}

/// 命令实现（不依赖 tauri::State，可离线集成测试）。
pub async fn browse_ugoira_frames_impl(state: &AppState, artwork_id: i64) -> Result<Value, String> {
    if artwork_id <= 0 {
        return Err("作品 ID 必须为正整数".to_string());
    }
    let api = build_browse_api(state)?;
    let cache_root = state.paths.data_dir.join("cache").join("img");
    let frames = ensure_frames(&api, &cache_root, artwork_id).await?;
    Ok(json!({
        "frames": frames.frames,
        "width": frames.width,
        "height": frames.height,
    }))
}
