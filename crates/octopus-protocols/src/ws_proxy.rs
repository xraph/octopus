//! WebSocket proxying functionality
//!
//! Handles upgrading HTTP connections to WebSocket and proxying frames
//! between client and upstream server.

use futures::{SinkExt, StreamExt};
use http::{HeaderMap, Uri};
use octopus_core::{Error, Result};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info, warn};

/// WebSocket proxy
#[derive(Debug)]
pub struct WebSocketProxy {
    /// Maximum message size in bytes
    pub max_message_size: usize,
}

impl WebSocketProxy {
    /// Create a new WebSocket proxy
    pub fn new(max_message_size: usize) -> Self {
        Self { max_message_size }
    }

    /// Proxy WebSocket connection to upstream
    ///
    /// This upgrades the client connection to WebSocket and establishes
    /// a WebSocket connection to the upstream server, then bidirectionally
    /// forwards messages between them.
    pub async fn proxy_websocket(
        &self,
        client_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
        upstream_uri: Uri,
        _headers: HeaderMap,
    ) -> Result<()> {
        info!(
            upstream = %upstream_uri,
            "Establishing WebSocket connection to upstream"
        );

        // Connect to upstream WebSocket server
        let (upstream_stream, _response) =
            connect_async(upstream_uri.to_string()).await.map_err(|e| {
                Error::Internal(format!("Failed to connect to upstream WebSocket: {}", e))
            })?;

        debug!("WebSocket connection to upstream established");

        // Split both streams
        let (mut client_write, mut client_read) = client_stream.split();
        let (mut upstream_write, mut upstream_read) = upstream_stream.split();

        // Bidirectional forwarding
        let max_size = self.max_message_size;
        let client_to_upstream = async move {
            while let Some(msg) = client_read.next().await {
                match msg {
                    Ok(message) => {
                        // Check message size
                        let size = match &message {
                            Message::Text(text) => text.len(),
                            Message::Binary(data) => data.len(),
                            _ => 0,
                        };

                        if size > max_size {
                            warn!(size, max = max_size, "Message size exceeds limit");
                            break;
                        }

                        debug!(
                            msg_type = ?message,
                            size,
                            "Client -> Upstream"
                        );

                        if let Err(e) = upstream_write.send(message).await {
                            error!("Failed to send to upstream: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Client read error: {}", e);
                        break;
                    }
                }
            }
        };

        let upstream_to_client = async move {
            while let Some(msg) = upstream_read.next().await {
                match msg {
                    Ok(message) => {
                        debug!(
                            msg_type = ?message,
                            "Upstream -> Client"
                        );

                        if let Err(e) = client_write.send(message).await {
                            error!("Failed to send to client: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Upstream read error: {}", e);
                        break;
                    }
                }
            }
        };

        // Run both directions concurrently
        tokio::select! {
            _ = client_to_upstream => {
                debug!("Client->Upstream stream closed");
            }
            _ = upstream_to_client => {
                debug!("Upstream->Client stream closed");
            }
        }

        info!("WebSocket proxy connection closed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_proxy_creation() {
        let proxy = WebSocketProxy::new(64 * 1024 * 1024);
        assert_eq!(proxy.max_message_size, 64 * 1024 * 1024);
    }
}
