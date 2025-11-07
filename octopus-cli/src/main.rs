//! Octopus CLI

use anyhow::Result;
use clap::{Parser, Subcommand};
use octopus_config::load_config;
use octopus_runtime::{ServerBuilder, SignalHandler};
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "octopus")]
#[command(about = "Octopus API Gateway", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Serve the gateway (start the server)
    Serve {
        /// Path to configuration file
        #[arg(short, long, default_value = "config.yaml")]
        config: PathBuf,

        /// Log level (trace, debug, info, warn, error)
        #[arg(short, long, default_value = "info")]
        log_level: String,
    },

    /// Validate configuration file
    Validate {
        /// Path to configuration file
        #[arg(short, long, default_value = "config.yaml")]
        config: PathBuf,
    },

    /// Show version information
    Version,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { config, log_level } => {
            // Initialize tracing
            init_tracing(&log_level)?;

            tracing::info!("Starting Octopus API Gateway");
            tracing::info!("Config file: {}", config.display());

            // Load configuration
            let config = load_config(config, true)?;

            tracing::info!(
                listen = %config.gateway.listen,
                workers = config.gateway.workers,
                "Configuration loaded"
            );

            // Build server
            let server = ServerBuilder::new().config(config).build()?;

            // Setup signal handler
            let shutdown_signal = server.shutdown_signal();
            tokio::spawn(async move {
                let handler = SignalHandler::new(shutdown_signal);
                handler.run().await;
            });

            // Run server
            tracing::info!("Server starting...");
            server.run().await?;

            tracing::info!("Server stopped");
            Ok(())
        }

        Commands::Validate { config } => {
            tracing_subscriber::fmt().with_target(false).init();

            tracing::info!("Validating configuration: {}", config.display());

            match load_config(&config, true) {
                Ok(cfg) => {
                    tracing::info!("✓ Configuration is valid");
                    tracing::info!("  Listen: {}", cfg.gateway.listen);
                    tracing::info!("  Upstreams: {}", cfg.upstreams.len());
                    tracing::info!("  Routes: {}", cfg.routes.len());
                    tracing::info!("  Plugins: {}", cfg.plugins.len());
                    Ok(())
                }
                Err(e) => {
                    tracing::error!("✗ Configuration validation failed: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Version => {
            println!("Octopus API Gateway");
            println!("Version: {}", env!("CARGO_PKG_VERSION"));
            println!("Rust version: {}", env!("CARGO_PKG_RUST_VERSION"));
            Ok(())
        }
    }
}

fn init_tracing(level: &str) -> Result<()> {
    let filter = match level.to_lowercase().as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_level(true),
        )
        .with(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(filter.into())
                // Suppress mdns-sd VPN errors - these are harmless library-level logs
                // that occur when attempting multicast on VPN tunnel interfaces (utun*)
                .add_directive("mdns_sd=warn".parse()?), // Only show WARN and above (suppress ERROR)
        )
        .init();

    Ok(())
}
