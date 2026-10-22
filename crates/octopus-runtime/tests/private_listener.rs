//! `admin.listen` serves the admin API on a second, private TCP listener and
//! takes it off the gateway port.

use octopus_runtime::ServerBuilder;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn free_port() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

/// One raw HTTP/1.1 GET; returns the status code and the body.
async fn get(addr: SocketAddr, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
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

/// Connect until the listener accepts, so the test never races `run`.
async fn wait_for(addr: SocketAddr) {
    for _ in 0..100 {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{addr} never started accepting");
}

#[tokio::test]
async fn admin_listener_serves_admin_api_and_gateway_does_not() {
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
"#
    );
    let cfg = octopus_config::load_str(&yaml, octopus_config::ConfigFormat::Yaml).unwrap();
    assert_eq!(cfg.admin.listen, Some(admin));

    let server = ServerBuilder::new().config(cfg).build().await.unwrap();
    assert_eq!(server.admin_listen_addr(), Some(admin));
    let shutdown = server.shutdown_signal();
    let run = tokio::spawn(async move { server.run().await });

    wait_for(gateway).await;
    wait_for(admin).await;

    let (status, body) = get(admin, "/admin/api/system/info").await;
    assert_eq!(status, 200, "admin port: {body}");
    assert!(body.contains("version"), "admin port body: {body}");

    let (status, _) = get(gateway, "/admin/api/system/info").await;
    assert_eq!(status, 404, "gateway port must not serve the admin API");

    let (status, body) = get(admin, "/metrics").await;
    assert_eq!(status, 200, "admin port /metrics: {body}");
    assert_eq!(
        get(gateway, "/metrics").await.0,
        404,
        "gateway port must not serve /metrics"
    );

    assert_eq!(get(gateway, "/livez").await.0, 200);
    assert_eq!(get(admin, "/livez").await.0, 200);

    shutdown.trigger();
    tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .expect("server stops after shutdown")
        .unwrap()
        .unwrap();

    // The admin listener stopped accepting along with the gateway.
    assert!(TcpStream::connect(admin).await.is_err());
}
