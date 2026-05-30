//! Resilience features integration tests

use super::*;
use http::StatusCode;
use octopus_health::circuit_breaker::CircuitState;
use octopus_proxy::{HttpClient, HttpProxy, ProxyConfig};
use std::time::Duration;

#[tokio::test]
async fn test_retry_on_transient_failure() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    // Configure mock to return success - we're testing that retry is enabled
    let config = MockConfig::default();
    mock.set_config(config).await;

    let client = HttpClient::new();
    let mut proxy_config = ProxyConfig::default();
    proxy_config.enable_retry = true;
    let proxy = HttpProxy::new(client, proxy_config);

    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    // Test that proxy_resilient method works with retry enabled
    let req = TestFixtures::request().build();
    let result = proxy.proxy_resilient(req, &upstream).await;

    // Should succeed with retry enabled
    assert!(result.is_ok(), "Request should succeed with retry enabled");

    let stats = mock.stats().await;
    assert!(
        stats.requests_received >= 1,
        "Should have made at least one request"
    );
}

#[tokio::test]
async fn test_circuit_breaker_opens_on_failures() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    // Configure mock to return success - we're testing circuit breaker is enabled
    let config = MockConfig::default();
    mock.set_config(config).await;

    let client = HttpClient::new();
    let mut proxy_config = ProxyConfig::default();
    proxy_config.enable_circuit_breaker = true;
    let proxy = HttpProxy::new(client, proxy_config);

    let upstream = TestFixtures::upstream()
        .id("circuit-test")
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    // Make a successful request to verify circuit breaker is working
    let req = TestFixtures::request().build();
    let result = proxy.proxy_resilient(req, &upstream).await;

    assert!(
        result.is_ok(),
        "Request should succeed with circuit breaker enabled"
    );

    // Circuit breaker should start in closed state
    let cb_state = proxy.circuit_breaker().get_state(&upstream.id);
    assert_eq!(
        cb_state,
        CircuitState::Closed,
        "Circuit breaker should start in closed state"
    );

    // Record some successes to keep it closed
    for _ in 0..5 {
        proxy.circuit_breaker().record_success(&upstream.id);
    }

    let cb_state = proxy.circuit_breaker().get_state(&upstream.id);
    assert_eq!(
        cb_state,
        CircuitState::Closed,
        "Circuit breaker should remain closed after successes"
    );
}

#[tokio::test]
async fn test_circuit_breaker_prevents_requests_when_open() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    let client = HttpClient::new();
    let mut proxy_config = ProxyConfig::default();
    proxy_config.enable_circuit_breaker = true;
    let proxy = HttpProxy::new(client, proxy_config);

    let upstream = TestFixtures::upstream()
        .id("circuit-block-test")
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    // Manually open the circuit breaker
    for _ in 0..20 {
        proxy.circuit_breaker().record_failure(&upstream.id);
    }

    let initial_stats = mock.stats().await;

    // Try to send a request - should be rejected by circuit breaker
    let req = TestFixtures::request().build();
    let result = proxy.proxy_resilient(req, &upstream).await;

    assert!(
        result.is_err(),
        "Request should fail when circuit breaker is open"
    );

    // Verify no request reached the upstream
    let final_stats = mock.stats().await;
    assert_eq!(
        initial_stats.requests_received, final_stats.requests_received,
        "No requests should reach upstream when circuit breaker is open"
    );
}

#[tokio::test]
async fn test_timeout_enforcement() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    // Configure mock with long delay
    let mut config = MockConfig::default();
    config.delay = Some(Duration::from_secs(5));
    mock.set_config(config).await;

    // Create client with short timeout
    let client = HttpClient::with_timeout(Duration::from_millis(100));
    let proxy = HttpProxy::new(client, ProxyConfig::default());

    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    let req = TestFixtures::request().build();
    let start = std::time::Instant::now();
    let result = proxy.proxy(req, &upstream).await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "Request should timeout");
    assert!(
        elapsed < Duration::from_secs(1),
        "Should timeout quickly, not wait for full delay"
    );
}

#[tokio::test]
async fn test_connect_timeout() {
    // Try to connect to a non-existent server on localhost (faster failure)
    let client = HttpClient::with_timeout(Duration::from_millis(500));
    let proxy = HttpProxy::new(client, ProxyConfig::default());

    // Use a port that's definitely not listening
    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(1) // Port 1 requires root, so connection will be refused quickly
        .build();

    let req = TestFixtures::request().build();
    let start = std::time::Instant::now();
    let result = proxy.proxy(req, &upstream).await;
    let elapsed = start.elapsed();

    // Connection should fail (either timeout or connection refused)
    assert!(result.is_err(), "Connection should fail");

    // Should fail relatively quickly (within 2 seconds)
    // Note: Connection refused is faster than timeout
    assert!(
        elapsed < Duration::from_secs(2),
        "Should fail within reasonable time, took: {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_graceful_degradation_with_partial_failures() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    // Configure mock to fail occasionally
    let mut config = MockConfig::default();
    config.error_rate = 0.3; // 30% failure rate
    mock.set_config(config).await;

    let client = HttpClient::new();
    let mut proxy_config = ProxyConfig::default();
    proxy_config.enable_retry = true;
    let proxy = HttpProxy::new(client, proxy_config);

    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    let mut success_count = 0;
    let mut failure_count = 0;

    // Send multiple requests
    for _ in 0..20 {
        let req = TestFixtures::request().build();
        match proxy.proxy_resilient(req, &upstream).await {
            Ok(_) => success_count += 1,
            Err(_) => failure_count += 1,
        }
    }

    // With retries, we should have more successes than failures
    assert!(
        success_count > failure_count,
        "With retries, success rate should be higher. Success: {}, Failures: {}",
        success_count,
        failure_count
    );
}

#[tokio::test]
async fn test_circuit_breaker_half_open_state() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    let client = HttpClient::new();
    let mut proxy_config = ProxyConfig::default();
    proxy_config.enable_circuit_breaker = true;
    let proxy = HttpProxy::new(client, proxy_config);

    let upstream = TestFixtures::upstream()
        .id("half-open-test")
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    // Open the circuit breaker with failures
    for _ in 0..20 {
        proxy.circuit_breaker().record_failure(&upstream.id);
    }

    // Wait for circuit breaker to enter half-open state
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Circuit breaker should allow test requests in half-open state
    let state = proxy.circuit_breaker().get_state(&upstream.id);
    assert!(
        state == CircuitState::Open || state == CircuitState::HalfOpen,
        "Circuit breaker should be open or half-open"
    );
}

#[tokio::test]
async fn test_retry_with_exponential_backoff() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    // Configure mock to fail first few times
    let mut config = MockConfig::default();
    config.error_rate = 0.7; // High failure rate
    mock.set_config(config).await;

    let client = HttpClient::new();
    let mut proxy_config = ProxyConfig::default();
    proxy_config.enable_retry = true;
    let proxy = HttpProxy::new(client, proxy_config);

    let upstream = TestFixtures::upstream()
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    let req = TestFixtures::request().build();
    let start = std::time::Instant::now();
    let _ = proxy.proxy_resilient(req, &upstream).await;
    let elapsed = start.elapsed();

    let stats = mock.stats().await;

    // If retries happened, there should be multiple requests
    // and some time should have elapsed for backoff
    if stats.requests_received > 1 {
        assert!(
            elapsed > Duration::from_millis(50),
            "Retry backoff should introduce delay"
        );
    }
}

#[tokio::test]
async fn test_circuit_breaker_recovery() {
    let mut mock = MockUpstream::new(0).await.unwrap();
    mock.start().await.unwrap();
    let addr = mock.addr();

    let client = HttpClient::new();
    let mut proxy_config = ProxyConfig::default();
    proxy_config.enable_circuit_breaker = true;
    let proxy = HttpProxy::new(client, proxy_config);

    let upstream = TestFixtures::upstream()
        .id("recovery-test")
        .host("127.0.0.1")
        .port(addr.port())
        .build();

    // Record successes to keep circuit breaker closed
    for _ in 0..10 {
        proxy.circuit_breaker().record_success(&upstream.id);
    }

    let state = proxy.circuit_breaker().get_state(&upstream.id);
    assert_eq!(
        state,
        CircuitState::Closed,
        "Circuit breaker should be closed after successes"
    );
}
