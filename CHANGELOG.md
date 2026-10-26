# Changelog

All notable changes to the Octopus API Gateway project will be documented in this file.

## [0.5.0] - 2026-10-23

You can now scope routes to hostnames, run admin and FARP on a private port, and authenticate both with tokens a machine can send. A config that sets none of the new keys still behaves as 0.4.1, apart from the items under Changed.

### Added

- `routes[].hosts` scopes a static route to exact hostnames or `*.zone` wildcards. Empty means any host. `*.acme.dev` never matches the apex `acme.dev`, and `/p/*` never matches `/p`.
- `routes[].id` gives a route a stable name. Ids are unique, and `route-<number>` is reserved for the positional ids the admin API shows for routes without one.
- Admin routes are addressed by id. `PUT /admin/api/routes/{id}` is an idempotent upsert that writes one entry per method and host, `GET` and `DELETE` answer 404 for a missing id, and `POST` generates an id when you leave it out. A conflict with another declared route on the same method, path and host answers 409 with the other route's id. `GET /admin/api/routes` reports `route_id`, `host` and `origin` for every entry.
- A route you declare, in config or through the admin API, shadows a FARP route on the same host and path whatever the priorities. Delete it and the FARP route comes back.
- `admin.listen` starts a second, plain-HTTP listener (9090 in the bundled manifests) for the admin UI, admin API, FARP and `/metrics`. With it set, the gateway port serves only proxied traffic and probes, which answer on both ports.
- `OCTOPUS_ADMIN_API_TOKEN`, accepted as a bearer token in the `Authorization` header or as the `X-Octopus-Admin-Token` header. Use `X-Octopus-Admin-Token` through the Kubernetes API service proxy, which strips `Authorization`.
- `admin.require_auth: true` refuses to start without admin credentials.
- A sign-in form on the admin 401 page when password login is enabled.
- `farp.registration_token`. With it set, every FARP request other than GET and HEAD needs the token as a bearer token or `X-Octopus-Farp-Token`.
- `farp.gateway.hostnames` binds FARP routes to every listed hostname and no other. The single `farp.gateway.hostname` still works.
- `GET` and `PUT /admin/api/farp/binding` read and change the FARP hostnames at runtime (in memory only).
- `farp.*` Service annotations on Kubernetes, so path keys such as `farp.openapi.path` can hold a `/`, and `farp.port` to pick one port by number or name.
- Push registration without a manifest builds one from `farp.openapi.path` in the instance metadata. When a schema fetch fails, the service keeps its current routes, the response says `schemas_applied: 0`, and Octopus retries on the next heartbeat.
- `PUT /admin/api/upstreams/{name}` accepts a body without `name`.
- Each GitHub release carries `octopus-image-digests.json` with the image index digest and the `linux/amd64` and `linux/arm64` digests.
- The Helm chart puts the admin port on its own ClusterIP Service (`<release>-octopus-admin`).

### Changed

- Empty `methods` on a route now means every method, as the docs always said.
- `octopus serve` now reads `OCTOPUS_ADMIN_PASSWORD` and the other `OCTOPUS_ADMIN_*` variables. Before, only the standalone admin binary did. The server-rendered admin pages need a session once auth is on.
- Push-registered FARP routes strip `/{service}` before proxying, the same as discovered ones.
- FARP deregistration removes the service's routes and upstream. Re-registration replaces them.
- A heartbeat for an instance the gateway doesn't know answers 404, so the caller re-registers.
- Operator-managed routes carry origin `kubernetes`. They don't shadow FARP routes: the two compete on host specificity, then priority.
- Lowercase `methods` (`get`) now route as GET.
- `upstreams[].lb_policy` and instance `weight` from config are honoured. A 0.4.1 config that set either will balance differently.
- Kubernetes Service annotations prefixed `farp.` merge into the instance metadata, so an annotation alone can opt a Service in when no `label_selector` is set.
- The bundled Kubernetes and Helm manifests set `admin.listen: "0.0.0.0:9090"` and pin `ghcr.io/xraph/octopus:0.5.0`.
- Release images are tagged `0.5.0`, `0.5` and `sha-<commit>` only. `latest` stays at 0.4.1 and won't move again.
- Outbound HTTP in `octopus-auth` (OIDC discovery and JWKS, introspection, forward auth, OPA, AuthZEN) and `octopus-farp` uses rustls with the platform verifier in place of native-tls. TLS 1.0 and 1.1 are refused. On a Linux host with no CA bundle those clients fail to build and the auth provider isn't registered; the official image ships `ca-certificates`.
- The minimum supported Rust version is now 1.88. Current dependency releases already needed it, and the image builds on `rust:1.88`.
- Library API: `apply_gateway_binding` is replaced by `expand_gateway_binding`, `GatewayBinding.host` became `hosts`, `RegistryBackend::Consul` holds an `octopus_farp::fetcher::ConsulKvClient`, and `IntrospectionProvider::with_client` takes a reqwest 0.13 `Client`.

### Fixed

- Layered configs keep routes with the same path on different hosts.
- The `Host` header is matched lowercased, without its port and without one trailing dot, and an IPv6 `[::1]:8080` host keeps its brackets.
- The `consul-backend` FARP registry works. The old blocking client never decoded values.
- `start_time` in `GET /admin/api/system/info` is captured once at startup and no longer drifts between calls.

### Security

- Cleared every open RustSec advisory without new audit ignores (h2, quinn-proto, rustls, rustls-webpki, webpki-roots). h2 0.3, hyper 0.14, reqwest 0.11 and tonic 0.11 are gone from the dependency tree.
- Moved zstd to 0.14.3, zstd-safe to 8.2.0 and zstd-sys to 2.1.2, past RUSTSEC-2026-0391, RUSTSEC-2026-0398, RUSTSEC-2026-0403 and RUSTSEC-2026-0410. octopus-compression and octopus-middleware now depend on zstd 0.14 directly, and async-compression 0.4.51 (compression-codecs 0.4.46) puts the tower-http compression path on the same fixed versions.
- Admin and FARP tokens are compared by SHA-256 digest, redacted from debug output and request logs, and never serialized.
