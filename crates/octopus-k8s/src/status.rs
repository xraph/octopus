//! Status subresource types and condition building for Octopus CRDs.
//!
//! Octopus reports reconcile outcomes back onto each custom resource's `.status`
//! using a Kubernetes-style [`Condition`] list (mirroring `metav1.Condition`), so
//! `kubectl get -o yaml` and tools like `kubectl wait --for=condition=Accepted`
//! work against Octopus resources just as they do for core/Gateway-API objects.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The condition type Octopus sets to report whether a resource was admitted.
pub const CONDITION_ACCEPTED: &str = "Accepted";
/// Reason used when a resource is accepted.
pub const REASON_ACCEPTED: &str = "Accepted";
/// Reason used when a resource is rejected as invalid.
pub const REASON_INVALID: &str = "Invalid";

/// A single status condition, wire-compatible with `metav1.Condition`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// Condition type, e.g. `Accepted`.
    #[serde(rename = "type")]
    pub type_: String,
    /// `True`, `False`, or `Unknown`.
    pub status: String,
    /// Machine-readable, PascalCase reason for the condition's last transition.
    pub reason: String,
    /// Human-readable detail.
    pub message: String,
    /// `.metadata.generation` the condition was computed from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    /// RFC3339 timestamp of the last status transition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transition_time: Option<String>,
}

/// The `.status` subresource shared by every Octopus CRD.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OctopusStatus {
    /// Reconcile conditions (currently just `Accepted`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
    /// The `.metadata.generation` most recently reconciled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
}

/// The outcome of reconciling a resource, used to build its status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReconcileOutcome {
    /// The resource was valid and admitted.
    Accepted,
    /// The resource was rejected; the string is a human-readable reason.
    Rejected(String),
}

/// Build an [`OctopusStatus`] reporting `outcome` for an object at `generation`.
///
/// `now` is the RFC3339 timestamp to stamp on the condition (injected so the
/// function stays pure and testable).
pub fn build_status(
    generation: Option<i64>,
    outcome: &ReconcileOutcome,
    now: &str,
) -> OctopusStatus {
    let (status, reason, message) = match outcome {
        ReconcileOutcome::Accepted => (
            "True".to_string(),
            REASON_ACCEPTED.to_string(),
            "Resource accepted".to_string(),
        ),
        ReconcileOutcome::Rejected(msg) => {
            ("False".to_string(), REASON_INVALID.to_string(), msg.clone())
        }
    };
    OctopusStatus {
        conditions: vec![Condition {
            type_: CONDITION_ACCEPTED.to_string(),
            status,
            reason,
            message,
            observed_generation: generation,
            last_transition_time: Some(now.to_string()),
        }],
        observed_generation: generation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_outcome_builds_true_condition() {
        let status = build_status(Some(3), &ReconcileOutcome::Accepted, "2026-01-01T00:00:00Z");
        assert_eq!(status.observed_generation, Some(3));
        assert_eq!(status.conditions.len(), 1);
        let c = &status.conditions[0];
        assert_eq!(c.type_, CONDITION_ACCEPTED);
        assert_eq!(c.status, "True");
        assert_eq!(c.reason, REASON_ACCEPTED);
        assert_eq!(c.observed_generation, Some(3));
        assert_eq!(
            c.last_transition_time.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
    }

    #[test]
    fn rejected_outcome_builds_false_condition_with_message() {
        let status = build_status(
            Some(7),
            &ReconcileOutcome::Rejected("upstream 'x' not found".into()),
            "2026-01-01T00:00:00Z",
        );
        assert_eq!(status.conditions.len(), 1);
        let c = &status.conditions[0];
        assert_eq!(c.type_, CONDITION_ACCEPTED);
        assert_eq!(c.status, "False");
        assert_eq!(c.reason, REASON_INVALID);
        assert_eq!(c.message, "upstream 'x' not found");
        assert_eq!(c.observed_generation, Some(7));
    }

    #[test]
    fn condition_serializes_with_camelcase_and_type_key() {
        let status = build_status(Some(1), &ReconcileOutcome::Accepted, "2026-01-01T00:00:00Z");
        let json = serde_json::to_value(&status).unwrap();
        let cond = &json["conditions"][0];
        assert!(
            cond.get("type").is_some(),
            "condition must use the `type` key"
        );
        assert!(cond.get("observedGeneration").is_some());
        assert!(cond.get("lastTransitionTime").is_some());
        assert!(json.get("observedGeneration").is_some());
    }
}
