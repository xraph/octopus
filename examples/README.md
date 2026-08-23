# Octopus Examples

This directory contains example code demonstrating various features of the Octopus API Gateway.

## Running Examples

```bash
# Quickstart example
cargo run --example quickstart

# mDNS service discovery example
cargo run --example mdns_service -- --name my-service --port 8080
```

## Available Examples

### `quickstart.rs`

A comprehensive example showing how to:
- Configure the gateway
- Set up routing
- Configure upstream services
- Enable health checking
- Set up authentication (RBAC, users)
- Configure middleware (CORS, compression, logging)
- Initialize the plugin system
- Set up the admin dashboard

This example doesn't actually start the server, but demonstrates the configuration API.

### `mdns_service.rs`

A working HTTP service that registers itself via mDNS/Bonjour for automatic discovery by the Octopus gateway. Perfect for local development!

Features:
- **Automatic mDNS registration** - Gateway discovers it automatically
- **OpenAPI 3.0 schema** - Auto-generates routes in gateway
- **Health check endpoint** - Gateway monitors service health
- **RESTful API** - Example CRUD endpoints
- **Graceful shutdown** - Properly unregisters from mDNS

```bash
# Start with default settings (port 8080)
cargo run --example mdns_service

# Custom name and port
cargo run --example mdns_service -- --name users-api --port 9000

# Custom service type
cargo run --example mdns_service -- \
  --name my-service \
  --port 8080 \
  --service-type _myapp._tcp.local.
```

**Quick Start**: See [docs/MDNS_QUICKSTART.md](../docs/MDNS_QUICKSTART.md)  
**Full Documentation**: See [docs/MDNS_DISCOVERY.md](../docs/MDNS_DISCOVERY.md)

---

## Creating Your Own Gateway

### Minimal Example

```rust
use octopus_config::GatewayConfigBuilder;
use octopus_runtime::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create configuration
    let config = GatewayConfigBuilder::new()
        .server_address("0.0.0.0:8080".parse()?)
        .build()?;

    // Start server
    let server = Server::new(config);
    server.run().await?;

    Ok(())
}
```

### With Middleware

```rust
use octopus_middleware::{CorsMiddleware, LoggingMiddleware};

// Add middleware to your configuration
let cors = CorsMiddleware::permissive();
let logging = LoggingMiddleware::new();

// Middleware will be applied to all requests
```

### With Authentication

```rust
use octopus_auth::{UserStore, User, RoleBasedAccessControl, Role, Permission};

// Set up user store
let user_store = UserStore::new();
let user = User::new("1", "alice", "alice@example.com")
    .with_role("admin");
user_store.add_user(user, "password".to_string());

// Set up RBAC
let rbac = RoleBasedAccessControl::new();
let role = Role::new("admin")
    .with_permission(Permission::wildcard("*"));
rbac.add_role(role);
```

### With Health Checking

```rust
use octopus_health::{HealthChecker, HealthConfig};

// Create health checker
let config = HealthConfig::default();
let checker = HealthChecker::new(config);

// Health checks run automatically
```

---

## Next Steps

1. **Read the Design Docs**: See `design/ARCHITECTURE.md` for system overview
2. **Check the Quickstart**: See `QUICKSTART.md` for deployment guide
3. **Explore the Admin Dashboard**: Start the gateway and visit http://localhost:8080/admin
4. **Write Plugins**: See `design/PLUGIN_SYSTEM.md` for plugin development

---

## Need Help?

- **Documentation**: See `docs/` directory
- **API Reference**: Run `cargo doc --open`
- **Examples**: This directory
- **Issues**: GitHub issues (when repository is public)


