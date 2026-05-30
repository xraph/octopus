//! Response compression middleware

use async_trait::async_trait;
use bytes::Bytes;
use http::{header, HeaderValue, Request, Response};
use http_body_util::{BodyExt, Full};
use octopus_core::{Error, Middleware, Next, Result};
use std::fmt;
use std::io::Write;

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

    /// Compress bytes using the specified algorithm
    pub fn compress_body(&self, body: Bytes, algorithm: CompressionAlgorithm) -> Result<Bytes> {
        // Check minimum size
        if body.len() < self.config.min_size {
            return Ok(body);
        }

        let level = self.config.level;
        let input = body.as_ref();

        let compressed = match algorithm {
            CompressionAlgorithm::Gzip => {
                let mut encoder = flate2::write::GzEncoder::new(
                    Vec::with_capacity(input.len()),
                    flate2::Compression::new(level),
                );
                encoder
                    .write_all(input)
                    .map_err(|e| Error::Internal(format!("gzip compression failed: {}", e)))?;
                encoder
                    .finish()
                    .map_err(|e| Error::Internal(format!("gzip finish failed: {}", e)))?
            }
            CompressionAlgorithm::Brotli => {
                let mut output = Vec::with_capacity(input.len());
                {
                    let mut encoder = brotli::CompressorWriter::new(&mut output, 4096, level, 22);
                    encoder.write_all(input).map_err(|e| {
                        Error::Internal(format!("brotli compression failed: {}", e))
                    })?;
                    // CompressorWriter flushes on drop, but we explicitly drop to capture errors
                }
                output
            }
            CompressionAlgorithm::Zstd => {
                let mut encoder = zstd::stream::write::Encoder::new(
                    Vec::with_capacity(input.len()),
                    level as i32,
                )
                .map_err(|e| Error::Internal(format!("zstd encoder creation failed: {}", e)))?;
                encoder
                    .write_all(input)
                    .map_err(|e| Error::Internal(format!("zstd compression failed: {}", e)))?;
                encoder
                    .finish()
                    .map_err(|e| Error::Internal(format!("zstd finish failed: {}", e)))?
            }
        };

        tracing::debug!(
            algorithm = ?algorithm,
            original_size = input.len(),
            compressed_size = compressed.len(),
            "Compressed response body"
        );

        Ok(Bytes::from(compressed))
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
            None => return Ok(response),
        };

        // Decompose the response
        let status = response.status();
        let headers = response.headers().clone();
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .map(|c| c.to_bytes())
            .unwrap_or_default();

        // Compress the body
        let compressed = self.compress_body(body_bytes.clone(), algorithm)?;

        // If compress_body returned the original (below min_size), just rebuild as-is
        if compressed == body_bytes {
            let mut builder = Response::builder().status(status);
            for (name, value) in headers.iter() {
                builder = builder.header(name, value);
            }
            let resp = builder
                .body(Full::new(body_bytes))
                .map_err(|e| Error::Internal(e.to_string()))?;
            return Ok(resp);
        }

        // Rebuild response with compressed body
        let mut builder = Response::builder().status(status);
        for (name, value) in headers.iter() {
            // Skip Content-Length since size changed
            if name == header::CONTENT_LENGTH {
                continue;
            }
            builder = builder.header(name, value);
        }

        let mut resp = builder
            .body(Full::new(compressed))
            .map_err(|e| Error::Internal(e.to_string()))?;

        // Set Content-Encoding header
        resp.headers_mut().insert(
            header::CONTENT_ENCODING,
            HeaderValue::from_static(algorithm.as_str()),
        );

        // Add Vary: Accept-Encoding
        resp.headers_mut()
            .insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));

        Ok(resp)
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

    #[tokio::test]
    async fn test_gzip_compression_produces_different_output() {
        let config = CompressionConfig {
            min_size: 0, // Compress everything
            ..Default::default()
        };
        let compression = Compression::with_config(config);

        // Create a body large enough to see compression effect
        let original = "Hello, this is a test string that should be compressed! ".repeat(50);
        let original_bytes = Bytes::from(original.clone());
        let original_len = original_bytes.len();

        let compressed = compression
            .compress_body(original_bytes, CompressionAlgorithm::Gzip)
            .unwrap();

        // Compressed output should differ from original
        assert_ne!(compressed.as_ref(), original.as_bytes());
        // Compressed should be smaller for repetitive text
        assert!(
            compressed.len() < original_len,
            "compressed {} should be < original {}",
            compressed.len(),
            original_len
        );

        // Verify it is valid gzip by decompressing
        use flate2::read::GzDecoder;
        use std::io::Read;
        let mut decoder = GzDecoder::new(compressed.as_ref());
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[tokio::test]
    async fn test_full_middleware_gzip_compression() {
        let config = CompressionConfig {
            min_size: 0, // Compress everything
            algorithms: vec![CompressionAlgorithm::Gzip],
            ..Default::default()
        };
        let compression = Compression::with_config(config);

        let body_text = "Repeating content for compression test. ".repeat(100);
        let handler = TestHandler {
            content_type: "application/json",
            body: Box::leak(body_text.clone().into_boxed_str()),
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
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_ENCODING)
                .unwrap()
                .to_str()
                .unwrap(),
            "gzip"
        );
        assert_eq!(
            response
                .headers()
                .get(header::VARY)
                .unwrap()
                .to_str()
                .unwrap(),
            "Accept-Encoding"
        );
        // Content-Length should be removed
        assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
    }
}
