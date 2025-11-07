//! Basic usage example for octopus-state

use octopus_state::{InMemoryBackend, Result, StateBackend};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Create an in-memory backend
    let backend = InMemoryBackend::new();

    // Basic set/get
    println!("=== Basic Operations ===");
    backend.set("user:123", b"Alice".to_vec(), None).await?;

    if let Some(value) = backend.get("user:123").await? {
        println!("User: {}", String::from_utf8_lossy(&value));
    }

    // Set with TTL
    println!("\n=== TTL Example ===");
    backend
        .set(
            "session:abc",
            b"session_data".to_vec(),
            Some(Duration::from_secs(5)),
        )
        .await?;

    println!("Session created with 5s TTL");
    println!("Session exists: {}", backend.exists("session:abc").await?);

    tokio::time::sleep(Duration::from_secs(6)).await;
    println!(
        "After 6s, session exists: {}",
        backend.exists("session:abc").await?
    );

    // Atomic increment (rate limiting)
    println!("\n=== Rate Limiting Example ===");
    for i in 1..=5 {
        let count = backend
            .increment("ratelimit:client1", 1, Some(Duration::from_secs(60)))
            .await?;
        println!("Request {}: count = {}", i, count);

        if count > 3 {
            println!("  ❌ Rate limit exceeded!");
        } else {
            println!("  ✅ Request allowed");
        }
    }

    // Compare-and-swap (distributed lock)
    println!("\n=== Distributed Lock Example ===");
    backend
        .set("lock:resource1", b"unlocked".to_vec(), None)
        .await?;

    let acquired = backend
        .compare_and_swap("lock:resource1", b"unlocked".to_vec(), b"locked".to_vec())
        .await?;
    println!("Lock acquired: {}", acquired);

    // Try to acquire again (should fail)
    let acquired_again = backend
        .compare_and_swap("lock:resource1", b"unlocked".to_vec(), b"locked".to_vec())
        .await?;
    println!("Lock acquired again: {}", acquired_again);

    // Batch operations
    println!("\n=== Batch Operations ===");
    backend
        .mset(vec![
            ("product:1".to_string(), b"Widget".to_vec(), None),
            ("product:2".to_string(), b"Gadget".to_vec(), None),
            ("product:3".to_string(), b"Doohickey".to_vec(), None),
        ])
        .await?;

    let values = backend
        .mget(&[
            "product:1".to_string(),
            "product:2".to_string(),
            "product:3".to_string(),
        ])
        .await?;

    println!("Products:");
    for (i, value) in values.iter().enumerate() {
        if let Some(v) = value {
            println!("  {}: {}", i + 1, String::from_utf8_lossy(v));
        }
    }

    // Pattern matching
    println!("\n=== Pattern Matching ===");
    let keys = backend.keys("product:*").await?;
    println!("Keys matching 'product:*': {:?}", keys);

    // Health check
    println!("\n=== Health Check ===");
    match backend.health_check().await {
        Ok(_) => println!("✅ Backend healthy"),
        Err(e) => println!("❌ Backend unhealthy: {}", e),
    }

    Ok(())
}
