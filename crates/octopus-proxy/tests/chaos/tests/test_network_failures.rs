//! Network failure chaos tests
//!
//! These tests use Toxiproxy to inject network failures and validate
//! that the proxy handles them gracefully.

use super::*;
use std::time::{Duration, Instant};

const PROXY_NAME: &str = "mock-upstream-1";
const UPSTREAM_URL: &str = "http://localhost:20000";

async fn setup() -> ToxiproxyClient {
    let client = ToxiproxyClient::new();
    
    // Verify Toxiproxy is available
    assert!(
        client.is_available().await,
        "Toxiproxy is not available. Run ./setup.sh first"
    );
    
    // Reset proxy to clean state
    client.reset_proxy(PROXY_NAME).await.unwrap();
    
    client
}

#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_high_latency_100ms() {
    let client = setup().await;
    
    // Add 100ms latency
    let toxic = Toxic::latency("test_latency", 100);
    client.add_toxic(PROXY_NAME, toxic).await.unwrap();
    
    // Make request and measure time
    let start = Instant::now();
    let response = reqwest::get(format!("{}/health", UPSTREAM_URL)).await;
    let elapsed = start.elapsed();
    
    // Should succeed but take >100ms
    assert!(response.is_ok(), "Request should succeed with latency");
    assert!(
        elapsed >= Duration::from_millis(100),
        "Request should take at least 100ms, took {:?}",
        elapsed
    );
    
    // Cleanup
    client.remove_toxic(PROXY_NAME, "test_latency").await.unwrap();
}

#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_very_high_latency_2s() {
    let client = setup().await;
    
    // Add 2s latency
    let toxic = Toxic::latency("test_latency", 2000);
    client.add_toxic(PROXY_NAME, toxic).await.unwrap();
    
    // Make request with appropriate timeout
    let start = Instant::now();
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    
    let response = http_client
        .get(format!("{}/health", UPSTREAM_URL))
        .send()
        .await;
    let elapsed = start.elapsed();
    
    // Should succeed but take >2s
    assert!(response.is_ok(), "Request should succeed with high latency");
    assert!(
        elapsed >= Duration::from_secs(2),
        "Request should take at least 2s, took {:?}",
        elapsed
    );
    
    // Cleanup
    client.remove_toxic(PROXY_NAME, "test_latency").await.unwrap();
}

#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_latency_with_jitter() {
    let client = setup().await;
    
    // Add 100ms latency with 50ms jitter
    let toxic = Toxic::latency_with_jitter("test_jitter", 100, 50);
    client.add_toxic(PROXY_NAME, toxic).await.unwrap();
    
    // Make multiple requests and measure variance
    let mut durations = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let response = reqwest::get(format!("{}/health", UPSTREAM_URL)).await;
        let elapsed = start.elapsed();
        
        assert!(response.is_ok());
        durations.push(elapsed);
    }
    
    // All should be >= 50ms (100ms - 50ms jitter)
    for duration in &durations {
        assert!(
            *duration >= Duration::from_millis(50),
            "Duration {:?} should be >= 50ms",
            duration
        );
    }
    
    // Should have some variance (not all exactly the same)
    let min = durations.iter().min().unwrap();
    let max = durations.iter().max().unwrap();
    assert!(
        max > min,
        "Should have jitter variance: min={:?}, max={:?}",
        min,
        max
    );
    
    // Cleanup
    client.remove_toxic(PROXY_NAME, "test_jitter").await.unwrap();
}

#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_bandwidth_throttling() {
    let client = setup().await;
    
    // Limit bandwidth to 10 KB/s
    let toxic = Toxic::bandwidth("test_bandwidth", 10);
    client.add_toxic(PROXY_NAME, toxic).await.unwrap();
    
    // Request large endpoint (should be slower)
    let start = Instant::now();
    let response = reqwest::get(format!("{}/large", UPSTREAM_URL)).await;
    let elapsed = start.elapsed();
    
    assert!(response.is_ok(), "Request should succeed with bandwidth limit");
    
    // With 10 KB/s limit, should take measurable time
    // (Actual time depends on response size)
    println!("Request with bandwidth limit took: {:?}", elapsed);
    
    // Cleanup
    client.remove_toxic(PROXY_NAME, "test_bandwidth").await.unwrap();
}

#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_timeout_causes_failure() {
    let client = setup().await;
    
    // Add timeout toxic (stops all data after timeout)
    let toxic = Toxic::timeout("test_timeout", 100);
    client.add_toxic(PROXY_NAME, toxic).await.unwrap();
    
    // Request should fail or timeout
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap();
    
    let result = http_client
        .get(format!("{}/health", UPSTREAM_URL))
        .send()
        .await;
    
    // Should fail due to timeout toxic
    assert!(result.is_err(), "Request should fail with timeout toxic");
    
    // Cleanup
    client.remove_toxic(PROXY_NAME, "test_timeout").await.unwrap();
}

#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_slow_close() {
    let client = setup().await;
    
    // Add slow close toxic (delays connection close)
    let toxic = Toxic::slow_close("test_slow_close", 1000);
    client.add_toxic(PROXY_NAME, toxic).await.unwrap();
    
    // Make request
    let response = reqwest::get(format!("{}/health", UPSTREAM_URL)).await;
    assert!(response.is_ok(), "Request should succeed");
    
    // Connection close will be delayed (not easily observable in test)
    
    // Cleanup
    client.remove_toxic(PROXY_NAME, "test_slow_close").await.unwrap();
}

#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_data_slicer() {
    let client = setup().await;
    
    // Slice data into small packets with delay
    let toxic = Toxic::slicer("test_slicer", 100, 100);
    client.add_toxic(PROXY_NAME, toxic).await.unwrap();
    
    // Request should still succeed but be slower
    let start = Instant::now();
    let response = reqwest::get(format!("{}/large", UPSTREAM_URL)).await;
    let elapsed = start.elapsed();
    
    assert!(response.is_ok(), "Request should succeed with slicer");
    println!("Request with slicer took: {:?}", elapsed);
    
    // Cleanup
    client.remove_toxic(PROXY_NAME, "test_slicer").await.unwrap();
}

#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_limit_data() {
    let client = setup().await;
    
    // Limit data to 1000 bytes
    let toxic = Toxic::limit_data("test_limit", 1000);
    client.add_toxic(PROXY_NAME, toxic).await.unwrap();
    
    // Request should be truncated
    let result = reqwest::get(format!("{}/large", UPSTREAM_URL)).await;
    
    // May succeed with partial data or fail
    if let Ok(response) = result {
        let body = response.text().await.unwrap();
        assert!(
            body.len() <= 1000,
            "Response should be truncated to <=1000 bytes, got {}",
            body.len()
        );
    }
    
    // Cleanup
    client.remove_toxic(PROXY_NAME, "test_limit").await.unwrap();
}

#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_combined_toxics() {
    let client = setup().await;
    
    // Add multiple toxics simultaneously
    let latency = Toxic::latency("test_latency", 100);
    let bandwidth = Toxic::bandwidth("test_bandwidth", 100);
    
    client.add_toxic(PROXY_NAME, latency).await.unwrap();
    client.add_toxic(PROXY_NAME, bandwidth).await.unwrap();
    
    // Request should succeed but be affected by both
    let start = Instant::now();
    let response = reqwest::get(format!("{}/health", UPSTREAM_URL)).await;
    let elapsed = start.elapsed();
    
    assert!(response.is_ok(), "Request should succeed with multiple toxics");
    assert!(
        elapsed >= Duration::from_millis(100),
        "Should have latency impact"
    );
    
    // Cleanup
    client.remove_all_toxics(PROXY_NAME).await.unwrap();
}

#[tokio::test]
#[ignore] // Requires Docker infrastructure
async fn test_baseline_without_toxics() {
    let client = setup().await;
    
    // Make request without any toxics (baseline)
    let start = Instant::now();
    let response = reqwest::get(format!("{}/health", UPSTREAM_URL)).await;
    let elapsed = start.elapsed();
    
    assert!(response.is_ok(), "Baseline request should succeed");
    assert!(response.unwrap().status().is_success());
    
    // Should be fast without toxics
    assert!(
        elapsed < Duration::from_millis(100),
        "Baseline should be fast, took {:?}",
        elapsed
    );
}
