//! Compression middleware for Octopus API Gateway
//!
//! Provides automatic response compression with support for:
//! - gzip (widely supported, good compression)
//! - brotli (better compression, modern browsers)
//! - zstd (fastest, best compression ratio)
//!
//! Features:
//! - Content-type aware (only compresses text-based content)
//! - Accept-Encoding negotiation
//! - Configurable compression levels
//! - Minimum size threshold
//! - Automatic Content-Encoding header handling

pub mod compressor;
pub mod config;
pub mod middleware;

pub use compressor::{CompressionAlgorithm, Compressor};
pub use config::CompressionConfig;
pub use middleware::CompressionMiddleware;
