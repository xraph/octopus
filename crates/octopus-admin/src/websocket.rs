//! WebSocket hub for real-time dashboard events
//!
//! Provides live updates to connected admin dashboard clients via WebSocket.
//! Uses a broadcast channel for efficient fan-out to all connected clients.

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use uuid::Uuid;

/// WebSocket event message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsMessage {
    /// Event type (e.g., "`stats_update`", "`route_changed`", "`upstream_health`")
    pub msg_type: String,
    /// Event timestamp
    pub timestamp: DateTime<Utc>,
    /// Event payload
    pub data: serde_json::Value,
}

impl WsMessage {
    /// Create a new WebSocket message
    pub fn new(msg_type: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            msg_type: msg_type.into(),
            timestamp: Utc::now(),
            data,
        }
    }

    /// Create a stats update message
    #[must_use]
    pub fn stats_update(data: serde_json::Value) -> Self {
        Self::new("stats_update", data)
    }

    /// Create a route change message
    #[must_use]
    pub fn route_changed(data: serde_json::Value) -> Self {
        Self::new("route_changed", data)
    }

    /// Create an upstream health message
    #[must_use]
    pub fn upstream_health(data: serde_json::Value) -> Self {
        Self::new("upstream_health", data)
    }

    /// Create a circuit breaker state change message
    #[must_use]
    pub fn circuit_breaker(data: serde_json::Value) -> Self {
        Self::new("circuit_breaker", data)
    }

    /// Create a request completed message
    #[must_use]
    pub fn request_completed(data: serde_json::Value) -> Self {
        Self::new("request_completed", data)
    }
}

/// Connected WebSocket client
#[allow(dead_code)]
struct WsClient {
    id: Uuid,
    tx: mpsc::UnboundedSender<Message>,
}

/// WebSocket hub for managing connected clients and broadcasting events
pub struct WsHub {
    clients: Arc<RwLock<HashMap<Uuid, WsClient>>>,
    broadcast_tx: broadcast::Sender<WsMessage>,
}

impl WsHub {
    /// Create a new WebSocket hub
    #[must_use]
    pub fn new() -> Self {
        let (broadcast_tx, _) = broadcast::channel(256);
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            broadcast_tx,
        }
    }

    /// Broadcast a message to all connected clients
    pub fn broadcast(&self, msg: WsMessage) {
        // Ignore send errors (no receivers)
        let _ = self.broadcast_tx.send(msg);
    }

    /// Get the number of connected clients
    pub async fn client_count(&self) -> usize {
        self.clients.read().await.len()
    }

    /// Handle a WebSocket upgrade request
    pub async fn handle_upgrade(hub: Arc<Self>, ws: WebSocketUpgrade) -> impl IntoResponse {
        ws.on_upgrade(move |socket| Self::handle_connection(hub, socket))
    }

    /// Handle a new WebSocket connection
    async fn handle_connection(hub: Arc<Self>, ws: WebSocket) {
        let client_id = Uuid::new_v4();
        let (mut ws_sender, mut ws_receiver) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

        // Register client
        {
            let client = WsClient {
                id: client_id,
                tx: tx.clone(),
            };
            hub.clients.write().await.insert(client_id, client);
        }

        tracing::info!(client_id = %client_id, "WebSocket client connected");

        // Subscribe to broadcast
        let mut broadcast_rx = hub.broadcast_tx.subscribe();

        // Spawn writer task: receives from broadcast and sends to WebSocket
        let writer_hub = hub.clone();
        let writer_id = client_id;
        let writer_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Messages from broadcast channel
                    result = broadcast_rx.recv() => {
                        match result {
                            Ok(msg) => {
                                if let Ok(json) = serde_json::to_string(&msg) {
                                    if ws_sender.send(Message::Text(json)).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                tracing::warn!(client_id = %writer_id, lagged = n, "WebSocket client lagged");
                            }
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    // Direct messages from channel
                    msg = rx.recv() => {
                        match msg {
                            Some(msg) => {
                                if ws_sender.send(msg).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            // Cleanup
            writer_hub.clients.write().await.remove(&writer_id);
            tracing::info!(client_id = %writer_id, "WebSocket client disconnected (writer)");
        });

        // Reader task: read from WebSocket (handle ping/pong and close)
        let reader_hub = hub.clone();
        let reader_task = tokio::spawn(async move {
            while let Some(result) = ws_receiver.next().await {
                match result {
                    Ok(Message::Ping(data)) => {
                        // Respond with pong via the direct channel
                        let _ = tx.send(Message::Pong(data));
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {} // Ignore text/binary from client
                }
            }
            reader_hub.clients.write().await.remove(&client_id);
            tracing::info!(client_id = %client_id, "WebSocket client disconnected (reader)");
        });

        // Wait for either task to finish
        tokio::select! {
            _ = writer_task => {}
            _ = reader_task => {}
        }
    }
}

impl Default for WsHub {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for WsHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsHub").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_message_serialization() {
        let msg = WsMessage::new("test_event", serde_json::json!({"key": "value"}));
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("test_event"));
        assert!(json.contains("key"));

        let parsed: WsMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.msg_type, "test_event");
    }

    #[test]
    fn test_ws_message_constructors() {
        let msg = WsMessage::stats_update(serde_json::json!({"rps": 1000}));
        assert_eq!(msg.msg_type, "stats_update");

        let msg = WsMessage::route_changed(serde_json::json!({"route": "/api"}));
        assert_eq!(msg.msg_type, "route_changed");

        let msg = WsMessage::upstream_health(serde_json::json!({"healthy": true}));
        assert_eq!(msg.msg_type, "upstream_health");

        let msg = WsMessage::circuit_breaker(serde_json::json!({"state": "open"}));
        assert_eq!(msg.msg_type, "circuit_breaker");

        let msg = WsMessage::request_completed(serde_json::json!({"latency_ms": 42}));
        assert_eq!(msg.msg_type, "request_completed");
    }

    #[tokio::test]
    async fn test_hub_broadcast_no_clients() {
        let hub = WsHub::new();
        // Should not panic
        hub.broadcast(WsMessage::new("test", serde_json::json!({})));
        assert_eq!(hub.client_count().await, 0);
    }

    #[tokio::test]
    async fn test_hub_broadcast_reaches_subscriber() {
        let hub = WsHub::new();
        let mut rx = hub.broadcast_tx.subscribe();

        hub.broadcast(WsMessage::new("test", serde_json::json!({"n": 1})));

        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.msg_type, "test");
        assert_eq!(msg.data["n"], 1);
    }

    #[tokio::test]
    async fn test_hub_multiple_broadcasts() {
        let hub = WsHub::new();
        let mut rx = hub.broadcast_tx.subscribe();

        for i in 0..5 {
            hub.broadcast(WsMessage::new("event", serde_json::json!({"i": i})));
        }

        for i in 0..5 {
            let msg = rx.recv().await.unwrap();
            assert_eq!(msg.data["i"], i);
        }
    }

    #[test]
    fn test_ws_message_has_timestamp() {
        let before = Utc::now();
        let msg = WsMessage::new("test", serde_json::json!({}));
        let after = Utc::now();
        assert!(msg.timestamp >= before);
        assert!(msg.timestamp <= after);
    }
}
