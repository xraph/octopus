//! Protocol handler trait

use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use octopus_core::Result;
use std::fmt;

/// Protocol type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolType {
    /// HTTP/REST
    Http,
    /// gRPC
    Grpc,
    /// WebSocket
    WebSocket,
    /// Server-Sent Events
    Sse,
    /// GraphQL
    GraphQL,
}

impl fmt::Display for ProtocolType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolType::Http => write!(f, "http"),
            ProtocolType::Grpc => write!(f, "grpc"),
            ProtocolType::WebSocket => write!(f, "websocket"),
            ProtocolType::Sse => write!(f, "sse"),
            ProtocolType::GraphQL => write!(f, "graphql"),
        }
    }
}

/// Protocol handler trait
#[async_trait]
pub trait ProtocolHandler: Send + Sync + fmt::Debug {
    /// Get the protocol type this handler supports
    fn protocol_type(&self) -> ProtocolType;

    /// Check if this handler can handle the given request
    fn can_handle(&self, req: &Request<Full<Bytes>>) -> bool;

    /// Handle the protocol-specific request
    async fn handle(&self, req: Request<Full<Bytes>>) -> Result<Response<Full<Bytes>>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_type_display() {
        assert_eq!(ProtocolType::Http.to_string(), "http");
        assert_eq!(ProtocolType::WebSocket.to_string(), "websocket");
        assert_eq!(ProtocolType::Grpc.to_string(), "grpc");
    }

    #[test]
    fn test_protocol_type_equality() {
        assert_eq!(ProtocolType::Http, ProtocolType::Http);
        assert_ne!(ProtocolType::Http, ProtocolType::Grpc);
    }
}


