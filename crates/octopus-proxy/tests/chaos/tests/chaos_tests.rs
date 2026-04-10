//! Chaos tests entry point

mod helpers;
mod test_network_failures;
mod test_upstream_failures;
mod test_resource_limits;

pub use helpers::{ToxiproxyClient, Toxic};

/// Check if Toxiproxy is available before running tests
pub async fn ensure_toxiproxy_available() -> bool {
    let client = ToxiproxyClient::new();
    client.is_available().await
}
