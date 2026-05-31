//! # Octopus Kubernetes Operator
//!
//! Kubernetes-native configuration for the Octopus API Gateway. Octopus is
//! configured from BOTH the standard Kubernetes Gateway API and custom Octopus
//! CRDs (the Envoy-Gateway pattern: standard resources + policy attachments for
//! Octopus-specific features).
//!
//! This crate provides:
//! - [`crds`] — custom resource definitions (`OctopusGateway`, `OctopusRoute`,
//!   `OctopusUpstream`, `OctopusPolicy`).
//! - [`ir`] — a source-agnostic intermediate routing representation and the
//!   [`ir::RouteStore`] that merges every source into one [`ir::RoutingTable`]
//!   using a deterministic precedence model.
//! - [`apply`] — applies a [`ir::RoutingTable`] to the live router.
//! - [`crd`] — emits CRD YAML for installation.

#![forbid(unsafe_code)]
#![warn(missing_docs, rust_2018_idioms)]

pub mod apply;
pub mod controller;
pub mod crd;
pub mod crds;
pub mod endpoints;
pub mod error;
pub mod gateway_api;
pub mod ir;
pub mod policy;
pub mod refgrant;
pub mod tls;
pub mod translate;

pub use endpoints::EndpointWatchManager;
pub use error::{K8sError, Result};
