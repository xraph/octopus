# Octopus

An API gateway written in Rust. Octopus routes HTTP traffic to upstream services and can
discover and configure those services automatically through FARP, the Forge API Gateway
Registration Protocol.

[![CI](https://github.com/xraph/octopus/workflows/CI/badge.svg)](https://github.com/xraph/octopus/actions)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)

Octopus is maintained by Xraph and is part of the [FARP](https://github.com/xraph/farp)
ecosystem. Most gateways expect you to declare every route by hand. Services that speak FARP
instead publish their own schemas — OpenAPI, AsyncAPI, gRPC, GraphQL — and Octopus turns those
schemas into routes as services come and go. You can also run it purely statically by listing
upstreams and routes in a config file; the two modes work together.

## Capabilities

- HTTP/1.1 and HTTP/2 reverse proxy with connection pooling, configurable timeouts, and
  WebSocket upgrades.
- Trie-based router with wildcard path matching, method filtering, prefix stripping, and
  per-route priorities.
- Upstream load balancing (round-robin, weighted) with active health checks and circuit
  breaking.
- Automatic service discovery via FARP, with backends for mDNS, Consul, Kubernetes, and DNS.
- A request/response middleware chain: request IDs, CORS, JWT authentication, rate limiting,
  compression (brotli, zstd, gzip), and inline [Rhai](https://rhai.rs) scripting.
- A plugin system for bundling custom middleware and behaviour.
- TLS termination, including mutual TLS and hot certificate reloading.
- Prometheus metrics, OpenTelemetry/Jaeger tracing, and structured JSON logging.
- Layered configuration in YAML, JSON, or TOML with environment-variable substitution.

## Installation

### Docker

Images are published for `linux/amd64` and `linux/arm64` on every release.

```bash
docker run --rm \
  -p 8080:8080 -p 9090:9090 \
  -v "$(pwd)/config.yaml:/etc/octopus/config.yaml" \
  ghcr.io/xraph/octopus:latest
```

### From source

Requires Rust 1.75 or newer.

```bash
git clone https://github.com/xraph/octopus.git
cd octopus
make release
```

The binary is written to `target/release/octopus`.

## Quick start

Create a `config.yaml`:

```yaml
gateway:
  listen: "0.0.0.0:8080"
  workers: 0                 # 0 = one worker per CPU core

upstreams:
  - name: user-service
    lb_policy: round_robin
    instances:
      - id: user-1
        host: 127.0.0.1
        port: 8081
    health_check:
      type: http
      path: /health
      interval: 10s

routes:
  - path: /api/users/*
    methods: [GET, POST, PUT, DELETE]
    upstream: user-service
    strip_prefix: /api
```

Validate it, then start the gateway:

```bash
octopus validate -c config.yaml
octopus serve -c config.yaml
```

Requests matching `/api/users/*` are now proxied to `user-service` with the `/api` prefix
removed:

```bash
curl http://localhost:8080/api/users/123
```

`octopus serve` accepts several `-c` flags or a directory. Files are merged in order, so base
settings and environment overrides can live in separate files.

## Configuration

Configuration is read from YAML, JSON, or TOML. Values support `${VAR}` and `${VAR:-default}`
environment substitution. The main sections are:

| Section | Purpose |
|---------|---------|
| `gateway` | Listen address, worker count, timeouts, body limits, TLS, compression. |
| `farp` | Schema watching and the service-discovery backends. |
| `middleware` | The ordered request/response chain. |
| `plugins` | Statically or dynamically loaded plugins. |
| `upstreams` / `routes` | Static service and route definitions, used when not relying on FARP. |
| `observability` | Metrics, tracing, and logging. |

[`config.example.yaml`](config.example.yaml) is a fully annotated reference covering every
option.

## FARP

A service that speaks FARP describes itself with a manifest pointing at one or more schemas.
Octopus watches the configured discovery backend, fetches each service's schemas, and generates
routes from them. When a service updates or goes away, its routes change with it — so the
gateway stays in step with the services behind it without manual edits.

See [design/FARP_INTEGRATION.md](design/FARP_INTEGRATION.md) for the protocol details.

## Building from source

Prerequisites:

- Rust 1.75+
- `protoc` (Protocol Buffers compiler), used by the gRPC code paths
- On Linux: `pkg-config` and `libssl-dev`

Common tasks (`make help` lists them all; a `justfile` mirrors the same recipes):

```bash
make build      # debug build
make release    # optimized build
make test       # run the test suite
make lint       # rustfmt + clippy
make run        # build and run the gateway
```

## Repository layout

Octopus is a Cargo workspace. The primary crates:

| Crate | Responsibility |
|-------|----------------|
| `octopus-cli` | The `octopus` binary. |
| `octopus-runtime` | Server lifecycle and wiring. |
| `octopus-router` | Request routing. |
| `octopus-proxy` | Upstream proxying and connection pooling. |
| `octopus-farp` | FARP client and route generation. |
| `octopus-discovery` | Service-discovery backends. |
| `octopus-middleware` | Built-in middleware. |
| `octopus-auth` | Authentication. |
| `octopus-plugins` | Plugin loading and lifecycle. |
| `octopus-scripting` | Rhai scripting engine. |
| `octopus-config` | Configuration loading and merging. |
| `octopus-tls` | TLS and certificate handling. |
| `octopus-metrics` / `octopus-health` | Metrics and health checking. |

## Documentation

- [Architecture](design/ARCHITECTURE.md)
- [FARP integration](design/FARP_INTEGRATION.md)
- [Plugin system](design/PLUGIN_SYSTEM.md)
- [Contributing](CONTRIBUTING.md)

## License

Dual-licensed under either the MIT or Apache-2.0 license, at your option. See [LICENSE](LICENSE).
