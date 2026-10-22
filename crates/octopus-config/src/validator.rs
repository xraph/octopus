//! Configuration validation

use crate::Config;
use octopus_core::{Error, Result};

/// Validate configuration
pub fn validate_config(config: &Config) -> Result<()> {
    // Validate gateway
    validate_gateway(config)?;

    // Validate upstreams
    validate_upstreams(config)?;

    // Validate routes
    validate_routes(config)?;

    // Validate the FARP gateway binding
    validate_farp(config)?;

    // Validate the admin section
    validate_admin(config)?;

    // Validate plugins
    validate_plugins(config)?;

    Ok(())
}

fn validate_gateway(config: &Config) -> Result<()> {
    // Check request timeout is reasonable
    if config.gateway.request_timeout.as_secs() == 0 {
        return Err(Error::Config("request_timeout must be > 0".to_string()));
    }

    if config.gateway.request_timeout.as_secs() > 300 {
        tracing::warn!("request_timeout is very high (>5 minutes)");
    }

    // Check max body size
    if config.gateway.max_body_size == 0 {
        return Err(Error::Config("max_body_size must be > 0".to_string()));
    }

    // A zero limit would reject every request, which is never the intent —
    // `enabled: false` is how you turn a limit off.
    let limits = &config.gateway.request_limits;
    for (value, name) in [
        (limits.max_body_size, "max_body_size"),
        (limits.max_header_size, "max_header_size"),
        (limits.max_uri_length, "max_uri_length"),
    ] {
        if value == Some(0) {
            return Err(Error::Config(format!(
                "gateway.request_limits.{name} must be > 0 \
                 (omit it to leave the limit unenforced, or set \
                 gateway.request_limits.enabled to false)"
            )));
        }
    }

    // QUIC has no plaintext mode — it mandates TLS 1.3. Without a TLS config
    // the UDP listener could only fail at bind time, long after the operator
    // has stopped reading logs, so reject it here.
    if config.gateway.http3.enabled {
        if config.gateway.tls.is_none() {
            return Err(Error::Config(
                "HTTP/3 requires TLS: QUIC mandates TLS 1.3 and has no plaintext mode. \
                 Configure gateway.tls or set gateway.http3.enabled to false."
                    .to_string(),
            ));
        }

        if config.gateway.http3.alt_svc && config.gateway.http3.alt_svc_max_age == 0 {
            return Err(Error::Config(
                "gateway.http3.alt_svc_max_age must be > 0 when alt_svc is enabled: \
                 ma=0 instructs clients to discard the HTTP/3 advertisement immediately."
                    .to_string(),
            ));
        }
    }

    // The private admin listener needs its own socket. An unspecified IP
    // (0.0.0.0 / ::) covers every interface, so it collides with any IP on
    // the same port.
    if let Some(admin) = config.admin.listen {
        let gateway = config.gateway.listen;
        let same_ip = admin.ip() == gateway.ip()
            || admin.ip().is_unspecified()
            || gateway.ip().is_unspecified();
        if admin.port() == gateway.port() && same_ip {
            return Err(Error::Config(
                "admin.listen must use a different port from gateway.listen".to_string(),
            ));
        }
    }

    // Validate TLS configuration if present
    if let Some(ref tls) = config.gateway.tls {
        if tls.cert_file.is_empty() {
            return Err(Error::Config("TLS cert_file cannot be empty".to_string()));
        }
        if tls.key_file.is_empty() {
            return Err(Error::Config("TLS key_file cannot be empty".to_string()));
        }

        // Validate TLS version
        match tls.min_tls_version.as_str() {
            "1.2" | "1.3" => {}
            _ => {
                return Err(Error::Config(format!(
                    "Invalid TLS version: {} (must be 1.2 or 1.3)",
                    tls.min_tls_version
                )));
            }
        }

        // Validate reload interval
        if tls.enable_cert_reload && tls.reload_interval_secs == 0 {
            return Err(Error::Config(
                "TLS reload_interval_secs must be > 0 when cert reload is enabled".to_string(),
            ));
        }
    }

    Ok(())
}

fn validate_upstreams(config: &Config) -> Result<()> {
    for upstream in &config.upstreams {
        if upstream.name.is_empty() {
            return Err(Error::Config("upstream name cannot be empty".to_string()));
        }

        if octopus_core::LoadBalanceStrategy::from_policy(&upstream.lb_policy).is_none() {
            tracing::warn!(
                upstream = %upstream.name,
                lb_policy = %upstream.lb_policy,
                "Unknown lb_policy; balancing round-robin"
            );
        }

        if upstream.instances.is_empty() {
            tracing::warn!(
                upstream = %upstream.name,
                "Upstream has no instances"
            );
        }

        // Validate instances
        for instance in &upstream.instances {
            if instance.id.is_empty() {
                return Err(Error::Config("instance id cannot be empty".to_string()));
            }

            if instance.host.is_empty() {
                return Err(Error::Config("instance host cannot be empty".to_string()));
            }

            if instance.port == 0 {
                return Err(Error::Config("instance port must be > 0".to_string()));
            }
        }
    }

    Ok(())
}

/// Whether `id` has the `route-<n>` shape the admin API uses for routes
/// without an id. A declared route may not take one.
fn is_positional_id(id: &str) -> bool {
    id.strip_prefix("route-")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// How an error names `routes[index]`.
fn route_label(index: usize, id: Option<&str>) -> String {
    id.map_or_else(
        || format!("routes[{index}]"),
        |id| format!("routes[{index}] (id {id})"),
    )
}

fn validate_routes(config: &Config) -> Result<()> {
    let mut ids = std::collections::HashSet::new();
    // Router entry key (method, trie key, host) -> the route that holds it.
    let mut entries: std::collections::HashMap<
        (http::Method, String, octopus_router::HostMatch),
        (usize, Option<&str>),
    > = std::collections::HashMap::new();
    for (index, route) in config.routes.iter().enumerate() {
        if let Some(id) = &route.id {
            if id.is_empty() {
                return Err(Error::Config("route id cannot be empty".to_string()));
            }
            if is_positional_id(id) {
                return Err(Error::Config(format!(
                    "route id {id} is reserved: route-<n> names a listed route without an id"
                )));
            }
            if !ids.insert(id.as_str()) {
                return Err(Error::Config(format!("duplicate route id: {id}")));
            }
        }

        if route.path.is_empty() {
            return Err(Error::Config("route path cannot be empty".to_string()));
        }

        if !route.path.starts_with('/') {
            return Err(Error::Config("route path must start with '/'".to_string()));
        }

        if route.upstream.is_empty() {
            return Err(Error::Config("route upstream cannot be empty".to_string()));
        }

        // Check that upstream exists
        if !config.upstreams.iter().any(|u| u.name == route.upstream) {
            return Err(Error::Config(format!(
                "Route references non-existent upstream: {}",
                route.upstream
            )));
        }

        // Hosts and methods must parse; this is the same expansion the
        // runtime performs when it loads the route.
        let id = route.id.as_deref();
        for entry in route.to_routes(octopus_router::RouteOrigin::Config)? {
            let key = (
                entry.method.clone(),
                octopus_router::node_key(&entry.path),
                entry.host.clone(),
            );
            match entries.get(&key) {
                // Two id-less routes on one entry load the way they did in
                // 0.4.1. Once either has an id, the pair is the conflict the
                // admin API answers with 409.
                Some(&(other, other_id))
                    if other != index && (id.is_some() || other_id.is_some()) =>
                {
                    return Err(Error::Config(format!(
                        "{} and {} both serve {} {} on {}",
                        route_label(other, other_id),
                        route_label(index, id),
                        entry.method,
                        entry.path,
                        entry
                            .host
                            .pattern()
                            .map_or_else(|| "any host".to_string(), |h| format!("host {h}")),
                    )));
                }
                Some(_) => {}
                None => {
                    entries.insert(key, (index, id));
                }
            }
        }
    }

    Ok(())
}

/// Check the FARP gateway hostnames while FARP is enabled.
///
/// `hostnames` is strict: every entry must pass
/// [`HostMatch::parse_pattern`](octopus_router::HostMatch::parse_pattern).
/// The legacy `hostname` keeps loading the way it did in 0.4.1: `""` and `*`
/// mean host-agnostic routes, and any other value the strict parser rejects
/// only logs a warning and is matched leniently.
fn validate_farp(config: &Config) -> Result<()> {
    if !config.farp.enabled {
        return Ok(());
    }
    // An unset token leaves registration open on purpose (startup warns). An
    // empty one is almost always an unset `${VAR}`, so say so.
    if config
        .farp
        .registration_token
        .as_deref()
        .is_some_and(|t| t.trim().is_empty())
    {
        return Err(Error::Config(
            "farp.registration_token is empty; remove it to leave registration open or set a value"
                .to_string(),
        ));
    }
    let Some(gateway) = &config.farp.gateway else {
        return Ok(());
    };

    if !gateway.hostnames.is_empty() {
        for (index, hostname) in gateway.hostnames.iter().enumerate() {
            octopus_router::HostMatch::parse_pattern(hostname)
                .map_err(|e| Error::Config(format!("farp.gateway.hostnames[{index}]: {e}")))?;
        }
        return Ok(());
    }

    let hostname = gateway.hostname.trim();
    if hostname.is_empty() || hostname == "*" {
        return Ok(());
    }
    if let Err(e) = octopus_router::HostMatch::parse_pattern(hostname) {
        tracing::warn!(
            "farp.gateway.hostname: {e}; matching it leniently as before. \
             Use farp.gateway.hostnames for strict checking"
        );
    }
    Ok(())
}

/// With `admin.require_auth`, an `admin.auth_provider` must name a provider
/// in `auth_providers`; otherwise the dashboard would start unprotected.
fn validate_admin(config: &Config) -> Result<()> {
    let admin = &config.admin;
    if !admin.require_auth {
        return Ok(());
    }
    if let Some(name) = admin.auth_provider.as_deref() {
        if !config.auth_providers.contains_key(name) {
            return Err(Error::Config(format!(
                "admin.auth_provider names {name}, which is not defined in auth_providers"
            )));
        }
    }
    Ok(())
}

fn validate_plugins(_config: &Config) -> Result<()> {
    // TODO: Validate plugin configurations
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use std::time::Duration;

    fn minimal_config() -> Config {
        Config {
            gateway: GatewayConfig {
                listen: "127.0.0.1:8080".parse().unwrap(),
                workers: 0,
                request_timeout: Duration::from_secs(30),
                shutdown_timeout: Duration::from_secs(30),
                pre_stop_delay: Duration::from_secs(5),
                max_body_size: 1024 * 1024,
                request_limits: crate::types::RequestLimitsConfig::default(),
                tls: None,
                compression: CompressionConfig::default(),
                internal_route_prefix: Some("__".to_string()),
                probes: ProbeConfig::default(),
                enforce_sni_check: true,
                security_headers: Default::default(),
                http3: Default::default(),
            },
            upstreams: vec![],
            routes: vec![],
            plugins: vec![],
            farp: Default::default(),
            observability: ObservabilityConfig::default(),
            auth_providers: Default::default(),
            auth: Default::default(),
            cors: None,
            admin: Default::default(),
            grpc: Default::default(),
            graphql: Default::default(),
            kubernetes: Default::default(),
        }
    }

    #[test]
    fn test_valid_minimal_config() {
        let config = minimal_config();
        assert!(validate_config(&config).is_ok());
    }

    /// QUIC mandates TLS 1.3 — there is no plaintext HTTP/3. Enabling the
    /// listener without TLS can only ever fail at bind time, so it must be
    /// rejected while the operator is still looking at the error.
    #[test]
    fn http3_without_tls_is_rejected() {
        let mut config = minimal_config();
        config.gateway.tls = None;
        config.gateway.http3.enabled = true;

        let err = validate_config(&config).expect_err("HTTP/3 without TLS must fail");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("http/3") && msg.contains("tls"),
            "error should name both HTTP/3 and TLS, got: {err}"
        );
    }

    /// `min_tls_version` is a floor, not an exact version — a floor of 1.2
    /// still permits 1.3, which is all QUIC needs. Serving TLS 1.2 to legacy
    /// TCP clients while offering HTTP/3 to modern ones is a legitimate
    /// deployment, so it must not be rejected.
    #[test]
    fn http3_accepts_a_tls_1_2_floor() {
        let mut config = minimal_config();
        config.gateway.tls = Some(crate::types::TlsConfig {
            cert_file: "cert.pem".to_string(),
            key_file: "key.pem".to_string(),
            client_ca_file: None,
            require_client_cert: false,
            min_tls_version: "1.2".to_string(),
            enable_cert_reload: false,
            reload_interval_secs: 0,
        });
        config.gateway.http3.enabled = true;

        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn http3_rejects_a_zero_alt_svc_max_age_while_advertising() {
        let mut config = minimal_config();
        config.gateway.tls = Some(crate::types::TlsConfig {
            cert_file: "cert.pem".to_string(),
            key_file: "key.pem".to_string(),
            client_ca_file: None,
            require_client_cert: false,
            min_tls_version: "1.3".to_string(),
            enable_cert_reload: false,
            reload_interval_secs: 0,
        });
        config.gateway.http3.enabled = true;
        config.gateway.http3.alt_svc = true;
        config.gateway.http3.alt_svc_max_age = 0;

        assert!(
            validate_config(&config).is_err(),
            "ma=0 tells clients to forget h3 immediately, defeating advertisement"
        );
    }

    #[test]
    fn http3_disabled_ignores_tls_requirements() {
        let mut config = minimal_config();
        config.gateway.tls = None;
        config.gateway.http3.enabled = false;

        assert!(validate_config(&config).is_ok());
    }

    /// The private admin listener and the gateway cannot share a socket: the
    /// second bind would fail at startup, long after the config was accepted.
    #[test]
    fn admin_listen_must_differ_from_gateway_listen() {
        let mut config = minimal_config();
        config.gateway.listen = "0.0.0.0:8080".parse().unwrap();

        config.admin.listen = Some("0.0.0.0:8080".parse().unwrap());
        let err = validate_config(&config).expect_err("same address must fail");
        assert!(
            err.to_string()
                .contains("admin.listen must use a different port from gateway.listen"),
            "unexpected error: {err}"
        );

        // An unspecified gateway IP covers every interface, so a specific
        // admin IP on the same port still collides.
        config.admin.listen = Some("127.0.0.1:8080".parse().unwrap());
        assert!(validate_config(&config).is_err());

        config.admin.listen = Some("127.0.0.1:9090".parse().unwrap());
        assert!(validate_config(&config).is_ok());

        // Two distinct, specific IPs may share a port.
        config.gateway.listen = "10.0.0.1:8080".parse().unwrap();
        config.admin.listen = Some("127.0.0.1:8080".parse().unwrap());
        assert!(validate_config(&config).is_ok());

        config.admin.listen = None;
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_zero_timeout() {
        let mut config = minimal_config();
        config.gateway.request_timeout = Duration::from_secs(0);

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_zero_body_size() {
        let mut config = minimal_config();
        config.gateway.max_body_size = 0;

        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_route_invalid_upstream() {
        let mut config = minimal_config();
        config.routes.push(RouteConfig {
            id: None,
            path: "/test".to_string(),
            methods: vec!["GET".to_string()],
            hosts: vec![],
            upstream: "nonexistent".to_string(),
            priority: 0,
            strip_prefix: None,
            add_prefix: None,
            metadata: std::collections::HashMap::new(),
            auth_provider: None,
            skip_auth: false,
            require_roles: vec![],
            require_scopes: vec![],
            authz_rule: None,
            timeout: None,
            rate_limit: None,
            cors: None,
            path_mode: None,
            upstream_origin: None,
            rewrite_redirects: None,
            rewrite_cookie_path: None,
            tls_verify: None,
        });

        assert!(validate_config(&config).is_err());
    }

    /// A config with one upstream (`web`) and the routes parsed from `yaml`.
    fn config_with_routes(yaml: &str) -> Config {
        let mut config = minimal_config();
        config.upstreams.push(UpstreamConfig {
            name: "web".to_string(),
            instances: vec![],
            lb_policy: "round_robin".to_string(),
            health_check: None,
            circuit_breaker: None,
        });
        config.routes = serde_yaml::from_str(yaml).unwrap();
        config
    }

    #[test]
    fn test_route_accepts_valid_hosts() {
        let config = config_with_routes(
            "- {path: /x, upstream: web, hosts: [api.acme.dev, '*.acme.dev', 10.0.0.1]}\n",
        );
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_route_rejects_invalid_host() {
        let config = config_with_routes("- {path: /x, upstream: web, hosts: ['api.*.dev']}\n");
        let err = validate_config(&config).unwrap_err();
        assert!(err.to_string().contains("api.*.dev"), "{err}");
    }

    #[test]
    fn test_route_rejects_invalid_method() {
        let config = config_with_routes("- {path: /x, upstream: web, methods: ['G T']}\n");
        let err = validate_config(&config).unwrap_err();
        assert!(err.to_string().contains("G T"), "{err}");
    }

    #[test]
    fn test_route_rejects_duplicate_ids() {
        let config = config_with_routes(
            "- {id: web, path: /a, upstream: web}\n- {id: web, path: /b, upstream: web}\n",
        );
        let err = validate_config(&config).unwrap_err();
        assert!(err.to_string().contains("duplicate route id: web"), "{err}");
    }

    #[test]
    fn test_route_allows_distinct_ids_and_unnamed_routes() {
        let config = config_with_routes(
            "- {id: a, path: /a, upstream: web}\n- {id: b, path: /b, upstream: web}\n\
             - {path: /c, upstream: web}\n- {path: /d, upstream: web}\n",
        );
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn empty_farp_registration_token_is_rejected_while_farp_is_enabled() {
        let mut config = minimal_config();
        config.farp.enabled = true;
        for empty in ["", "   "] {
            config.farp.registration_token = Some(empty.to_string());
            let err = validate_config(&config).expect_err("empty token must fail");
            assert!(
                err.to_string().contains(
                    "farp.registration_token is empty; remove it to leave registration open or set a value"
                ),
                "{err}"
            );
        }

        config.farp.registration_token = Some("s3cret".to_string());
        assert!(validate_config(&config).is_ok());

        // A missing token stays allowed (registration open, startup warns).
        config.farp.registration_token = None;
        assert!(validate_config(&config).is_ok());

        // FARP off: the token is never read.
        config.farp.enabled = false;
        config.farp.registration_token = Some(String::new());
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn route_ids_of_the_positional_shape_are_rejected() {
        let config = config_with_routes("- {id: route-3, path: /a, upstream: web}\n");
        let err = validate_config(&config).unwrap_err();
        assert!(err.to_string().contains("route-3"), "{err}");
        assert!(err.to_string().contains("reserved"), "{err}");

        for fine in ["route-x", "route-", "route-1a", "my-route-1"] {
            let config =
                config_with_routes(&format!("- {{id: {fine}, path: /a, upstream: web}}\n"));
            assert!(validate_config(&config).is_ok(), "{fine} should be allowed");
        }
    }

    #[test]
    fn declared_routes_with_distinct_ids_must_not_collide() {
        // Same method, same trie key (`/x/:`), same host.
        let config = config_with_routes(
            "- {id: a, path: '/x/:id', methods: [GET], hosts: [a.dev], upstream: web}\n\
             - {id: b, path: '/x/:uid', methods: [GET, POST], hosts: [a.dev], upstream: web}\n",
        );
        let err = validate_config(&config).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains('a') && msg.contains('b') && msg.contains("GET"),
            "{msg}"
        );

        // An empty method list means every method, so it collides too, and an
        // id-less route collides with an id-carrying one.
        let config = config_with_routes(
            "- {id: a, path: /x, upstream: web}\n- {path: /x, methods: [DELETE], upstream: web}\n",
        );
        assert!(validate_config(&config).is_err());

        // Different host, method or path: no collision.
        let config = config_with_routes(
            "- {id: a, path: /x, methods: [GET], hosts: [a.dev], upstream: web}\n\
             - {id: b, path: /x, methods: [GET], hosts: [b.dev], upstream: web}\n\
             - {id: c, path: /x, methods: [POST], hosts: [a.dev], upstream: web}\n\
             - {id: d, path: /y, methods: [GET], hosts: [a.dev], upstream: web}\n",
        );
        assert!(validate_config(&config).is_ok());

        // Two id-less routes keep loading the way they did in 0.4.1.
        let config =
            config_with_routes("- {path: /x, upstream: web}\n- {path: /x, upstream: web}\n");
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn required_admin_auth_provider_must_be_defined() {
        let mut config = minimal_config();
        config.admin.require_auth = true;
        config.admin.auth_provider = Some("dashboard".to_string());
        let err = validate_config(&config).expect_err("unknown provider must fail");
        assert!(err.to_string().contains("dashboard"), "{err}");
        assert!(err.to_string().contains("auth_providers"), "{err}");

        config.auth_providers.insert(
            "dashboard".to_string(),
            serde_yaml::from_str("type: jwt\nsecret: s\n").unwrap(),
        );
        assert!(validate_config(&config).is_ok());

        // Without require_auth the old behavior stands.
        config.auth_providers.clear();
        config.admin.require_auth = false;
        assert!(validate_config(&config).is_ok());
    }
}
