//! Chrome DevTools Protocol WebSocket 客户端（tokio-tungstenite）。
//!
//! **桩（phase B 实现）**：本文件只定稿公共契约，函数体待填充。
#![allow(unused)]

use anyhow::Result;
use serde_json::Value;

/// CDP 浏览器级 WebSocket 客户端（连 /json/version 返回的 webSocketDebuggerUrl）。
pub struct CdpClient {
    // phase B：WebSocketStream<MaybeTlsStream<TcpStream>> + 自增 call id
}

impl CdpClient {
    /// 连接 ws://127.0.0.1:{port}/devtools/browser/{uuid}（open_timeout ~3s）。
    pub async fn connect(ws_url: &str) -> Result<Self> {
        todo!("phase B")
    }

    /// 调用 CDP 方法：发送 {"id":n,"method","params"}，循环 recv 直到
    /// id 匹配的应答（跳过事件消息）；应答含 error 字段 → Err。
    /// params 传 Value::Null 表示无参数。
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        todo!("phase B")
    }

    /// 发送 Browser.close 并关闭 WebSocket（失败静默由调用方兜底 kill）。
    pub async fn close(&mut self) -> Result<()> {
        todo!("phase B")
    }
}
