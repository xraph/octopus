//! Admission validation for reconciled Octopus CRDs.
//!
//! Pure functions that decide whether a resource spec is admissible. The
//! reconciler runs these before applying a resource to the live router and
//! reports the [`ReconcileOutcome`] back onto the resource's `.status` (see
//! [`crate::status`]). Rejected resources are not applied.

use crate::crds::{OctopusPolicySpec, OctopusRouteSpec, OctopusUpstreamSpec};
use crate::gateway_api::{GatewayClassSpec, CONTROLLER_NAME};
use crate::status::ReconcileOutcome;

/// Whether a `GatewayClass` names Octopus as its controller (and so should be
/// claimed and marked `Accepted`). GatewayClasses for other controllers are
/// ignored entirely — Octopus must not touch their status.
pub fn gatewayclass_is_ours(spec: &GatewayClassSpec) -> bool {
    spec.controller_name == CONTROLLER_NAME
}

/// Validate an [`OctopusRouteSpec`]: it must declare a path and an upstream.
pub fn validate_route(spec: &OctopusRouteSpec) -> ReconcileOutcome {
    if spec.path.trim().is_empty() {
        return ReconcileOutcome::Rejected("spec.path must not be empty".into());
    }
    if spec.upstream.trim().is_empty() {
        return ReconcileOutcome::Rejected("spec.upstream must not be empty".into());
    }
    ReconcileOutcome::Accepted
}

/// Validate an [`OctopusUpstreamSpec`]: it must declare at least one target.
pub fn validate_upstream(spec: &OctopusUpstreamSpec) -> ReconcileOutcome {
    if spec.targets.is_empty() {
        return ReconcileOutcome::Rejected("spec.targets must declare at least one target".into());
    }
    ReconcileOutcome::Accepted
}

/// Validate an [`OctopusPolicySpec`]: its `targetRef` must name a kind + name.
pub fn validate_policy(spec: &OctopusPolicySpec) -> ReconcileOutcome {
    if spec.target_ref.kind.trim().is_empty() {
        return ReconcileOutcome::Rejected("spec.targetRef.kind must not be empty".into());
    }
    if spec.target_ref.name.trim().is_empty() {
        return ReconcileOutcome::Rejected("spec.targetRef.name must not be empty".into());
    }
    ReconcileOutcome::Accepted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crds::{PolicyTargetRef, UpstreamTarget};

    fn rejected(o: &ReconcileOutcome) -> bool {
        matches!(o, ReconcileOutcome::Rejected(_))
    }

    #[test]
    fn route_requires_path_and_upstream() {
        let ok = OctopusRouteSpec {
            path: "/api".into(),
            upstream: "svc".into(),
            ..Default::default()
        };
        assert_eq!(validate_route(&ok), ReconcileOutcome::Accepted);

        let no_path = OctopusRouteSpec {
            path: String::new(),
            upstream: "svc".into(),
            ..Default::default()
        };
        assert!(rejected(&validate_route(&no_path)));

        let no_upstream = OctopusRouteSpec {
            path: "/api".into(),
            upstream: "  ".into(),
            ..Default::default()
        };
        assert!(rejected(&validate_route(&no_upstream)));
    }

    #[test]
    fn upstream_requires_a_target() {
        let ok = OctopusUpstreamSpec {
            targets: vec![UpstreamTarget {
                host: "10.0.0.1".into(),
                port: 80,
                weight: None,
            }],
            ..Default::default()
        };
        assert_eq!(validate_upstream(&ok), ReconcileOutcome::Accepted);

        let empty = OctopusUpstreamSpec::default();
        assert!(rejected(&validate_upstream(&empty)));
    }

    #[test]
    fn gatewayclass_ours_only_when_controller_matches() {
        let ours = GatewayClassSpec {
            controller_name: CONTROLLER_NAME.into(),
        };
        assert!(gatewayclass_is_ours(&ours));

        let theirs = GatewayClassSpec {
            controller_name: "example.com/other-controller".into(),
        };
        assert!(!gatewayclass_is_ours(&theirs));
    }

    #[test]
    fn policy_requires_target_kind_and_name() {
        let ok = OctopusPolicySpec {
            target_ref: PolicyTargetRef {
                group: "gateway.networking.k8s.io".into(),
                kind: "HTTPRoute".into(),
                name: "r".into(),
                section_name: None,
            },
            ..Default::default()
        };
        assert_eq!(validate_policy(&ok), ReconcileOutcome::Accepted);

        let no_name = OctopusPolicySpec {
            target_ref: PolicyTargetRef {
                group: String::new(),
                kind: "HTTPRoute".into(),
                name: String::new(),
                section_name: None,
            },
            ..Default::default()
        };
        assert!(rejected(&validate_policy(&no_name)));
    }
}
