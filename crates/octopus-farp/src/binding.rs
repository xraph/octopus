//! Binding FARP-discovered routes to a virtual gateway.
//!
//! By default FARP registers discovered services as host-agnostic routes
//! (`/{service}/...` on any host). A [`GatewayBinding`] scopes them to the
//! gateway's hostnames (e.g. `api.example.cloud` and `*.example.dev`): every
//! FARP route lands on each listed hostname and on no other. The binding also
//! attaches the routes to that gateway for attribution and applies its default
//! auth provider, rate limit and timeout, so a federated API surface lives
//! under the gateway's hosts with service-scoped prefixes.

use arc_swap::ArcSwap;
use octopus_router::{HostMatch, RouteBuilder};
use std::sync::Arc;
use std::time::Duration;

/// A hot-swappable, shareable FARP gateway binding.
///
/// The k8s controller and the admin API can update it at runtime, and both the
/// push handler and the discovery watcher observe the change, because they
/// hold clones of the same cell.
pub type BindingCell = Arc<ArcSwap<Option<GatewayBinding>>>;

/// Create an empty (unbound) binding cell.
pub fn new_binding_cell() -> BindingCell {
    Arc::new(ArcSwap::from_pointee(None))
}

/// Scopes FARP-discovered routes to a virtual gateway.
#[derive(Debug, Clone)]
pub struct GatewayBinding {
    /// Hosts every FARP route is copied onto (the gateway's hostnames). Empty
    /// means host-agnostic routes that still carry the rest of the policy.
    pub hosts: Vec<HostMatch>,
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
    /// Create a binding for one `hostname` (Gateway API syntax: exact or
    /// `*.suffix`). An empty hostname or `*` gives host-agnostic routes, as
    /// the legacy `farp.gateway.hostname` always did.
    pub fn new(hostname: &str) -> Self {
        Self::for_hostnames(&[hostname.to_string()])
    }

    /// Create a binding for every hostname in `hostnames`.
    ///
    /// Each one is normalized the way configuration hosts are (lowercase, one
    /// trailing dot dropped) and duplicates are dropped. A value the strict
    /// parser rejects falls back to the lenient one, because the legacy
    /// `hostname` key and Kubernetes resources reach here without strict
    /// validation. Blank values and `*` are skipped, so a list of only those
    /// (or an empty one) gives host-agnostic routes and `hosts` never holds
    /// [`HostMatch::Any`].
    pub fn for_hostnames(hostnames: &[String]) -> Self {
        let mut hosts = Vec::with_capacity(hostnames.len());
        for hostname in hostnames
            .iter()
            .map(|h| h.trim())
            .filter(|h| !h.is_empty() && *h != "*")
        {
            let host =
                HostMatch::parse_pattern(hostname).unwrap_or_else(|_| HostMatch::parse(hostname));
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }
        Self::with_hosts(hosts)
    }

    fn with_hosts(hosts: Vec<HostMatch>) -> Self {
        Self {
            hosts,
            gateway_id: None,
            default_auth_provider: None,
            rate_limit: None,
            timeout: None,
        }
    }

    /// The bound hostnames as patterns (`api.example.cloud`, `*.example.dev`).
    /// A host-agnostic entry has no pattern and is left out.
    pub fn hostnames(&self) -> Vec<String> {
        self.hosts.iter().filter_map(HostMatch::pattern).collect()
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

/// Apply `binding` to a FARP route `builder`, one builder per bound host.
///
/// The `builder` must already have its path/method/upstream/strip set. Each
/// returned builder carries the gateway id, the binding's rate limit and
/// timeout, and the default auth provider when `route_has_auth` is false.
/// A `None` binding, or one with no hosts, gives back a single builder that
/// keeps its own host (host-agnostic unless the caller set one).
pub fn expand_gateway_binding(
    builder: RouteBuilder,
    binding: Option<&GatewayBinding>,
    route_has_auth: bool,
) -> Vec<RouteBuilder> {
    let Some(binding) = binding else {
        return vec![builder];
    };
    let mut builder = builder.gateway_id(binding.gateway_id.as_deref());
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
    if binding.hosts.is_empty() {
        return vec![builder];
    }
    binding
        .hosts
        .iter()
        .map(|host| builder.clone().host(host.clone()))
        .collect()
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
        let binding = GatewayBinding::new("api.example.cloud")
            .with_gateway_id(Some("platform-api".into()))
            .with_default_auth(Some("jwt".into()));
        let route = single(expand_gateway_binding(
            base_builder(),
            Some(&binding),
            false,
        ));
        assert_eq!(route.host, HostMatch::Exact("api.example.cloud".into()));
        assert_eq!(route.gateway_id.as_deref(), Some("platform-api"));
        assert_eq!(route.auth_provider.as_deref(), Some("jwt"));
    }

    #[test]
    fn binding_does_not_override_existing_auth() {
        let binding =
            GatewayBinding::new("api.example.cloud").with_default_auth(Some("jwt".into()));
        // route_has_auth = true → the route's own auth provider must be preserved.
        let route = single(expand_gateway_binding(
            base_builder().auth_provider(Some("service-specific")),
            Some(&binding),
            true,
        ));
        assert_eq!(route.auth_provider.as_deref(), Some("service-specific"));
    }

    #[test]
    fn no_binding_leaves_route_host_agnostic() {
        let route = single(expand_gateway_binding(base_builder(), None, false));
        assert_eq!(route.host, HostMatch::Any);
        assert!(route.gateway_id.is_none());
    }

    #[test]
    fn binding_applies_rate_limit_and_timeout() {
        let binding = GatewayBinding::new("api.example.cloud")
            .with_rate_limit(Some((100, Duration::from_secs(60))))
            .with_timeout(Some(Duration::from_secs(7)));
        let route = single(expand_gateway_binding(
            base_builder(),
            Some(&binding),
            false,
        ));
        assert_eq!(route.rate_limit, Some((100, Duration::from_secs(60))));
        assert_eq!(route.timeout, Some(Duration::from_secs(7)));
    }

    fn single(builders: Vec<RouteBuilder>) -> octopus_router::Route {
        assert_eq!(builders.len(), 1);
        builders.into_iter().next().unwrap().build().unwrap()
    }

    fn built(builders: Vec<RouteBuilder>) -> Vec<octopus_router::Route> {
        builders.into_iter().map(|b| b.build().unwrap()).collect()
    }

    #[test]
    fn binding_with_two_hostnames_expands_to_two_routes() {
        let binding = GatewayBinding::for_hostnames(&["a.dev".into(), "*.b.dev".into()])
            .with_gateway_id(Some("platform-api".into()));
        let routes = built(expand_gateway_binding(
            base_builder(),
            Some(&binding),
            false,
        ));
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].host, HostMatch::Exact("a.dev".into()));
        assert_eq!(routes[1].host, HostMatch::Wildcard(".b.dev".into()));
        assert!(routes
            .iter()
            .all(|r| r.gateway_id.as_deref() == Some("platform-api")));
        assert!(routes.iter().all(|r| r.path == "/orders/list"));
    }

    #[test]
    fn no_binding_expands_to_one_host_agnostic_route() {
        let routes = built(expand_gateway_binding(base_builder(), None, false));
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].host, HostMatch::Any);
        assert!(routes[0].gateway_id.is_none());
    }

    #[test]
    fn empty_hostnames_binding_is_host_agnostic_but_keeps_policy() {
        let binding = GatewayBinding::for_hostnames(&[])
            .with_gateway_id(Some("platform-api".into()))
            .with_default_auth(Some("jwt".into()));
        let routes = built(expand_gateway_binding(
            base_builder(),
            Some(&binding),
            false,
        ));
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].host, HostMatch::Any);
        assert_eq!(routes[0].auth_provider.as_deref(), Some("jwt"));
        assert_eq!(routes[0].gateway_id.as_deref(), Some("platform-api"));
    }

    #[test]
    fn for_hostnames_normalizes_and_dedupes() {
        let binding = GatewayBinding::for_hostnames(&[
            "API.Acme.Dev.".into(),
            "api.acme.dev".into(),
            " ".into(),
        ]);
        assert_eq!(binding.hosts, vec![HostMatch::Exact("api.acme.dev".into())]);
    }

    #[test]
    fn star_and_blank_hostnames_mean_host_agnostic() {
        let binding = GatewayBinding::for_hostnames(&["*".into(), String::new()]);
        assert!(binding.hosts.is_empty());
        assert!(binding.hostnames().is_empty());

        let binding = GatewayBinding::for_hostnames(&["*".into(), "a.dev".into()]);
        assert_eq!(binding.hosts, vec![HostMatch::Exact("a.dev".into())]);

        assert!(GatewayBinding::new("*").hosts.is_empty());
        assert!(GatewayBinding::new("").hosts.is_empty());
    }

    #[test]
    fn legacy_hostname_the_strict_parser_rejects_still_binds() {
        let binding = GatewayBinding::new("my_svc.local");
        assert_eq!(binding.hosts, vec![HostMatch::Exact("my_svc.local".into())]);
    }

    #[test]
    fn wildcard_hostname_parses_to_wildcard_match() {
        let binding = GatewayBinding::new("*.example.cloud");
        assert_eq!(
            binding.hosts,
            vec![HostMatch::Wildcard(".example.cloud".into())]
        );
    }
}
