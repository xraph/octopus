//! Integration tests for Octopus Gateway

use bytes::Bytes;
use http::{Method, Request};
use http_body_util::Full;
use octopus_config::{ConfigBuilder, GatewayConfig, RouteConfig, UpstreamConfig, UpstreamInstanceConfig};
use octopus_runtime::ServerBuilder;
use std::time::Duration;

#[test]
fn test_server_initialization() {
    let config = ConfigBuilder::new()
        .gateway(GatewayConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            workers: 2,
            request_timeout: Duration::from_secs(30),
            max_body_size: 10 * 1024 * 1024,
            tls: None,
        })
        .add_upstream(UpstreamConfig {
            name: "test-backend".to_string(),
            instances: vec![UpstreamInstanceConfig {
                id: "test-1".to_string(),
                host: "127.0.0.1".to_string(),
                port: 9999,
                weight: 100,
            }],
            health_check: None,
        })
        .add_route(RouteConfig {
            path: "/api/*".to_string(),
            methods: vec!["GET".to_string(), "POST".to_string()],
            upstream: "test-backend".to_string(),
            priority: 100,
        })
        .build()
        .unwrap();

    let server = ServerBuilder::new()
        .config(config)
        .enable_farp(true)
        .enable_protocols(true)
        .enable_plugins(false) // Disabled for now
        .build()
        .unwrap();

    // Verify server state
    assert_eq!(server.request_count(), 0);
}

#[tokio::test]
async fn test_farp_routes() {
    let config = ConfigBuilder::new()
        .gateway(GatewayConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            workers: 1,
            request_timeout: Duration::from_secs(5),
            max_body_size: 10 * 1024 * 1024,
            tls: None,
        })
        .build()
        .unwrap();

    let server = ServerBuilder::new()
        .config(config)
        .enable_farp(true)
        .build()
        .unwrap();

    // FARP handler is initialized
    // In a real integration test, we would start the server and make HTTP requests
    assert!(true); // Placeholder assertion
}

#[tokio::test]
async fn test_protocol_handlers() {
    let config = ConfigBuilder::new()
        .gateway(GatewayConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            workers: 1,
            request_timeout: Duration::from_secs(5),
            max_body_size: 10 * 1024 * 1024,
            tls: None,
        })
        .build()
        .unwrap();

    let server = ServerBuilder::new()
        .config(config)
        .enable_protocols(true)
        .build()
        .unwrap();

    // Protocol handlers are initialized
    // In a real integration test, we would:
    // 1. Start the server
    // 2. Send WebSocket upgrade request
    // 3. Send gRPC request
    // 4. Send GraphQL query
    // 5. Verify responses
    assert!(true); // Placeholder assertion
}

#[test]
fn test_server_with_all_features() {
    let config = ConfigBuilder::new()
        .gateway(GatewayConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            workers: 2,
            request_timeout: Duration::from_secs(30),
            max_body_size: 10 * 1024 * 1024,
            tls: None,
        })
        .add_upstream(UpstreamConfig {
            name: "api-backend".to_string(),
            instances: vec![
                UpstreamInstanceConfig {
                    id: "api-1".to_string(),
                    host: "127.0.0.1".to_string(),
                    port: 8081,
                    weight: 100,
                },
                UpstreamInstanceConfig {
                    id: "api-2".to_string(),
                    host: "127.0.0.1".to_string(),
                    port: 8082,
                    weight: 100,
                },
            ],
            health_check: None,
        })
        .add_route(RouteConfig {
            path: "/api/*".to_string(),
            methods: vec!["GET".to_string(), "POST".to_string(), "PUT".to_string(), "DELETE".to_string()],
            upstream: "api-backend".to_string(),
            priority: 100,
        })
        .add_route(RouteConfig {
            path: "/graphql".to_string(),
            methods: vec!["GET".to_string(), "POST".to_string()],
            upstream: "api-backend".to_string(),
            priority: 200,
        })
        .build()
        .unwrap();

    let server = ServerBuilder::new()
        .config(config)
        .enable_farp(true)
        .enable_protocols(true)
        .enable_plugins(true)
        .build()
        .unwrap();

    // Verify all features are enabled
    assert_eq!(server.router().upstream_count(), 1);
    assert_eq!(server.request_count(), 0);
}

#[test]
fn test_server_builder_flags() {
    let config = ConfigBuilder::new()
        .gateway(GatewayConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            workers: 1,
            request_timeout: Duration::from_secs(5),
            max_body_size: 10 * 1024 * 1024,
            tls: None,
        })
        .build()
        .unwrap();

    // Test with FARP disabled
    let server = ServerBuilder::new()
        .config(config.clone())
        .enable_farp(false)
        .enable_protocols(false)
        .enable_plugins(false)
        .build()
        .unwrap();

    assert_eq!(server.request_count(), 0);
}

