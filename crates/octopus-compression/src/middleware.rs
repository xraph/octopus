//! Compression middleware implementation

use crate::compressor::{CompressionAlgorithm, Compressor};
use crate::config::CompressionConfig;
use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderValue, Request, Response, StatusCode};
use http_body_util::BodyExt;
use octopus_core::middleware::{Body, Middleware, Next};
use octopus_core::Result;
use std::sync::Arc;
use tracing::{debug, warn};

/// Compression middleware
#[derive(Debug)]
pub struct CompressionMiddleware {
    config: Arc<CompressionConfig>,
}

impl CompressionMiddleware {
    /// Create a new compression middleware
    pub fn new(config: CompressionConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

#[async_trait]
impl Middleware for CompressionMiddleware {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        if !self.config.enabled {
            return next.run(req).await;
        }

        // Get Accept-Encoding header
        let accept_encoding = req
            .headers()
            .get(http::header::ACCEPT_ENCODING)
            .and_then(|v| v.to_str().ok());

        // Negotiate compression algorithm
        let algorithm = Compressor::negotiate_algorithm(accept_encoding, &self.config.algorithms);

        // If no algorithm is negotiated, pass through
        let Some(algo) = algorithm else {
            return next.run(req).await;
        };

        // Process the request
        let response = next.run(req).await?;

        // Check if response should be compressed
        if !should_compress_response(&response, &self.config) {
            return Ok(response);
        }

        // Compress the response
        match compress_response(response, algo, self.config.level).await {
            Ok(compressed) => {
                debug!(
                    algorithm = algo.encoding_name(),
                    "Response compressed successfully"
                );
                Ok(compressed)
            }
            Err(e) => {
                warn!(error = %e, "Failed to compress response, returning uncompressed");
                // On compression error, we should return the original uncompressed response
                // But we've already consumed it, so return an error response
                let mut response = Response::new(Body::from("Compression error"));
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                Ok(response)
            }
        }
    }
}

/// Check if response should be compressed
fn should_compress_response(response: &Response<Body>, config: &CompressionConfig) -> bool {
    // Don't compress if already encoded
    if response
        .headers()
        .contains_key(http::header::CONTENT_ENCODING)
    {
        return false;
    }

    // Check content type
    if let Some(content_type) = response.headers().get(http::header::CONTENT_TYPE) {
        if let Ok(ct) = content_type.to_str() {
            if !config.is_compressible_content_type(ct) {
                return false;
            }
        }
    }

    // Check status code (only compress successful responses)
    if !response.status().is_success() {
        return false;
    }

    true
}

/// Compress response body
async fn compress_response(
    response: Response<Body>,
    algorithm: CompressionAlgorithm,
    level: u32,
) -> Result<Response<Body>> {
    let (mut parts, body) = response.into_parts();

    // Collect the body
    let body_bytes = body
        .collect()
        .await
        .map_err(|e| octopus_core::Error::Internal(format!("Failed to read body: {}", e)))?
        .to_bytes();

    // Check if body is large enough to compress
    let original_size = body_bytes.len();

    // Compress the body
    let compressed = Compressor::compress(&body_bytes, algorithm, level)
        .map_err(|e| octopus_core::Error::Internal(format!("Compression failed: {}", e)))?;

    let compressed_size = compressed.len();

    // Only use compressed version if it's actually smaller
    let (final_body, encoding) = if compressed_size < original_size {
        (Bytes::from(compressed), Some(algorithm.encoding_name()))
    } else {
        debug!("Compressed size not smaller, using original");
        (body_bytes, None)
    };

    // Update headers
    if let Some(encoding) = encoding {
        parts.headers.insert(
            http::header::CONTENT_ENCODING,
            HeaderValue::from_static(encoding),
        );
    }

    parts.headers.insert(
        http::header::CONTENT_LENGTH,
        HeaderValue::from_str(&final_body.len().to_string())
            .map_err(|e| octopus_core::Error::Internal(format!("Invalid content length: {}", e)))?,
    );

    // Remove transfer-encoding if present (we're setting content-length)
    parts.headers.remove(http::header::TRANSFER_ENCODING);

    let response = Response::from_parts(parts, Body::from(final_body));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::{ACCEPT_ENCODING, CONTENT_TYPE};

    #[tokio::test]
    async fn test_middleware_disabled() {
        let mut config = CompressionConfig::default();
        config.enabled = false;
        let middleware = CompressionMiddleware::new(config);

        let req = Request::builder()
            .header(ACCEPT_ENCODING, "gzip")
            .body(Body::from(Bytes::new()))
            .unwrap();

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([]);
        let next = Next::with_handler(
            stack,
            Box::new(|_req| Box::pin(async { Ok(Response::new(Body::from("test"))) })),
        );

        let result = middleware.call(req, next).await.unwrap();
        assert!(!result
            .headers()
            .contains_key(http::header::CONTENT_ENCODING));
    }

    #[tokio::test]
    async fn test_no_accept_encoding() {
        let config = CompressionConfig::default();
        let middleware = CompressionMiddleware::new(config);

        let req = Request::builder().body(Body::from(Bytes::new())).unwrap();

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([]);
        let next = Next::with_handler(
            stack,
            Box::new(|_req| Box::pin(async { Ok(Response::new(Body::from("test"))) })),
        );

        let result = middleware.call(req, next).await.unwrap();
        assert!(!result
            .headers()
            .contains_key(http::header::CONTENT_ENCODING));
    }

    #[tokio::test]
    async fn test_should_compress_response() {
        let config = CompressionConfig::default();

        // Should compress JSON
        let response = Response::builder()
            .header(CONTENT_TYPE, "application/json")
            .status(200)
            .body(Body::from(Bytes::new()))
            .unwrap();
        assert!(should_compress_response(&response, &config));

        // Should not compress image
        let response = Response::builder()
            .header(CONTENT_TYPE, "image/png")
            .status(200)
            .body(Body::from(Bytes::new()))
            .unwrap();
        assert!(!should_compress_response(&response, &config));

        // Should not compress error responses
        let response = Response::builder()
            .header(CONTENT_TYPE, "application/json")
            .status(500)
            .body(Body::from(Bytes::new()))
            .unwrap();
        assert!(!should_compress_response(&response, &config));
    }
}
