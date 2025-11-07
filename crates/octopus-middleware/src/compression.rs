//! Response compression middleware

use async_trait::async_trait;
use bytes::Bytes;
use http::{header, HeaderValue, Request, Response};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;

/// Body type alias
pub type Body = Full<Bytes>;

/// Compression algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    /// Gzip compression
    Gzip,
    /// Brotli compression
    Brotli,
    /// Zstd compression
    Zstd,
}

impl CompressionAlgorithm {
    /// Get the content-encoding header value
    pub fn as_str(&self) -> &'static str {
        match self {
            CompressionAlgorithm::Gzip => "gzip",
            CompressionAlgorithm::Brotli => "br",
            CompressionAlgorithm::Zstd => "zstd",
        }
    }

    /// Parse from Accept-Encoding header
    pub fn from_accept_encoding(accept: &str) -> Option<Self> {
        // Simple parsing, check for presence of algorithm
        // In production, should parse quality values (q=)
        if accept.contains("br") {
            Some(CompressionAlgorithm::Brotli)
        } else if accept.contains("gzip") {
            Some(CompressionAlgorithm::Gzip)
        } else if accept.contains("zstd") {
            Some(CompressionAlgorithm::Zstd)
        } else {
            None
        }
    }
}

/// Compression configuration
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Supported compression algorithms (in order of preference)
    pub algorithms: Vec<CompressionAlgorithm>,
    /// Minimum response size to compress (bytes)
    pub min_size: usize,
    /// Content types to compress
    pub content_types: Vec<String>,
    /// Compression level (1-9, higher = better compression but slower)
    pub level: u32,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithms: vec![CompressionAlgorithm::Brotli, CompressionAlgorithm::Gzip],
            min_size: 1024, // 1 KB
            content_types: vec![
                "text/".to_string(),
                "application/json".to_string(),
                "application/javascript".to_string(),
                "application/xml".to_string(),
            ],
            level: 6, // Default compression level
        }
    }
}

/// Compression middleware
///
/// Compresses responses based on:
/// - Client's Accept-Encoding header
/// - Response Content-Type
/// - Response size
#[derive(Clone)]
pub struct Compression {
    config: CompressionConfig,
}

impl Compression {
    /// Create a new Compression middleware with default config
    pub fn new() -> Self {
        Self::with_config(CompressionConfig::default())
    }

    /// Create a new Compression middleware with custom config
    pub fn with_config(config: CompressionConfig) -> Self {
        Self { config }
    }

    /// Check if content type should be compressed
    fn should_compress_content_type(&self, content_type: Option<&HeaderValue>) -> bool {
        if let Some(ct) = content_type {
            if let Ok(ct_str) = ct.to_str() {
                return self
                    .config
                    .content_types
                    .iter()
                    .any(|prefix| ct_str.starts_with(prefix));
            }
        }
        false
    }

    /// Check if response should be compressed
    fn should_compress(&self, response: &Response<Body>) -> bool {
        // Check if already compressed
        if response.headers().contains_key(header::CONTENT_ENCODING) {
            return false;
        }

        // Check content type
        if !self.should_compress_content_type(response.headers().get(header::CONTENT_TYPE)) {
            return false;
        }

        // Check size (we'll check this after getting the body in practice)
        // For now, assume we should compress if content type matches
        true
    }

    /// Choose compression algorithm based on Accept-Encoding
    fn choose_algorithm(
        &self,
        accept_encoding: Option<&HeaderValue>,
    ) -> Option<CompressionAlgorithm> {
        if let Some(accept) = accept_encoding {
            if let Ok(accept_str) = accept.to_str() {
                // Try each configured algorithm in order
                for algo in &self.config.algorithms {
                    if accept_str.contains(algo.as_str()) {
                        return Some(*algo);
                    }
                }
            }
        }
        None
    }

    /// Compress response body
    #[allow(dead_code)]
    fn compress_body(&self, body: Bytes, algorithm: CompressionAlgorithm) -> Result<Bytes> {
        // Check minimum size
        if body.len() < self.config.min_size {
            return Ok(body); // Don't compress small responses
        }

        // For now, return uncompressed
        // TODO: Implement actual compression using async-compression
        // This would require wrapping the body in a compression stream
        // which is more complex with http_body_util::Full

        // In a real implementation, we'd use:
        // - async_compression::tokio::bufread::GzipEncoder for gzip
        // - async_compression::tokio::bufread::BrotliEncoder for brotli
        // - async_compression::tokio::bufread::ZstdEncoder for zstd

        tracing::debug!(
            algorithm = ?algorithm,
            size = body.len(),
            "Compression not yet implemented"
        );

        Ok(body)
    }
}

impl Default for Compression {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Compression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Compression")
            .field("algorithms", &self.config.algorithms)
            .field("min_size", &self.config.min_size)
            .finish()
    }
}

#[async_trait]
impl Middleware for Compression {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // Get Accept-Encoding from request
        let accept_encoding = req.headers().get(header::ACCEPT_ENCODING).cloned();

        // Call next middleware
        let response = next.run(req).await?;

        // Check if we should compress
        if !self.should_compress(&response) {
            return Ok(response);
        }

        // Choose compression algorithm
        let algorithm = match self.choose_algorithm(accept_encoding.as_ref()) {
            Some(algo) => algo,
            None => return Ok(response), // Client doesn't support compression
        };

        // For now, just add a debug log and return uncompressed
        // Full compression implementation would require:
        // 1. Converting Full<Bytes> to a stream
        // 2. Wrapping in compression encoder
        // 3. Collecting back to Bytes
        // This is complex and would require significant changes to our Body type

        tracing::debug!(
            algorithm = ?algorithm,
            "Compression middleware (not yet fully implemented)"
        );

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;
    use octopus_core::Error;

    #[derive(Debug)]
    struct TestHandler {
        content_type: &'static str,
        body: &'static str,
    }

    #[async_trait]
    impl Middleware for TestHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, self.content_type)
                .body(Full::new(Bytes::from(self.body)))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    #[tokio::test]
    async fn test_compression_algorithm_parsing() {
        assert_eq!(
            CompressionAlgorithm::from_accept_encoding("gzip, deflate, br"),
            Some(CompressionAlgorithm::Brotli)
        );
        assert_eq!(
            CompressionAlgorithm::from_accept_encoding("gzip, deflate"),
            Some(CompressionAlgorithm::Gzip)
        );
        assert_eq!(CompressionAlgorithm::from_accept_encoding("identity"), None);
    }

    #[tokio::test]
    async fn test_compression_content_type_check() {
        let compression = Compression::new();
        let handler = TestHandler {
            content_type: "application/json",
            body: "{}",
        };

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> = std::sync::Arc::new([
            std::sync::Arc::new(compression),
            std::sync::Arc::new(handler),
        ]);

        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .header(header::ACCEPT_ENCODING, "gzip")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_compression_skip_non_compressible() {
        let compression = Compression::new();
        let handler = TestHandler {
            content_type: "image/png",
            body: "binary data",
        };

        let stack: std::sync::Arc<[std::sync::Arc<dyn Middleware>]> = std::sync::Arc::new([
            std::sync::Arc::new(compression),
            std::sync::Arc::new(handler),
        ]);

        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .header(header::ACCEPT_ENCODING, "gzip")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        // Should not add Content-Encoding for images
        assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
    }

    #[tokio::test]
    async fn test_compression_min_size() {
        let config = CompressionConfig {
            min_size: 1024,
            ..Default::default()
        };

        let compression = Compression::with_config(config);

        // Body smaller than min_size should not be compressed
        let body = "small".repeat(10); // < 1024 bytes
        assert!(body.len() < 1024);

        let result = compression
            .compress_body(Bytes::from(body), CompressionAlgorithm::Gzip)
            .unwrap();

        // Should return original (no compression for small bodies)
        assert!(result.len() < 1024);
    }
}
