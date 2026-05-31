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
        };
        assert!(c.resolve("foo.platform.com").is_none());
    }
}
