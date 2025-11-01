# Octopus API Gateway - Quick Start Guide

**Welcome to Octopus!** 🐙

This guide will get you up and running with Octopus API Gateway in minutes.

---

## Project Status

**Phase**: Foundation (Week 1-4)  
**Current State**: Design complete, core crate structure initialized

### ✅ Completed

- [x] Comprehensive design documentation
- [x] Architecture specification (FARP, plugins, protocols)
- [x] Project structure with Cargo workspace
- [x] Core types and error handling (`octopus-core`)
- [x] CI/CD pipeline (GitHub Actions)
- [x] Docker configuration
- [x] Agent guide for AI assistants

### 🚧 In Progress

- [ ] Router implementation (trie-based matching)
- [ ] HTTP proxy with connection pooling
- [ ] Middleware pipeline
- [ ] Basic runtime and lifecycle management

### 📋 Upcoming

- FARP client for service discovery
- Protocol handlers (gRPC, WebSocket, GraphQL)
- Plugin system with dynamic loading
- Admin dashboard
- Authentication system
- Health tracking and observability

---

## Prerequisites

```bash
# Rust 1.75 or later
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Development tools
cargo install cargo-watch cargo-nextest cargo-audit cargo-deny
```

---

## Building from Source

```bash
# Clone repository
git clone https://github.com/xraph/octopus.git
cd octopus

# Build all crates
cargo build --all-features

# Run tests
cargo test --all-features

# Build release binary
cargo build --release
```

---

## Project Structure

```
octopus/
├── design/                       # Architecture & design docs
│   ├── ARCHITECTURE.md           # System architecture
│   ├── PLUGIN_SYSTEM.md          # Plugin design
│   └── FARP_INTEGRATION.md       # Service discovery
├── docs/                         # User documentation
│   └── AGENT_GUIDE.md            # For AI agents & contributors
├── crates/                       # Rust workspace crates
│   ├── octopus-core/             # ✅ Core types & traits
│   ├── octopus-runtime/          # ⏳ Async runtime
│   ├── octopus-router/           # ⏳ Routing logic
│   ├── octopus-proxy/            # ⏳ HTTP proxy
│   ├── octopus-farp/             # ⏳ FARP client
│   ├── octopus-discovery/        # ⏳ Discovery backends
│   ├── octopus-protocols/        # ⏳ Protocol handlers
│   ├── octopus-middleware/       # ⏳ Middleware
│   ├── octopus-auth/             # ⏳ Authentication
│   ├── octopus-plugins/          # ⏳ Plugin system
│   ├── octopus-scripting/        # ⏳ Rhai scripting
│   ├── octopus-health/           # ⏳ Health tracking
│   ├── octopus-admin/            # ⏳ Admin API + UI
│   ├── octopus-config/           # ⏳ Configuration
│   └── octopus-metrics/          # ⏳ Observability
├── plugins/                      # Built-in plugins
│   ├── auth-jwt/
│   ├── rate-limiter/
│   ├── cache-redis/
│   └── kafka-producer/
├── Cargo.toml                    # Workspace manifest
├── README.md                     # Project overview
├── QUICKSTART.md                 # This file
├── config.example.yaml           # Example configuration
└── Dockerfile                    # Container image

✅ = Complete   🚧 = In Progress   ⏳ = Not Started
```

---

## Configuration

Create a `config.yaml` from the example:

```bash
cp config.example.yaml config.yaml
```

Minimal configuration:

```yaml
server:
  http_port: 8080
  admin_port: 9090

discovery:
  backend: static

upstreams:
  - name: my-service
    instances:
      - address: localhost
        port: 8081

routes:
  - path: /api/*
    upstream: my-service
```

---

## Running Locally

```bash
# Development mode
cargo run --bin octopus -- --config config.yaml

# With hot reload
cargo watch -x 'run --bin octopus -- --config config.yaml'

# Production build
cargo build --release
./target/release/octopus --config config.yaml
```

---

## Docker

```bash
# Build image
docker build -t octopus:latest .

# Run container
docker run -p 8080:8080 -p 9090:9090 \
  -v $(pwd)/config.yaml:/etc/octopus/config.yaml \
  octopus:latest
```

---

## Development Workflow

### 1. Create a new feature

```bash
git checkout -b feature/my-feature
```

### 2. Make changes and test

```bash
# Run tests
cargo test --all-features

# Format code
cargo fmt --all

# Lint
cargo clippy --all-features -- -D warnings

# Security audit
cargo audit
```

### 3. Commit and push

```bash
git commit -m "feat: add my feature"
git push origin feature/my-feature
```

### 4. Create Pull Request

GitHub Actions will automatically:
- Run tests on Ubuntu, macOS, Windows
- Check formatting and linting
- Run security audit
- Generate code coverage

---

## Key Documentation

### Design Documents

- **[Architecture Guide](design/ARCHITECTURE.md)** - Complete system architecture
- **[FARP Integration](design/FARP_INTEGRATION.md)** - Service discovery protocol
- **[Plugin System](design/PLUGIN_SYSTEM.md)** - Extending Octopus

### Reference

- **[Agent Guide](docs/AGENT_GUIDE.md)** - For AI agents and new contributors
- **[Cargo Workspace](Cargo.toml)** - Crate dependencies
- **[CI/CD Pipeline](.github/workflows/ci.yml)** - Continuous integration

### External

- **[Forge Framework](https://github.com/xraph/forge)** - Go web framework (inspiration)
- **[FARP Spec](/Users/rexraphael/Work/Web-Mobile/xraph/forge/farp/)** - Service discovery protocol

---

## Common Tasks

### Add a new crate

```bash
# Create crate
cargo new --lib crates/octopus-newfeature

# Add to Cargo.toml workspace members
[workspace]
members = [
    "crates/octopus-newfeature",
    # ...
]
```

### Run specific tests

```bash
# Test single crate
cargo test -p octopus-core

# Test with specific feature
cargo test --features "full"

# Run integration tests
cargo test --test integration_test
```

### Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench --bench router_bench
```

### Profile performance

```bash
# Install flamegraph
cargo install flamegraph

# Generate flamegraph
sudo cargo flamegraph --bin octopus
```

---

## Architecture Highlights

### 1. Modular Crate Design

Each major component is a separate crate for:
- Clean separation of concerns
- Parallel compilation
- Independent versioning
- Selective feature inclusion

### 2. FARP Protocol Integration

Automatic service discovery and route generation:
- Watch discovery backends (K8s, Consul, etcd)
- Fetch API schemas (OpenAPI, AsyncAPI, gRPC)
- Generate routes dynamically
- Zero-downtime updates

### 3. Plugin System

Extend gateway without modifying core:
- Static plugins (compiled in)
- Dynamic plugins (.so/.dylib/.dll)
- Middleware, protocol handlers, admin UI
- Type-safe with Rust traits

### 4. Multi-Protocol Support

Native support for:
- HTTP/1.1, HTTP/2, HTTP/3 (QUIC)
- gRPC (with reflection)
- WebSocket (bidirectional)
- Server-Sent Events (SSE)
- GraphQL Federation
- WebTransport
- Custom protocols via plugins

---

## Performance Targets

- **Throughput**: 100k+ RPS per instance (8 cores)
- **Latency**: P99 < 10ms (proxy overhead)
- **Memory**: < 100MB baseline
- **Connections**: 10k+ concurrent

---

## Getting Help

1. **Check documentation** - Start with [AGENT_GUIDE.md](docs/AGENT_GUIDE.md)
2. **Search issues** - Existing solutions on GitHub
3. **Ask questions** - GitHub Discussions
4. **Report bugs** - GitHub Issues
5. **Join community** - Discord/Slack (coming soon)

---

## Contributing

We welcome contributions! See [AGENT_GUIDE.md](docs/AGENT_GUIDE.md) for:
- Code style guidelines
- Testing strategy
- Commit message format
- Pull request process

---

## License

Dual-licensed under MIT and Apache-2.0. Choose the license that best suits your needs.

- [LICENSE-MIT](LICENSE-MIT)
- [LICENSE-APACHE](LICENSE-APACHE)

---

## What's Next?

**For users**: Watch this space! Octopus is in active development.

**For contributors**: Check the [Architecture Guide](design/ARCHITECTURE.md) and pick a task:
- Implement router with trie-based matching
- Build HTTP proxy with connection pooling
- Create FARP client for K8s
- Develop plugin system
- Build admin dashboard

**For AI agents**: Read the [Agent Guide](docs/AGENT_GUIDE.md) for complete context.

---

**Let's build the future of API gateways! 🚀🐙**


