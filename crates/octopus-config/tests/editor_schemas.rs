//! The editor JSON schemas (`schema/` and the VS Code extension's copy) use
//! `additionalProperties: false`, so every config key Octopus reads must be
//! declared there or editors flag valid configs as errors.

use serde_json::Value;
use std::path::PathBuf;

/// A config that uses every key added for host-scoped routes, the private
/// admin listener, machine auth and multi-host FARP binding.
const CONFIG: &str = r#"
gateway:
  listen: "0.0.0.0:8080"
admin:
  listen: "0.0.0.0:9090"
  require_auth: true
upstreams:
  - name: lp-billing-8080
    lb_policy: round_robin
    instances:
      - {id: lp-billing-0, host: billing, port: 8080, weight: 1}
routes:
  - id: lpr-billing
    path: /billing/*
    methods: [GET, POST]
    hosts: [api.acme.dev, "*.acme.dev"]
    upstream: lp-billing-8080
    strip_prefix: /billing
    priority: 0
farp:
  enabled: true
  registration_token: "s3cret"
  gateway:
    hostnames: [api.acme.dev]
    hostname: legacy.acme.dev
    gateway_id: public
    default_auth_provider: corp
"#;

fn schema(relative: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

/// Follow a local `$ref` (`#/$defs/X` or `#/definitions/X`).
fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    match node.get("$ref").and_then(Value::as_str) {
        Some(reference) => {
            let pointer = reference.trim_start_matches('#');
            let target = root
                .pointer(pointer)
                .unwrap_or_else(|| panic!("dangling $ref {reference}"));
            resolve(root, target)
        }
        None => node,
    }
}

/// Collect every key path in `value` that `node` does not declare while it
/// forbids additional properties. Only the keywords that decide whether a key
/// is known are interpreted.
fn undeclared(root: &Value, node: &Value, value: &Value, at: &str, out: &mut Vec<String>) {
    let node = resolve(root, node);
    for combinator in ["oneOf", "anyOf"] {
        if let Some(branches) = node.get(combinator).and_then(Value::as_array) {
            let fits = branches.iter().any(|branch| {
                let mut misses = Vec::new();
                undeclared(root, branch, value, at, &mut misses);
                misses.is_empty()
            });
            if !fits {
                out.push(format!("{at} (no {combinator} branch accepts it)"));
            }
            return;
        }
    }
    match value {
        Value::Object(map) => {
            let properties = node.get("properties").and_then(Value::as_object);
            for (key, child) in map {
                let path = format!("{at}.{key}");
                if let Some(child_schema) = properties.and_then(|p| p.get(key)) {
                    undeclared(root, child_schema, child, &path, out);
                } else {
                    match node.get("additionalProperties") {
                        Some(Value::Bool(false)) => out.push(path),
                        Some(extra @ Value::Object(_)) => {
                            undeclared(root, extra, child, &path, out);
                        }
                        _ => {}
                    }
                }
            }
        }
        Value::Array(items) => {
            if let Some(item_schema) = node.get("items") {
                for (index, item) in items.iter().enumerate() {
                    undeclared(root, item_schema, item, &format!("{at}[{index}]"), out);
                }
            }
        }
        _ => {}
    }
}

fn assert_schema_accepts(relative: &str) {
    // The sample is a real config, so the schema check pins keys Octopus reads.
    octopus_config::load_str(CONFIG, octopus_config::ConfigFormat::Yaml)
        .expect("sample config loads");
    let config: Value = serde_yaml::from_str(CONFIG).unwrap();
    let root = schema(relative);
    let mut misses = Vec::new();
    undeclared(&root, &root, &config, "$", &mut misses);
    assert!(
        misses.is_empty(),
        "{relative} rejects keys Octopus reads:\n{}",
        misses.join("\n")
    );
}

#[test]
fn repo_schema_accepts_every_new_key() {
    assert_schema_accepts("schema/octopus-config.schema.json");
}

#[test]
fn vscode_schema_accepts_every_new_key() {
    assert_schema_accepts("vscode-octopus/schema/octopus-config.schema.json");
}
