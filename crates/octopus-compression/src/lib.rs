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
pub mod middleware;
pub mod config;

pub use compressor::{Compressor, CompressionAlgorithm};
pub use middleware::CompressionMiddleware;
pub use config::CompressionConfig;

