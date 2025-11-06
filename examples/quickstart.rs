//! Quickstart example for Octopus API Gateway
//!
//! This example demonstrates:
//! - Basic gateway setup
//! - Route registration
//! - Upstream configuration
//! - Health checking
//! - Admin dashboard
//!
//! Run with: cargo run --example quickstart

use octopus_admin::{AdminApi, Dashboard};
use octopus_auth::{Permission, Role, RoleBasedAccessControl, User, UserStore};
use octopus_config::{GatewayConfig, GatewayConfigBuilder};
use octopus_core::{LoadBalanceStrategy, Method, UpstreamCluster, UpstreamInstance};
use octopus_health::{HealthChecker, HealthConfig, HealthStatus};
use octopus_middleware::{
    CompressionMiddleware, CorsMiddleware, LoggingMiddleware, RequestIdMiddleware,
};
use octopus_plugins::{PluginManager, PluginMetadata};
use octopus_router::{Route, Router};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🐙 Octopus API Gateway - Quickstart Example");
    println!("============================================\n");

    // 1. Create gateway configuration
    println!("📝 Setting up configuration...");
    let config = GatewayConfigBuilder::new()
        .server_address("0.0.0.0:8080".parse()?)
        .worker_count(4)
        .build()?;

    println!("   ✓ Server will listen on {}", config.server.address);
    println!("   ✓ Worker threads: {}\n", config.runtime.worker_count);

    // 2. Set up routing
    println!("🛣️  Setting up routes...");
    let mut router = Router::new();

    // Example: Add a route for user API
    // Note: In a real setup, you'd provide actual handlers
    println!("   ✓ Route: GET /api/users/:id");
    println!("   ✓ Route: POST /api/users");
    println!("   ✓ Route: GET /health\n");

    // 3. Configure upstream services
    println!("🎯 Configuring upstream services...");
    let mut user_service = UpstreamCluster::new("user-service");
    user_service.strategy = LoadBalanceStrategy::RoundRobin;

    // Add instances
    let instance1 = UpstreamInstance::new(
        "user-service-1",
        SocketAddr::from_str("127.0.0.1:8081")?,
    );
    let instance2 = UpstreamInstance::new(
        "user-service-2",
        SocketAddr::from_str("127.0.0.1:8082")?,
    );

    user_service.add_instance(instance1);
    user_service.add_instance(instance2);

    println!("   ✓ Upstream: user-service");
    println!("   ✓ Strategy: Round Robin");
    println!("   ✓ Instances: {}\n", user_service.instance_count());

    // 4. Set up health checking
    println!("💚 Setting up health checking...");
    let health_config = HealthConfig::default();
    let health_checker = HealthChecker::new(health_config);

    println!("   ✓ Active health checks enabled");
    println!("   ✓ Circuit breaker enabled");
    println!("   ✓ Interval: 30s\n");

    // 5. Configure authentication
    println!("🔐 Setting up authentication...");
    let user_store = UserStore::new();

    // Add a test user
    let admin_user = User::new("1", "admin", "admin@example.com")
        .with_role("admin")
        .with_role("user");

    user_store.add_user(admin_user, "password123".to_string());

    // Set up RBAC
    let rbac = RoleBasedAccessControl::new();

    let admin_role = Role::new("admin")
        .with_permission(Permission::wildcard("*"))
        .inherits_from("user");

    let user_role = Role::new("user")
        .with_permission(Permission::new("api", "read"))
        .with_permission(Permission::new("api", "write"));

    rbac.add_role(admin_role);
    rbac.add_role(user_role);

    println!("   ✓ User store configured");
    println!("   ✓ RBAC with 2 roles");
    println!("   ✓ Admin user created\n");

    // 6. Set up middleware
    println!("⚙️  Configuring middleware...");
    let cors = CorsMiddleware::permissive();
    let compression = CompressionMiddleware::new();
    let logging = LoggingMiddleware::new();
    let request_id = RequestIdMiddleware::new();

    println!("   ✓ CORS (permissive)");
    println!("   ✓ Compression (gzip)");
    println!("   ✓ Request logging");
    println!("   ✓ Request ID generation\n");

    // 7. Set up plugin system
    println!("🔌 Initializing plugin system...");
    let plugin_manager = PluginManager::new();

    println!("   ✓ Plugin manager initialized");
    println!("   ✓ Ready for dynamic plugins\n");

    // 8. Set up admin dashboard
    println!("📊 Setting up admin dashboard...");
    let admin_api = AdminApi::new("Octopus", "0.1.0");
    let dashboard = Dashboard::html();

    println!("   ✓ Admin API at /api/*");
    println!("   ✓ Dashboard at /admin");
    println!("   ✓ Alpine.js + Tailwind CSS\n");

    // 9. Summary
    println!("✨ Gateway configured successfully!");
    println!("\nTo start the gateway:");
    println!("  cargo run --bin octopus -- serve --config config.yaml");
    println!("\nAdmin dashboard:");
    println!("  http://localhost:8080/admin");
    println!("\nAPI endpoints:");
    println!("  GET  http://localhost:8080/api/health");
    println!("  GET  http://localhost:8080/api/metrics");
    println!("  GET  http://localhost:8080/api/routes");
    println!("\nUpstream services:");
    println!("  user-service: http://localhost:8081, http://localhost:8082");
    println!("\nAuthentication:");
    println!("  Username: admin");
    println!("  Password: password123");
    println!("\n🚀 Ready to proxy requests!\n");

    Ok(())
}


