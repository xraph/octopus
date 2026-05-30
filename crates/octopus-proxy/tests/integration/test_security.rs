//! Security integration tests - limits, rate limiting, TLS validation

use super::*;
use bytes::Bytes;
use http::{HeaderValue, Method, StatusCode};
use octopus_proxy::{InMemoryRateLimiter, ProxyLimits, RateLimitConfig, RateLimitKeyBuilder};
use std::time::Duration;

#[tokio::test]
async fn test_body_size_limits_configured() {
    let limits = ProxyLimits {
        max_request_body_size: 1024,  // 1KB limit
        max_response_body_size: 2048, // 2KB limit
        ..Default::default()
    };

    // Verify limits are set correctly
    assert_eq!(limits.max_request_body_size, 1024);
    assert_eq!(limits.max_response_body_size, 2048);

    // Body size validation happens during streaming with LimitedBody
    // which is tested separately in the proxy integration tests
}

#[tokio::test]
async fn test_proxy_limits_builder() {
    let limits = ProxyLimits::new()
        .with_max_request_body_size(2048)
        .with_max_response_body_size(4096);

    assert_eq!(limits.max_request_body_size, 2048);
    assert_eq!(limits.max_response_body_size, 4096);
}

#[tokio::test]
async fn test_uri_length_limit() {
    let limits = ProxyLimits {
        max_uri_length: 128, // 128 chars limit
        ..Default::default()
    };

    // Create a very long URI
    let long_path = format!("/api/v1/users/{}", "x".repeat(200));

    let req = TestFixtures::request()
        .method(Method::GET)
        .uri(&long_path)
        .body(Bytes::new())
        .build();

    // Validate should fail
    let result = limits.validate_request(&req);
    assert!(result.is_err(), "Should reject oversized URI");
}

#[tokio::test]
async fn test_header_count_limit() {
    let limits = ProxyLimits {
        max_headers_count: 10, // Max 10 headers
        ..Default::default()
    };

    let mut req = TestFixtures::request()
        .method(Method::GET)
        .uri("/api/test")
        .body(Bytes::new())
        .build();

    // Add 15 headers (exceeds limit)
    for i in 0..15 {
        let header_name: http::HeaderName = format!("x-custom-{}", i).parse().unwrap();
        req.headers_mut()
            .insert(header_name, HeaderValue::from_static("value"));
    }

    // Validate should fail
    let result = limits.validate_request(&req);
    assert!(result.is_err(), "Should reject too many headers");
}

#[tokio::test]
async fn test_header_total_size_limit() {
    let limits = ProxyLimits {
        max_header_size: 256, // 256 bytes total
        ..Default::default()
    };

    let mut req = TestFixtures::request()
        .method(Method::GET)
        .uri("/api/test")
        .body(Bytes::new())
        .build();

    // Add headers that exceed total size limit
    for i in 0..10 {
        let value = "x".repeat(50); // 50 chars each
        let header_name: http::HeaderName = format!("x-header-{}", i).parse().unwrap();
        req.headers_mut()
            .insert(header_name, HeaderValue::from_str(&value).unwrap());
    }

    // Validate should fail (10 headers * ~50 chars = 500+ bytes)
    let result = limits.validate_request(&req);
    assert!(result.is_err(), "Should reject oversized headers");
}

#[tokio::test]
async fn test_rate_limiter_basic() {
    let config = RateLimitConfig {
        requests_per_window: 10,
        window_duration: Duration::from_secs(1),
        burst_size: 10,
        ..Default::default()
    };

    let limiter = InMemoryRateLimiter::new(config);

    // First 10 requests should succeed (burst)
    for i in 0..10 {
        let key = format!("user-123");
        let result = limiter.check(&key);
        assert!(result.is_allowed(), "Request {} should be allowed", i);
    }

    // 11th request should be rate limited
    let result = limiter.check("user-123");
    assert!(!result.is_allowed(), "Request 11 should be rate limited");
}

#[tokio::test]
async fn test_rate_limiter_per_key_isolation() {
    let config = RateLimitConfig {
        requests_per_window: 5,
        window_duration: Duration::from_secs(1),
        burst_size: 5,
        ..Default::default()
    };

    let limiter = InMemoryRateLimiter::new(config);

    // Exhaust limit for user-1
    for _ in 0..5 {
        let result = limiter.check("user-1");
        assert!(result.is_allowed());
    }
    let result = limiter.check("user-1");
    assert!(!result.is_allowed(), "user-1 should be rate limited");

    // user-2 should still have capacity
    let result = limiter.check("user-2");
    assert!(result.is_allowed(), "user-2 should not be rate limited");
}

#[tokio::test]
async fn test_rate_limiter_token_refill() {
    let config = RateLimitConfig {
        requests_per_window: 10,
        window_duration: Duration::from_secs(1),
        burst_size: 5,
        ..Default::default()
    };

    let limiter = InMemoryRateLimiter::new(config);
    let key = "user-123";

    // Exhaust the bucket
    for _ in 0..5 {
        let result = limiter.check(key);
        assert!(result.is_allowed());
    }

    // Should be limited now
    let result = limiter.check(key);
    assert!(!result.is_allowed());

    // Wait for tokens to refill (150ms = ~1-2 tokens at 10 per second)
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Should be allowed again
    let result = limiter.check(key);
    assert!(result.is_allowed(), "Should allow after token refill");
}

#[tokio::test]
async fn test_rate_limit_key_builder_ip() {
    let key = RateLimitKeyBuilder::by_ip("192.168.1.100");
    assert!(key.contains("192.168.1.100"), "Key should include IP");
    assert!(key.contains("ip:"), "Key should have ip: prefix");
}

#[tokio::test]
async fn test_rate_limit_key_builder_path() {
    let key = RateLimitKeyBuilder::by_path("/api/users/123");
    assert!(key.contains("/api/users/123"), "Key should include path");
    assert!(key.contains("path:"), "Key should have path: prefix");
}

#[tokio::test]
async fn test_rate_limit_key_builder_api_key() {
    let key = RateLimitKeyBuilder::by_api_key("secret123");
    assert!(key.contains("secret123"), "Key should include API key");
    assert!(key.contains("apikey:"), "Key should have apikey: prefix");
}

#[tokio::test]
async fn test_rate_limit_key_builder_user() {
    let key = RateLimitKeyBuilder::by_user("user-456");
    assert!(key.contains("user-456"), "Key should include user ID");
    assert!(key.contains("user:"), "Key should have user: prefix");
}

#[tokio::test]
async fn test_rate_limit_key_builder_composite() {
    let key = RateLimitKeyBuilder::composite(&["user", "123", "/api/users"]);
    assert!(key.contains("user"), "Key should include user");
    assert!(key.contains("123"), "Key should include ID");
    assert!(key.contains("/api/users"), "Key should include path");
}

#[tokio::test]
async fn test_combined_limits() {
    let limits = ProxyLimits {
        max_request_body_size: 1024,
        max_uri_length: 256,
        max_headers_count: 20,
        max_header_size: 512,
        ..Default::default()
    };

    // Create a valid request
    let req = TestFixtures::request()
        .method(Method::POST)
        .uri("/api/test")
        .header("x-request-id", "12345")
        .body(TestFixtures::body(512))
        .build();

    // Should pass all validations
    let result = limits.validate_request(&req);
    assert!(result.is_ok(), "Valid request should pass all limits");
}

#[tokio::test]
async fn test_empty_body_bypass_limit() {
    let limits = ProxyLimits {
        max_request_body_size: 1024,
        ..Default::default()
    };

    // GET request with no body
    let req = TestFixtures::request()
        .method(Method::GET)
        .uri("/api/test")
        .body(Bytes::new())
        .build();

    // Should pass even with strict limits
    let result = limits.validate_request(&req);
    assert!(result.is_ok(), "Empty body should bypass size limit");
}

#[tokio::test]
async fn test_default_limits_are_reasonable() {
    let limits = ProxyLimits::default();

    // Verify defaults are reasonable
    assert_eq!(limits.max_request_body_size, 10 * 1024 * 1024); // 10MB
    assert_eq!(limits.max_response_body_size, 100 * 1024 * 1024); // 100MB
    assert_eq!(limits.max_header_size, 8 * 1024); // 8KB
    assert_eq!(limits.max_uri_length, 8192);
    assert_eq!(limits.max_headers_count, 100);
}

#[tokio::test]
async fn test_limits_builder_pattern() {
    let limits = ProxyLimits::new()
        .with_max_request_body_size(2048)
        .with_max_uri_length(256)
        .with_max_header_size(1024)
        .with_request_timeout(Duration::from_secs(10));

    assert_eq!(limits.max_request_body_size, 2048);
    assert_eq!(limits.max_uri_length, 256);
    assert_eq!(limits.max_header_size, 1024);
    assert_eq!(limits.request_timeout, Duration::from_secs(10));
}

#[tokio::test]
async fn test_rate_limiter_check_method() {
    let config = RateLimitConfig {
        requests_per_window: 5,
        window_duration: Duration::from_secs(1),
        burst_size: 5,
        ..Default::default()
    };

    let limiter = InMemoryRateLimiter::new(config);

    // Use check method
    for _ in 0..5 {
        let result = limiter.check("test-key");
        assert!(result.is_allowed());
    }

    // Should be limited
    let result = limiter.check("test-key");
    assert!(!result.is_allowed());
}

#[tokio::test]
async fn test_rate_limiter_check_tokens() {
    let config = RateLimitConfig {
        requests_per_window: 10,
        window_duration: Duration::from_secs(1),
        burst_size: 10,
        ..Default::default()
    };

    let limiter = InMemoryRateLimiter::new(config);

    // Consume 5 tokens at once
    let result = limiter.check_tokens("test-key", 5);
    assert!(result.is_allowed());

    // Consume another 5 tokens
    let result = limiter.check_tokens("test-key", 5);
    assert!(result.is_allowed());

    // Try to consume 1 more - should fail
    let result = limiter.check_tokens("test-key", 1);
    assert!(!result.is_allowed());
}
