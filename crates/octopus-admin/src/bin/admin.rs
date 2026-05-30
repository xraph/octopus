//! Standalone admin dashboard binary
//!
//! Run the admin dashboard independently, optionally connecting to a running gateway.
//!
//! Usage:
//!   octopus-admin                          # Standalone on port 9000
//!   octopus-admin --port 3000              # Custom port
//!   octopus-admin --config gateway.yaml    # Load config for display

use clap::Parser;
use octopus_admin::{AppState, DashboardRouter};
use std::net::SocketAddr;
use std::sync::Arc;

/// Octopus Admin Dashboard - Standalone Mode
#[derive(Parser, Debug)]
#[command(name = "octopus-admin", about = "Octopus API Gateway Admin Dashboard")]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value = "9000")]
    port: u16,

    /// Bind address
    #[arg(short, long, default_value = "0.0.0.0")]
    bind: String,

    /// Path to gateway configuration file (optional, for displaying config)
    #[arg(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    // Build AppState
    let mut state = AppState::new();

    // Load config if provided
    if let Some(config_path) = &args.config {
        match octopus_config::load_config(config_path, true) {
            Ok(config) => {
                tracing::info!("Loaded gateway config from {}", config_path);

                // Register upstreams and routes from config
                let router = Arc::new(octopus_router::Router::new());

                for upstream_config in &config.upstreams {
                    let mut cluster =
                        octopus_core::UpstreamCluster::new(&upstream_config.name);
                    for instance_config in &upstream_config.instances {
                        let instance = octopus_core::UpstreamInstance::new(
                            &instance_config.id,
                            &instance_config.host,
                            instance_config.port,
                        );
                        cluster.add_instance(instance);
                    }
                    router.register_upstream(cluster);
                }

                for route_config in &config.routes {
                    for method_str in &route_config.methods {
                        if let Ok(method) = method_str.parse() {
                            if let Ok(route) = octopus_router::RouteBuilder::new()
                                .path(&route_config.path)
                                .method(method)
                                .upstream_name(&route_config.upstream)
                                .priority(route_config.priority)
                                .build()
                            {
                                let _ = router.add_route(route);
                            }
                        }
                    }
                }

                state.router = Some(router);
                state.config = Some(Arc::new(config));
            }
            Err(e) => {
                tracing::warn!("Failed to load config from {}: {}", config_path, e);
            }
        }
    }

    // Create metrics collector for the standalone instance
    state.metrics = Some(Arc::new(octopus_metrics::MetricsCollector::new()));
    state.activity_log = Some(Arc::new(octopus_metrics::ActivityLog::default()));
    state.health_tracker = Some(Arc::new(octopus_health::HealthTracker::default_config()));
    state.circuit_breaker = Some(Arc::new(octopus_health::CircuitBreaker::default_config()));

    let app_state = Arc::new(state);
    let app = DashboardRouter::build(app_state);

    let addr: SocketAddr = format!("{}:{}", args.bind, args.port).parse()?;
    tracing::info!("Octopus Admin Dashboard listening on http://{}", addr);
    tracing::info!("  Dashboard UI: http://{}/admin/ui/", addr);
    tracing::info!("  Dashboard SSR: http://{}/admin", addr);
    tracing::info!("  API: http://{}/admin/api/", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
