//! Source-agnostic intermediate routing representation.
//!
//! Every route source (static config, Gateway API HTTPRoutes, Octopus CRDs,
//! FARP discovery) translates into [`IntermediateRoute`]s and [`UpstreamCluster`]s
//! held in a [`RouteStore`]. [`RouteStore::merge`] flattens all sources into one
//! [`RoutingTable`] using a deterministic precedence model, so the live router is
//! always programmed from a single coherent set.

use http::Method;
use octopus_core::UpstreamCluster;
use octopus_router::{Convention, HostMatch};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;

/// Which kind of source produced a route. Lower precedence value wins when two
/// sources declare the same `method`+`path` at the same priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteSource {
    /// Static gateway configuration (highest precedence).
    Static,
    /// A custom `OctopusRoute` resource.
    OctopusRoute,
    /// A standard Gateway API `HTTPRoute`/`GRPCRoute`.
    GatewayApi,
    /// A route auto-generated from FARP service discovery (lowest precedence).
    Farp,
}

impl RouteSource {
    /// Precedence rank — lower wins.
    pub fn precedence(self) -> u8 {
        match self {
            RouteSource::Static => 0,
            RouteSource::OctopusRoute => 1,
            RouteSource::GatewayApi => 2,
            RouteSource::Farp => 3,
        }
    }
}

/// Identifies the originating resource for a set of routes in the [`RouteStore`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceKey {
    /// The kind of source.
    pub source: RouteSource,
    /// A stable identity within that source (e.g. `namespace/name`).
    pub id: String,
}

impl SourceKey {
    /// Create a new source key.
    pub fn new(source: RouteSource, id: impl Into<String>) -> Self {
        Self {
            source,
            id: id.into(),
        }
    }
}

/// A token-bucket rate limit carried on a route.
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimit {
    /// Requests allowed per window.
    pub requests: u32,
    /// Window duration.
    pub window: Duration,
}

/// One routing rule, independent of which source produced it.
#[derive(Debug, Clone)]
pub struct IntermediateRoute {
    /// HTTP method.
    pub method: Method,
    /// Host this route is scoped to (Gateway API `hostnames`). Defaults to
    /// [`HostMatch::Any`] for host-agnostic sources.
    pub host: HostMatch,
    /// Match path (may contain `:param` / `*wildcard`).
    pub path: String,
    /// Target upstream cluster name.
    pub upstream: String,
    /// Explicit priority; higher wins within the same `method`+`path`.
    pub priority: i32,
    /// Producing source.
    pub source: RouteSource,
    /// Originating resource identity (`namespace/name`), used to match policy
    /// attachments to the routes a given resource produced.
    pub source_id: String,
    /// Strip this prefix before proxying.
    pub strip_prefix: Option<String>,
    /// Add this prefix before proxying.
    pub add_prefix: Option<String>,
    /// Named auth provider to enforce.
    pub auth_provider: Option<String>,
    /// Skip auth for this route.
    pub skip_auth: bool,
    /// Required roles.
    pub require_roles: Vec<String>,
    /// Required scopes.
    pub require_scopes: Vec<String>,
    /// Authorization rule expression.
    pub authz_rule: Option<String>,
    /// Per-route timeout.
    pub timeout: Option<Duration>,
    /// Per-route rate limit.
    pub rate_limit: Option<RateLimit>,
    /// Convention for deriving the upstream from the request host. When set, the
    /// route is host-wildcarded and its upstream is resolved per request.
    pub convention: Option<Convention>,
    /// Virtual gateway this route attaches to (`None` = implicit `default`).
    /// Routes inherit their gateway's policy defaults during apply.
    pub gateway_id: Option<String>,
    /// Reverse-proxy behavior (origin override, path mode, header rewrites).
    /// `None` preserves the legacy in-cluster, strip-only, no-rewrite behavior.
    pub proxy: Option<octopus_router::ProxySpec>,
}

impl IntermediateRoute {
    /// Create a route with defaults for all optional attributes.
    pub fn new(
        method: Method,
        path: impl Into<String>,
        upstream: impl Into<String>,
        source: RouteSource,
    ) -> Self {
        Self {
            method,
            host: HostMatch::Any,
            path: path.into(),
            upstream: upstream.into(),
            priority: 0,
            source,
            source_id: String::new(),
            strip_prefix: None,
            add_prefix: None,
            auth_provider: None,
            skip_auth: false,
            require_roles: Vec::new(),
            require_scopes: Vec::new(),
            authz_rule: None,
            timeout: None,
            rate_limit: None,
            convention: None,
            gateway_id: None,
            proxy: None,
        }
    }

    /// Set an explicit priority.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Scope this route to a host.
    pub fn with_host(mut self, host: HostMatch) -> Self {
        self.host = host;
        self
    }

    /// Attach this route to a virtual gateway by id.
    pub fn with_gateway(mut self, id: impl Into<String>) -> Self {
        self.gateway_id = Some(id.into());
        self
    }
}

/// The merged, conflict-free routing intent ready to apply to the router.
#[derive(Debug, Clone, Default)]
pub struct RoutingTable {
    /// Winning routes after precedence resolution.
    pub routes: Vec<IntermediateRoute>,
    /// Upstream clusters, deduplicated by name.
    pub upstreams: Vec<UpstreamCluster>,
}

/// One source's contribution to the store.
#[derive(Debug, Clone, Default)]
struct RouteSet {
    routes: Vec<IntermediateRoute>,
    upstreams: Vec<UpstreamCluster>,
}

/// Accumulates routes/upstreams per source and merges them deterministically.
#[derive(Debug, Default)]
pub struct RouteStore {
    entries: HashMap<SourceKey, RouteSet>,
}

impl RouteStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert (or replace) the contribution of one source resource.
    pub fn insert(
        &mut self,
        key: SourceKey,
        routes: Vec<IntermediateRoute>,
        upstreams: Vec<UpstreamCluster>,
    ) {
        self.entries.insert(key, RouteSet { routes, upstreams });
    }

    /// Remove a source resource's contribution (e.g. on delete).
    pub fn remove(&mut self, key: &SourceKey) {
        self.entries.remove(key);
    }

    /// Number of source resources currently tracked.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Merge every source into one conflict-free [`RoutingTable`].
    ///
    /// Routes that collide on `method`+`path`+`host` are resolved by highest
    /// `priority`, then by [`RouteSource::precedence`]. Routes scoped to
    /// different hosts do not collide. Upstreams are deduplicated by name,
    /// preferring the higher-precedence source.
    pub fn merge(&self) -> RoutingTable {
        // Resolve route collisions on (method, path, host) — routes scoped to
        // different hosts never collide.
        let mut best: HashMap<(Method, String, HostMatch), &IntermediateRoute> = HashMap::new();
        for set in self.entries.values() {
            for candidate in &set.routes {
                let key = (
                    candidate.method.clone(),
                    candidate.path.clone(),
                    candidate.host.clone(),
                );
                match best.get(&key) {
                    Some(existing) if !route_wins(candidate, existing) => {}
                    _ => {
                        best.insert(key, candidate);
                    }
                }
            }
        }
        let mut routes: Vec<IntermediateRoute> = best.into_values().cloned().collect();
        // Stable ordering for deterministic output.
        routes.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| a.method.as_str().cmp(b.method.as_str()))
        });

        // Deduplicate upstreams by name, preferring the higher-precedence source.
        let mut best_up: HashMap<String, (&UpstreamCluster, u8)> = HashMap::new();
        for (key, set) in &self.entries {
            let prec = key.source.precedence();
            for upstream in &set.upstreams {
                match best_up.get(&upstream.name) {
                    Some(&(_, existing_prec)) if existing_prec <= prec => {}
                    _ => {
                        best_up.insert(upstream.name.clone(), (upstream, prec));
                    }
                }
            }
        }
        let mut upstreams: Vec<UpstreamCluster> =
            best_up.into_values().map(|(u, _)| u.clone()).collect();
        upstreams.sort_by(|a, b| a.name.cmp(&b.name));

        RoutingTable { routes, upstreams }
    }
}

/// Whether `candidate` should replace `existing` for the same `method`+`path`:
/// higher priority wins; ties go to the higher-precedence source; remaining ties
/// break on upstream name for deterministic output.
fn route_wins(candidate: &IntermediateRoute, existing: &IntermediateRoute) -> bool {
    match candidate.priority.cmp(&existing.priority) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => match candidate
            .source
            .precedence()
            .cmp(&existing.source.precedence())
        {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => candidate.upstream < existing.upstream,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(method: Method, path: &str, upstream: &str, source: RouteSource) -> IntermediateRoute {
        IntermediateRoute::new(method, path, upstream, source)
    }

    #[test]
    fn route_defaults_to_no_gateway() {
        let r = IntermediateRoute::new(Method::GET, "/x", "up", RouteSource::OctopusRoute);
        assert!(r.gateway_id.is_none());
    }

    #[test]
    fn with_gateway_sets_id() {
        let r = IntermediateRoute::new(Method::GET, "/x", "up", RouteSource::OctopusRoute)
            .with_gateway("platform-api");
        assert_eq!(r.gateway_id.as_deref(), Some("platform-api"));
    }

    fn find<'a>(t: &'a RoutingTable, method: &Method, path: &str) -> Option<&'a IntermediateRoute> {
        t.routes
            .iter()
            .find(|r| r.method == *method && r.path == path)
    }

    #[test]
    fn same_method_path_different_host_both_survive_merge() {
        let mut store = RouteStore::new();
        store.insert(
            SourceKey::new(RouteSource::GatewayApi, "acme"),
            vec![
                route(Method::GET, "/api", "acme-up", RouteSource::GatewayApi)
                    .with_host(HostMatch::Exact("acme.example.com".into())),
            ],
            vec![],
        );
        store.insert(
            SourceKey::new(RouteSource::GatewayApi, "globex"),
            vec![
                route(Method::GET, "/api", "globex-up", RouteSource::GatewayApi)
                    .with_host(HostMatch::Exact("globex.example.com".into())),
            ],
            vec![],
        );

        let table = store.merge();
        assert_eq!(
            table.routes.iter().filter(|r| r.path == "/api").count(),
            2,
            "same method+path but different hosts must NOT collide"
        );
    }

    #[test]
    fn higher_precedence_source_wins_same_route() {
        let mut store = RouteStore::new();
        store.insert(
            SourceKey::new(RouteSource::Farp, "svc-a"),
            vec![route(Method::GET, "/api", "farp-up", RouteSource::Farp)],
            vec![],
        );
        store.insert(
            SourceKey::new(RouteSource::OctopusRoute, "default/my-route"),
            vec![route(
                Method::GET,
                "/api",
                "octopus-up",
                RouteSource::OctopusRoute,
            )],
            vec![],
        );

        let table = store.merge();
        assert_eq!(
            table.routes.iter().filter(|r| r.path == "/api").count(),
            1,
            "collision on GET /api deduped to one winner"
        );
        let winner = find(&table, &Method::GET, "/api").unwrap();
        assert_eq!(winner.upstream, "octopus-up", "OctopusRoute beats FARP");
    }

    #[test]
    fn higher_priority_beats_source_precedence() {
        let mut store = RouteStore::new();
        store.insert(
            SourceKey::new(RouteSource::Static, "cfg"),
            vec![route(Method::GET, "/api", "static-up", RouteSource::Static)],
            vec![],
        );
        store.insert(
            SourceKey::new(RouteSource::Farp, "svc"),
            vec![route(Method::GET, "/api", "farp-up", RouteSource::Farp).with_priority(10)],
            vec![],
        );

        let table = store.merge();
        let winner = find(&table, &Method::GET, "/api").unwrap();
        assert_eq!(
            winner.upstream, "farp-up",
            "priority 10 beats higher-precedence static at priority 0"
        );
    }

    #[test]
    fn distinct_method_or_path_both_kept() {
        let mut store = RouteStore::new();
        store.insert(
            SourceKey::new(RouteSource::OctopusRoute, "r"),
            vec![
                route(Method::GET, "/a", "up-a", RouteSource::OctopusRoute),
                route(Method::POST, "/a", "up-a", RouteSource::OctopusRoute),
                route(Method::GET, "/b", "up-b", RouteSource::OctopusRoute),
            ],
            vec![],
        );
        let table = store.merge();
        assert_eq!(
            table.routes.len(),
            3,
            "different method or path are not collisions"
        );
    }

    #[test]
    fn remove_drops_that_sources_routes() {
        let mut store = RouteStore::new();
        let farp = SourceKey::new(RouteSource::Farp, "svc");
        store.insert(
            farp.clone(),
            vec![route(Method::GET, "/farp", "farp-up", RouteSource::Farp)],
            vec![],
        );
        store.insert(
            SourceKey::new(RouteSource::Static, "cfg"),
            vec![route(
                Method::GET,
                "/static",
                "static-up",
                RouteSource::Static,
            )],
            vec![],
        );
        assert_eq!(store.merge().routes.len(), 2);

        store.remove(&farp);
        let table = store.merge();
        assert_eq!(table.routes.len(), 1);
        assert!(find(&table, &Method::GET, "/static").is_some());
        assert!(find(&table, &Method::GET, "/farp").is_none());
    }

    #[test]
    fn upstreams_deduped_by_name_preferring_precedence() {
        let mut store = RouteStore::new();
        store.insert(
            SourceKey::new(RouteSource::Farp, "svc"),
            vec![],
            vec![UpstreamCluster::new("shared")],
        );
        store.insert(
            SourceKey::new(RouteSource::Static, "cfg"),
            vec![],
            vec![
                UpstreamCluster::new("shared"),
                UpstreamCluster::new("only-static"),
            ],
        );

        let table = store.merge();
        assert_eq!(
            table.upstreams.len(),
            2,
            "'shared' deduped, 'only-static' kept"
        );
        assert_eq!(
            table
                .upstreams
                .iter()
                .filter(|u| u.name == "shared")
                .count(),
            1
        );
    }
}
