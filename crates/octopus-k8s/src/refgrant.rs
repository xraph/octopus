//! `ReferenceGrant` — authorizes cross-namespace references.
//!
//! A grant lives in the *target* namespace and permits references *from* a set
//! of (group, kind, namespace) tuples *to* a set of (group, kind, [name]) in the
//! grant's own namespace. Same-namespace references never need a grant.

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The "from" side of a grant — who may reference.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceGrantFrom {
    /// API group of the referencing resource (core group = "").
    pub group: String,
    /// Kind of the referencing resource (e.g. `HTTPRoute`, `Gateway`).
    pub kind: String,
    /// Namespace the reference originates from.
    pub namespace: String,
}

/// The "to" side of a grant — what may be referenced (in the grant's namespace).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceGrantTo {
    /// API group of the target (core group = "").
    pub group: String,
    /// Kind of the target (e.g. `Service`, `Secret`).
    pub kind: String,
    /// Specific target name; `None` permits all names of that kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `ReferenceGrant` spec.
#[derive(CustomResource, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[kube(
    group = "gateway.networking.k8s.io",
    version = "v1beta1",
    kind = "ReferenceGrant",
    namespaced
)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceGrantSpec {
    /// Permitted referencing resources.
    pub from: Vec<ReferenceGrantFrom>,
    /// Permitted targets in this grant's namespace.
    pub to: Vec<ReferenceGrantTo>,
}

/// A cross-namespace reference to authorize.
#[derive(Debug, Clone)]
pub struct RefRequest<'a> {
    /// Group of the referencing resource.
    pub from_group: &'a str,
    /// Kind of the referencing resource.
    pub from_kind: &'a str,
    /// Namespace of the referencing resource.
    pub from_namespace: &'a str,
    /// Group of the target.
    pub to_group: &'a str,
    /// Kind of the target.
    pub to_kind: &'a str,
    /// Name of the target.
    pub to_name: &'a str,
}

/// Whether any grant (all of which must live in the target's namespace)
/// authorizes the reference.
pub fn is_permitted(grants_in_target_ns: &[ReferenceGrantSpec], req: &RefRequest<'_>) -> bool {
    grants_in_target_ns.iter().any(|grant| {
        let from_ok = grant.from.iter().any(|f| {
            f.group == req.from_group
                && f.kind == req.from_kind
                && f.namespace == req.from_namespace
        });
        let to_ok = grant.to.iter().any(|t| {
            t.group == req.to_group
                && t.kind == req.to_kind
                && t.name.as_deref().map_or(true, |n| n == req.to_name)
        });
        from_ok && to_ok
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(
        from: (&str, &str, &str),
        to_group: &str,
        to_kind: &str,
        to_name: Option<&str>,
    ) -> ReferenceGrantSpec {
        ReferenceGrantSpec {
            from: vec![ReferenceGrantFrom {
                group: from.0.into(),
                kind: from.1.into(),
                namespace: from.2.into(),
            }],
            to: vec![ReferenceGrantTo {
                group: to_group.into(),
                kind: to_kind.into(),
                name: to_name.map(|n| n.into()),
            }],
        }
    }

    fn req<'a>(from_ns: &'a str, to_kind: &'a str, to_name: &'a str) -> RefRequest<'a> {
        RefRequest {
            from_group: "gateway.networking.k8s.io",
            from_kind: "HTTPRoute",
            from_namespace: from_ns,
            to_group: "",
            to_kind,
            to_name,
        }
    }

    #[test]
    fn grant_permits_matching_reference() {
        let grants = vec![grant(
            ("gateway.networking.k8s.io", "HTTPRoute", "app"),
            "",
            "Service",
            None,
        )];
        assert!(is_permitted(&grants, &req("app", "Service", "any-svc")));
    }

    #[test]
    fn name_specific_grant_only_permits_that_name() {
        let grants = vec![grant(
            ("gateway.networking.k8s.io", "HTTPRoute", "app"),
            "",
            "Service",
            Some("allowed"),
        )];
        assert!(is_permitted(&grants, &req("app", "Service", "allowed")));
        assert!(!is_permitted(&grants, &req("app", "Service", "other")));
    }

    #[test]
    fn wrong_from_namespace_is_denied() {
        let grants = vec![grant(
            ("gateway.networking.k8s.io", "HTTPRoute", "app"),
            "",
            "Service",
            None,
        )];
        assert!(!is_permitted(&grants, &req("evil", "Service", "any")));
    }

    #[test]
    fn no_grants_denies() {
        assert!(!is_permitted(&[], &req("app", "Secret", "tls")));
    }
}
