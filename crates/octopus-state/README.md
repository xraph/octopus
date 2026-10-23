# octopus-state

Pluggable state management backends for Octopus API Gateway.

## Overview

While Octopus is a **stateless process**, it requires **stateful features** for production use:
- **Rate limiting** - Track request counts per client
- **Session management** - Store user session data
- **Circuit breaker state** - Track upstream health
- **Distributed locks** - Coordinate across instances

This crate provides a unified `StateBackend` trait with multiple implementations.

## Backends

### In-Memory (Default)

Fast, zero dependencies, single-instance only.

```toml
[dependencies]
octopus-state = "0.1"
```

```rust
use octopus_state::InMemoryBackend;

let backend = InMemoryBackend::new();
backend.set("key", b"value".to_vec(), None).await?;
```

**Use Cases:**
- Development and testing
- Single-instance deployments
- Proof-of-concept

**Pros:** Simple, fast, no external dependencies  
**Cons:** Not distributed, no persistence

---

### Redis Backend

Distributed, persistent, production-ready.

```toml
[dependencies]
octopus-state = { version = "0.1", features = ["redis-backend"] }
```

```rust
use octopus_state::RedisBackend;

let backend = RedisBackend::new("redis://localhost:6379").await?;
backend.set("key", b"value".to_vec(), Some(Duration::from_secs(60))).await?;
```

**Use Cases:**
- Multi-instance production deployments
- High-traffic applications
- Distributed rate limiting

**Pros:** Battle-tested, fast (1-2ms), distributed, persistent  
**Cons:** Requires Redis instance, network hop

---

### PostgreSQL Backend

ACID guarantees, durable storage, compliance-heavy.

```toml
[dependencies]
octopus-state = { version = "0.1", features = ["postgres-backend"] }
```

```rust
use octopus_state::PostgresBackend;

let backend = PostgresBackend::new(
    "postgresql://user:pass@localhost/db",
    10,
    "octopus_state".to_string()
).await?;
```

**Use Cases:**
- Compliance-heavy environments (SOC2, HIPAA)
- Audit trail requirements
- When you already have PostgreSQL

**Pros:** ACID guarantees, SQL audit capabilities, transactional  
**Cons:** Slower (5-10ms), overkill for transient state

---

### Hybrid Backend

Local cache + Redis for optimal performance.

```toml
[dependencies]
octopus-state = { version = "0.1", features = ["hybrid"] }
```

```rust
use octopus_state::HybridBackend;

let backend = HybridBackend::new(
    "redis://localhost:6379",
    Duration::from_secs(60) // cache TTL
).await?;
```

**Strategy:**
- **Reads**: Local cache first (μs), Redis on miss (1-2ms)
- **Writes**: Write-through to Redis, update local cache
- **Invalidation**: TTL-based expiration + manual invalidation

**Use Cases:**
- High-traffic production (>10k req/s)
- Latency-sensitive applications
- Hot key workloads

**Pros:** Fast reads, distributed consistency, cache invalidation  
**Cons:** Cache invalidation complexity, memory overhead

---

## Configuration

YAML config example:

```yaml
state:
  backend:
    type: redis
    url: "redis://localhost:6379"
    pool_size: 10
    timeout: 5s
    prefix: "octopus"
```

Multiple backends:

```yaml
state:
  backend:
    type: hybrid
    redis_url: "redis://prod-redis:6379"
    cache_ttl: 60s
    pool_size: 20
```

## API Operations

All backends implement the `StateBackend` trait:

```rust
#[async_trait]
pub trait StateBackend {
    // Core operations
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
    
    // Atomic operations
    async fn increment(&self, key: &str, delta: i64, ttl: Option<Duration>) -> Result<i64>;
    async fn compare_and_swap(&self, key: &str, expected: Vec<u8>, new: Vec<u8>) -> Result<bool>;
    
    // Batch operations
    async fn mget(&self, keys: &[String]) -> Result<Vec<Option<Vec<u8>>>>;
    async fn mset(&self, items: Vec<(String, Vec<u8>, Option<Duration>)>) -> Result<()>;
    async fn mdel(&self, keys: &[String]) -> Result<()>;
    
    // Utilities
    async fn exists(&self, key: &str) -> Result<bool>;
    async fn expire(&self, key: &str, ttl: Duration) -> Result<bool>;
    async fn keys(&self, pattern: &str) -> Result<Vec<String>>;
    async fn flush(&self) -> Result<()>;
    async fn health_check(&self) -> Result<()>;
}
```

## Examples

### Rate Limiting

```rust
use octopus_state::{StateBackend, RedisBackend};
use std::time::Duration;

async fn check_rate_limit<B: StateBackend>(
    backend: &B,
    client_ip: &str,
    limit: i64,
) -> Result<bool> {
    let key = format!("ratelimit:{}", client_ip);
    let count = backend.increment(&key, 1, Some(Duration::from_secs(60))).await?;
    Ok(count <= limit)
}
```

### Session Storage

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct Session {
    user_id: String,
    expires_at: u64,
}

async fn store_session<B: StateBackend>(
    backend: &B,
    session_id: &str,
    session: &Session,
) -> Result<()> {
    let key = format!("session:{}", session_id);
    let data = serde_json::to_vec(session)?;
    backend.set(&key, data, Some(Duration::from_secs(3600))).await?;
    Ok(())
}
```

### Circuit Breaker State

```rust
async fn record_failure<B: StateBackend>(
    backend: &B,
    upstream: &str,
) -> Result<()> {
    let key = format!("circuit:{}:failures", upstream);
    let failures = backend.increment(&key, 1, Some(Duration::from_secs(30))).await?;
    
    if failures > 10 {
        // Open circuit
        let open_key = format!("circuit:{}:open", upstream);
        backend.set(&open_key, b"1".to_vec(), Some(Duration::from_secs(60))).await?;
    }
    
    Ok(())
}
```

## Performance Characteristics

| Backend | Read Latency | Write Latency | Throughput | Distributed |
|---------|--------------|---------------|------------|-------------|
| InMemory | < 1μs | < 1μs | 1M+ ops/s | ❌ |
| Redis | 1-2ms | 1-2ms | 100K ops/s | ✅ |
| PostgreSQL | 5-10ms | 5-10ms | 10K ops/s | ✅ |
| Hybrid | < 1μs (hit)<br>1-2ms (miss) | 1-2ms | 500K+ ops/s | ✅ |

## Testing

Run tests with different backends:

```bash
# In-memory only (no external dependencies)
cargo test

# With Redis (requires Redis running)
docker run -d -p 6379:6379 redis:7-alpine
cargo test --features redis-backend

# With PostgreSQL
docker run -d -p 5432:5432 -e POSTGRES_PASSWORD=postgres postgres:16-alpine
cargo test --features postgres-backend

# All backends
cargo test --all-features
```

## Production Recommendations

### Single Instance
Use **InMemory** for simplicity.

### 2-5 Instances
Use **Redis** with connection pooling.

### 5+ Instances (High Traffic)
Use **Hybrid** (local cache + Redis) for optimal performance.

### Compliance/Audit Requirements
Use **PostgreSQL** for ACID guarantees and audit trails.

### Example Production Setup

```yaml
# High-traffic production
state:
  backend:
    type: hybrid
    redis_url: "redis://prod-redis:6379"
    cache_ttl: 30s
    pool_size: 20
    prefix: "octopus:prod"

# Cleanup expired entries every minute
cleanup_interval: 60s
```

## License

MIT OR Apache-2.0

