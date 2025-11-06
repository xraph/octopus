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
pub use websocket::WebSocketHandler;
pub use ws_proxy::WebSocketProxy;

/// Re-export commonly used types
pub mod prelude {
    pub use crate::graphql::{GraphQLHandler, GraphQLRequest, GraphQLResponse};
    pub use crate::grpc::GrpcHandler;
    pub use crate::handler::{ProtocolHandler, ProtocolType};
    pub use crate::http::HttpHandler;
    pub use crate::sse::SseHandler;
    pub use crate::websocket::WebSocketHandler;
}

