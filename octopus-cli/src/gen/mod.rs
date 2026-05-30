//! Code generation module for Octopus API Gateway
//!
//! Generates:
//! - Octopus config fragments (routes, upstreams) from OpenAPI/AsyncAPI/FARP specs
//! - Octopus Schema (`.octopus.json`) — intermediate representation for client codegen
//! - TypeScript client SDK with namespace-chained API and TanStack Query hooks

pub mod config_gen;
pub mod schema_gen;
pub mod client_gen;
pub mod scope;
pub mod types;

use crate::gen::types::{GenConfig, GenOutput};
use anyhow::{Context, Result};
use std::path::Path;
use tracing::{info, warn};

/// Run the full generation pipeline
pub async fn run_gen(gen_config_path: &Path) -> Result<()> {
    info!("Loading gen config from {}", gen_config_path.display());

    let content = std::fs::read_to_string(gen_config_path)
        .with_context(|| format!("Failed to read gen config: {}", gen_config_path.display()))?;

    let gen_config: GenConfig = serde_yaml::from_str(&content)
        .with_context(|| "Failed to parse octopus-gen.yaml")?;

    // Create output directory
    std::fs::create_dir_all(&gen_config.output_dir)
        .with_context(|| format!("Failed to create output dir: {}", gen_config.output_dir))?;

    info!(
        services = gen_config.services.len(),
        output_dir = %gen_config.output_dir,
        "Starting code generation"
    );

    // Phase 1: Fetch all specs and build GenOutput for each service
    let mut outputs: Vec<GenOutput> = Vec::new();

    for service in &gen_config.services {
        info!(service = %service.name, "Processing service");

        match config_gen::process_service(service).await {
            Ok(output) => outputs.push(output),
            Err(e) => {
                warn!(service = %service.name, error = %e, "Failed to process service, skipping");
            }
        }
    }

    // Phase 2: Generate octopus config fragments (routes + upstreams YAML)
    if let Some(ref config_opts) = gen_config.config {
        info!("Generating octopus config fragments");
        config_gen::write_config_fragments(&gen_config, config_opts, &outputs)?;
    }

    // Phase 3: Generate Octopus Schema (.octopus.json)
    if let Some(ref schema_opts) = gen_config.schema {
        info!("Generating Octopus Schema");
        schema_gen::generate_schema(&gen_config, schema_opts, &outputs)?;
    }

    // Phase 4: Generate TypeScript client
    if let Some(ref client_opts) = gen_config.client {
        if client_opts.enabled {
            info!("Generating TypeScript client SDK");
            // Load the schema we just generated (or generate in-memory)
            let schema = schema_gen::build_schema(&gen_config, &outputs)?;
            client_gen::generate_client(client_opts, &schema, &gen_config)?;
        }
    }

    info!("Code generation complete");
    Ok(())
}
