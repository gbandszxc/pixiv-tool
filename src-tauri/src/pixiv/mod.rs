//! Pixiv HTTP 访问层：client（指纹伪装 + 限速/重试/429）、api（typed 封装）、
//! browse_api（浏览模式 ajax 接口层，IPC 契约 v2）、csrf（登录态探测）。

pub mod api;
pub mod browse_api;
pub mod client;
pub mod csrf;
