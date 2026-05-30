//! Integration test runner for octopus-proxy

// Tests favor `let mut cfg = T::default(); cfg.field = ...;` for readability.
#![allow(clippy::field_reassign_with_default)]

mod helpers;
mod test_observability;
mod test_proxy_basic;
mod test_resilience;
mod test_routing;
mod test_security;
mod test_shutdown;

// Re-export helpers for use in test modules
pub use helpers::{MockConfig, MockResponse, MockUpstream, TestFixtures};
