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
// Subjective pedantic/nursery/cargo lints are muted; substantive lints stay active.
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::missing_const_for_fn,
    clippy::option_if_let_else,
    clippy::similar_names,
    clippy::field_reassign_with_default,
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::return_self_not_must_use,
    clippy::unnecessary_wraps,
    clippy::significant_drop_tightening,
    clippy::match_same_arms,
    clippy::manual_let_else,
    clippy::unused_self,
    clippy::unused_async,
    clippy::only_used_in_recursion,
    clippy::type_complexity,
    clippy::needless_pass_by_value,
    clippy::trivially_copy_pass_by_ref,
    clippy::missing_fields_in_debug,
    clippy::implicit_hasher,
    clippy::used_underscore_binding,
    clippy::struct_field_names,
    clippy::format_push_string,
    clippy::doc_markdown,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::multiple_crate_versions,
    clippy::cargo_common_metadata
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
pub use sse::{format_comment, format_data, format_event, is_sse_request};
pub use websocket::{build_upgrade_response, is_websocket_upgrade, WebSocketConfig};
pub use ws_proxy::{
    build_forwarded_headers, connect_upstream, proxy_websocket_connected, WebSocketSessionStats,
};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::graphql::{GraphQLHandler, GraphQLRequest, GraphQLResponse};
    pub use crate::grpc::GrpcHandler;
    pub use crate::handler::{ProtocolHandler, ProtocolType};
    pub use crate::http::HttpHandler;
    pub use crate::sse::{format_comment, format_data, format_event, is_sse_request};
    pub use crate::websocket::{build_upgrade_response, is_websocket_upgrade, WebSocketConfig};
    pub use crate::ws_proxy::{
        build_forwarded_headers, connect_upstream, proxy_websocket_connected,
    };
}
