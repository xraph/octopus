//! Octopus CLI

mod gen;

use anyhow::Result;
use clap::{Parser, Subcommand};
use octopus_config::{load_and_merge, load_config};
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
        /// Config file(s) or directory. Multiple values are merged in order.
        /// If a directory is given, all *.yaml/*.yml/*.json/*.toml files are merged.
        #[arg(short, long, default_value = "config.yaml")]
        config: Vec<PathBuf>,

        /// Log level (trace, debug, info, warn, error). Overrides
        /// `observability.logging.level` from the config when set.
        #[arg(short, long)]
        log_level: Option<String>,
    },

    /// Validate configuration file(s)
    Validate {
        /// Config file(s) or directory
        #[arg(short, long, default_value = "config.yaml")]
        config: Vec<PathBuf>,
    },

    /// Generate config, schema, and TypeScript client from API specs
    Gen {
        /// Path to octopus-gen.yaml configuration file
        #[arg(short, long, default_value = "octopus-gen.yaml")]
        config: PathBuf,
    },

    /// Print the Octopus CRD definitions as YAML (pipe to `kubectl apply -f -`)
    Crd,

    /// Show version information
    Version,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { config, log_level } => {
            // Load configuration first so logging can honor observability.logging
            // (level/format). The CLI --log-level still overrides the config level.
            let config = load_config_paths(&config)?;
            init_tracing(log_level.as_deref(), Some(&config.observability))?;

            tracing::info!("Starting Octopus API Gateway");
            tracing::info!(
                listen = %config.gateway.listen,
                workers = config.gateway.workers,
                "Configuration loaded"
            );

            // Build server
            let server = ServerBuilder::new().config(config).build().await?;

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

            tracing::info!("Validating configuration");

            match load_config_paths(&config) {
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

        Commands::Gen { config } => {
            init_tracing(Some("info"), None)?;

            tracing::info!("Running code generation");
            gen::run_gen(&config).await?;
            Ok(())
        }

        Commands::Crd => {
            let yaml = octopus_k8s::crd::all_crds_yaml()
                .map_err(|e| anyhow::anyhow!("failed to generate CRDs: {e}"))?;
            print!("{yaml}");
            Ok(())
        }

        Commands::Version => {
            println!("Octopus API Gateway");
            println!("Version: {}", env!("CARGO_PKG_VERSION"));
            println!("Rust version: {}", env!("CARGO_PKG_RUST_VERSION"));
            Ok(())
        }
    }
}

/// Load config from one or more paths, supporting directories
fn load_config_paths(paths: &[PathBuf]) -> octopus_core::Result<octopus_config::Config> {
    if paths.is_empty() {
        return load_config("config.yaml", true);
    }

    // Single path
    if paths.len() == 1 {
        let path = &paths[0];

        if path.is_dir() {
            // Directory: glob all config files, sort, merge
            let mut files: Vec<PathBuf> = Vec::new();

            for ext in &["yaml", "yml", "json", "toml"] {
                let glob_pattern = path.join(format!("*.{ext}"));
                if let Ok(entries) = glob::glob(&glob_pattern.to_string_lossy()) {
                    for entry in entries.flatten() {
                        files.push(entry);
                    }
                }
            }

            files.sort();

            if files.is_empty() {
                return Err(octopus_core::Error::Config(format!(
                    "No config files found in directory: {}",
                    path.display()
                )));
            }

            tracing::info!(
                dir = %path.display(),
                files = files.len(),
                "Loading config from directory"
            );

            for f in &files {
                tracing::info!("  Loading: {}", f.display());
            }

            return load_and_merge(files);
        }

        // Single file
        return load_config(path, true);
    }

    // Multiple paths — expand directories, then merge all
    let mut all_files: Vec<PathBuf> = Vec::new();

    for path in paths {
        if path.is_dir() {
            for ext in &["yaml", "yml", "json", "toml"] {
                let glob_pattern = path.join(format!("*.{ext}"));
                if let Ok(entries) = glob::glob(&glob_pattern.to_string_lossy()) {
                    let mut dir_files: Vec<PathBuf> = entries.flatten().collect();
                    dir_files.sort();
                    all_files.extend(dir_files);
                }
            }
        } else {
            all_files.push(path.clone());
        }
    }

    tracing::info!(files = all_files.len(), "Merging config files");
    for f in &all_files {
        tracing::info!("  Loading: {}", f.display());
    }

    load_and_merge(all_files)
}

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogFormat {
    Text,
    Json,
}

/// Resolve the effective log level and output format.
///
/// Level precedence: explicit `--log-level` > `observability.logging.level` >
/// `info`. `RUST_LOG` still layers on top via the env filter. The format comes
/// from `observability.logging.format` (`json` → JSON lines, otherwise text).
fn resolve_logging(
    cli_level: Option<&str>,
    obs: Option<&octopus_config::types::ObservabilityConfig>,
) -> (tracing::Level, LogFormat) {
    let level_str = cli_level
        .or_else(|| obs.map(|o| o.logging.level.as_str()))
        .unwrap_or("info");
    let level = match level_str.to_lowercase().as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    };
    let format = match obs.map(|o| o.logging.format.to_lowercase()).as_deref() {
        Some("json") => LogFormat::Json,
        _ => LogFormat::Text,
    };
    (level, format)
}

fn init_tracing(
    cli_level: Option<&str>,
    obs: Option<&octopus_config::types::ObservabilityConfig>,
) -> Result<()> {
    let (level, format) = resolve_logging(cli_level, obs);

    let filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive(level.into())
        // Suppress mdns-sd VPN errors - harmless library-level logs that occur
        // when attempting multicast on VPN tunnel interfaces (utun*).
        .add_directive("mdns_sd=warn".parse()?);

    let registry = tracing_subscriber::registry().with(filter);
    match format {
        LogFormat::Json => registry
            .with(tracing_subscriber::fmt::layer().with_target(false).json())
            .init(),
        LogFormat::Text => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(false)
                    .with_level(true),
            )
            .init(),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_config::types::{LoggingConfig, ObservabilityConfig};

    fn obs_with(level: &str, format: &str) -> ObservabilityConfig {
        ObservabilityConfig {
            logging: LoggingConfig {
                level: level.to_string(),
                format: format.to_string(),
            },
            ..Default::default()
        }
    }

    #[test]
    fn cli_level_overrides_config() {
        let obs = obs_with("info", "text");
        let (level, _) = resolve_logging(Some("debug"), Some(&obs));
        assert_eq!(level, tracing::Level::DEBUG);
    }

    #[test]
    fn config_level_used_when_no_cli() {
        let obs = obs_with("warn", "text");
        let (level, _) = resolve_logging(None, Some(&obs));
        assert_eq!(level, tracing::Level::WARN);
    }

    #[test]
    fn defaults_to_info_text() {
        let (level, format) = resolve_logging(None, None);
        assert_eq!(level, tracing::Level::INFO);
        assert_eq!(format, LogFormat::Text);
    }

    #[test]
    fn json_format_from_config() {
        let obs = obs_with("info", "json");
        let (_, format) = resolve_logging(None, Some(&obs));
        assert_eq!(format, LogFormat::Json);
    }
}
