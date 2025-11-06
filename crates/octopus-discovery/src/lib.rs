//! Service Discovery for Octopus Gateway
//!
//! This crate provides service discovery integrations for dynamic service registration
//! and health monitoring.

#![forbid(unsafe_code)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

pub mod consul;
#[cfg(feature = "dns")]
pub mod dns;
#[cfg(feature = "kubernetes")]
pub mod kubernetes;
#[cfg(feature = "mdns")]
pub mod mdns;
pub mod provider;

pub use provider::{
    DiscoveryEvent, DiscoveryProvider, ServiceEndpoint, ServiceHealth, ServiceInstance,
    ServiceMetadata,
};

#[cfg(feature = "consul")]
pub use consul::ConsulDiscovery;

#[cfg(feature = "dns")]
pub use dns::DnsDiscovery;

#[cfg(feature = "kubernetes")]
pub use kubernetes::K8sDiscovery;

#[cfg(feature = "mdns")]
pub use mdns::MdnsDiscovery;

/// Re-export commonly used types
pub mod prelude {
    pub use crate::provider::{
        DiscoveryEvent, DiscoveryProvider, ServiceEndpoint, ServiceHealth, ServiceInstance,
        ServiceMetadata,
    };

    #[cfg(feature = "consul")]
    pub use crate::consul::ConsulDiscovery;

    #[cfg(feature = "dns")]
    pub use crate::dns::DnsDiscovery;

    #[cfg(feature = "kubernetes")]
    pub use crate::kubernetes::K8sDiscovery;

    #[cfg(feature = "mdns")]
    pub use crate::mdns::MdnsDiscovery;
}
