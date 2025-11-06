//! Example service that registers itself via mDNS
//!
//! This example demonstrates how to create a service that advertises
//! itself using mDNS/Bonjour so it can be automatically discovered by
//! the Octopus gateway.
//!
//! Run with:
//! ```bash
//! cargo run --example mdns_service -- --name my-service --port 8080
//! ```

use anyhow::Result;
use axum::{
    extract::Path,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use clap::Parser;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::signal;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(author, version, about = "mDNS-enabled example service")]
struct Args {
    /// Service name
    #[arg(short, long, default_value = "example-service")]
    name: String,

    /// HTTP port
    #[arg(short, long, default_value = "8080")]
    port: u16,

    /// Service type
    #[arg(short, long, default_value = "_octopus._tcp.local.")]
    service_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct User {
    id: u32,
    name: String,
    email: String,
}

#[derive(Debug, Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: T,
}

/// OpenAPI 3.0 schema for this service
fn openapi_schema(service_name: &str) -> serde_json::Value {
    serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": format!("{} API", service_name),
            "version": "1.0.0",
            "description": "Example service with mDNS discovery"
        },
        "servers": [
            {
                "url": "http://localhost:8080",
                "description": "Local development server"
            }
        ],
        "paths": {
            "/health": {
                "get": {
                    "summary": "Health check",
                    "responses": {
                        "200": {
                            "description": "Service is healthy",
                            "content": {
                                "text/plain": {
                                    "schema": {
                                        "type": "string"
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/users": {
                "get": {
                    "summary": "List all users",
                    "tags": ["users"],
                    "responses": {
                        "200": {
                            "description": "List of users",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "success": { "type": "boolean" },
                                            "data": {
                                                "type": "array",
                                                "items": {
                                                    "$ref": "#/components/schemas/User"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/users/{id}": {
                "get": {
                    "summary": "Get user by ID",
                    "tags": ["users"],
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {
                                "type": "integer"
                            }
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "User details",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/User"
                                    }
                                }
                            }
                        },
                        "404": {
                            "description": "User not found"
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "User": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "integer",
                            "format": "int32"
                        },
                        "name": {
                            "type": "string"
                        },
                        "email": {
                            "type": "string",
                            "format": "email"
                        }
                    }
                }
            }
        }
    })
}

/// Health check handler
async fn health() -> impl IntoResponse {
    "OK"
}

/// OpenAPI schema handler
async fn openapi(axum::extract::State(service_name): axum::extract::State<String>) -> Json<serde_json::Value> {
    Json(openapi_schema(&service_name))
}

/// List all users
async fn list_users() -> Json<ApiResponse<Vec<User>>> {
    let users = vec![
        User {
            id: 1,
            name: "Alice Johnson".to_string(),
            email: "alice@example.com".to_string(),
        },
        User {
            id: 2,
            name: "Bob Smith".to_string(),
            email: "bob@example.com".to_string(),
        },
        User {
            id: 3,
            name: "Charlie Brown".to_string(),
            email: "charlie@example.com".to_string(),
        },
    ];

    Json(ApiResponse {
        success: true,
        data: users,
    })
}

/// Get user by ID
async fn get_user(Path(id): Path<u32>) -> Json<ApiResponse<Option<User>>> {
    let users = vec![
        User {
            id: 1,
            name: "Alice Johnson".to_string(),
            email: "alice@example.com".to_string(),
        },
        User {
            id: 2,
            name: "Bob Smith".to_string(),
            email: "bob@example.com".to_string(),
        },
        User {
            id: 3,
            name: "Charlie Brown".to_string(),
            email: "charlie@example.com".to_string(),
        },
    ];

    let user = users.into_iter().find(|u| u.id == id);

    Json(ApiResponse {
        success: user.is_some(),
        data: user,
    })
}

/// Register service with mDNS
fn register_mdns(args: &Args) -> Result<ServiceDaemon> {
    info!(
        service_name = %args.name,
        port = args.port,
        service_type = %args.service_type,
        "Registering service with mDNS"
    );

    // Create mDNS daemon
    let mdns = ServiceDaemon::new()?;

    // Get local IP address
    let local_ip = local_ip_address::local_ip()?;
    info!(local_ip = %local_ip, "Using local IP address");

    // TXT records with service metadata (as tuples for mdns-sd)
    // Following FARP v1.0.0 standard metadata keys
    let txt_properties = [
        ("version", "1.0.0"),
        ("tags", "api,example"),
        ("datacenter", "local"),
        // FARP v1.0.0 Standard Metadata Keys
        ("farp.enabled", "true"),
        ("farp.manifest", &format!("http://{}:{}/_farp/manifest", local_ip, args.port)),
        ("farp.openapi", &format!("http://{}:{}/api/openapi.json", local_ip, args.port)),
        ("farp.openapi.path", "/api/openapi.json"),
        ("farp.capabilities", "[rest]"),
        ("farp.strategy", "hybrid"),
        // Legacy keys for backward compatibility
        ("openapi", "/api/openapi.json"),
        ("health", "/health"),
    ];

    // Create service info
    let service_info = ServiceInfo::new(
        &args.service_type,
        &args.name,
        &format!("{}.local.", hostname::get()?.to_string_lossy()),
        local_ip,
        args.port,
        &txt_properties[..],
    )?;

    // Register the service
    mdns.register(service_info)?;

    info!("Service successfully registered with mDNS");

    Ok(mdns)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    let args = Args::parse();

    info!(
        service_name = %args.name,
        port = args.port,
        "Starting mDNS example service"
    );

    // Register with mDNS
    let mdns = register_mdns(&args)?;

    // Build HTTP router
    let app = Router::new()
        .route("/health", get(health))
        .route("/api/openapi.json", get(openapi))
        .route("/users", get(list_users))
        .route("/users/:id", get(get_user))
        .with_state(args.name.clone());

    // Start HTTP server
    let addr = format!("0.0.0.0:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!(address = %addr, "HTTP server listening");

    // Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(mdns))
        .await?;

    Ok(())
}

/// Wait for shutdown signal and cleanup mDNS
async fn shutdown_signal(mdns: ServiceDaemon) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }

    info!("Shutting down gracefully...");

    // Unregister from mDNS
    if let Err(e) = mdns.shutdown() {
        warn!(error = %e, "Failed to shutdown mDNS cleanly");
    } else {
        info!("mDNS service unregistered");
    }

    // Give time for cleanup
    tokio::time::sleep(Duration::from_millis(100)).await;
}

