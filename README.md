# 🐙 Octopus API Gateway

FARP™ and Octopus Gateway™ are open-source projects maintained by XRAPH™.
Octopus Gateway™ is an HTTP/API gateway that natively speaks FARP™.

**High-Performance, Extensible API Gateway in Rust**

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![CI](https://github.com/xraph/octopus/workflows/CI/badge.svg)](https://github.com/xraph/octopus/actions)

> A cloud-native API gateway with automatic service discovery (FARP), dynamic routing, multi-protocol support, and a powerful plugin system.

---

## ✨ Features

### 🚀 Core Capabilities

- **🔄 Automatic Service Discovery** - FARP protocol integration for zero-config routing
- **⚡ High Performance** - 100k+ RPS per instance, P99 latency < 10ms
- **🔌 Extensible** - Dynamic plugin system for custom functionality
- **🌐 Multi-Protocol** - REST, gRPC, WebSocket, SSE, GraphQL, WebTransport
- **📊 Observability** - Prometheus metrics, OpenTelemetry tracing, structured logging
- **🛡️ Production Ready** - Health checks, circuit breakers, graceful shutdown
- **📝 Auto-Generated Docs** - Federated OpenAPI/AsyncAPI from services

### 🔌 Protocol Support

| Protocol | Status | Description |
|----------|--------|-------------|
| HTTP/1.1 | ✅ | Full support with connection pooling |
| HTTP/2 | ✅ | Multiplexing, server push |
| HTTP/3 (QUIC) | 🚧 | Experimental |
| gRPC | ✅ | Proxying, reflection, transcoding |
| WebSocket | ✅ | Bidirectional proxying |
| SSE | ✅ | Server-Sent Events |
| GraphQL | ✅ | Federation support |
| WebTransport | 🚧 | Experimental |
| Custom | ✅ | Via plugins |

### 🎯 Service Discovery Backends

- ✅ Kubernetes (native support)
- ✅ Consul
- ✅ etcd
- ✅ Eureka
- ✅ DNS SRV records
- ✅ Static configuration

---

## 🚀 Quick Start

### Installation

```bash
# From source
cargo install octopus-gateway

# Or download binary
curl -L https://github.com/xraph/octopus/releases/latest/download/octopus-linux-amd64 -o octopus
chmod +x octopus
```

### Basic Usage

```yaml
# config.yaml
server:
  http_port: 8080
  admin_port: 9090

discovery:
  backend: kubernetes
  kubernetes:
    namespace: default

farp:
  enabled: true
  watch_interval: 5s
```

```bash
# Run the gateway
octopus --config config.yaml

# Check health
curl http://localhost:9090/_/health

# View auto-generated OpenAPI docs
curl http://localhost:9090/_/openapi.json
```

---

## 📖 Documentation

- **[Architecture Guide](design/ARCHITECTURE.md)** - System design and components
- **[FARP Integration](design/FARP_INTEGRATION.md)** - Service discovery details
- **[Plugin System](design/PLUGIN_SYSTEM.md)** - Creating plugins
- **[Agent Guide](docs/AGENT_GUIDE.md)** - For AI agents and contributors
- **[API Reference](docs/API.md)** - REST API documentation

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Client Layer                             │
│  HTTP/HTTPS │ gRPC │ WebSocket │ GraphQL │ Custom           │
└───────────────────────────────┬─────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────┐
│              Octopus API Gateway                             │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Protocol Layer: HTTP │ gRPC │ WS │ GraphQL         │  │
│  └────────────────────────┬─────────────────────────────┘  │
│  ┌────────────────────────▼─────────────────────────────┐  │
│  │  Middleware: Auth │ RateLimit │ CORS │ Scripting    │  │
│  └────────────────────────┬─────────────────────────────┘  │
│  ┌────────────────────────▼─────────────────────────────┐  │
│  │  Router: Trie-based matching, Load balancing        │  │
│  └────────────────────────┬─────────────────────────────┘  │
│  ┌────────────────────────▼─────────────────────────────┐  │
│  │  Service Registry (FARP Client)                      │  │
│  └────────────────────────┬─────────────────────────────┘  │
│  ┌────────────────────────▼─────────────────────────────┐  │
│  │  Plugin System: Dynamic loading, lifecycle mgmt      │  │
│  └──────────────────────────────────────────────────────┘  │
└───────────────────────────────┬─────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────┐
│          Discovery: K8s │ Consul │ etcd │ Eureka            │
└───────────────────────────────┬─────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────┐
│                    Upstream Services                         │
└──────────────────────────────────────────────────────────────┘
```

---

## 🔌 Plugin System

### Built-in Plugins

- **JWT Authentication** - RS256, HS256, ES256 support
- **Rate Limiter** - Token bucket, distributed (Redis)
- **Redis Cache** - Response caching with TTL
- **Kafka Producer** - Event logging
- **Metrics Exporter** - InfluxDB, Datadog

### Create a Plugin

```rust
use octopus_plugins::prelude::*;

pub struct MyPlugin;

#[async_trait]
impl Plugin for MyPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "my-plugin".to_string(),
            version: semver::Version::new(1, 0, 0),
            // ...
        }
    }
    
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![Arc::new(MyMiddleware)]
    }
}

octopus_plugins::export_plugin!(MyPlugin);
```

---

## 🎯 FARP Integration

**FARP (Forge API Gateway Registration Protocol)** enables automatic service discovery and route generation.

### How It Works

1. Services register with a `SchemaManifest` (OpenAPI, AsyncAPI, gRPC protos)
2. Octopus watches discovery backend for manifest changes
3. Gateway fetches schemas and generates routes dynamically
4. Auto-generates federated API documentation
5. Zero-downtime updates on schema changes

### Example Service Manifest

```json
{
  "version": "1.0.0",
  "service_name": "user-service",
  "service_version": "v1.2.3",
  "schemas": [
    {
      "type": "openapi",
      "spec_version": "3.1.0",
      "location": {
        "type": "http",
        "url": "http://user-service:8080/openapi.json"
      }
    }
  ],
  "capabilities": ["rest", "websocket"],
  "endpoints": {
    "health": "/health",
    "metrics": "/metrics"
  }
}
```

See [FARP Integration Guide](design/FARP_INTEGRATION.md) for details.

---

## ⚙️ Configuration

### Full Example

```yaml
server:
  http_port: 8080
  https_port: 8443
  admin_port: 9090
  workers: auto  # CPU cores

tls:
  enabled: true
  cert: /etc/octopus/tls/cert.pem
  key: /etc/octopus/tls/key.pem

discovery:
  backend: kubernetes
  kubernetes:
    namespace: default
    label_selector: "app.kubernetes.io/part-of=octopus"

farp:
  enabled: true
  watch_interval: 5s
  schema_cache_ttl: 5m

routing:
  load_balance: round-robin
  timeout: 30s
  retries: 3
  circuit_breaker:
    failure_threshold: 5
    timeout: 60s

middleware:
  - auth
  - rate_limit
  - cors
  - compression

plugins:
  - name: jwt-auth
    path: /plugins/jwt-auth.so
    config:
      secret: ${JWT_SECRET}

admin:
  enabled: true
  address: 0.0.0.0:9090
  auth:
    type: basic
    username: admin
    password_hash: $2b$...

observability:
  metrics:
    enabled: true
    path: /metrics
  tracing:
    enabled: true
    exporter: otlp
    endpoint: http://jaeger:4317
  logging:
    level: info
    format: json
```

---

## 📊 Observability

### Metrics (Prometheus)

```
# Gateway metrics
octopus_requests_total{method, status, route}
octopus_request_duration_seconds{method, route}
octopus_upstream_requests_total{upstream, status}
octopus_active_connections
octopus_circuit_breaker_state{upstream}

# FARP metrics
octopus_farp_services_total
octopus_farp_routes_total{service, type}
octopus_farp_schema_updates_total{service}

# Plugin metrics
octopus_plugin_loaded{plugin, version}
```

### Distributed Tracing

- OpenTelemetry support
- W3C Trace Context propagation
- Jaeger, Zipkin, Datadog exporters

### Structured Logging

```json
{
  "timestamp": "2025-11-01T12:00:00Z",
  "level": "info",
  "message": "request processed",
  "trace_id": "abc123",
  "method": "GET",
  "path": "/users/123",
  "status": 200,
  "duration_ms": 45,
  "upstream": "user-service-v1"
}
```

---

## 🚢 Deployment

### Docker

```bash
docker run -p 8080:8080 -p 9090:9090 \
  -v $(pwd)/config.yaml:/etc/octopus/config.yaml \
  octopus/octopus:latest
```

### Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: octopus-gateway
spec:
  replicas: 3
  template:
    spec:
      containers:
      - name: octopus
        image: octopus/octopus:latest
        ports:
        - containerPort: 8080
          name: http
        - containerPort: 9090
          name: admin
        env:
        - name: DISCOVERY_BACKEND
          value: kubernetes
        resources:
          requests:
            memory: "512Mi"
            cpu: "500m"
          limits:
            memory: "2Gi"
            cpu: "2000m"
```

### Helm Chart

```bash
helm repo add octopus https://octopus.xraph.com/charts
helm install my-gateway xraph/octopus \
  --set discovery.backend=kubernetes \
  --set farp.enabled=true
```

---

## 🧪 Development

### Prerequisites

```bash
# Rust 1.75+
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Tools
cargo install cargo-watch cargo-nextest cargo-audit
```

### Build & Test

```bash
# Clone repository
git clone https://github.com/xraph/octopus.git
cd octopus

# Build
cargo build --all-features

# Run tests
cargo test --all-features

# Run benchmarks
cargo bench

# Development mode with hot reload
cargo watch -x 'run --bin octopus -- --config config.dev.yaml'
```

### Code Quality

```bash
cargo fmt --all              # Format
cargo clippy --all-features  # Lint
cargo audit                  # Security
cargo deny check             # Dependencies
```

---

## 🎯 Performance

### Benchmarks

- **Throughput**: 100k+ RPS per instance (8 cores)
- **Latency**: P50: 2ms, P99: 8ms, P99.9: 15ms
- **Memory**: < 100MB baseline, scales linearly
- **Connection Pool**: 10k+ concurrent connections

### Load Testing

```bash
# Using wrk
wrk -t12 -c400 -d30s http://localhost:8080/test

# Using k6
k6 run --vus 1000 --duration 30s load-test.js
```

---

## 🤝 Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Workflow

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Write tests for your changes
4. Ensure all tests pass (`cargo test`)
5. Commit your changes (`git commit -m 'feat: add amazing feature'`)
6. Push to the branch (`git push origin feature/amazing-feature`)
7. Open a Pull Request

### Conventional Commits

```
feat: add WebSocket support
fix: resolve race condition in router
docs: update FARP guide
perf: optimize route matching
refactor: simplify plugin loading
test: add integration tests
```

---

## 📄 License

This project is dual-licensed under:

- **MIT License** - [LICENSE-MIT](LICENSE-MIT)
- **Apache License 2.0** - [LICENSE-APACHE](LICENSE-APACHE)

Choose the license that best suits your needs.

---

## 🙏 Acknowledgments

Built with ❤️ by the XRaph.

**Powered by:**
- [Tokio](https://tokio.rs) - Async runtime
- [Hyper](https://hyper.rs) - HTTP library
- [Tower](https://docs.rs/tower) - Middleware framework
- [Tonic](https://docs.rs/tonic) - gRPC implementation
- [Forge Framework](https://github.com/xraph/forge) - Go web framework

**Inspired by:**
- Kong, Traefik, Envoy - Production API gateways

---

## 📞 Support

- **📖 Documentation**: [docs/](docs/)
- **💬 Discussions**: [GitHub Discussions](https://github.com/xraph/octopus/discussions)
- **🐛 Issues**: [GitHub Issues](https://github.com/xraph/octopus/issues)
- **📧 Email**: support@octopus.xraph.com

---

## 🔗 Links

- **Website**: https://octopus.xraph.com
- **GitHub**: https://github.com/xraph/octopus
- **Docs**: https://octopus.xraph.com/docs
- **Blog**: https://octopus.xraph.com/blog

---

**Ready to build? Let's go! 🐙**