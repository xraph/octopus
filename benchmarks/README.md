# Octopus vs Bastion — Gateway Benchmark Suite

Comparative performance benchmarks between Octopus (Rust) and Bastion (Go) API gateways.

## Quick Start

```bash
# Run all benchmarks
make bench

# Run individual benchmarks
make bench-octopus    # Octopus only
make bench-bastion    # Bastion only
make bench-compare    # Side-by-side comparison
```

## What's Measured

| Metric | Description |
|--------|-------------|
| **Requests/sec** | Throughput under sustained load |
| **Latency (p50/p95/p99)** | Response time percentiles |
| **Error rate** | Failed requests under load |
| **Memory usage** | RSS during benchmark |
| **CPU usage** | CPU time during benchmark |

## Test Scenarios

1. **Passthrough** — Simple proxy, no middleware, GET requests
2. **JSON body** — POST with 1KB JSON body, proxy and return
3. **Auth + rate limit** — JWT auth middleware + rate limiting enabled
4. **Compression** — Gzip compression of 10KB response
5. **Concurrent connections** — Scaling from 10 to 1000 concurrent connections

## Architecture

```
                     ┌──────────────┐
   Load Generator ──▶│  Gateway     │──▶ Mock Upstream (:9999)
   (hey/wrk)         │  (octopus    │    Returns configurable
                     │   or bastion)│    response body
                     └──────────────┘
```

## Prerequisites

- Rust toolchain (for octopus)
- Go 1.21+ (for bastion harness)
- `hey` HTTP load generator: `go install github.com/rakyll/hey@latest`
- `jq` for JSON parsing: `brew install jq`
