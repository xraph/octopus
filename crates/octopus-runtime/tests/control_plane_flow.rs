//! End to end through `ServerBuilder`: the calls a deployment control plane
//! makes against a gateway with a private admin listener, machine admin auth,
//! host-scoped routes and token-protected FARP registration.
//!
//! This file holds a single test because it sets process environment
//! variables (`OCTOPUS_ADMIN_API_TOKEN`, `OCTOPUS_FARP_TOKEN`) that the
//! server reads at build time. Each integration-test file is its own
//! process, so no other test can observe them.

use octopus_runtime::ServerBuilder;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ADMIN_TOKEN: &str = "e2e-admin-token";
const FARP_TOKEN: &str = "e2e-farp-token";

fn free_port() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

async fn wait_for(addr: SocketAddr) {
    for _ in 0..100 {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{addr} never started accepting");
}

/// One raw HTTP/1.1 request; returns the status code and the raw body.
async fn send(
    addr: SocketAddr,
    method: &str,
    target: &str,
    host: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let mut req = format!("{method} {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    for (name, value) in headers {
        req.push_str(&format!("{name}: {value}\r\n"));
    }
    let body = body.unwrap_or("");
    if !body.is_empty() || matches!(method, "POST" | "PUT") {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut raw))
        .await
        .expect("response within 10s")
        .unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no status line in {text:?}"));
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

#[tokio::test]
async fn control_plane_flow_through_the_private_listener() {
    std::env::remove_var("OCTOPUS_ADMIN_PASSWORD");
    std::env::set_var("OCTOPUS_ADMIN_API_TOKEN", ADMIN_TOKEN);
    std::env::set_var("OCTOPUS_FARP_TOKEN", FARP_TOKEN);

    // One mock backend plays both the manually routed service and the FARP
    // service: it serves a manifest (naming another instance id), an OpenAPI
    // document, and the proxied endpoints.
    let backend = MockServer::start().await;
    let backend_addr = *backend.address();
    let mut manifest = octopus_farp::SchemaManifest::new("billing-svc", "1.0.0", "pod-abc");
    manifest.endpoints.health = format!("{}/health", backend.uri());
    manifest.add_openapi_http(&format!("{}/openapi.json", backend.uri()));
    manifest.update_checksum().unwrap();
    Mock::given(method("GET"))
        .and(path("/_farp/manifest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&manifest))
        .mount(&backend)
        .await;
    Mock::given(method("GET"))
        .and(path("/openapi.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "openapi": "3.0.0",
            "info": {"title": "Billing", "version": "1.0.0"},
            "paths": {"/invoices": {"get": {"summary": "List invoices"}}}
        })))
        .mount(&backend)
        .await;
    for (route, body) in [
        ("/invoices", "farp-invoices"),
        ("/hello", "manual-hello"),
        ("/static", "config-static"),
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&backend)
            .await;
    }

    let gateway = free_port();
    let admin = free_port();
    let yaml = format!(
        r#"
gateway:
  listen: "{gateway}"
  shutdown_timeout: 1s
  pre_stop_delay: 0s
admin:
  listen: "{admin}"
  require_auth: true
upstreams:
  - name: lp-config
    instances:
      - {{id: lp-config-0, host: "127.0.0.1", port: {port}}}
routes:
  - id: lpr-config
    path: /cfg/*
    methods: [GET]
    hosts: [api.acme.dev]
    upstream: lp-config
    strip_prefix: /cfg
farp:
  enabled: true
  registration_token: "${{OCTOPUS_FARP_TOKEN}}"
  gateway:
    hostnames: [api.acme.dev]
"#,
        port = backend_addr.port()
    );
    let cfg = octopus_config::load_str(&yaml, octopus_config::ConfigFormat::Yaml).unwrap();
    let server = ServerBuilder::new()
        .config(cfg)
        .enable_plugins(false)
        .build()
        .await
        .unwrap();
    let shutdown = server.shutdown_signal();
    let run = tokio::spawn(async move { server.run().await });
    wait_for(gateway).await;
    wait_for(admin).await;

    let token = [("X-Octopus-Admin-Token", ADMIN_TOKEN)];
    let farp_token = [("Authorization", "Bearer e2e-farp-token")];

    // The config route answers on the gateway port for its host only.
    let (status, body) = send(gateway, "GET", "/cfg/static", "api.acme.dev", &[], None).await;
    assert_eq!(status, 200, "config route: {body}");
    assert!(body.contains("config-static"), "{body}");
    assert_eq!(
        send(gateway, "GET", "/cfg/static", "other.dev", &[], None)
            .await
            .0,
        404
    );

    // Upstream upsert with the contract body: 401 without the token, 200 with it.
    let upstream = format!(
        r#"{{"strategy":"round_robin","instances":[{{"id":"lp-billing-0","address":"127.0.0.1","port":{}}}]}}"#,
        backend_addr.port()
    );
    let target = "/admin/api/upstreams/lp-billing-8080";
    let (status, body) = send(admin, "PUT", target, "admin", &[], Some(&upstream)).await;
    assert_eq!(status, 401, "{body}");
    assert!(body.contains("Authentication required"), "{body}");
    let (status, body) = send(admin, "PUT", target, "admin", &token, Some(&upstream)).await;
    assert_eq!(status, 200, "upstream PUT: {body}");
    assert!(body.contains("lp-billing-8080"), "{body}");

    // Route upsert naming that upstream.
    let route = r#"{"path":"/lp/*","methods":["GET","POST"],"hosts":["api.acme.dev"],"upstream":"lp-billing-8080","strip_prefix":"/lp","priority":0}"#;
    let target = "/admin/api/routes/lpr-manual";
    assert_eq!(
        send(admin, "PUT", target, "admin", &[], Some(route))
            .await
            .0,
        401
    );
    let (status, body) = send(admin, "PUT", target, "admin", &token, Some(route)).await;
    assert_eq!(status, 200, "route PUT: {body}");

    // The admin API is not on the gateway port.
    assert_eq!(
        send(gateway, "PUT", target, "api.acme.dev", &token, Some(route))
            .await
            .0,
        404
    );

    // The manual route matches the Host shapes a proxy sends, and no other host.
    for host in ["api.acme.dev", "API.Acme.Dev.:8443"] {
        let (status, body) = send(gateway, "GET", "/lp/hello", host, &[], None).await;
        assert_eq!(status, 200, "{host}: {body}");
        assert!(body.contains("manual-hello"), "{body}");
    }
    assert_eq!(
        send(gateway, "GET", "/lp/hello", "other.dev", &[], None)
            .await
            .0,
        404
    );

    // FARP registration: 401 without the token, 201 with it.
    let register = format!(
        r#"{{"instance":{{"id":"billing-1","service_name":"billing","address":"127.0.0.1","port":{},"metadata":{{"farp.openapi.path":"/openapi.json"}}}},"manifest":null}}"#,
        backend_addr.port()
    );
    let (status, body) = send(
        admin,
        "POST",
        "/_farp/v1/register",
        "admin",
        &[],
        Some(&register),
    )
    .await;
    assert_eq!(status, 401, "{body}");
    assert!(
        body.contains("FARP registration requires a token"),
        "{body}"
    );
    let (status, body) = send(
        admin,
        "POST",
        "/_farp/v1/register",
        "admin",
        &farp_token,
        Some(&register),
    )
    .await;
    assert_eq!(status, 201, "register: {body}");
    assert!(body.contains(r#""schemas_applied":1"#), "{body}");

    // The FARP route lands on the bound hostname only.
    let (status, body) = send(
        gateway,
        "GET",
        "/billing/invoices",
        "api.acme.dev",
        &[],
        None,
    )
    .await;
    assert_eq!(status, 200, "FARP route: {body}");
    assert!(body.contains("farp-invoices"), "{body}");
    assert_eq!(
        send(gateway, "GET", "/billing/invoices", "other.dev", &[], None)
            .await
            .0,
        404
    );

    // Heartbeat and deregister use the pushed instance id.
    let heartbeat = r#"{"status":"healthy"}"#;
    let target = "/_farp/v1/heartbeat/billing-1";
    assert_eq!(
        send(admin, "PUT", target, "admin", &[], Some(heartbeat))
            .await
            .0,
        401
    );
    let (status, body) = send(admin, "PUT", target, "admin", &farp_token, Some(heartbeat)).await;
    assert_eq!(status, 200, "heartbeat: {body}");

    let target = "/_farp/v1/deregister/billing-1";
    let (status, body) = send(admin, "DELETE", target, "admin", &farp_token, None).await;
    assert_eq!(status, 200, "deregister: {body}");
    assert!(body.contains("deregistered"), "{body}");
    assert_eq!(
        send(
            gateway,
            "GET",
            "/billing/invoices",
            "api.acme.dev",
            &[],
            None
        )
        .await
        .0,
        404
    );
    let (status, _) = send(
        admin,
        "PUT",
        "/_farp/v1/heartbeat/billing-1",
        "admin",
        &farp_token,
        Some(heartbeat),
    )
    .await;
    assert_eq!(
        status, 404,
        "a deregistered instance must ask for re-registration"
    );

    // The manual and config routes outlive the FARP service.
    let (status, body) = send(gateway, "GET", "/lp/hello", "api.acme.dev", &[], None).await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = send(
        admin,
        "GET",
        "/admin/api/routes/lpr-manual",
        "admin",
        &token,
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        send(gateway, "GET", "/cfg/static", "api.acme.dev", &[], None)
            .await
            .0,
        200
    );

    shutdown.trigger();
    tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .expect("server stops after shutdown")
        .unwrap()
        .unwrap();
}
