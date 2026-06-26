//! Convention-based upstream derivation for multi-tenant subdomain routing.
//!
//! A *convention* turns a request host into a backend target without a
//! per-tenant route. For example, with base domain `platform.com` and layout
//! `<service>.<tenant>`, the host `orders.acme.platform.com` derives Service
//! `orders` in namespace `acme`. One wildcard route (`*.platform.com`) backed by
//! a convention therefore serves every tenant.
//!
//! [`Convention::resolve`] is pure string logic; the caller turns the resulting
//! [`ConventionTarget`] into a concrete upstream (e.g. cluster DNS).

/// How a convention-derived backend is load-balanced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BackendStrategy {
    /// Single upstream at the Kubernetes Service DNS name (`<svc>.<ns>.svc`);
    /// kube-proxy/CoreDNS load-balances. The default.
    #[default]
    ServiceDns,
    /// Watch the Service's EndpointSlices and load-balance across pod IPs directly.
    EndpointSlice,
}

/// Role of a single host label within the tenant-specific prefix (left to right).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelRole {
    /// This label is the backend Service name.
    Service,
    /// This label is the Kubernetes namespace.
    Namespace,
    /// This label is ignored (placeholder).
    Ignore,
}

/// Derives a `{namespace, service}` target from the request host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Convention {
    /// Base domain suffix with a leading dot, e.g. `.platform.com`.
    pub base_suffix: String,
    /// Roles for each label of the tenant prefix, left to right. The prefix must
    /// have exactly this many labels to resolve.
    pub roles: Vec<LabelRole>,
    /// Service name used when no [`LabelRole::Service`] is present in `roles`.
    pub default_service: Option<String>,
    /// Upstream port for the derived Service.
    pub port: u16,
    /// Optional inline Rhai script that maps `host` → `{namespace, service[, port]}`,
    /// overriding label-based [`resolve`](Self::resolve) when it returns a mapping.
    /// Evaluated by the request handler (kept here only as data; [`resolve`] ignores it).
    pub script: Option<String>,
    /// How the derived backend is load-balanced (Service DNS vs direct pods).
    pub backend: BackendStrategy,
    /// Path-split rules evaluated against the request path *after* host
    /// resolution. The first rule whose `path_prefix` matches overrides the
    /// derived service/port and may rewrite the path — e.g. `customer.t.cloud/api/*`
    /// → the tenant API Service, `customer.t.cloud/*` → the tenant frontend.
    /// Empty means a single target per host (legacy behavior).
    pub route_rules: Vec<ConventionRouteRule>,
}

/// A path-prefix rule that refines a convention-derived target. Rules are
/// evaluated in declaration order; the first whose `path_prefix` matches the
/// request path wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConventionRouteRule {
    /// Path prefix that triggers this rule (e.g. `/api`). `/` matches everything,
    /// so place it last as a catch-all.
    pub path_prefix: String,
    /// Strip `path_prefix` from the path before forwarding.
    pub strip_prefix: bool,
    /// Override the derived Service name (`None` keeps the convention's result).
    pub service_override: Option<String>,
    /// Override the upstream port (`None` keeps the convention's port).
    pub port_override: Option<u16>,
    /// Prefix to prepend to the path after any stripping.
    pub add_prefix: Option<String>,
}

/// How the request path should be rewritten before forwarding, derived from the
/// matching [`ConventionRouteRule`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PathRewrite {
    /// Prefix to strip from the front of the path.
    pub strip: Option<String>,
    /// Prefix to prepend to the path.
    pub add: Option<String>,
}

/// The concrete target a convention produced for a given host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConventionTarget {
    /// Target Kubernetes namespace.
    pub namespace: String,
    /// Target Service name.
    pub service: String,
    /// Target port.
    pub port: u16,
}

impl Convention {
    /// Resolve `host` (lowercased) to a target, or `None` if the host is not
    /// under the base domain, has the wrong number of prefix labels, or the
    /// roles can't yield both a namespace and a service.
    pub fn resolve(&self, host: &str) -> Option<ConventionTarget> {
        // The tenant prefix is the host with the base domain stripped.
        let prefix = host.strip_suffix(&self.base_suffix)?;
        if prefix.is_empty() {
            return None;
        }
        let labels: Vec<&str> = prefix.split('.').collect();
        if labels.len() != self.roles.len() {
            return None;
        }

        let mut service = self.default_service.clone();
        let mut namespace = None;
        for (label, role) in labels.iter().zip(&self.roles) {
            match role {
                LabelRole::Service => service = Some((*label).to_string()),
                LabelRole::Namespace => namespace = Some((*label).to_string()),
                LabelRole::Ignore => {}
            }
        }

        Some(ConventionTarget {
            namespace: namespace?,
            service: service?,
            port: self.port,
        })
    }

    /// Resolve `host` *and* `path` together. After deriving the base target from
    /// the host, the first [`route_rules`](Self::route_rules) entry whose
    /// `path_prefix` matches `path` overrides the target's service/port and
    /// yields a [`PathRewrite`]. With no rules (or no match), returns the base
    /// target and no rewrite — identical to [`resolve`](Self::resolve).
    pub fn resolve_with_path(
        &self,
        host: &str,
        path: &str,
    ) -> Option<(ConventionTarget, Option<PathRewrite>)> {
        let base = self.resolve(host)?;
        Some(self.apply_route_rules(base, path))
    }

    /// Apply this convention's [`route_rules`](Self::route_rules) to an already
    /// resolved `base` target. The first rule whose `path_prefix` matches `path`
    /// overrides the target's service/port and yields a [`PathRewrite`]; with no
    /// match the base target passes through unchanged.
    ///
    /// Pure and reusable: the request handler derives `base` via the optional
    /// Rhai script (which [`resolve`](Self::resolve) ignores) and then applies the
    /// rules here, so path-splitting works for both label- and script-derived
    /// targets.
    pub fn apply_route_rules(
        &self,
        base: ConventionTarget,
        path: &str,
    ) -> (ConventionTarget, Option<PathRewrite>) {
        for rule in &self.route_rules {
            if path.starts_with(&rule.path_prefix) {
                let mut target = base.clone();
                if let Some(service) = &rule.service_override {
                    target.service = service.clone();
                }
                if let Some(port) = rule.port_override {
                    target.port = port;
                }
                let rewrite = if rule.strip_prefix || rule.add_prefix.is_some() {
                    Some(PathRewrite {
                        strip: rule.strip_prefix.then(|| rule.path_prefix.clone()),
                        add: rule.add_prefix.clone(),
                    })
                } else {
                    None
                };
                return (target, rewrite);
            }
        }
        (base, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc_tenant() -> Convention {
        Convention {
            base_suffix: ".platform.com".into(),
            roles: vec![LabelRole::Service, LabelRole::Namespace],
            default_service: None,
            port: 8080,
            script: None,
            backend: BackendStrategy::default(),
            route_rules: Vec::new(),
        }
    }

    #[test]
    fn derives_service_and_namespace_from_subdomain() {
        let t = svc_tenant().resolve("orders.acme.platform.com").unwrap();
        assert_eq!(t.service, "orders");
        assert_eq!(t.namespace, "acme");
        assert_eq!(t.port, 8080);
    }

    #[test]
    fn wrong_label_count_does_not_resolve() {
        let c = svc_tenant();
        assert!(c.resolve("acme.platform.com").is_none(), "one prefix label");
        assert!(
            c.resolve("a.b.c.platform.com").is_none(),
            "three prefix labels"
        );
    }

    #[test]
    fn host_outside_base_domain_does_not_resolve() {
        let c = svc_tenant();
        assert!(c.resolve("orders.acme.other.com").is_none());
        assert!(c.resolve("platform.com").is_none());
    }

    #[test]
    fn tenant_only_layout_uses_default_service() {
        let c = Convention {
            base_suffix: ".platform.com".into(),
            roles: vec![LabelRole::Namespace],
            default_service: Some("web".into()),
            port: 80,
            script: None,
            backend: BackendStrategy::default(),
            route_rules: Vec::new(),
        };
        let t = c.resolve("acme.platform.com").unwrap();
        assert_eq!(t.namespace, "acme");
        assert_eq!(t.service, "web");
    }

    #[test]
    fn missing_namespace_role_does_not_resolve() {
        let c = Convention {
            base_suffix: ".platform.com".into(),
            roles: vec![LabelRole::Service],
            default_service: None,
            port: 80,
            script: None,
            backend: BackendStrategy::default(),
            route_rules: Vec::new(),
        };
        assert!(c.resolve("foo.platform.com").is_none());
    }

    fn tenants_convention() -> Convention {
        Convention {
            base_suffix: ".example.cloud".into(),
            roles: vec![LabelRole::Namespace],
            default_service: Some("studio".into()),
            port: 3000,
            script: None,
            backend: BackendStrategy::default(),
            route_rules: vec![
                ConventionRouteRule {
                    path_prefix: "/api".into(),
                    strip_prefix: true,
                    service_override: Some("api".into()),
                    port_override: Some(7900),
                    add_prefix: None,
                },
                ConventionRouteRule {
                    path_prefix: "/".into(),
                    strip_prefix: false,
                    service_override: Some("studio".into()),
                    port_override: Some(3000),
                    add_prefix: None,
                },
            ],
        }
    }

    #[test]
    fn path_split_routes_api_prefix_to_api_service() {
        let (target, rewrite) = tenants_convention()
            .resolve_with_path("customer-a.example.cloud", "/api/orders")
            .unwrap();
        assert_eq!(target.namespace, "customer-a");
        assert_eq!(target.service, "api");
        assert_eq!(target.port, 7900);
        assert_eq!(
            rewrite,
            Some(PathRewrite {
                strip: Some("/api".into()),
                add: None
            })
        );
    }

    #[test]
    fn path_split_routes_other_paths_to_frontend() {
        let (target, rewrite) = tenants_convention()
            .resolve_with_path("customer-a.example.cloud", "/dashboard")
            .unwrap();
        assert_eq!(target.namespace, "customer-a");
        assert_eq!(target.service, "studio");
        assert_eq!(target.port, 3000);
        assert_eq!(rewrite, None);
    }

    #[test]
    fn resolve_with_path_without_rules_matches_resolve() {
        // no route_rules → single target, no rewrite (legacy behavior)
        let (target, rewrite) = svc_tenant()
            .resolve_with_path("orders.acme.platform.com", "/anything")
            .unwrap();
        assert_eq!(target.service, "orders");
        assert_eq!(target.namespace, "acme");
        assert_eq!(target.port, 8080);
        assert_eq!(rewrite, None);
    }

    #[test]
    fn resolve_with_path_host_outside_domain_is_none() {
        assert!(tenants_convention()
            .resolve_with_path("evil.com", "/api")
            .is_none());
    }
}
