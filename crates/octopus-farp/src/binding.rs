//! Binding FARP-discovered routes to a virtual gateway.
//!
//! By default FARP registers discovered services as host-agnostic routes
//! (`/{service}/...` on any host). A [`GatewayBinding`] scopes them to a single
//! gateway's hostname (e.g. `api.twinos.cloud`), attaches them to that gateway
//! for attribution, and applies its default auth provider — so a federated API
//! surface lives under one host with service-scoped prefixes.

use arc_swap::ArcSwap;
use octopus_router::{HostMatch, RouteBuilder};
use std::sync::Arc;
use std::time::Duration;

/// A hot-swappable, shareable FARP gateway binding.
///
/// The k8s controller can update it at runtime (CRD-driven) and both the push
/// handler and the discovery watcher observe the change, because they hold clones
/// of the same cell.
pub type BindingCell = Arc<ArcSwap<Option<GatewayBinding>>>;

/// Create an empty (unbound) binding cell.
pub fn new_binding_cell() -> BindingCell {
    Arc::new(ArcSwap::from_pointee(None))
}

/// Scopes FARP-discovered routes to a virtual gateway.
#[derive(Debug, Clone)]
pub struct GatewayBinding {
    /// Host all FARP routes are scoped to (the gateway's hostname).
    pub host: HostMatch,
    /// Virtual gateway id attached to each route (attribution / policy).
    pub gateway_id: Option<String>,
    /// Auth provider applied to FARP routes that don't already set one.
    pub default_auth_provider: Option<String>,
    /// Rate limit `(requests, window)` applied to FARP routes. FARP routes don't
    /// declare their own, so this gives the federated surface a gateway-wide cap.
    pub rate_limit: Option<(u32, Duration)>,
    /// Per-request timeout applied to FARP routes.
    pub timeout: Option<Duration>,
}

impl GatewayBinding {
    /// Create a binding for `hostname` (Gateway API syntax: exact or `*.suffix`).
    pub fn new(hostname: &str) -> Self {
        Self {
            host: HostMatch::parse(hostname),
            gateway_id: None,
            default_auth_provider: None,
            rate_limit: None,
            timeout: None,
        }
    }

    /// Set the virtual gateway id.
    #[must_use]
    pub fn with_gateway_id(mut self, id: Option<String>) -> Self {
        self.gateway_id = id;
        self
    }

    /// Set the default auth provider applied to routes without their own.
    #[must_use]
    pub fn with_default_auth(mut self, provider: Option<String>) -> Self {
        self.default_auth_provider = provider;
        self
    }

    /// Set the rate limit applied to FARP routes.
    #[must_use]
    pub fn with_rate_limit(mut self, rate_limit: Option<(u32, Duration)>) -> Self {
        self.rate_limit = rate_limit;
        self
    }

    /// Set the per-request timeout applied to FARP routes.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Apply `binding` to a FARP route `builder`, scoping it to the gateway host.
///
/// The `builder` must already have its path/method/upstream/strip set. This
/// attaches the gateway id and defaults the auth provider when `route_has_auth`
/// is false; a `None` binding leaves the builder host-agnostic (legacy behavior).
pub fn apply_gateway_binding(
    builder: RouteBuilder,
    binding: Option<&GatewayBinding>,
    route_has_auth: bool,
) -> RouteBuilder {
    let Some(binding) = binding else {
        return builder;
    };
    let mut builder = builder
        .host(binding.host.clone())
        .gateway_id(binding.gateway_id.as_deref());
    if !route_has_auth {
        if let Some(provider) = &binding.default_auth_provider {
            builder = builder.auth_provider(Some(provider));
        }
    }
    // FARP routes don't declare their own rate-limit/timeout, so the binding's
    // values give the federated surface gateway-wide policy parity.
    if let Some((requests, window)) = binding.rate_limit {
        builder = builder.rate_limit(requests, window);
    }
    if binding.timeout.is_some() {
        builder = builder.timeout(binding.timeout);
    }
    builder
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Method;

    fn base_builder() -> RouteBuilder {
        RouteBuilder::new()
            .method(Method::GET)
            .path("/orders/list")
            .upstream_name("orders")
    }

    #[test]
    fn binding_scopes_host_and_attaches_gateway() {
        let binding = GatewayBinding::new("api.twinos.cloud")
            .with_gateway_id(Some("platform-api".into()))
            .with_default_auth(Some("jwt".into()));
        let route = apply_gateway_binding(base_builder(), Some(&binding), false)
            .build()
            .unwrap();
        assert_eq!(route.host, HostMatch::Exact("api.twinos.cloud".into()));
        assert_eq!(route.gateway_id.as_deref(), Some("platform-api"));
        assert_eq!(route.auth_provider.as_deref(), Some("jwt"));
    }

    #[test]
    fn binding_does_not_override_existing_auth() {
        let binding = GatewayBinding::new("api.twinos.cloud").with_default_auth(Some("jwt".into()));
        // route_has_auth = true → the route's own auth provider must be preserved.
        let route = apply_gateway_binding(
            base_builder().auth_provider(Some("service-specific")),
            Some(&binding),
            true,
        )
        .build()
        .unwrap();
        assert_eq!(route.auth_provider.as_deref(), Some("service-specific"));
    }

    #[test]
    fn no_binding_leaves_route_host_agnostic() {
        let route = apply_gateway_binding(base_builder(), None, false)
            .build()
            .unwrap();
        assert_eq!(route.host, HostMatch::Any);
        assert!(route.gateway_id.is_none());
    }

    #[test]
    fn binding_applies_rate_limit_and_timeout() {
        let binding = GatewayBinding::new("api.twinos.cloud")
            .with_rate_limit(Some((100, Duration::from_secs(60))))
            .with_timeout(Some(Duration::from_secs(7)));
        let route = apply_gateway_binding(base_builder(), Some(&binding), false)
            .build()
            .unwrap();
        assert_eq!(route.rate_limit, Some((100, Duration::from_secs(60))));
        assert_eq!(route.timeout, Some(Duration::from_secs(7)));
    }

    #[test]
    fn wildcard_hostname_parses_to_wildcard_match() {
        let binding = GatewayBinding::new("*.twinos.cloud");
        assert_eq!(binding.host, HostMatch::Wildcard(".twinos.cloud".into()));
    }
}
