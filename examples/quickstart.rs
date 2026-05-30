//! Quickstart example for Octopus API Gateway
//!
//! This example demonstrates:
//! - Basic gateway setup concepts
//! - Upstream configuration
//! - Middleware overview
//!
//! Run with: cargo run --bin quickstart

use octopus_core::{LoadBalanceStrategy, UpstreamCluster, UpstreamInstance};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🐙 Octopus API Gateway - Quickstart Example");
    println!("============================================\n");

    // 1. Overview
    println!("📝 Octopus API Gateway Overview");
    println!("   The gateway provides:");
    println!("   • Automatic service discovery (mDNS, Consul, etcd, Kubernetes)");
    println!("   • Dynamic routing and load balancing");
    println!("   • Health checking and circuit breakers");
    println!("   • Authentication and authorization (RBAC)");
    println!("   • Middleware (CORS, compression, logging, rate limiting)");
    println!("   • Admin dashboard and metrics");
    println!("   • Plugin system for extensibility\n");

    // 2. Configure upstream services
    println!("🎯 Configuring Upstream Services...");
    let mut user_service = UpstreamCluster::new("user-service");
    user_service.strategy = LoadBalanceStrategy::RoundRobin;

    // Add instances
    let instance1 = UpstreamInstance::new("user-service-1", "127.0.0.1", 8081);
    let instance2 = UpstreamInstance::new("user-service-2", "127.0.0.1", 8082);

    user_service.add_instance(instance1);
    user_service.add_instance(instance2);

    println!("   ✓ Upstream: {}", user_service.name);
    println!("   ✓ Strategy: {:?}", user_service.strategy);
    println!("   ✓ Instances: {}", user_service.instance_count());
    println!("   ✓ Healthy: {}\n", user_service.healthy_count());

    // 3. Available load balancing strategies
    println!("⚖️  Available Load Balancing Strategies:");
    println!("   • Round Robin - Distribute requests evenly");
    println!("   • Least Connections - Route to instance with fewest connections");
    println!("   • IP Hash - Consistent routing based on client IP");
    println!("   • Random - Random selection\n");

    // 4. Middleware overview
    println!("⚙️  Available Middleware:");
    println!("   • CORS - Cross-Origin Resource Sharing");
    println!("   • Compression - gzip, brotli, zstd");
    println!("   • Request Logging - Structured logging with trace IDs");
    println!("   • Rate Limiting - Token bucket, distributed (Redis)");
    println!("   • Request ID - Generate unique IDs for tracing");
    println!("   • JWT Authentication - RS256, HS256, ES256");
    println!("   • Security Headers - HSTS, CSP, etc.");
    println!("   • Timeouts - Request timeout enforcement");
    println!("   • Connection Limits - Max concurrent connections\n");

    // 5. Service Discovery
    println!("🔍 Service Discovery Options:");
    println!("   • mDNS/Bonjour - Zero-config local discovery");
    println!("   • Kubernetes - Native K8s service discovery");
    println!("   • Consul - Distributed service mesh");
    println!("   • etcd - Distributed key-value store");
    println!("   • Eureka - Netflix OSS discovery");
    println!("   • Static - Manual configuration\n");

    // 6. Getting Started
    println!("✨ Getting Started:");
    println!("\n  1. Start the gateway:");
    println!("     cargo run --bin octopus -- serve --config config.yaml");
    println!("\n  2. Or use the convenient commands:");
    println!("     make dev           # Auto-reload development");
    println!("     make run           # Run once");
    println!("     just dev           # With Just\n");

    println!("  3. Try the mDNS example:");
    println!("     Terminal 1: make example-mdns");
    println!("     Terminal 2: make dev");
    println!("     Terminal 3: curl http://localhost:8080/api/users\n");

    println!("📊 Access Points:");
    println!("   • Gateway: http://localhost:8080");
    println!("   • Admin Dashboard: http://localhost:8080/admin");
    println!("   • Health Check: http://localhost:8080/_/health");
    println!("   • Metrics: http://localhost:8080/_/metrics");
    println!("   • OpenAPI Docs: http://localhost:8080/_/openapi.json\n");

    println!("📚 Documentation:");
    println!("   • Quick Start: QUICKSTART.md");
    println!("   • Build System: BUILD.md");
    println!("   • Examples: EXAMPLES_GUIDE.md");
    println!("   • Architecture: design/ARCHITECTURE.md");
    println!("   • API Docs: cargo doc --open\n");

    println!("🔧 Configuration:");
    println!("   The gateway can be configured via:");
    println!("   • YAML files (config.yaml)");
    println!("   • Environment variables");
    println!("   • Command-line arguments");
    println!("   • Programmatic API (for embedded use)\n");

    println!("🎯 Next Steps:");
    println!("   1. Check examples: make examples-list");
    println!("   2. Run mDNS service: make example-mdns");
    println!("   3. Start gateway: make dev");
    println!("   4. Read the docs: QUICK_REFERENCE.md");
    println!("   5. Explore admin UI: http://localhost:8080/admin\n");

    println!("🚀 Ready to proxy requests!\n");

    Ok(())
}
