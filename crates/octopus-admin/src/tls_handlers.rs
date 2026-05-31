//! TLS certificate inspection endpoints.
//!
//! Surfaces the gateway's configured TLS certificate (from `gateway.tls`),
//! parsing the on-disk PEM to expose subject, SANs, issuer and expiry so the
//! dashboard can warn about soon-to-expire certificates. Upload writes the PEM
//! pair back to the configured paths; when the gateway's certificate reloader
//! is enabled the new material is picked up without a restart.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use x509_parser::prelude::*;

use crate::handlers::AppState;
use crate::models::{TlsCertInfo, TlsCertUpload};

/// Parsed X.509 fields extracted from a PEM file.
#[derive(Default)]
struct CertMetadata {
    subject_cn: Option<String>,
    sans: Vec<String>,
    issuer: Option<String>,
    not_before: Option<i64>,
    not_after: Option<i64>,
}

/// Parse the first certificate in a PEM file, extracting display metadata.
fn parse_cert_file(path: &str) -> Option<CertMetadata> {
    let data = std::fs::read(path).ok()?;
    for pem in Pem::iter_from_buffer(&data) {
        let Ok(pem) = pem else { continue };
        let Ok(cert) = pem.parse_x509() else { continue };

        let subject_cn = cert
            .subject()
            .iter_common_name()
            .next()
            .and_then(|cn| cn.as_str().ok())
            .map(ToString::to_string);

        let mut sans = Vec::new();
        if let Ok(Some(ext)) = cert.subject_alternative_name() {
            for gn in &ext.value.general_names {
                if let GeneralName::DNSName(dns) = gn {
                    sans.push((*dns).to_string());
                }
            }
        }

        return Some(CertMetadata {
            subject_cn,
            sans,
            issuer: Some(cert.issuer().to_string()),
            not_before: Some(cert.validity().not_before.timestamp()),
            not_after: Some(cert.validity().not_after.timestamp()),
        });
    }
    None
}

/// Format a unix timestamp as RFC3339.
fn fmt_ts(ts: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(ts, 0).map(|dt| dt.to_rfc3339())
}

/// Build the cert info list from gateway config.
fn collect_certs(state: &AppState) -> Vec<TlsCertInfo> {
    let Some(ref config) = state.config else {
        return vec![];
    };
    let Some(ref tls) = config.gateway.tls else {
        return vec![];
    };

    let meta = parse_cert_file(&tls.cert_file).unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    let days_until_expiry = meta.not_after.map(|exp| (exp - now) / 86_400);
    let status = match days_until_expiry {
        Some(d) if d < 0 => "expired",
        Some(d) if d < 30 => "expiring",
        Some(_) => "valid",
        None => "unknown",
    }
    .to_string();

    let name = meta
        .subject_cn
        .clone()
        .or_else(|| meta.sans.first().cloned())
        .unwrap_or_else(|| {
            std::path::Path::new(&tls.cert_file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("server")
                .to_string()
        });

    vec![TlsCertInfo {
        name,
        cert_file: Some(tls.cert_file.clone()),
        key_file: Some(tls.key_file.clone()),
        sni_hosts: meta.sans.clone(),
        subject_cn: meta.subject_cn,
        sans: meta.sans,
        issuer: meta.issuer,
        not_before: meta.not_before.and_then(fmt_ts),
        not_after: meta.not_after.and_then(fmt_ts),
        days_until_expiry,
        status,
        min_tls_version: Some(tls.min_tls_version.clone()),
        require_client_cert: tls.require_client_cert,
        source: "config".to_string(),
    }]
}

/// List TLS certificates.
/// `GET /admin/api/tls/certs`
pub async fn api_tls_certs_list_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(collect_certs(&state))
}

/// Get a single certificate by logical name.
/// `GET /admin/api/tls/certs/:name`
pub async fn api_tls_cert_detail_handler(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let certs = collect_certs(&state);
    certs.into_iter().find(|c| c.name == name).map_or_else(
        || {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Certificate not found", "name": name })),
            )
        },
        |cert| {
            (
                StatusCode::OK,
                Json(serde_json::to_value(cert).unwrap_or_default()),
            )
        },
    )
}

/// Trigger a certificate reload.
/// `POST /admin/api/tls/reload`
///
/// File-backed certificates are hot-reloaded by the gateway's own certificate
/// watcher when `enable_cert_reload` is set; this endpoint reports that state
/// rather than swapping certs directly (the admin process does not own the
/// listener's TLS acceptor).
pub async fn api_tls_reload_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let reload_enabled = state
        .config
        .as_ref()
        .and_then(|c| c.gateway.tls.as_ref())
        .map_or(false, |t| t.enable_cert_reload);

    Json(serde_json::json!({
        "success": true,
        "auto_reload_enabled": reload_enabled,
        "message": if reload_enabled {
            "Certificate files are watched and hot-reloaded automatically."
        } else {
            "Automatic reload is disabled; restart the gateway to apply new certificates."
        }
    }))
}

/// Upload a PEM certificate/key pair, writing it to the configured paths.
/// `POST /admin/api/tls/certs`
pub async fn api_tls_cert_upload_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TlsCertUpload>,
) -> impl IntoResponse {
    let Some(tls) = state.config.as_ref().and_then(|c| c.gateway.tls.clone()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "No file-backed TLS certificate is configured; cannot persist upload"
            })),
        );
    };

    // Validate that the uploaded certificate actually parses before writing.
    let valid = Pem::iter_from_buffer(req.cert_pem.as_bytes())
        .next()
        .and_then(Result::ok)
        .map_or(false, |pem| pem.parse_x509().is_ok());
    if !valid {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Uploaded certificate is not valid PEM/X.509" })),
        );
    }

    if let Err(e) = std::fs::write(&tls.cert_file, req.cert_pem.as_bytes()) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to write certificate: {e}") })),
        );
    }
    if let Err(e) = std::fs::write(&tls.key_file, req.key_pem.as_bytes()) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("Failed to write key: {e}") })),
        );
    }

    tracing::info!(
        "Uploaded TLS certificate '{}' to {}",
        req.name,
        tls.cert_file
    );
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "message": "Certificate written; reload to apply if auto-reload is disabled."
        })),
    )
}
