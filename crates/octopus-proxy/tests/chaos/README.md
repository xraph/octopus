# Chaos Testing for Octopus Proxy

This directory contains chaos testing infrastructure for validating the resilience of the `octopus-proxy` crate under adverse network conditions.

## Overview

Chaos testing uses **Toxiproxy** to inject various network failures and conditions into the proxy's upstream connections, validating:

- **Network failures**: Latency, packet loss, bandwidth throttling
- **Upstream failures**: Connection drops, timeouts, flapping services
- **Resource limits**: Connection exhaustion, rate limiting
- **Cascading failures**: Multi-service failures, partial outages

## Prerequisites

- **Docker** and **Docker Compose** installed
- **curl** for API interactions
- Rust toolchain for running tests

## Quick Start

### 1. Start Infrastructure

```bash
# From the chaos directory
./setup.sh

# Or manually
docker-compose up -d
```

This starts:
- **Toxiproxy** on `localhost:8474` (API)
- **Mock upstream 1** via Toxiproxy on `localhost:20000`
- **Mock upstream 2** via Toxiproxy on `localhost:20001`
- **Mock upstream 3** via Toxiproxy on `localhost:20002`

### 2. Run Chaos Tests

```bash
# Run all chaos tests
cargo test --package octopus-proxy --test chaos

# Run specific chaos test suite
cargo test --package octopus-proxy --test chaos network_failures
cargo test --package octopus-proxy --test chaos upstream_failures
cargo test --package octopus-proxy --test chaos resource_limits
```

### 3. Stop Infrastructure

```bash
docker-compose down
```

## Infrastructure

### Toxiproxy

**Toxiproxy** is a TCP proxy that simulates network conditions and failure scenarios:

- **Latency**: Add delays to requests/responses
- **Bandwidth**: Throttle connection speed
- **Slow close**: Delay connection close
- **Timeout**: Cause connection timeouts
- **Slicer**: Slice data into smaller packets
- **Limit data**: Limit total data transmitted

### Mock Upstreams

Three Nginx-based mock services provide test endpoints:

- `/health` - Health check endpoint (returns 200)
- `/echo` - Echoes request information as JSON
- `/slow` - Simulates slow response (2s delay)
- `/large` - Returns large response for streaming tests
- `/api` - Generic API endpoint

## Toxiproxy API

### Create a Toxic (Add Network Condition)

```bash
# Add 100ms latency
curl -X POST http://localhost:8474/proxies/mock-upstream-1/toxics \
  -H "Content-Type: application/json" \
  -d '{
    "name": "latency_toxic",
    "type": "latency",
    "attributes": {
      "latency": 100
    }
  }'
```

### Remove a Toxic

```bash
curl -X DELETE http://localhost:8474/proxies/mock-upstream-1/toxics/latency_toxic
```

### List All Toxics

```bash
curl http://localhost:8474/proxies/mock-upstream-1/toxics
```

### Disable/Enable Proxy

```bash
# Disable (simulates upstream down)
curl -X POST http://localhost:8474/proxies/mock-upstream-1 \
  -H "Content-Type: application/json" \
  -d '{"enabled": false}'

# Enable
curl -X POST http://localhost:8474/proxies/mock-upstream-1 \
  -H "Content-Type: application/json" \
  -d '{"enabled": true}'
```

## Test Scenarios

### Network Failures
- ✅ High latency (100ms, 500ms, 2s)
- ✅ Intermittent packet loss
- ✅ Connection drops mid-request
- ✅ Slow response streaming
- ✅ Bandwidth throttling

### Upstream Failures
- ✅ All upstreams down
- ✅ Partial upstream failures
- ✅ Flapping service (up/down cycles)
- ✅ Slow upstream recovery
- ✅ Cascading failures across upstreams

### Resource Limits
- ✅ Connection pool exhaustion
- ✅ Rate limit exceeded scenarios
- ✅ Memory pressure (large responses)
- ✅ High concurrency (1000+ requests)

### Cascading Failures
- ✅ Circuit breaker prevents cascades
- ✅ Retry exhaustion scenarios
- ✅ Partial service degradation
- ✅ Multi-tier failure propagation

## Toxiproxy Toxic Types

| Toxic | Description | Attributes |
|-------|-------------|------------|
| `latency` | Add delay | `latency` (ms), `jitter` (ms) |
| `bandwidth` | Limit bandwidth | `rate` (KB/s) |
| `slow_close` | Delay close | `delay` (ms) |
| `timeout` | Stop all data | `timeout` (ms) |
| `slicer` | Slice data | `average_size` (bytes), `size_variation` (bytes), `delay` (μs) |
| `limit_data` | Limit total bytes | `bytes` (total bytes to transmit) |

## Architecture

```
┌─────────────────┐
│  Chaos Tests    │
│   (Rust)        │
└────────┬────────┘
         │
         │ HTTP requests
         │
┌────────▼────────────────────────────────────┐
│         Octopus Proxy (DUT)                 │
│  - Connection Pool                          │
│  - Retry Logic                              │
│  - Circuit Breaker                          │
└────────┬────────────────────────────────────┘
         │
         │ Proxied requests
         │
┌────────▼────────┐
│   Toxiproxy     │ ◄── Inject chaos here
│  (localhost:    │
│   20000-20002)  │
└────────┬────────┘
         │
         │ Upstream requests
         │
┌────────▼────────┐
│ Mock Upstreams  │
│  (Nginx)        │
└─────────────────┘
```

## Troubleshooting

### Services not starting

```bash
# Check Docker status
docker ps

# View logs
docker-compose logs -f

# Restart services
docker-compose restart
```

### Toxiproxy not responding

```bash
# Check if Toxiproxy is running
docker ps | grep toxiproxy

# Check health
curl http://localhost:8474/version

# Restart Toxiproxy
docker-compose restart toxiproxy
```

### Ports already in use

```bash
# Check what's using the ports
lsof -i :8474
lsof -i :20000-20002

# Stop conflicting services or change ports in docker-compose.yml
```

## Best Practices

1. **Clean state between tests**: Reset toxics after each test
2. **Use appropriate timeouts**: Tests with high latency need longer timeouts
3. **Verify baseline**: Always test without toxics first
4. **Isolate scenarios**: Test one failure mode at a time
5. **Document expectations**: Clearly state expected behavior under each scenario

## Examples

### Example 1: Test Retry Logic with Latency

```rust
#[tokio::test]
async fn test_retry_with_high_latency() {
    // Add 500ms latency toxic
    toxiproxy.add_toxic("latency", ToxicType::Latency {
        latency: 500,
        jitter: 0,
    }).await;

    // Make request - should retry and succeed
    let response = proxy.request("/api").await;
    assert!(response.is_ok());

    // Remove toxic
    toxiproxy.remove_toxic("latency").await;
}
```

### Example 2: Test Circuit Breaker

```rust
#[tokio::test]
async fn test_circuit_breaker_opens() {
    // Disable upstream (all requests fail)
    toxiproxy.disable().await;

    // Send requests until circuit opens
    for _ in 0..10 {
        let _ = proxy.request("/api").await;
    }

    // Verify circuit is open
    assert!(circuit_breaker.is_open());

    // Re-enable upstream
    toxiproxy.enable().await;
}
```

## Resources

- [Toxiproxy Documentation](https://github.com/Shopify/toxiproxy)
- [Chaos Engineering Principles](https://principlesofchaos.org/)
- [Testing Distributed Systems](https://asatarin.github.io/testing-distributed-systems/)

---

**Status**: Infrastructure ready for chaos testing  
**Next**: Implement chaos test suites
