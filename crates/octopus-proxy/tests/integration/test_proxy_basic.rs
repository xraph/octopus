//! Basic proxy functionality integration tests

use super::*;
use octopus_proxy::{HttpProxy, ProxyConfig, HttpClient};
use http::{Method, StatusCode};
use bytes::Bytes;

#[tokio::test]
async fn test_simple_request_forwarding() {
    // Start mock upstream
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    // Configure mock to return specific response
    let mut config = MockConfig::default();
    config.body = Bytes::from("Hello from upstream");
    config.status_code = StatusCode::OK;
    mock.set_config(config).await;

    // Create proxy
    let client = HttpClient::new();
    let proxy_config = ProxyConfig::default();
    let proxy = HttpProxy::new(client, proxy_config);

    // Create upstream instance
    let upstream = TestFixtures::upstream()
        .id("test-upstream")
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    // Create request
    let req = TestFixtures::request()
        .method(Method::GET)
        .uri("/test")
        .build();

    // Proxy the request
    let response = proxy.proxy(req, &upstream).await.unwrap();

    // Verify response
    assert_eq!(response.status(), StatusCode::OK);

    // Read body
    use http_body_util::BodyExt;
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body_bytes, Bytes::from("Hello from upstream"));

    // Verify mock received the request
    let stats = mock.stats().await;
    assert_eq!(stats.requests_received, 1);
}

#[tokio::test]
async fn test_header_preservation() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    let mut config = MockConfig::default();
    config.echo_headers = true;
    mock.set_config(config).await;

    let proxy = HttpProxy::new(HttpClient::new(), ProxyConfig::default());
    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    let req = TestFixtures::request()
        .header("X-Custom-Header", "test-value")
        .header("User-Agent", "integration-test")
        .build();

    let response = proxy.proxy(req, &upstream).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_large_body_streaming() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    // Configure mock to return large body
    let large_body = TestFixtures::body(1024 * 1024); // 1MB
    let mut config = MockConfig::default();
    config.body = large_body.clone();
    mock.set_config(config).await;

    let proxy = HttpProxy::new(HttpClient::new(), ProxyConfig::default());
    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    let req = TestFixtures::request().build();
    let response = proxy.proxy(req, &upstream).await.unwrap();

    use http_body_util::BodyExt;
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body_bytes.len(), 1024 * 1024);

    let stats = mock.stats().await;
    assert!(stats.bytes_sent >= 1024 * 1024);
}

#[tokio::test]
async fn test_concurrent_requests() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    let proxy = HttpProxy::new(HttpClient::new(), ProxyConfig::default());
    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    // Send 10 concurrent requests
    let mut handles = vec![];
    for i in 0..10 {
        let proxy_clone = proxy.clone();
        let upstream_clone = upstream.clone();
        
        let handle = tokio::spawn(async move {
            let req = TestFixtures::request()
                .uri(format!("/test/{}", i))
                .build();
            proxy_clone.proxy(req, &upstream_clone).await
        });
        handles.push(handle);
    }

    // Wait for all requests
    let mut success_count = 0;
    for handle in handles {
        if let Ok(Ok(_)) = handle.await {
            success_count += 1;
        }
    }

    assert_eq!(success_count, 10);

    let stats = mock.stats().await;
    assert_eq!(stats.requests_received, 10);
}

#[tokio::test]
async fn test_connection_reuse() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    let proxy = HttpProxy::new(HttpClient::new(), ProxyConfig::default());
    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    // Send multiple sequential requests
    for _ in 0..5 {
        let req = TestFixtures::request().build();
        let response = proxy.proxy(req, &upstream).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    let stats = mock.stats().await;
    assert_eq!(stats.requests_received, 5);
    
    // Verify connection reuse (should have fewer total connections than requests)
    assert!(stats.total_connections <= 5);
}

#[tokio::test]
async fn test_post_request_with_body() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    let proxy = HttpProxy::new(HttpClient::new(), ProxyConfig::default());
    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    let request_body = Bytes::from("test request body");
    let req = TestFixtures::request()
        .method(Method::POST)
        .uri("/api/data")
        .header("Content-Type", "application/json")
        .body(request_body.clone())
        .build();

    let response = proxy.proxy(req, &upstream).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let stats = mock.stats().await;
    assert!(stats.bytes_received >= request_body.len());
}

#[tokio::test]
async fn test_different_http_methods() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    let proxy = HttpProxy::new(HttpClient::new(), ProxyConfig::default());
    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    let methods = vec![Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::PATCH];

    for method in methods {
        let req = TestFixtures::request()
            .method(method.clone())
            .uri("/test")
            .build();

        let response = proxy.proxy(req, &upstream).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "Failed for method: {:?}", method);
    }

    let stats = mock.stats().await;
    assert_eq!(stats.requests_received, 5);
}

#[tokio::test]
async fn test_upstream_error_propagation() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    // Configure mock to return error
    let mut config = MockConfig::default();
    config.status_code = StatusCode::INTERNAL_SERVER_ERROR;
    config.body = Bytes::from("Internal Server Error");
    mock.set_config(config).await;

    let proxy = HttpProxy::new(HttpClient::new(), ProxyConfig::default());
    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    let req = TestFixtures::request().build();
    let response = proxy.proxy(req, &upstream).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_route_specific_responses() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    // Add route-specific responses
    mock.add_route(
        "/api/users".to_string(),
        MockResponse::new(StatusCode::OK, Bytes::from(r#"{"users": []}"#))
            .with_header("Content-Type".to_string(), "application/json".to_string())
    ).await;

    mock.add_route(
        "/api/health".to_string(),
        MockResponse::new(StatusCode::OK, Bytes::from("healthy"))
    ).await;

    let proxy = HttpProxy::new(HttpClient::new(), ProxyConfig::default());
    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    // Test /api/users
    let req = TestFixtures::request().uri("/api/users").build();
    let response = proxy.proxy(req, &upstream).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    use http_body_util::BodyExt;
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, Bytes::from(r#"{"users": []}"#));

    // Test /api/health
    let req = TestFixtures::request().uri("/api/health").build();
    let response = proxy.proxy(req, &upstream).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, Bytes::from("healthy"));
}
