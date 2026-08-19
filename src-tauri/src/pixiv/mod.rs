//! Pixiv HTTP 访问层：client（指纹伪装 + 限速/重试/429）、api（typed 封装）、
//! csrf（登录态探测）。

pub mod api;
pub mod client;
pub mod csrf;
