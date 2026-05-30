//! CRD YAML generation for installation (`octopus crd dump`).

use crate::crds::{OctopusGateway, OctopusPolicy, OctopusRoute, OctopusUpstream};
use crate::{K8sError, Result};
use kube::CustomResourceExt;

/// Emit all Octopus CRDs as a single multi-document YAML string, ready for
/// `kubectl apply -f -`.
pub fn all_crds_yaml() -> Result<String> {
    let crds = [
        OctopusGateway::crd(),
        OctopusRoute::crd(),
        OctopusUpstream::crd(),
        OctopusPolicy::crd(),
    ];

    let mut out = String::new();
    for crd in crds {
        out.push_str("---\n");
        out.push_str(&serde_yaml::to_string(&crd).map_err(|e| K8sError::Serialize(e.to_string()))?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn yaml_contains_all_four_crds() {
        let yaml = all_crds_yaml().unwrap();
        for kind in [
            "OctopusGateway",
            "OctopusRoute",
            "OctopusUpstream",
            "OctopusPolicy",
        ] {
            assert!(yaml.contains(kind), "expected CRD for {kind} in output");
        }
    }

    #[test]
    fn yaml_parses_as_four_crd_documents() {
        let yaml = all_crds_yaml().unwrap();
        let docs: Vec<serde_yaml::Value> = serde_yaml::Deserializer::from_str(&yaml)
            .map(|doc| serde_yaml::Value::deserialize(doc).expect("each document parses"))
            .collect();
        assert_eq!(docs.len(), 4, "one document per CRD");
        for doc in &docs {
            assert_eq!(
                doc.get("kind").and_then(|k| k.as_str()),
                Some("CustomResourceDefinition")
            );
        }
    }
}
