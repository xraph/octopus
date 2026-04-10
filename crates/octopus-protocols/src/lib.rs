//! Protocol handlers for Octopus API Gateway

#![warn(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::cargo,
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod graphql;
pub mod grpc;
pub mod handler;
pub mod http;
pub mod sse;
pub mod websocket;
pub mod ws_proxy;

pub use graphql::{GraphQLHandler, GraphQLRequest, GraphQLResponse};
pub use grpc::GrpcHandler;
pub use handler::{ProtocolHandler, ProtocolType};
pub use websocket::{
    build_upgrade_response, is_websocket_upgrade, WebSocketConfig,
};
pub use ws_proxy::{
    build_forwarded_headers, connect_upstream, proxy_websocket_connected, WebSocketSessionStats,
};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::graphql::{GraphQLHandler, GraphQLRequest, GraphQLResponse};
    pub use crate::grpc::GrpcHandler;
    pub use crate::handler::{ProtocolHandler, ProtocolType};
    pub use crate::http::HttpHandler;
    pub use crate::sse::SseHandler;
    pub use crate::websocket::{is_websocket_upgrade, build_upgrade_response, WebSocketConfig};
    pub use crate::ws_proxy::{connect_upstream, proxy_websocket_connected, build_forwarded_headers};
}
