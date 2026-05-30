//! `OctopusPolicy` attachment (GEP-713).
//!
//! A policy never creates routes; it *enriches* the routes produced by the
//! resource it targets (an `HTTPRoute`, `GRPCRoute`, or `OctopusRoute`). With
//! `override_route` it overrides route-inline settings; otherwise it only fills
//! gaps ("default" semantics).

use crate::crds::OctopusPolicySpec;
use crate::ir::{IntermediateRoute, RateLimit, RouteSource};
use std::time::Duration;

/// A resolved policy overlay: which routes it targets and what it applies.
#[derive(Debug, Clone)]
pub struct PolicyOverlay {
    /// Source kind of the targeted routes.
    pub target_source: RouteSource,
    /// `namespace/name` of the targeted resource.
    pub target_id: String,
    /// Auth provider to apply.
    pub auth_provider: Option<String>,
    /// Rate limit to apply.
    pub rate_limit: Option<RateLimit>,
    /// Override route-inline settings (vs. only filling gaps).
    pub override_route: bool,
}

impl PolicyOverlay {
    /// Build an overlay from a policy spec in `namespace`. Returns `None` for
    /// target kinds that don't map to a route source.
    pub fn from_spec(namespace: &str, spec: &OctopusPolicySpec) -> Option<Self> {
        let target_source = match spec.target_ref.kind.as_str() {
            "HTTPRoute" | "GRPCRoute" => RouteSource::GatewayApi,
            "OctopusRoute" => RouteSource::OctopusRoute,
            _ => return None,
        };
        Some(Self {
            target_source,
            target_id: format!("{namespace}/{}", spec.target_ref.name),
            auth_provider: spec.auth_provider.clone(),
            rate_limit: spec.rate_limit.as_ref().map(|rl| RateLimit {
                requests: rl.requests,
                window: Duration::from_secs(rl.window_seconds),
            }),
            override_route: spec.override_route,
        })
    }

    fn matches(&self, route: &IntermediateRoute) -> bool {
        route.source == self.target_source && route.source_id == self.target_id
    }

    fn apply(&self, route: &mut IntermediateRoute) {
        if let Some(auth) = &self.auth_provider {
            if self.override_route || route.auth_provider.is_none() {
                route.auth_provider = Some(auth.clone());
            }
        }
        if let Some(rl) = &self.rate_limit {
            if self.override_route || route.rate_limit.is_none() {
                route.rate_limit = Some(rl.clone());
            }
        }
    }
}

/// Apply every overlay to the routes it targets.
pub fn apply_overlays(routes: &mut [IntermediateRoute], overlays: &[PolicyOverlay]) {
    for route in routes.iter_mut() {
        for overlay in overlays {
            if overlay.matches(route) {
                overlay.apply(route);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crds::{OctopusPolicySpec, PolicyTargetRef};
    use http::Method;

    fn gateway_route(source_id: &str) -> IntermediateRoute {
        let mut r = IntermediateRoute::new(Method::GET, "/api", "up", RouteSource::GatewayApi);
        r.source_id = source_id.to_string();
        r
    }

    fn policy(
        kind: &str,
        name: &str,
        auth: Option<&str>,
        override_route: bool,
    ) -> OctopusPolicySpec {
        OctopusPolicySpec {
            target_ref: PolicyTargetRef {
                group: "gateway.networking.k8s.io".into(),
                kind: kind.into(),
                name: name.into(),
                section_name: None,
            },
            auth_provider: auth.map(|a| a.into()),
            override_route,
            ..Default::default()
        }
    }

    #[test]
    fn attaches_auth_to_targeted_route_only() {
        let overlay = PolicyOverlay::from_spec(
            "default",
            &policy("HTTPRoute", "my-route", Some("jwt"), false),
        )
        .expect("HTTPRoute is a supported target");

        let mut routes = vec![
            gateway_route("default/my-route"),
            gateway_route("default/other"),
        ];
        apply_overlays(&mut routes, &[overlay]);

        assert_eq!(
            routes[0].auth_provider.as_deref(),
            Some("jwt"),
            "targeted route enriched"
        );
        assert_eq!(
            routes[1].auth_provider, None,
            "non-targeted route untouched"
        );
    }

    #[test]
    fn default_semantics_do_not_override_existing() {
        let mut route = gateway_route("default/my-route");
        route.auth_provider = Some("oidc".into());

        let default_overlay = PolicyOverlay::from_spec(
            "default",
            &policy("HTTPRoute", "my-route", Some("jwt"), false),
        )
        .unwrap();
        apply_overlays(std::slice::from_mut(&mut route), &[default_overlay]);
        assert_eq!(
            route.auth_provider.as_deref(),
            Some("oidc"),
            "default does not override"
        );

        let override_overlay = PolicyOverlay::from_spec(
            "default",
            &policy("HTTPRoute", "my-route", Some("jwt"), true),
        )
        .unwrap();
        apply_overlays(std::slice::from_mut(&mut route), &[override_overlay]);
        assert_eq!(
            route.auth_provider.as_deref(),
            Some("jwt"),
            "override replaces"
        );
    }

    #[test]
    fn unsupported_target_kind_is_ignored() {
        assert!(
            PolicyOverlay::from_spec("default", &policy("Service", "svc", Some("jwt"), false))
                .is_none()
        );
    }

    #[test]
    fn targets_octopus_route_by_source() {
        let overlay = PolicyOverlay::from_spec(
            "shop",
            &policy("OctopusRoute", "orders", Some("apikey"), false),
        )
        .unwrap();
        assert_eq!(overlay.target_source, RouteSource::OctopusRoute);

        let mut octopus_route = {
            let mut r =
                IntermediateRoute::new(Method::GET, "/orders", "up", RouteSource::OctopusRoute);
            r.source_id = "shop/orders".into();
            r
        };
        apply_overlays(std::slice::from_mut(&mut octopus_route), &[overlay]);
        assert_eq!(octopus_route.auth_provider.as_deref(), Some("apikey"));
    }
}
