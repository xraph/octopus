//! Core compression functionality

use bytes::Bytes;
use std::io::Write;

/// Supported compression algorithms
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    Gzip,
    Brotli,
    Zstd,
}

impl CompressionAlgorithm {
    /// Get the Content-Encoding header value
    pub fn encoding_name(&self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::Brotli => "br",
            Self::Zstd => "zstd",
        }
    }

    /// Parse from Accept-Encoding header value
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "gzip" => Some(Self::Gzip),
            "br" => Some(Self::Brotli),
            "zstd" => Some(Self::Zstd),
            _ => None,
        }
    }
}

/// Compressor for response bodies
pub struct Compressor;

impl Compressor {
    /// Compress data using the specified algorithm and level
    pub fn compress(
        data: &[u8],
        algorithm: CompressionAlgorithm,
        level: u32,
    ) -> Result<Bytes, std::io::Error> {
        match algorithm {
            CompressionAlgorithm::Gzip => Self::compress_gzip(data, level),
            CompressionAlgorithm::Brotli => Self::compress_brotli(data, level),
            CompressionAlgorithm::Zstd => Self::compress_zstd(data, level),
        }
    }

    /// Compress using gzip
    fn compress_gzip(data: &[u8], level: u32) -> Result<Bytes, std::io::Error> {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let compression_level = Compression::new(level.min(9));
        let mut encoder = GzEncoder::new(Vec::new(), compression_level);
        encoder.write_all(data)?;
        let compressed = encoder.finish()?;
        Ok(Bytes::from(compressed))
    }

    /// Compress using brotli
    fn compress_brotli(data: &[u8], level: u32) -> Result<Bytes, std::io::Error> {
        let mut compressed = Vec::new();
        let quality = level.min(11);
        
        brotli::BrotliCompress(
            &mut std::io::Cursor::new(data),
            &mut compressed,
            &brotli::enc::BrotliEncoderParams {
                quality: quality as i32,
                ..Default::default()
            },
        )?;
        
        Ok(Bytes::from(compressed))
    }

    /// Compress using zstd
    fn compress_zstd(data: &[u8], level: u32) -> Result<Bytes, std::io::Error> {
        let compression_level = level.min(22) as i32;
        let compressed = zstd::encode_all(data, compression_level)?;
        Ok(Bytes::from(compressed))
    }

    /// Negotiate compression algorithm based on Accept-Encoding header
    pub fn negotiate_algorithm(
        accept_encoding: Option<&str>,
        preferred: &[String],
    ) -> Option<CompressionAlgorithm> {
        let accept = accept_encoding?;
        
        // Parse Accept-Encoding header (e.g., "gzip, deflate, br")
        let accepted: Vec<&str> = accept
            .split(',')
            .map(|s| s.trim().split(';').next().unwrap_or(""))
            .collect();

        // Find first preferred algorithm that client accepts
        for pref in preferred {
            if accepted.contains(&pref.as_str()) {
                if let Some(algo) = CompressionAlgorithm::from_str(pref) {
                    return Some(algo);
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithm_encoding_name() {
        assert_eq!(CompressionAlgorithm::Gzip.encoding_name(), "gzip");
        assert_eq!(CompressionAlgorithm::Brotli.encoding_name(), "br");
        assert_eq!(CompressionAlgorithm::Zstd.encoding_name(), "zstd");
    }

    #[test]
    fn test_algorithm_from_str() {
        assert_eq!(
            CompressionAlgorithm::from_str("gzip"),
            Some(CompressionAlgorithm::Gzip)
        );
        assert_eq!(
            CompressionAlgorithm::from_str("br"),
            Some(CompressionAlgorithm::Brotli)
        );
        assert_eq!(
            CompressionAlgorithm::from_str("zstd"),
            Some(CompressionAlgorithm::Zstd)
        );
        assert_eq!(CompressionAlgorithm::from_str("unknown"), None);
    }

    #[test]
    fn test_compress_gzip() {
        // Use larger, highly repetitive data that will definitely compress
        let data = "Hello, World! This is a test string that should compress well. ".repeat(100);
        let compressed = Compressor::compress(data.as_bytes(), CompressionAlgorithm::Gzip, 6).unwrap();
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_compress_brotli() {
        // Use larger, highly repetitive data that will definitely compress
        let data = "Hello, World! This is a test string that should compress well. ".repeat(100);
        let compressed = Compressor::compress(data.as_bytes(), CompressionAlgorithm::Brotli, 6).unwrap();
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_compress_zstd() {
        // Use larger, highly repetitive data that will definitely compress
        let data = "Hello, World! This is a test string that should compress well. ".repeat(100);
        let compressed = Compressor::compress(data.as_bytes(), CompressionAlgorithm::Zstd, 6).unwrap();
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_negotiate_algorithm() {
        let preferred = vec![
            "br".to_string(),
            "zstd".to_string(),
            "gzip".to_string(),
        ];

        // Client accepts brotli
        let algo = Compressor::negotiate_algorithm(Some("gzip, br"), &preferred);
        assert_eq!(algo, Some(CompressionAlgorithm::Brotli));

        // Client only accepts gzip
        let algo = Compressor::negotiate_algorithm(Some("gzip"), &preferred);
        assert_eq!(algo, Some(CompressionAlgorithm::Gzip));

        // Client accepts nothing we support
        let algo = Compressor::negotiate_algorithm(Some("deflate"), &preferred);
        assert_eq!(algo, None);

        // No Accept-Encoding header
        let algo = Compressor::negotiate_algorithm(None, &preferred);
        assert_eq!(algo, None);
    }
}

