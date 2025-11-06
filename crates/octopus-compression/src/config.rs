//! Configuration for compression middleware

use serde::{Deserialize, Serialize};

/// Compression configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    /// Enable compression
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Compression level (1-9 for gzip/zstd, 1-11 for brotli)
    #[serde(default = "default_level")]
    pub level: u32,

    /// Minimum response size to compress (in bytes)
    #[serde(default = "default_min_size")]
    pub min_size: usize,

    /// Preferred compression algorithms in order
    #[serde(default = "default_algorithms")]
    pub algorithms: Vec<String>,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            level: 6,
            min_size: 1024, // 1KB
            algorithms: vec![
                "br".to_string(),    // brotli (best compression)
                "zstd".to_string(),  // zstd (fast)
                "gzip".to_string(),  // gzip (universal)
            ],
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_level() -> u32 {
    6
}

fn default_min_size() -> usize {
    1024
}

fn default_algorithms() -> Vec<String> {
    vec![
        "br".to_string(),
        "zstd".to_string(),
        "gzip".to_string(),
    ]
}

impl CompressionConfig {
    /// Check if compression should be applied based on content size
    pub fn should_compress(&self, size: usize) -> bool {
        self.enabled && size >= self.min_size
    }

    /// Check if a content type should be compressed
    pub fn is_compressible_content_type(&self, content_type: &str) -> bool {
        let ct = content_type.to_lowercase();
        
        // Text-based content types that benefit from compression
        ct.starts_with("text/")
            || ct.contains("json")
            || ct.contains("xml")
            || ct.contains("javascript")
            || ct.contains("ecmascript")
            || ct.contains("wasm")
            || ct == "application/graphql"
            || ct == "application/x-yaml"
            || ct == "application/yaml"
            || ct == "image/svg+xml"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CompressionConfig::default();
        assert!(config.enabled);
        assert_eq!(config.level, 6);
        assert_eq!(config.min_size, 1024);
        assert_eq!(config.algorithms.len(), 3);
    }

    #[test]
    fn test_should_compress() {
        let config = CompressionConfig::default();
        assert!(!config.should_compress(512)); // Too small
        assert!(config.should_compress(2048)); // Large enough
    }

    #[test]
    fn test_compressible_content_types() {
        let config = CompressionConfig::default();
        
        // Should compress
        assert!(config.is_compressible_content_type("text/html"));
        assert!(config.is_compressible_content_type("application/json"));
        assert!(config.is_compressible_content_type("text/plain"));
        assert!(config.is_compressible_content_type("application/javascript"));
        assert!(config.is_compressible_content_type("image/svg+xml"));
        
        // Should not compress
        assert!(!config.is_compressible_content_type("image/png"));
        assert!(!config.is_compressible_content_type("video/mp4"));
        assert!(!config.is_compressible_content_type("application/octet-stream"));
    }
}

