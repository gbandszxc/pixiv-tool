//! Chrome DevTools Protocol WebSocket 客户端（tokio-tungstenite）。
//!
//! 语义对齐 Python `auth/browser_login.py` 的 `_cdp_call`：
//! - 自增 id 发 `{"id":N,"method":...,"params":...}`，循环读 Text 帧直到
//!   id 匹配（事件通知帧无 id，直接跳过）；
//! - 应答含 `error` 字段 → `Err("CDP {method} 失败：{error}")`；
//! - 调用方串行 call（单一读循环内匹配应答，无并发读取问题）。

use std::time::Duration;

use anyhow::{Result, anyhow};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

/// CDP 握手超时（秒），对齐 Python `connect(url, open_timeout=3)`。
const CONNECT_TIMEOUT_SEC: u64 = 3;

/// CDP 浏览器级 WebSocket 客户端（连 /json/version 返回的 webSocketDebuggerUrl）。
pub struct CdpClient {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl CdpClient {
    /// 连接 ws://127.0.0.1:{port}/devtools/browser/{uuid}（open_timeout ~3s）。
    pub async fn connect(ws_url: &str) -> Result<Self> {
        let connect = tokio_tungstenite::connect_async(ws_url);
        let (ws, _response) =
            tokio::time::timeout(Duration::from_secs(CONNECT_TIMEOUT_SEC), connect)
                .await
                .map_err(|_| anyhow!("连接 CDP 超时（{CONNECT_TIMEOUT_SEC}s）"))??;
        Ok(Self { ws, next_id: 0 })
    }

    /// 调用 CDP 方法：发送 {"id":n,"method","params"}，循环 recv 直到
    /// id 匹配的应答（跳过事件消息）；应答含 error 字段 → Err。
    /// params 传 Value::Null 表示无参数（请求体省略 params 字段）。
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let mut request = serde_json::json!({"id": id, "method": method});
        if !params.is_null() {
            request["params"] = params;
        }
        self.ws
            .send(Message::Text(request.to_string().into()))
            .await
            .map_err(|err| anyhow!("发送 CDP 请求失败: {err}"))?;

        loop {
            let frame = self
                .ws
                .next()
                .await
                .ok_or_else(|| anyhow!("CDP 连接已关闭"))?
                .map_err(|err| anyhow!("读取 CDP 应答失败: {err}"))?;
            match frame {
                Message::Text(text) => {
                    let message: Value = serde_json::from_str(text.as_str())
                        .map_err(|err| anyhow!("CDP 应答不是有效 JSON: {err}"))?;
                    // 事件通知帧（无 id 或 id 不匹配）跳过
                    if message.get("id").and_then(Value::as_u64) != Some(id) {
                        continue;
                    }
                    if let Some(error) = message.get("error") {
                        return Err(anyhow!("CDP {method} 失败：{error}"));
                    }
                    return Ok(message.get("result").cloned().unwrap_or(Value::Null));
                }
                Message::Close(_) => return Err(anyhow!("CDP 连接已关闭")),
                // Ping 由 tungstenite 自动回 Pong；Pong / Binary 与本协议无关
                _ => {}
            }
        }
    }

    /// 发送 Browser.close 并关闭 WebSocket（失败静默由调用方兜底 kill）。
    pub async fn close(&mut self) -> Result<()> {
        // Browser.close 会让浏览器自行优雅退出；失败不阻塞清理流程。
        let _ = self.call("Browser.close", Value::Null).await;
        self.ws.close(None).await?;
        Ok(())
    }
}
