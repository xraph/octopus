//! Octopus Schema generation (.octopus.json)
//!
//! The Octopus Schema is the intermediate representation that captures:
//! - Service metadata (name, upstream, auth)
//! - Scoped operations with full typing (params, request, response)
//! - DTO type definitions (extracted from OpenAPI components.schemas)
//! - Type transformations (renames, pattern replacements)

use crate::gen::types::*;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::time::SystemTime;
use tracing::info;

/// Generate and write the Octopus Schema file
pub fn generate_schema(
    gen_config: &GenConfig,
    schema_opts: &SchemaGenOptions,
    outputs: &[GenOutput],
) -> Result<()> {
    let schema = build_schema(gen_config, outputs)?;

    let output_path = if schema_opts.output.starts_with('/') || schema_opts.output.starts_with('.')
    {
        schema_opts.output.clone()
    } else {
        format!("{}/{}", gen_config.output_dir, schema_opts.output)
    };

    // Ensure parent dir exists
    if let Some(parent) = std::path::Path::new(&output_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(&schema)?;
    std::fs::write(&output_path, &json)
        .with_context(|| format!("Failed to write schema to {output_path}"))?;

    info!(path = %output_path, "Octopus Schema written");
    Ok(())
}

/// Build the Octopus Schema in memory
pub fn build_schema(gen_config: &GenConfig, outputs: &[GenOutput]) -> Result<OctopusSchema> {
    let mut services = HashMap::new();
    let mut all_types: HashMap<String, Value> = HashMap::new();

    // Type transformation config
    let type_transforms = gen_config.client.as_ref().and_then(|c| c.types.as_ref());

    for output in outputs {
        // Build operations map
        let mut operations = HashMap::new();

        for op in &output.scoped_operations {
            let mut params = None;

            let has_params = !op.path_params.is_empty() || !op.query_params.is_empty();

            if has_params {
                let mut path_map = HashMap::new();
                for p in &op.path_params {
                    path_map.insert(
                        p.name.clone(),
                        OctopusParamDef {
                            param_type: p.param_type.clone(),
                            required: p.required,
                            description: p.description.clone(),
                        },
                    );
                }

                let mut query_map = HashMap::new();
                for p in &op.query_params {
                    query_map.insert(
                        p.name.clone(),
                        OctopusParamDef {
                            param_type: p.param_type.clone(),
                            required: p.required,
                            description: p.description.clone(),
                        },
                    );
                }

                params = Some(OctopusParams {
                    path: path_map,
                    query: query_map,
                    header: HashMap::new(),
                });
            }

            let request_body = op.request_body.as_ref().map(|t| {
                let transformed = transform_type_name(t, type_transforms);
                OctopusTypeRef {
                    ref_path: Some(format!("#/types/{transformed}")),
                    inline_type: None,
                }
            });

            let response = op.response_type.as_ref().map(|t| {
                // Handle array types like "User[]"
                let (base_type, is_array) = if t.ends_with("[]") {
                    (&t[..t.len() - 2], true)
                } else {
                    (t.as_str(), false)
                };
                let transformed = transform_type_name(base_type, type_transforms);
                if is_array {
                    OctopusTypeRef {
                        ref_path: Some(format!("#/types/{transformed}[]")),
                        inline_type: None,
                    }
                } else {
                    OctopusTypeRef {
                        ref_path: Some(format!("#/types/{transformed}")),
                        inline_type: None,
                    }
                }
            });

            operations.insert(
                op.scope.clone(),
                OctopusOperation {
                    method: op.method.clone(),
                    path: op.path.clone(),
                    summary: op.summary.clone(),
                    tags: op.tags.clone(),
                    params,
                    request_body,
                    response,
                },
            );
        }

        // Build auth schema
        let auth = output.auth.as_ref().map(|a| OctopusAuthSchema {
            strategy: "bearer".to_string(),
            provider: a.provider.clone(),
            require_roles: a.require_roles.clone(),
            require_scopes: a.require_scopes.clone(),
        });

        // Build channels map from extracted channel info
        let mut schema_channels = HashMap::new();
        for ch in &output.channels {
            let protocol_str = match ch.protocol {
                ProtocolKind::WebSocket => "websocket",
                ProtocolKind::Sse => "sse",
                ProtocolKind::Http => "http",
            };

            let send_msgs: Vec<OctopusMessageDef> = ch
                .send_messages
                .iter()
                .map(|m| OctopusMessageDef {
                    name: m.name.clone(),
                    payload: m.payload_type.as_ref().map(|t| {
                        let transformed = transform_type_name(t, type_transforms);
                        OctopusTypeRef {
                            ref_path: Some(format!("#/types/{transformed}")),
                            inline_type: None,
                        }
                    }),
                    description: m.description.clone(),
                })
                .collect();

            let receive_msgs: Vec<OctopusMessageDef> = ch
                .receive_messages
                .iter()
                .map(|m| OctopusMessageDef {
                    name: m.name.clone(),
                    payload: m.payload_type.as_ref().map(|t| {
                        let transformed = transform_type_name(t, type_transforms);
                        OctopusTypeRef {
                            ref_path: Some(format!("#/types/{transformed}")),
                            inline_type: None,
                        }
                    }),
                    description: m.description.clone(),
                })
                .collect();

            // Serialize bindings if present
            let bindings = ch.ws_bindings.as_ref().map(|b| {
                serde_json::json!({
                    "query": b.query_params.iter().map(|p| {
                        serde_json::json!({
                            "name": p.name,
                            "type": p.param_type,
                            "required": p.required,
                        })
                    }).collect::<Vec<_>>(),
                    "headers": b.headers.iter().map(|p| {
                        serde_json::json!({
                            "name": p.name,
                            "type": p.param_type,
                        })
                    }).collect::<Vec<_>>(),
                })
            });

            schema_channels.insert(
                ch.scope.clone(),
                OctopusChannel {
                    protocol: protocol_str.to_string(),
                    path: ch.path.clone(),
                    summary: ch.description.clone(),
                    send_messages: send_msgs,
                    receive_messages: receive_msgs,
                    bindings,
                },
            );
        }

        services.insert(
            output.name.clone(),
            OctopusServiceSchema {
                base_path: output
                    .prefix
                    .clone()
                    .unwrap_or_else(|| format!("/{}", output.name)),
                upstream: OctopusUpstream {
                    host: output.upstream.host.clone(),
                    port: output.upstream.port,
                },
                auth,
                operations,
                channels: schema_channels,
            },
        );

        // Extract types from OpenAPI or AsyncAPI spec
        if let Some(ref spec) = output.openapi_spec {
            let types = extract_types(spec, type_transforms);
            all_types.extend(types);

            // Also extract AsyncAPI component schemas (same structure)
            if spec.get("asyncapi").is_some() {
                let async_types = extract_asyncapi_types(spec, type_transforms);
                all_types.extend(async_types);
            }
        }

        // Generate union types for multi-message channels
        for ch in &output.channels {
            generate_channel_union_types(ch, type_transforms, &mut all_types);
        }
    }

    Ok(OctopusSchema {
        schema_url: "https://octopus.dev/schema/v1.1".to_string(),
        version: "1.1.0".to_string(),
        generated_at: {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            format!("{now}")
        },
        services,
        types: all_types,
    })
}

/// Extract type definitions from OpenAPI components.schemas
fn extract_types(
    spec: &Value,
    transforms: Option<&TypeTransformOptions>,
) -> HashMap<String, Value> {
    let mut types = HashMap::new();

    let schemas = spec
        .get("components")
        .and_then(|c| c.get("schemas"))
        .and_then(|s| s.as_object());

    if let Some(schemas) = schemas {
        for (name, schema) in schemas {
            let transformed_name = transform_type_name(name, transforms);
            // Clean up the schema — remove OpenAPI-specific fields, keep JSON Schema
            let clean_schema = clean_schema_for_output(schema);
            types.insert(transformed_name, clean_schema);
        }
    }

    types
}

/// Clean an OpenAPI schema for output (strip OpenAPI-specific extensions)
fn clean_schema_for_output(schema: &Value) -> Value {
    match schema {
        Value::Object(obj) => {
            let mut clean = serde_json::Map::new();
            for (key, value) in obj {
                // Skip OpenAPI extensions (x-*) and xml
                if key.starts_with("x-") || key == "xml" || key == "example" {
                    continue;
                }
                // Transform $ref paths to use #/types/ prefix
                if key == "$ref" {
                    if let Some(ref_str) = value.as_str() {
                        let type_name = ref_str.rsplit('/').next().unwrap_or(ref_str);
                        clean.insert(
                            "$ref".to_string(),
                            Value::String(format!("#/types/{type_name}")),
                        );
                        continue;
                    }
                }
                clean.insert(key.clone(), clean_schema_for_output(value));
            }
            Value::Object(clean)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(clean_schema_for_output).collect()),
        other => other.clone(),
    }
}

/// Apply type name transformations
fn transform_type_name(name: &str, transforms: Option<&TypeTransformOptions>) -> String {
    let transforms = match transforms {
        Some(t) => t,
        None => return name.to_string(),
    };

    // Check direct renames first
    if let Some(renamed) = transforms.rename.get(name) {
        return renamed.clone();
    }

    // Apply regex transforms
    let mut result = name.to_string();
    for t in &transforms.transforms {
        if let Ok(re) = regex::Regex::new(&t.pattern) {
            result = re.replace_all(&result, t.replace.as_str()).to_string();
        }
    }

    result
}

/// Extract type definitions from AsyncAPI components.schemas
fn extract_asyncapi_types(
    spec: &Value,
    transforms: Option<&TypeTransformOptions>,
) -> HashMap<String, Value> {
    let mut types = HashMap::new();

    // AsyncAPI uses the same components.schemas structure as OpenAPI
    let schemas = spec
        .get("components")
        .and_then(|c| c.get("schemas"))
        .and_then(|s| s.as_object());

    if let Some(schemas) = schemas {
        for (name, schema) in schemas {
            let transformed_name = transform_type_name(name, transforms);
            let clean_schema = clean_schema_for_output(schema);
            types.insert(transformed_name, clean_schema);
        }
    }

    // Also extract payload schemas from components.messages
    if let Some(messages) = spec
        .get("components")
        .and_then(|c| c.get("messages"))
        .and_then(|m| m.as_object())
    {
        for (_name, message) in messages {
            // If the message has an inline payload schema with a title, extract it
            if let Some(payload) = message.get("payload") {
                if let Some(title) = payload.get("title").and_then(|t| t.as_str()) {
                    // Only add if it's not a $ref (those are already in schemas)
                    if payload.get("$ref").is_none() {
                        let transformed = transform_type_name(title, transforms);
                        let clean = clean_schema_for_output(payload);
                        types.insert(transformed, clean);
                    }
                }
            }
        }
    }

    types
}

/// Generate union types for channels with multiple send/receive messages
fn generate_channel_union_types(
    channel: &ChannelInfo,
    transforms: Option<&TypeTransformOptions>,
    all_types: &mut HashMap<String, Value>,
) {
    // Generate send union type if multiple messages
    if channel.send_messages.len() > 1 {
        let union_name = build_channel_union_name(&channel.channel_name, "Send", transforms);
        let refs: Vec<Value> = channel
            .send_messages
            .iter()
            .filter_map(|m| m.payload_type.as_ref())
            .map(|t| {
                let transformed = transform_type_name(t, transforms);
                serde_json::json!({ "$ref": format!("#/types/{transformed}") })
            })
            .collect();
        if !refs.is_empty() {
            all_types.insert(union_name, serde_json::json!({ "oneOf": refs }));
        }
    }

    // Generate receive union type if multiple messages
    if channel.receive_messages.len() > 1 {
        let union_name = build_channel_union_name(&channel.channel_name, "Receive", transforms);
        let refs: Vec<Value> = channel
            .receive_messages
            .iter()
            .filter_map(|m| m.payload_type.as_ref())
            .map(|t| {
                let transformed = transform_type_name(t, transforms);
                serde_json::json!({ "$ref": format!("#/types/{transformed}") })
            })
            .collect();
        if !refs.is_empty() {
            all_types.insert(union_name, serde_json::json!({ "oneOf": refs }));
        }
    }
}

fn build_channel_union_name(
    channel_name: &str,
    direction: &str,
    transforms: Option<&TypeTransformOptions>,
) -> String {
    let clean = channel_name
        .trim_start_matches('/')
        .replace(['/', '-'], "_");
    let pascal = clean
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut chars = s.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let mut r = c.to_uppercase().to_string();
                    r.extend(chars);
                    r
                }
            }
        })
        .collect::<String>();
    let name = format!("{pascal}{direction}Message");
    transform_type_name(&name, transforms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_type_name_direct_rename() {
        let transforms = TypeTransformOptions {
            rename: {
                let mut m = HashMap::new();
                m.insert("OldName".to_string(), "NewName".to_string());
                m
            },
            transforms: Vec::new(),
        };

        assert_eq!(transform_type_name("OldName", Some(&transforms)), "NewName");
        assert_eq!(transform_type_name("Other", Some(&transforms)), "Other");
    }

    #[test]
    fn test_transform_type_name_regex() {
        let transforms = TypeTransformOptions {
            rename: HashMap::new(),
            transforms: vec![
                TypeTransform {
                    pattern: "^Dto".to_string(),
                    replace: String::new(),
                },
                TypeTransform {
                    pattern: "Response$".to_string(),
                    replace: String::new(),
                },
            ],
        };

        assert_eq!(transform_type_name("DtoUser", Some(&transforms)), "User");
        assert_eq!(
            transform_type_name("UserResponse", Some(&transforms)),
            "User"
        );
        assert_eq!(
            transform_type_name("DtoUserResponse", Some(&transforms)),
            "User"
        );
    }
}
