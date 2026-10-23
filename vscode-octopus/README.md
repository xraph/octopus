# Octopus API Gateway - VS Code Extension

Schema validation, IntelliSense, and snippets for [Octopus API Gateway](https://github.com/xraph/octopus) configuration files.

## Features

- **Schema Validation** -- Real-time error highlighting and validation for Octopus YAML and JSON configuration files.
- **Autocomplete** -- Context-aware suggestions for all configuration fields including gateway settings, routes, upstreams, auth providers, CORS, gRPC, FARP, and more.
- **Rich Snippets** -- Over 20 snippets for quickly scaffolding common configuration patterns. Type `octopus-` to see all available snippets.
- **Status Bar** -- Shows an "Octopus" indicator in the status bar when editing configuration files.

## Supported File Patterns

| Pattern | Schema |
|---|---|
| `config.yaml`, `config.*.yaml` | Gateway config |
| `octopus.yaml`, `octopus.*.yaml` | Gateway config |
| `octopus-config.yaml`, `octopus-config.*.yaml` | Gateway config |
| `octopus-gen.yaml`, `octopus-gen.*.yaml` | Code generation config |
| `*.octopus.json`, `octopus-schema.json` | Octopus schema intermediate format |

## Installation

### From VSIX (local)

```sh
cd vscode-octopus
npm install
npm run build
npm run package
code --install-extension octopus-gateway-0.1.0.vsix
```

### From Source

1. Clone the repository and open the `vscode-octopus` directory in VS Code.
2. Run `npm install` then `npm run build`.
3. Press `F5` to launch the Extension Development Host.

## Prerequisites

For full YAML IntelliSense, install the [YAML extension](https://marketplace.visualstudio.com/items?itemName=redhat.vscode-yaml) by Red Hat. The extension will automatically register schema associations when both extensions are active.

## Snippets

All snippets are prefixed with `octopus-`. Available snippets include:

| Prefix | Description |
|---|---|
| `octopus-config` | Full gateway configuration boilerplate |
| `octopus-route` | Route definition |
| `octopus-upstream` | Upstream with health checks |
| `octopus-auth-jwt` | JWT auth provider |
| `octopus-auth-oidc` | OIDC auth provider |
| `octopus-auth-apikey` | API key auth provider |
| `octopus-auth-forward` | Forward auth provider |
| `octopus-auth-mtls` | mTLS auth provider |
| `octopus-auth-global` | Global auth settings |
| `octopus-authz-rule` | Authorization rule |
| `octopus-cors` | CORS configuration |
| `octopus-tls` | TLS/mTLS configuration |
| `octopus-rate-limit` | Rate limiting |
| `octopus-health-check` | Health check |
| `octopus-farp` | FARP service discovery |
| `octopus-grpc` | gRPC gateway config |
| `octopus-middleware` | Middleware entry |
| `octopus-plugin` | Plugin entry |
| `octopus-script` | Rhai script middleware |
| `octopus-circuit-breaker` | Circuit breaker |
| `octopus-observability` | Observability settings |
| `octopus-compression` | Compression settings |
| `octopus-admin` | Admin dashboard config |

## License

MIT
