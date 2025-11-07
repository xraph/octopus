//! Rate limiting example using state backend

use octopus_state::{InMemoryBackend, Result, StateBackend};
use std::time::Duration;

/// Simple rate limiter using sliding window
struct RateLimiter<B: StateBackend> {
    backend: B,
    requests_per_window: i64,
    window_size: Duration,
}

impl<B: StateBackend> RateLimiter<B> {
    fn new(backend: B, requests_per_window: i64, window_size: Duration) -> Self {
        Self {
            backend,
            requests_per_window,
            window_size,
        }
    }

    async fn check_limit(&self, client_id: &str) -> Result<bool> {
        let key = format!("ratelimit:{}", client_id);

        let count = self
            .backend
            .increment(&key, 1, Some(self.window_size))
            .await?;

        Ok(count <= self.requests_per_window)
    }

    async fn get_remaining(&self, client_id: &str) -> Result<i64> {
        let key = format!("ratelimit:{}", client_id);

        if let Some(value) = self.backend.get(&key).await? {
            let count: i64 = String::from_utf8_lossy(&value).parse().unwrap_or(0);
            Ok((self.requests_per_window - count).max(0))
        } else {
            Ok(self.requests_per_window)
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Rate Limiting Demo ===\n");

    // Create rate limiter: 5 requests per 10 seconds
    let backend = InMemoryBackend::new();
    let limiter = RateLimiter::new(backend, 5, Duration::from_secs(10));

    // Simulate requests from different clients
    let clients = vec!["client_a", "client_b", "client_a", "client_a"];

    for (i, client) in clients.iter().enumerate() {
        let allowed = limiter.check_limit(client).await?;
        let remaining = limiter.get_remaining(client).await?;

        println!("Request {} from {}:", i + 1, client);
        if allowed {
            println!("  ✅ ALLOWED (remaining: {})", remaining);
        } else {
            println!("  ❌ RATE LIMITED (remaining: 0)");
        }
        println!();

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Spam requests to trigger rate limit
    println!("=== Spam Test (client_c) ===\n");
    for i in 1..=10 {
        let allowed = limiter.check_limit("client_c").await?;
        let remaining = limiter.get_remaining("client_c").await?;

        print!("Request {}: ", i);
        if allowed {
            println!("✅ ALLOWED (remaining: {})", remaining);
        } else {
            println!("❌ RATE LIMITED");
        }
    }

    // Wait for window to reset
    println!("\n⏳ Waiting 11 seconds for rate limit window to reset...");
    tokio::time::sleep(Duration::from_secs(11)).await;

    // Try again
    println!("\n=== After Reset ===");
    let allowed = limiter.check_limit("client_c").await?;
    let remaining = limiter.get_remaining("client_c").await?;

    if allowed {
        println!("✅ Request allowed again! (remaining: {})", remaining);
    }

    Ok(())
}
