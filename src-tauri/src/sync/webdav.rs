//! WebDAV 最小客户端：MKCOL 建目录 + PUT 上传 + GET 下载。
//!
//! 独立 wreq client，**不使用指纹伪装**（`.emulation()` 会注入 Chrome 头，
//! 对第三方 WebDAV 服务没有意义）：
//! - 总超时 30s，重定向 `Policy::none()`
//! - 默认跟随系统代理；连接 / 超时类错误用 `.no_proxy()` 直连重试一次
//! - Basic Auth 由 `username` + `password` 构造；两者都不进 URL、不进日志
//!
//! MKCOL 必须显式 `.body(Vec::new())`，否则 wreq 会发 chunked 编码，部分
//! WebDAV 服务（含 Nextcloud 的部分版本）会拒绝。

use std::time::Duration;

use futures_util::StreamExt;
use wreq::Method;

use super::{SyncError, REMOTE_FILE_NAME};

/// 单次请求总超时（秒）。
pub const REQUEST_TIMEOUT_SECS: u64 = 30;
/// 下载响应体上限 = 8 MiB。
pub const DOWNLOAD_LIMIT_BYTES: u64 = 8 * 1024 * 1024;
/// 同步客户端 UA（无浏览器指纹伪装）。
const USER_AGENT: &str = "pixiv-tool-sync/1";

/// 校验并归一化 WebDAV 目录地址（去尾斜杠；只允许 http/https，无查询 / 片段 / 内嵌凭据）。
fn normalize_base_url(raw: &str) -> Result<String, SyncError> {
    let trimmed = raw.trim().trim_end_matches('/');
    let url = tauri::Url::parse(trimmed).map_err(|_| SyncError::url_invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(SyncError::url_invalid());
    }
    Ok(trimmed.to_string())
}

/// 已校验的远端端点：目录地址 + 固定文件名 + Basic Auth 凭据。
#[derive(Debug, Clone)]
pub struct WebDavEndpoint {
    /// 目录地址（已去尾斜杠）。
    pub base_url: String,
    /// `${base_url}/pixiv-tool-sync.bin`。
    pub file_url: String,
    /// 用户名（空 = 匿名，不发 Authorization）。
    pub username: String,
    /// 密码（仅用于 Basic Auth，绝不进 URL / 日志）。
    pub password: String,
}

impl WebDavEndpoint {
    /// 构造端点；地址非法 → `sync_url_invalid`。
    pub fn new(base_url: &str, username: &str, password: &str) -> Result<Self, SyncError> {
        let base_url = normalize_base_url(base_url)?;
        let file_url = format!("{base_url}/{REMOTE_FILE_NAME}");
        Ok(Self {
            base_url,
            file_url,
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    fn authorize(&self, builder: wreq::RequestBuilder) -> wreq::RequestBuilder {
        if self.username.is_empty() {
            builder
        } else {
            builder.basic_auth(&self.username, Some(&self.password))
        }
    }
}

/// 构造同步专用客户端；`direct = true` 时强制直连（代理重试用）。
fn build_client(direct: bool) -> Result<wreq::Client, SyncError> {
    let mut headers = wreq::header::HeaderMap::new();
    headers.insert(
        wreq::header::USER_AGENT,
        wreq::header::HeaderValue::from_static(USER_AGENT),
    );
    let builder = wreq::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .redirect(wreq::redirect::Policy::none())
        .default_headers(headers);
    let builder = if direct { builder.no_proxy() } else { builder };
    builder
        .build()
        .map_err(|_| SyncError::internal("创建同步 HTTP 客户端失败"))
}

/// 网络层错误 → `sync_network_error`（文案不含 URL / 凭据）。
fn map_send_error(err: wreq::Error) -> SyncError {
    if err.is_timeout() {
        SyncError::network("连接 WebDAV 服务超时")
    } else if err.is_connect() || err.is_dns() {
        SyncError::network("无法连接 WebDAV 服务")
    } else {
        SyncError::network("WebDAV 请求失败")
    }
}

/// MKCOL 状态码判定：认证失败 / 明确的失败状态 / 可继续。
fn check_mkcol_status(status: u16) -> Result<u16, SyncError> {
    match status {
        // 已存在、已创建、无内容、重定向、不支持 MKCOL、父目录缺失：
        // 都继续往下走，最终由 PUT 的成功与否定性（409 会在 PUT 报错）。
        200 | 201 | 204 | 301 | 302 | 405 | 409 => Ok(status),
        401 | 403 => Err(SyncError::auth_failed()),
        other => Err(SyncError::http_status(other)),
    }
}

/// PUT 状态码判定。
fn check_put_status(status: u16) -> Result<u16, SyncError> {
    match status {
        200 | 201 | 204 => Ok(status),
        401 | 403 => Err(SyncError::auth_failed()),
        other => Err(SyncError::http_status(other)),
    }
}

/// GET 状态码判定。
fn check_get_status(status: u16) -> Result<(), SyncError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    match status {
        404 => Err(SyncError::not_found()),
        401 | 403 => Err(SyncError::auth_failed()),
        other => Err(SyncError::http_status(other)),
    }
}

/// MKCOL 目录（单次尝试）。
async fn mkcol_once(client: &wreq::Client, endpoint: &WebDavEndpoint) -> Result<u16, SyncError> {
    let method = Method::from_bytes(b"MKCOL").expect("MKCOL 是合法 HTTP 方法");
    let response = endpoint
        .authorize(client.request(method, &endpoint.base_url))
        // 必须显式给空 body：否则 wreq 会发 chunked 编码；
        // 且空 Vec 不会自动带 Content-Length，需显式补 0（部分 WebDAV 服务要求）。
        .header(wreq::header::CONTENT_LENGTH, "0")
        .body(Vec::new())
        .send()
        .await
        .map_err(map_send_error)?;
    check_mkcol_status(response.status().as_u16())
}

/// PUT 上传（单次尝试）。
async fn put_once(
    client: &wreq::Client,
    endpoint: &WebDavEndpoint,
    bytes: &[u8],
) -> Result<u16, SyncError> {
    let response = endpoint
        .authorize(client.put(&endpoint.file_url))
        .body(bytes.to_vec())
        .send()
        .await
        .map_err(map_send_error)?;
    check_put_status(response.status().as_u16())
}

/// GET 下载（单次尝试，带 8 MiB 上限）。
async fn get_once(client: &wreq::Client, endpoint: &WebDavEndpoint) -> Result<Vec<u8>, SyncError> {
    let response = endpoint
        .authorize(client.get(&endpoint.file_url))
        .send()
        .await
        .map_err(map_send_error)?;
    check_get_status(response.status().as_u16())?;
    if response
        .content_length()
        .is_some_and(|len| len > DOWNLOAD_LIMIT_BYTES)
    {
        return Err(SyncError::oversize(DOWNLOAD_LIMIT_BYTES));
    }
    // 流式读取并逐块计长：即使服务端谎报 / 省略 Content-Length，也不会把
    // 超大响应整个读进内存（超限立刻中止）。
    let mut stream = response.bytes_stream();
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(map_send_error)?;
        if (out.len() + chunk.len()) as u64 > DOWNLOAD_LIMIT_BYTES {
            return Err(SyncError::oversize(DOWNLOAD_LIMIT_BYTES));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// 测试连接：建目录（已存在也算通）。返回实际 MKCOL 状态码。
pub async fn test_connection(endpoint: &WebDavEndpoint) -> Result<u16, SyncError> {
    let first = mkcol_once(&build_client(false)?, endpoint).await;
    match first {
        Err(error) if error.code == "sync_network_error" => {
            mkcol_once(&build_client(true)?, endpoint).await
        }
        other => other,
    }
}

/// 上传：先 MKCOL 再 PUT。返回 PUT 的状态码。
pub async fn upload(endpoint: &WebDavEndpoint, bytes: &[u8]) -> Result<u16, SyncError> {
    // MKCOL 失败（认证 / HTTP 状态）直接返回，不写数据。
    let client = build_client(false)?;
    match mkcol_once(&client, endpoint).await {
        Ok(_) => {}
        Err(error) if error.code == "sync_network_error" => {
            // 代理链路失败：直连重试一次（仅一次，避免重复写）。
            let direct = build_client(true)?;
            mkcol_once(&direct, endpoint).await?;
            return put_once(&direct, endpoint, bytes).await;
        }
        Err(error) => return Err(error),
    }
    match put_once(&client, endpoint, bytes).await {
        Err(error) if error.code == "sync_network_error" => {
            let direct = build_client(true)?;
            put_once(&direct, endpoint, bytes).await
        }
        other => other,
    }
}

/// 下载远端包。
pub async fn download(endpoint: &WebDavEndpoint) -> Result<Vec<u8>, SyncError> {
    let first = get_once(&build_client(false)?, endpoint).await;
    match first {
        Err(error) if error.code == "sync_network_error" => {
            get_once(&build_client(true)?, endpoint).await
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[test]
    fn url_validation_and_file_path() {
        let endpoint = WebDavEndpoint::new(
            " https://dav.example.com/remote.php/dav/files/u/ ",
            "alice",
            "pw",
        )
        .unwrap();
        assert_eq!(
            endpoint.base_url,
            "https://dav.example.com/remote.php/dav/files/u"
        );
        assert_eq!(
            endpoint.file_url,
            "https://dav.example.com/remote.php/dav/files/u/pixiv-tool-sync.bin"
        );

        for bad in [
            "",
            "ftp://dav.example.com/x",
            "dav.example.com/x",
            "https://dav.example.com/x?a=1",
            "https://dav.example.com/x#frag",
            "https://user:pw@dav.example.com/x",
        ] {
            assert_eq!(
                WebDavEndpoint::new(bad, "u", "p").unwrap_err().code,
                "sync_url_invalid",
                "{bad} 不应通过"
            );
        }
    }

    #[test]
    fn status_mapping() {
        // MKCOL：已存在 / 不支持 / 重定向 / 父目录缺失都可继续
        for status in [200, 201, 204, 301, 302, 405, 409] {
            assert_eq!(check_mkcol_status(status).unwrap(), status);
        }
        assert_eq!(
            check_mkcol_status(401).unwrap_err().code,
            "sync_auth_failed"
        );
        assert_eq!(
            check_mkcol_status(403).unwrap_err().code,
            "sync_auth_failed"
        );
        let error = check_mkcol_status(500).unwrap_err();
        assert_eq!(error.code, "sync_http_error");
        assert!(error.message.contains("500"));

        // PUT
        for status in [200, 201, 204] {
            assert_eq!(check_put_status(status).unwrap(), status);
        }
        assert_eq!(check_put_status(403).unwrap_err().code, "sync_auth_failed");
        assert_eq!(check_put_status(507).unwrap_err().code, "sync_http_error");

        // GET
        check_get_status(200).unwrap();
        assert_eq!(check_get_status(404).unwrap_err().code, "sync_not_found");
        assert_eq!(check_get_status(401).unwrap_err().code, "sync_auth_failed");
        assert_eq!(check_get_status(503).unwrap_err().code, "sync_http_error");
    }

    /// 本地 TcpListener mock：校验 MKCOL 空 body 与 Content-Length: 0、
    /// PUT/GET 方法行、Basic Auth 头；不访问外网。
    #[tokio::test]
    async fn wire_format_mkcol_put_get() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let mut captured = Vec::new();
            for _ in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let (head, body) = read_request(&mut socket).await;
                let request_line = head.lines().next().unwrap_or_default().to_string();
                let is_put = request_line.starts_with("PUT ");
                let is_get = request_line.starts_with("GET ");
                let response = if is_get {
                    "HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\n04\x01\x02\x03\x04remote".to_string()
                } else {
                    assert!(
                        is_put || request_line.starts_with("MKCOL "),
                        "{request_line}"
                    );
                    "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_string()
                };
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.flush().await.unwrap();
                let _ = socket.shutdown().await;
                captured.push((head, body));
            }
            captured
        });

        let endpoint =
            WebDavEndpoint::new(&format!("http://{addr}/dav"), "alice", "s3cret").unwrap();
        let client = build_client(true).unwrap();

        assert_eq!(mkcol_once(&client, &endpoint).await.unwrap(), 201);
        let payload = b"encrypted-package-bytes".to_vec();
        assert_eq!(put_once(&client, &endpoint, &payload).await.unwrap(), 201);
        let downloaded = get_once(&client, &endpoint).await.unwrap();
        assert_eq!(downloaded, b"04\x01\x02\x03\x04remote");

        let captured = server.await.unwrap();
        assert_eq!(captured.len(), 3);

        // ① MKCOL 目录：空 body + Content-Length: 0，绝不能是 chunked
        let (mkcol_head, mkcol_body) = &captured[0];
        assert!(
            mkcol_head.starts_with("MKCOL /dav HTTP/1.1\r\n"),
            "{mkcol_head}"
        );
        let mkcol_lower = mkcol_head.to_ascii_lowercase();
        assert!(mkcol_lower.contains("content-length: 0"), "{mkcol_head}");
        assert!(!mkcol_lower.contains("transfer-encoding"), "{mkcol_head}");
        assert!(mkcol_body.is_empty());

        // ② PUT 方法行 + 包体
        let (put_head, put_body) = &captured[1];
        assert!(
            put_head.starts_with("PUT /dav/pixiv-tool-sync.bin HTTP/1.1\r\n"),
            "{put_head}"
        );
        assert_eq!(put_body, &payload);
        let put_lower = put_head.to_ascii_lowercase();
        assert!(
            put_lower.contains(&format!("content-length: {}", payload.len())),
            "{put_head}"
        );
        assert!(!put_lower.contains("transfer-encoding"), "{put_head}");

        // ③ GET 方法行
        let (get_head, _) = &captured[2];
        assert!(
            get_head.starts_with("GET /dav/pixiv-tool-sync.bin HTTP/1.1\r\n"),
            "{get_head}"
        );

        // 三个请求都带 Basic Auth，且头部不出现明文密码
        let expected = "authorization: basic ywxpy2u6cznjcmv0";
        for (head, _) in &captured {
            let lower = head.to_ascii_lowercase();
            assert!(lower.contains(expected), "{head}");
            assert!(!lower.contains("s3cret"), "凭据原文出现在请求头: {head}");
        }
    }

    /// 下载超过 8 MiB 上限：按 Content-Length 提前拒绝，不读 body。
    #[tokio::test]
    async fn download_rejects_oversized_body() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = read_request(&mut socket).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                DOWNLOAD_LIMIT_BYTES + 1
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
            let _ = socket.shutdown().await;
        });

        let endpoint = WebDavEndpoint::new(&format!("http://{addr}/dav"), "", "").unwrap();
        let client = build_client(true).unwrap();
        let error = get_once(&client, &endpoint).await.unwrap_err();
        assert_eq!(error.code, "sync_oversize");
        server.await.unwrap();
    }

    /// 读一个 HTTP/1.1 请求（头部 + 按 Content-Length 读满 body）。
    async fn read_request(socket: &mut TcpStream) -> (String, Vec<u8>) {
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        let header_end = loop {
            let n = socket.read(&mut tmp).await.unwrap();
            assert!(n > 0, "连接在请求头完成前关闭");
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = find_header_end(&buf) {
                break pos;
            }
        };
        let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let mut body = buf[header_end + 4..].to_vec();
        let content_length = head
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                if key.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        while body.len() < content_length {
            let n = socket.read(&mut tmp).await.unwrap();
            assert!(n > 0, "连接在请求体完成前关闭");
            body.extend_from_slice(&tmp[..n]);
        }
        (head, body)
    }

    fn find_header_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4).position(|window| window == b"\r\n\r\n")
    }
}
