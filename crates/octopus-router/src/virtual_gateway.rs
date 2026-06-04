//! Virtual gateways: host-scoped route/policy partitions ("gateway-in-a-gateway").
//!
//! A *virtual gateway* groups routes and upstreams under a set of domains and
//! pushes inherited [`GatewayPolicy`] defaults onto its child routes. The
//! [`VirtualGatewayIndex`] maps an incoming request host to the virtual gateway
//! that owns it, reusing [`HostMatch`] specificity so an exact gateway
//! (`api.twinos.cloud`) and a wildcard tenant gateway (`*.twinos.cloud`) can
//! coexist and the most specific one wins.
//!
//! The index is consulted on the request hot path before route matching, so it
//! is a small specificity-sorted `Vec` scanned linearly (a handful of gateways
//! in practice) rather than a hash map.

use crate::host::HostMatch;
use crate::route::RouteCorsOverride;
use std::sync::Arc;
use std::time::Duration;

/// Defaults a virtual gateway pushes onto its child routes.
///
/// Each field is applied to a route only when that route does not set the value
/// explicitly — an explicit route field always wins over the gateway default.
#[derive(Debug, Clone, Default)]
pub struct GatewayPolicy {
    /// Default auth provider name for routes that don't specify one.
    pub auth_provider: Option<String>,
    /// Path prefix prepended to every child route's match path.
    pub base_path_prefix: Option<String>,
    /// Default CORS behavior for routes without their own override.
    pub cors: Option<RouteCorsOverride>,
    /// Default rate limit `(requests, window)` for routes without one.
    pub rate_limit: Option<(u32, Duration)>,
    /// Default per-request timeout for routes without one.
    pub timeout: Option<Duration>,
}

/// A single virtual gateway: an id, the domains it owns, and its default policy.
#[derive(Debug, Clone)]
pub struct GatewayEntry {
    /// Stable gateway identifier (e.g. `"platform-api"`); also used as the
    /// prefix for this gateway's upstream names to keep resource scopes isolated.
    pub id: Arc<str>,
    /// Domains this gateway owns. May mix exact and wildcard matchers.
    pub domains: Vec<HostMatch>,
    /// Defaults inherited by routes attached to this gateway.
    pub policy: GatewayPolicy,
}

impl GatewayEntry {
    /// Highest [`HostMatch::specificity`] among this gateway's domains
    /// (`0` if it owns no domains).
    fn max_specificity(&self) -> u8 {
        self.domains
            .iter()
            .map(HostMatch::specificity)
            .max()
            .unwrap_or(0)
    }

    /// Specificity of the most specific domain that matches `host`, or `None`
    /// if no domain matches.
    fn match_rank(&self, host: &str) -> Option<u8> {
        self.domains
            .iter()
            .filter(|d| d.matches(host))
            .map(HostMatch::specificity)
            .max()
    }
}

/// Host → virtual gateway lookup, ordered so the most specific match wins.
#[derive(Debug, Clone, Default)]
pub struct VirtualGatewayIndex {
    entries: Vec<GatewayEntry>,
}

impl VirtualGatewayIndex {
    /// Build an index from gateway entries. Entries are sorted by descending
    /// domain specificity so ties during [`resolve`](Self::resolve) deterministically
    /// prefer the more specific (and, among equals, the earlier-declared) gateway.
    pub fn new(mut entries: Vec<GatewayEntry>) -> Self {
        entries.sort_by_key(|e| std::cmp::Reverse(e.max_specificity()));
        Self { entries }
    }

    /// Resolve `host` (already lowercased) to the virtual gateway that owns it,
    /// choosing the gateway whose most specific matching domain ranks highest.
    /// Returns `None` when no gateway matches (no `HostMatch::Any` fallback exists).
    pub fn resolve(&self, host: &str) -> Option<&GatewayEntry> {
        let mut best: Option<(&GatewayEntry, u8)> = None;
        for entry in &self.entries {
            if let Some(rank) = entry.match_rank(host) {
                match best {
                    // strictly-greater keeps the earlier entry on a tie (entries
                    // are pre-sorted by specificity, so the earlier one is preferred)
                    Some((_, best_rank)) if best_rank >= rank => {}
                    _ => best = Some((entry, rank)),
                }
            }
        }
        best.map(|(entry, _)| entry)
    }

    /// Look up a gateway by its id (used to resolve a route's `gateway_id` to its
    /// policy during route application).
    pub fn by_id(&self, id: &str) -> Option<&GatewayEntry> {
        self.entries.iter().find(|e| &*e.id == id)
    }

    /// Attach a route to the virtual gateway that owns its host *scope*.
    ///
    /// Unlike [`resolve`](Self::resolve) (which takes a concrete request host),
    /// this takes a route's [`HostMatch`] — so a wildcard route attaches to the
    /// gateway that owns that wildcard (or a broader one). The most specific
    /// owning gateway wins. Returns `None` when no gateway owns the route's scope.
    pub fn attach(&self, route_host: &HostMatch) -> Option<&GatewayEntry> {
        let mut best: Option<(&GatewayEntry, u8)> = None;
        for entry in &self.entries {
            let rank = entry
                .domains
                .iter()
                .filter(|domain| host_match_within(route_host, domain))
                .map(HostMatch::specificity)
                .max();
            if let Some(rank) = rank {
                match best {
                    Some((_, best_rank)) if best_rank >= rank => {}
                    _ => best = Some((entry, rank)),
                }
            }
        }
        best.map(|(entry, _)| entry)
    }

    /// Number of gateways in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index holds no gateways.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Upstream cluster name scoped to a virtual gateway, so two gateways referencing
/// the same upstream get isolated load-balancer / circuit-breaker / pool state.
/// Ungated routes (`None`) keep the bare name (backward compatible).
pub fn gateway_scoped_upstream(gateway_id: Option<&str>, upstream: &str) -> String {
    match gateway_id {
        Some(gateway) => format!("{gateway}:{upstream}"),
        None => upstream.to_string(),
    }
}

/// Whether a route scoped to `route_host` is owned by a gateway domain `domain`.
///
/// - any `domain` of [`HostMatch::Any`] owns every route;
/// - an exact `domain` owns only the identical exact route;
/// - a wildcard `domain` owns an exact route it matches, and a wildcard route
///   whose suffix is the same or more specific (`.eu.acme.com` ⊆ `.acme.com`).
fn host_match_within(route_host: &HostMatch, domain: &HostMatch) -> bool {
    match (route_host, domain) {
        (_, HostMatch::Any) => true,
        (HostMatch::Exact(h), HostMatch::Exact(d)) => h == d,
        (HostMatch::Exact(h), HostMatch::Wildcard(_)) => domain.matches(h),
        (HostMatch::Wildcard(rs), HostMatch::Wildcard(ds)) => rs == ds || rs.ends_with(ds),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, domains: Vec<HostMatch>) -> GatewayEntry {
        GatewayEntry {
            id: Arc::from(id),
            domains,
            policy: GatewayPolicy::default(),
        }
    }

    fn twinos_index() -> VirtualGatewayIndex {
        VirtualGatewayIndex::new(vec![
            // declared wildcard-first on purpose to prove specificity wins over order
            entry("tenants", vec![HostMatch::parse("*.twinos.cloud")]),
            entry("platform-api", vec![HostMatch::parse("api.twinos.cloud")]),
            entry("platform-apps", vec![HostMatch::parse("app.twinos.cloud")]),
        ])
    }

    #[test]
    fn exact_gateway_wins_over_wildcard_for_its_host() {
        let idx = twinos_index();
        assert_eq!(&*idx.resolve("api.twinos.cloud").unwrap().id, "platform-api");
        assert_eq!(&*idx.resolve("app.twinos.cloud").unwrap().id, "platform-apps");
    }

    #[test]
    fn wildcard_gateway_owns_tenant_subdomains() {
        let idx = twinos_index();
        assert_eq!(
            &*idx.resolve("customer-a.twinos.cloud").unwrap().id,
            "tenants"
        );
        assert_eq!(
            &*idx.resolve("customer-b.twinos.cloud").unwrap().id,
            "tenants"
        );
    }

    #[test]
    fn any_gateway_is_the_fallback() {
        let idx = VirtualGatewayIndex::new(vec![
            entry("platform-api", vec![HostMatch::parse("api.twinos.cloud")]),
            entry("default", vec![HostMatch::Any]),
        ]);
        assert_eq!(&*idx.resolve("api.twinos.cloud").unwrap().id, "platform-api");
        // unrelated host falls through to the Any gateway
        assert_eq!(&*idx.resolve("random.example.org").unwrap().id, "default");
    }

    #[test]
    fn no_match_without_a_fallback_returns_none() {
        let idx = VirtualGatewayIndex::new(vec![entry(
            "platform-api",
            vec![HostMatch::parse("api.twinos.cloud")],
        )]);
        assert!(idx.resolve("nope.example.com").is_none());
    }

    #[test]
    fn resolved_gateway_carries_its_policy() {
        let mut gw = entry("platform-api", vec![HostMatch::parse("api.twinos.cloud")]);
        gw.policy.auth_provider = Some("jwt".to_string());
        let idx = VirtualGatewayIndex::new(vec![gw]);
        let resolved = idx.resolve("api.twinos.cloud").unwrap();
        assert_eq!(resolved.policy.auth_provider.as_deref(), Some("jwt"));
    }

    #[test]
    fn gateway_scoped_upstream_prefixes_only_when_gated() {
        assert_eq!(
            gateway_scoped_upstream(Some("platform-api"), "users-svc"),
            "platform-api:users-svc"
        );
        assert_eq!(gateway_scoped_upstream(None, "users-svc"), "users-svc");
    }

    #[test]
    fn by_id_looks_up_gateway_by_identifier() {
        let idx = twinos_index();
        assert_eq!(&*idx.by_id("platform-api").unwrap().id, "platform-api");
        assert_eq!(&*idx.by_id("tenants").unwrap().id, "tenants");
        assert!(idx.by_id("does-not-exist").is_none());
    }

    #[test]
    fn attach_binds_exact_host_route_to_its_gateway() {
        let idx = twinos_index();
        let gw = idx
            .attach(&HostMatch::Exact("api.twinos.cloud".into()))
            .unwrap();
        assert_eq!(&*gw.id, "platform-api");
    }

    #[test]
    fn attach_binds_wildcard_route_to_owning_wildcard_gateway() {
        let idx = twinos_index();
        // a route scoped to *.twinos.cloud belongs to the tenants gateway
        let gw = idx
            .attach(&HostMatch::Wildcard(".twinos.cloud".into()))
            .unwrap();
        assert_eq!(&*gw.id, "tenants");
    }

    #[test]
    fn attach_binds_subdomain_wildcard_to_broader_gateway() {
        let idx = twinos_index();
        // a more-specific wildcard route still attaches to the broader gateway
        let gw = idx
            .attach(&HostMatch::Wildcard(".eu.twinos.cloud".into()))
            .unwrap();
        assert_eq!(&*gw.id, "tenants");
    }

    #[test]
    fn attach_any_route_needs_an_any_gateway() {
        let only_exact = VirtualGatewayIndex::new(vec![entry(
            "platform-api",
            vec![HostMatch::parse("api.twinos.cloud")],
        )]);
        assert!(only_exact.attach(&HostMatch::Any).is_none());

        let with_default = VirtualGatewayIndex::new(vec![
            entry("platform-api", vec![HostMatch::parse("api.twinos.cloud")]),
            entry("default", vec![HostMatch::Any]),
        ]);
        assert_eq!(
            &*with_default.attach(&HostMatch::Any).unwrap().id,
            "default"
        );
    }

    #[test]
    fn attach_wildcard_route_not_owned_by_exact_gateway() {
        let only_exact = VirtualGatewayIndex::new(vec![entry(
            "platform-api",
            vec![HostMatch::parse("api.twinos.cloud")],
        )]);
        // an exact gateway does not own a wildcard route
        assert!(only_exact
            .attach(&HostMatch::Wildcard(".twinos.cloud".into()))
            .is_none());
    }
}
