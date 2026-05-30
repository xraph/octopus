//! Gateway listener TLS: extract Secret references and build cert resolvers.

use crate::gateway_api::GatewaySpec;
use crate::refgrant::{is_permitted, RefRequest, ReferenceGrantSpec};
use octopus_tls::{SniCertResolver, SwappableTlsAcceptor};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A TLS certificate a listener needs, resolved to a concrete Secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsListenerRef {
    /// SNI hostname this certificate serves (None = default/catch-all listener).
    pub hostname: Option<String>,
    /// Namespace of the Secret (defaults to the Gateway's namespace).
    pub secret_namespace: String,
    /// Secret name.
    pub secret_name: String,
}

/// Extract the TLS Secret references from a Gateway's terminating listeners.
///
/// `gateway_namespace` is the Gateway's own namespace, used to default
/// `certificateRefs` that omit a namespace.
pub fn tls_secret_refs(gateway_namespace: &str, spec: &GatewaySpec) -> Vec<TlsListenerRef> {
    let mut refs = Vec::new();
    for listener in &spec.listeners {
        let Some(tls) = &listener.tls else { continue };
        // Only terminating listeners need a certificate; Passthrough does not.
        if matches!(tls.mode.as_deref(), Some("Passthrough")) {
            continue;
        }
        for cert_ref in &tls.certificate_refs {
            refs.push(TlsListenerRef {
                hostname: listener.hostname.clone(),
                secret_namespace: cert_ref
                    .namespace
                    .clone()
                    .unwrap_or_else(|| gateway_namespace.to_string()),
                secret_name: cert_ref.name.clone(),
            });
        }
    }
    refs
}

/// Filter listener cert references to those permitted: same-namespace Secrets
/// are always allowed; cross-namespace Secrets require a matching ReferenceGrant
/// in the Secret's namespace.
pub fn authorized_secret_refs<'a>(
    gateway_namespace: &str,
    refs: &'a [TlsListenerRef],
    grants_by_ns: &HashMap<String, Vec<ReferenceGrantSpec>>,
) -> Vec<&'a TlsListenerRef> {
    refs.iter()
        .filter(|r| {
            if r.secret_namespace == gateway_namespace {
                return true;
            }
            let grants = grants_by_ns
                .get(&r.secret_namespace)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            is_permitted(
                grants,
                &RefRequest {
                    from_group: "gateway.networking.k8s.io",
                    from_kind: "Gateway",
                    from_namespace: gateway_namespace,
                    to_group: "",
                    to_kind: "Secret",
                    to_name: &r.secret_name,
                },
            )
        })
        .collect()
}

struct GatewayCerts {
    namespace: String,
    refs: Vec<TlsListenerRef>,
}

/// A TLS Secret's PEM payload: `(cert chain, private key)`.
type SecretPem = (Vec<u8>, Vec<u8>);

/// Reconciles Gateway listener TLS into a hot-swappable acceptor.
///
/// Tracks Gateways' cert references, Secret payloads, and ReferenceGrants;
/// rebuilds an SNI cert resolver and swaps it into the [`SwappableTlsAcceptor`]
/// whenever any of them changes.
pub struct TlsReconciler {
    acceptor: SwappableTlsAcceptor,
    gateways: Mutex<HashMap<String, GatewayCerts>>,
    secrets: Mutex<HashMap<String, SecretPem>>,
    grants: Mutex<HashMap<String, Vec<ReferenceGrantSpec>>>,
    /// Number of certificates loaded into the resolver on the last rebuild.
    loaded_certs: AtomicUsize,
}

impl std::fmt::Debug for TlsReconciler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsReconciler").finish()
    }
}

impl TlsReconciler {
    /// Create a reconciler that swaps certificates into `acceptor`.
    pub fn new(acceptor: SwappableTlsAcceptor) -> Self {
        Self {
            acceptor,
            gateways: Mutex::new(HashMap::new()),
            secrets: Mutex::new(HashMap::new()),
            grants: Mutex::new(HashMap::new()),
            loaded_certs: AtomicUsize::new(0),
        }
    }

    /// Number of certificates loaded into the resolver on the last rebuild.
    pub fn loaded_cert_count(&self) -> usize {
        self.loaded_certs.load(Ordering::Acquire)
    }

    /// Track (or replace) a Gateway's TLS listener cert references.
    pub fn set_gateway(&self, name: &str, namespace: &str, spec: &GatewaySpec) {
        let refs = tls_secret_refs(namespace, spec);
        if let Ok(mut gws) = self.gateways.lock() {
            gws.insert(
                format!("{namespace}/{name}"),
                GatewayCerts {
                    namespace: namespace.to_string(),
                    refs,
                },
            );
        }
        self.rebuild();
    }

    /// Stop tracking a Gateway.
    pub fn remove_gateway(&self, name: &str, namespace: &str) {
        if let Ok(mut gws) = self.gateways.lock() {
            gws.remove(&format!("{namespace}/{name}"));
        }
        self.rebuild();
    }

    /// Track (or replace) a TLS Secret's cert/key payload.
    pub fn set_secret(&self, name: &str, namespace: &str, cert: Vec<u8>, key: Vec<u8>) {
        if let Ok(mut secrets) = self.secrets.lock() {
            secrets.insert(format!("{namespace}/{name}"), (cert, key));
        }
        self.rebuild();
    }

    /// Stop tracking a Secret.
    pub fn remove_secret(&self, name: &str, namespace: &str) {
        if let Ok(mut secrets) = self.secrets.lock() {
            secrets.remove(&format!("{namespace}/{name}"));
        }
        self.rebuild();
    }

    /// Replace the ReferenceGrants for a namespace.
    pub fn set_grants(&self, namespace: &str, grants: Vec<ReferenceGrantSpec>) {
        if let Ok(mut g) = self.grants.lock() {
            g.insert(namespace.to_string(), grants);
        }
        self.rebuild();
    }

    /// Rebuild the SNI resolver from current state and swap it in.
    fn rebuild(&self) {
        let mut resolver = SniCertResolver::new();
        let (Ok(gateways), Ok(secrets), Ok(grants)) = (
            self.gateways.lock(),
            self.secrets.lock(),
            self.grants.lock(),
        ) else {
            tracing::error!("TlsReconciler lock poisoned; skipping rebuild");
            return;
        };

        let mut loaded = 0usize;
        for gc in gateways.values() {
            for r in authorized_secret_refs(&gc.namespace, &gc.refs, &grants) {
                let key = format!("{}/{}", r.secret_namespace, r.secret_name);
                let Some((cert, keyb)) = secrets.get(&key) else {
                    continue; // Secret not present (yet).
                };
                let result = match &r.hostname {
                    Some(host) => resolver.add(host, cert, keyb),
                    None => resolver.set_default(cert, keyb),
                };
                match result {
                    Ok(()) => loaded += 1,
                    Err(e) => {
                        tracing::warn!(secret = %key, error = %e, "Invalid TLS Secret; skipping")
                    }
                }
            }
        }

        self.loaded_certs.store(loaded, Ordering::Release);
        self.acceptor.swap(Arc::new(resolver.into_server_config()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_terminating_https_listener_refs() {
        let yaml = r#"
gatewayClassName: octopus
listeners:
  - name: https
    hostname: api.example.com
    port: 443
    protocol: HTTPS
    tls:
      mode: Terminate
      certificateRefs:
        - kind: Secret
          name: api-tls
  - name: http
    port: 80
    protocol: HTTP
"#;
        let spec: GatewaySpec = serde_yaml::from_str(yaml).unwrap();
        let refs = tls_secret_refs("prod", &spec);
        assert_eq!(refs.len(), 1, "only the HTTPS listener contributes a cert");
        assert_eq!(refs[0].secret_name, "api-tls");
        assert_eq!(
            refs[0].secret_namespace, "prod",
            "defaults to the Gateway namespace"
        );
        assert_eq!(refs[0].hostname.as_deref(), Some("api.example.com"));
    }

    #[test]
    fn preserves_cross_namespace_secret_ref() {
        let yaml = r#"
gatewayClassName: octopus
listeners:
  - name: https
    port: 443
    protocol: HTTPS
    tls:
      certificateRefs:
        - name: shared-tls
          namespace: certs
"#;
        let spec: GatewaySpec = serde_yaml::from_str(yaml).unwrap();
        let refs = tls_secret_refs("prod", &spec);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].secret_namespace, "certs");
        assert_eq!(refs[0].hostname, None);
    }

    #[test]
    fn ignores_passthrough_and_plain_listeners() {
        let yaml = r#"
gatewayClassName: octopus
listeners:
  - name: tls-passthrough
    port: 443
    protocol: TLS
    tls:
      mode: Passthrough
      certificateRefs:
        - name: ignored
  - name: http
    port: 80
    protocol: HTTP
"#;
        let spec: GatewaySpec = serde_yaml::from_str(yaml).unwrap();
        assert!(tls_secret_refs("prod", &spec).is_empty());
    }

    const CERT: &str = "-----BEGIN CERTIFICATE-----
MIIBqTCCAVCgAwIBAgIUdXZJNtio8+gPkOsw2TsTczF8LiAwCgYIKoZIzj0EAwIw
GDEWMBQGA1UEAwwNb2N0b3B1cy5sb2NhbDAeFw0yNjA1MzAxOTAxMDBaFw0zNjA1
MjcxOTAxMDBaMBgxFjAUBgNVBAMMDW9jdG9wdXMubG9jYWwwWTATBgcqhkjOPQIB
BggqhkjOPQMBBwNCAARg9r23sThOLJ0CVVqTeLLbkQSbl/fAMZJwLhzCrGHJXk0e
xP7K73agVp3RiDz7w/rmMBCmhSCppD+vpl7vMnZ9o3gwdjAdBgNVHQ4EFgQU4Lgf
Lbz635DVurCsZ3dWSqQ2eJAwHwYDVR0jBBgwFoAU4LgfLbz635DVurCsZ3dWSqQ2
eJAwDwYDVR0TAQH/BAUwAwEB/zAjBgNVHREEHDAagg1vY3RvcHVzLmxvY2Fsggls
b2NhbGhvc3QwCgYIKoZIzj0EAwIDRwAwRAIgZo1rDiv07r7Sc8bMkOb/WVCmL6m8
AbWTroKXTQjea7oCIFC3gsegwlyDazwLWcXPoq/9orb8RokhQlRjTtmCzW6P
-----END CERTIFICATE-----
";
    const KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgdWBkKWLdsDaJ1ERt
VsIFX7+uAgAU2d0mbk+Hls1GCeKhRANCAARg9r23sThOLJ0CVVqTeLLbkQSbl/fA
MZJwLhzCrGHJXk0exP7K73agVp3RiDz7w/rmMBCmhSCppD+vpl7vMnZ9
-----END PRIVATE KEY-----
";

    use crate::refgrant::{ReferenceGrantFrom, ReferenceGrantSpec, ReferenceGrantTo};

    fn lref(host: Option<&str>, ns: &str, name: &str) -> TlsListenerRef {
        TlsListenerRef {
            hostname: host.map(|h| h.into()),
            secret_namespace: ns.into(),
            secret_name: name.into(),
        }
    }

    #[test]
    fn same_namespace_secret_always_authorized() {
        let refs = vec![lref(Some("api"), "prod", "api-tls")];
        let permitted = authorized_secret_refs("prod", &refs, &HashMap::new());
        assert_eq!(permitted.len(), 1);
    }

    #[test]
    fn cross_namespace_secret_denied_without_grant() {
        let refs = vec![lref(Some("api"), "certs", "shared-tls")];
        let permitted = authorized_secret_refs("prod", &refs, &HashMap::new());
        assert!(
            permitted.is_empty(),
            "cross-ns Secret needs a ReferenceGrant"
        );
    }

    #[test]
    fn cross_namespace_secret_allowed_with_grant() {
        let refs = vec![lref(Some("api"), "certs", "shared-tls")];
        let mut grants = HashMap::new();
        grants.insert(
            "certs".to_string(),
            vec![ReferenceGrantSpec {
                from: vec![ReferenceGrantFrom {
                    group: "gateway.networking.k8s.io".into(),
                    kind: "Gateway".into(),
                    namespace: "prod".into(),
                }],
                to: vec![ReferenceGrantTo {
                    group: "".into(),
                    kind: "Secret".into(),
                    name: None,
                }],
            }],
        );
        let permitted = authorized_secret_refs("prod", &refs, &grants);
        assert_eq!(permitted.len(), 1, "grant authorizes the cross-ns Secret");
    }

    fn https_gateway_spec(secret: &str) -> GatewaySpec {
        serde_yaml::from_str(&format!(
            "gatewayClassName: octopus\nlisteners:\n  - name: https\n    hostname: api.example.com\n    port: 443\n    protocol: HTTPS\n    tls:\n      certificateRefs:\n        - name: {secret}\n"
        ))
        .unwrap()
    }

    #[test]
    fn reconciler_loads_cert_only_once_secret_present() {
        let initial = Arc::new(SniCertResolver::new().into_server_config());
        let acceptor = SwappableTlsAcceptor::new(initial);
        let rec = TlsReconciler::new(acceptor.clone());

        // Gateway references a Secret that isn't present yet → no cert loaded.
        rec.set_gateway("gw", "prod", &https_gateway_spec("api-tls"));
        assert_eq!(
            rec.loaded_cert_count(),
            0,
            "no cert until the Secret exists"
        );

        // Secret arrives → the cert is loaded into the resolver and swapped in.
        rec.set_secret(
            "api-tls",
            "prod",
            CERT.as_bytes().to_vec(),
            KEY.as_bytes().to_vec(),
        );
        assert_eq!(
            rec.loaded_cert_count(),
            1,
            "Secret provides the listener cert"
        );

        // Removing the Secret drops the cert again.
        rec.remove_secret("api-tls", "prod");
        assert_eq!(
            rec.loaded_cert_count(),
            0,
            "removing the Secret drops the cert"
        );
    }
}
