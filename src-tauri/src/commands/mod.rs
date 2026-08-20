//! Tauri 命令层（全部已在 lib.rs 的 invoke_handler 注册）。
//!
//! **桩（phase B/C/D 实现）**：本目录只定稿命令签名与返回体形状，函数体待填充。
//!
//! 约定：
//! - JS 侧 camelCase 传参，Tauri 2 自动映射到 Rust snake_case 形参
//!   （deleteFile → delete_file、novelIds → novel_ids、taskIds → task_ids）
//! - 返回体结构与旧 HTTP 后端一致（snake_case）：业务错误（记录不存在等）
//!   走 200 + `{"error": ...}` 风格的返回 Value；参数校验类错误走 `Err(String)`
//! - 异步命令借用 `tauri::State`，按 Tauri 约定一律返回 `Result<_, String>`
//! - 目录选择不设命令：前端直接用 plugin-dialog 的 open API

pub mod auth_cmds;
pub mod history_cmds;
pub mod misc_cmds;
pub mod settings_cmds;
pub mod task_cmds;
