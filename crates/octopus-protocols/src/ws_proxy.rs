//! Zero-copy WebSocket proxy
//!
//! Production-grade bidirectional frame forwarding between client and upstream.
//!
//! Features:
//! - Zero-copy frame forwarding via `Bytes` reference counting
//! - Backpressure via `tokio::select!` — slow side throttles the fast side
//! - RFC 6455 §7.1.1 close handshake with configurable timeout
//! - Header forwarding (X-Forwarded-For, Origin, Cookie, etc.)
//! - Configurable connect timeout
//! - Frame/message size limits enforced via tungstenite config
//! - Ping/pong keepalive with dead connection detection
//! - Sends Close frame when peer disconnects unexpectedly

use crate::websocket::WebSocketConfig;
use futures::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{client::IntoClientRequest, protocol::Message},
    WebSocketStream,
};
use tracing::{debug, info, warn};

/// Statistics for a completed WebSocket session
#[derive(Debug, Clone)]
pub struct WebSocketSessionStats {
    /// Messages forwarded client → upstream
    pub client_to_upstream: u64,
    /// Messages forwarded upstream → client
    pub upstream_to_client: u64,
    /// Total bytes transferred in both directions
    pub bytes_transferred: u64,
    /// Session duration
    pub duration: Duration,
}

/// Connect to an upstream WebSocket server with timeout and header forwarding.
///
/// Returns the connected stream, or an error if connection fails/times out.
pub async fn connect_upstream(
    upstream_url: &str,
    forwarded_headers: &http::HeaderMap,
    config: &WebSocketConfig,
) -> Result<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, String> {
    // Build request with forwarded headers
    let mut request = upstream_url
        .into_client_request()
        .map_err(|e| format!("Invalid upstream WebSocket URL '{upstream_url}': {e}"))?;

    // Inject forwarded headers
    for (key, value) in forwarded_headers {
        request.headers_mut().insert(key, value.clone());
    }

    let ws_config = config.to_tungstenite_config();

    // Connect with timeout
    let (stream, _response) = tokio::time::timeout(config.connect_timeout, async {
        connect_async_with_config(request, Some(ws_config), false).await
    })
    .await
    .map_err(|_| format!("Upstream WebSocket connect timeout after {:?}", config.connect_timeout))?
    .map_err(|e| format!("Upstream WebSocket connect failed: {e}"))?;

    info!(upstream = %upstream_url, "Upstream WebSocket connected");
    Ok(stream)
}

/// Run a bidirectional WebSocket proxy between two already-connected streams.
///
/// This is the core proxy loop. Call this after both client upgrade and
/// upstream connect have succeeded.
pub async fn proxy_websocket_connected<C, U>(
    client_stream: WebSocketStream<C>,
    upstream_stream: WebSocketStream<U>,
    config: &WebSocketConfig,
) -> Result<WebSocketSessionStats, String>
where
    C: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    U: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let start = Instant::now();

    let (mut client_sink, mut client_rx) = client_stream.split();
    let (mut upstream_sink, mut upstream_rx) = upstream_stream.split();

    let mut c2u: u64 = 0;
    let mut u2c: u64 = 0;
    let mut bytes: u64 = 0;

    let mut ping_interval = tokio::time::interval(config.ping_interval);
    ping_interval.tick().await; // consume immediate first tick

    // ── Main bidirectional proxy loop ───────────────────────────────
    loop {
        tokio::select! {
            biased;

            // Client → Upstream
            msg = client_rx.next() => {
                match msg {
                    Some(Ok(Message::Close(frame))) => {
                        debug!("Client sent Close");
                        let _ = upstream_sink.send(Message::Close(frame)).await;
                        // Wait for upstream's Close reply (RFC 6455 §7.1.1)
                        drain_until_close(&mut upstream_rx, config.close_timeout).await;
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = client_sink.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(msg)) => {
                        bytes += msg.len() as u64;
                        c2u += 1;
                        if upstream_sink.send(msg).await.is_err() {
                            warn!("Upstream send failed");
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        debug!(error = %e, "Client error");
                        // Send Close to upstream before exiting
                        let _ = upstream_sink.send(Message::Close(None)).await;
                        break;
                    }
                    None => {
                        debug!("Client disconnected");
                        let _ = upstream_sink.send(Message::Close(None)).await;
                        break;
                    }
                }
            }

            // Upstream → Client
            msg = upstream_rx.next() => {
                match msg {
                    Some(Ok(Message::Close(frame))) => {
                        debug!("Upstream sent Close");
                        let _ = client_sink.send(Message::Close(frame)).await;
                        // Wait for client's Close reply
                        drain_until_close(&mut client_rx, config.close_timeout).await;
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = upstream_sink.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(msg)) => {
                        bytes += msg.len() as u64;
                        u2c += 1;
                        if client_sink.send(msg).await.is_err() {
                            warn!("Client send failed");
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        debug!(error = %e, "Upstream error");
                        let _ = client_sink.send(Message::Close(None)).await;
                        break;
                    }
                    None => {
                        debug!("Upstream disconnected");
                        let _ = client_sink.send(Message::Close(None)).await;
                        break;
                    }
                }
            }

            // Keepalive ping
            _ = ping_interval.tick() => {
                if client_sink.send(Message::Ping(vec![].into())).await.is_err() {
                    debug!("Keepalive ping failed");
                    break;
                }
            }
        }
    }

    let duration = start.elapsed();
    info!(c2u, u2c, bytes, ms = duration.as_millis() as u64, "WebSocket session closed");

    Ok(WebSocketSessionStats {
        client_to_upstream: c2u,
        upstream_to_client: u2c,
        bytes_transferred: bytes,
        duration,
    })
}

/// Drain a stream until we receive a Close frame or timeout.
///
/// Per RFC 6455 §7.1.1: after sending a Close frame, wait for the peer's
/// Close response before dropping the connection. If the peer doesn't respond
/// within `timeout`, we close anyway.
async fn drain_until_close<S>(stream: &mut S, timeout: Duration)
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let _ = tokio::time::timeout(timeout, async {
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(Message::Close(_)) => {
                    debug!("Received Close acknowledgment");
                    break;
                }
                Ok(_) => {} // ignore non-close frames during drain
                Err(_) => break,
            }
        }
    })
    .await;
}

/// Build forwarded headers from the original client request.
///
/// Extracts and forwards: X-Forwarded-For, X-Forwarded-Proto, X-Real-IP,
/// Origin, Cookie, Authorization, Sec-WebSocket-Protocol.
pub fn build_forwarded_headers<B>(req: &http::Request<B>) -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();

    // Forward existing X-Forwarded-For or create from connection
    if let Some(xff) = req.headers().get("x-forwarded-for") {
        headers.insert("x-forwarded-for", xff.clone());
    }
    if let Some(real_ip) = req.headers().get("x-real-ip") {
        headers.insert("x-real-ip", real_ip.clone());
    }

    // Protocol hint
    if let Ok(val) = "ws".parse() {
        headers.insert("x-forwarded-proto", val);
    }

    // Security-relevant headers
    if let Some(origin) = req.headers().get("origin") {
        headers.insert("origin", origin.clone());
    }
    if let Some(cookie) = req.headers().get("cookie") {
        headers.insert("cookie", cookie.clone());
    }
    if let Some(auth) = req.headers().get("authorization") {
        headers.insert("authorization", auth.clone());
    }

    // WebSocket subprotocol negotiation
    if let Some(proto) = req.headers().get("sec-websocket-protocol") {
        headers.insert("sec-websocket-protocol", proto.clone());
    }

    headers
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::Request;
    use http_body_util::Full;

    #[test]
    fn test_session_stats() {
        let stats = WebSocketSessionStats {
            client_to_upstream: 100,
            upstream_to_client: 200,
            bytes_transferred: 50_000,
            duration: Duration::from_secs(30),
        };
        assert_eq!(stats.client_to_upstream, 100);
        assert_eq!(stats.upstream_to_client, 200);
    }

    #[test]
    fn test_build_forwarded_headers_basic() {
        let req = Request::builder()
            .uri("/ws")
            .header("x-forwarded-for", "10.0.0.1")
            .header("origin", "https://example.com")
            .header("cookie", "session=abc123")
            .header("sec-websocket-protocol", "graphql-ws")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let headers = build_forwarded_headers(&req);
        assert_eq!(headers.get("x-forwarded-for").unwrap(), "10.0.0.1");
        assert_eq!(headers.get("origin").unwrap(), "https://example.com");
        assert_eq!(headers.get("cookie").unwrap(), "session=abc123");
        assert_eq!(headers.get("x-forwarded-proto").unwrap(), "ws");
        assert_eq!(headers.get("sec-websocket-protocol").unwrap(), "graphql-ws");
    }

    #[test]
    fn test_build_forwarded_headers_empty() {
        let req = Request::builder()
            .uri("/ws")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let headers = build_forwarded_headers(&req);
        assert!(headers.get("x-forwarded-for").is_none());
        assert!(headers.get("origin").is_none());
        assert_eq!(headers.get("x-forwarded-proto").unwrap(), "ws");
    }

    #[test]
    fn test_build_forwarded_headers_auth() {
        let req = Request::builder()
            .uri("/ws")
            .header("authorization", "Bearer tok123")
            .body(Full::new(Bytes::new()))
            .unwrap();

        let headers = build_forwarded_headers(&req);
        assert_eq!(headers.get("authorization").unwrap(), "Bearer tok123");
    }
}
