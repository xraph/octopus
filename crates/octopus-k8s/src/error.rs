//! Error types for the Kubernetes operator.

use thiserror::Error;

/// Errors produced by the Octopus Kubernetes operator.
#[derive(Debug, Error)]
pub enum K8sError {
    /// A watched resource could not be translated into routing intent.
    #[error("translation error: {0}")]
    Translate(String),

    /// Applying the desired routing table to the live router failed.
    #[error("apply error: {0}")]
    Apply(String),

    /// Serializing a CRD definition failed.
    #[error("serialization error: {0}")]
    Serialize(String),

    /// An error from the Kubernetes client.
    #[error("kube error: {0}")]
    Kube(#[from] kube::Error),
}

/// Convenience result alias for the operator.
pub type Result<T> = std::result::Result<T, K8sError>;
