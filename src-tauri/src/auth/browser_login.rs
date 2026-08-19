//! 用真实 Chromium 浏览器完成 Pixiv 登录并通过 CDP 读取 Cookie。
//!
//! **桩（phase B 实现）**：本文件只定稿公共契约与轮询语义，函数体待填充。
#![allow(unused)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::pixiv::csrf::UserInfo;

pub const LOGIN_URL: &str = "https://accounts.pixiv.net/login";
pub const LOGIN_TIMEOUT_SEC: u64 = 300;
/// 每轮探测间隔。
pub const POLL_INTERVAL_MILLIS: u64 = 500;

/// 系统没有可通过 CDP 控制的 Chrome / Edge / Chromium。
#[derive(thiserror::Error, Debug)]
#[error("未找到 Chrome、Edge 或 Chromium")]
pub struct BrowserNotFoundError;

/// 浏览器登录结果（status 与旧 Python 版一致）。
#[derive(Debug, Clone, Serialize)]
pub struct LoginResult {
    /// "success" | "cancelled" | "timeout" | "error"
    pub status: String,
    /// 仅 success：完整 pixiv.net cookie（含 PHPSESSID 与补入的 x-csrf-token）。
    pub cookies: Option<HashMap<String, String>>,
    /// 仅 success：探测到的用户信息。
    pub user: Option<UserInfo>,
    /// timeout/error 时的说明。
    pub message: Option<String>,
}

/// 返回首个已安装的 Chromium 浏览器可执行文件：
/// - macOS: /Applications 下 Google Chrome / Microsoft Edge / Chromium（按序）
/// - Windows: PROGRAMFILES / PROGRAMFILES(X86) / LOCALAPPDATA 下
///   Google/Chrome/Application/chrome.exe、Microsoft/Edge/Application/msedge.exe
/// - Linux: which google-chrome | microsoft-edge | chromium | chromium-browser
pub fn find_login_browser() -> Result<PathBuf, BrowserNotFoundError> {
    todo!("phase B")
}

/// 从 CDP Storage.getCookies 结果（Value 数组）提取 Pixiv 域 Cookie：
/// domain 去掉前导 '.' 后以 "pixiv.net" 结尾，且 name/value 非空。
pub fn extract_pixiv_cookies(items: &[Value]) -> HashMap<String, String> {
    todo!("phase B")
}

/// 登录页跳回 Pixiv 主站后才探测 Session（url host ∈ {pixiv.net, www.pixiv.net}，
/// accounts.pixiv.net 不算；type=="page"），避免验证码阶段制造额外请求。
pub fn has_pixiv_main_target(items: &[Value]) -> bool {
    todo!("phase B")
}

/// 打开隔离的真实浏览器等待登录完成：
///
/// 1. spawn：`<browser> --remote-debugging-port=<随机空闲端口>
///    --remote-debugging-address=127.0.0.1 --user-data-dir=<profile_dir>
///    --no-first-run --no-default-browser-check --app=https://accounts.pixiv.net/login`
///    （profile_dir 由调用方传 `config_dir/login-browser-profile`，先建目录）
/// 2. 轮询 `http://127.0.0.1:{port}/json/version` 拿 webSocketDebuggerUrl
/// 3. 主循环至多 LOGIN_TIMEOUT_SEC（每轮 sleep 0.5s）：
///    - 进程退出 → {"status":"cancelled"}
///    - Target.getTargets 有 pixiv 主站 page → Storage.getCookies →
///      extract_pixiv_cookies → 有 PHPSESSID → fetch_session_probe 验证
///      （ProbeError::Invalid 忽略继续轮询；成功补 x-csrf-token 返回 success）
/// 4. 超时 → {"status":"timeout","message":"登录超时（300s）"}
/// 5. finally：CdpClient Browser.close → 进程 terminate → 3s 内不退则 kill
///
/// Cookie 值不得写入日志。
pub async fn open_browser_login(profile_dir: &Path) -> LoginResult {
    todo!("phase B")
}
