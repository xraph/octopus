//! Config fragment generation — produces octopus routes + upstream YAML from specs

use crate::gen::scope;
use crate::gen::types::*;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, info};

/// Process a single service: fetch spec, extract routes, resolve scopes
pub async fn process_service(service: &ServiceGenConfig) -> Result<GenOutput> {
    let spec = fetch_spec(service).await?;

    let mut routes = Vec::new();
    let mut scoped_ops = Vec::new();

    let mut channels = Vec::new();

    match service.service_type.as_str() {
        "openapi" | "farp" => {
            let (r, ops) = extract_openapi_routes(&spec, service)?;
            routes = r;
            scoped_ops = ops;
        }
        "asyncapi" => {
            let (r, ops, ch) = extract_asyncapi_routes(&spec, service)?;
            routes = r;
            scoped_ops = ops;
            channels = ch;
        }
        other => anyhow::bail!("Unsupported service type: {other}"),
    }

    info!(
        service = %service.name,
        routes = routes.len(),
        operations = scoped_ops.len(),
        channels = channels.len(),
        "Extracted routes from spec"
    );

    Ok(GenOutput {
        name: service.name.clone(),
        routes,
        upstream: service.upstream.clone(),
        auth: service.auth.clone(),
        prefix: service.prefix.clone(),
        openapi_spec: Some(spec),
        scoped_operations: scoped_ops,
        channels,
    })
}

/// Fetch a spec from file or HTTP endpoint
async fn fetch_spec(service: &ServiceGenConfig) -> Result<Value> {
    let source = service
        .spec
        .as_deref()
        .or(service.endpoint.as_deref())
        .context("Service must have either 'spec' or 'endpoint'")?;

    if source.starts_with("http://") || source.starts_with("https://") {
        fetch_spec_http(source).await
    } else {
        fetch_spec_file(source)
    }
}

/// Fetch spec from HTTP
async fn fetch_spec_http(url: &str) -> Result<Value> {
    debug!(url = %url, "Fetching spec from HTTP");

    use bytes::Bytes;
    use http_body_util::Full;
    use hyper_util::client::legacy::connect::HttpConnector;
    use hyper_util::client::legacy::Client;
    use hyper_util::rt::TokioExecutor;

    let client: Client<HttpConnector, Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build_http();

    let uri: hyper::Uri = url.parse().with_context(|| format!("Invalid URL: {url}"))?;

    let req = hyper::Request::builder()
        .uri(uri)
        .header("Accept", "application/json")
        .body(Full::new(Bytes::new()))
        .context("Failed to build request")?;

    let response = client
        .request(req)
        .await
        .with_context(|| format!("Failed to fetch spec from {url}"))?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP {} fetching spec from {url}", response.status());
    }

    use http_body_util::BodyExt;
    let body = response
        .into_body()
        .collect()
        .await
        .context("Failed to read response body")?
        .to_bytes();

    let spec: Value = serde_json::from_slice(&body).context("Failed to parse spec as JSON")?;

    Ok(spec)
}

/// Fetch spec from local file
fn fetch_spec_file(path: &str) -> Result<Value> {
    debug!(path = %path, "Loading spec from file");

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read spec file: {path}"))?;

    // Try JSON first, then YAML
    if let Ok(spec) = serde_json::from_str::<Value>(&content) {
        return Ok(spec);
    }

    let spec: Value =
        serde_yaml::from_str(&content).with_context(|| format!("Failed to parse spec: {path}"))?;

    Ok(spec)
}

/// Extract routes from an OpenAPI spec
fn extract_openapi_routes(
    spec: &Value,
    service: &ServiceGenConfig,
) -> Result<(Vec<GenRoute>, Vec<ScopedOperation>)> {
    let paths = spec
        .get("paths")
        .and_then(|p| p.as_object())
        .unwrap_or(&serde_json::Map::new())
        .clone();

    let mut routes = Vec::new();
    let mut scoped_ops = Vec::new();

    for (path, methods) in &paths {
        // Skip excluded paths
        if service.skip_paths.iter().any(|skip| {
            if skip.ends_with('*') {
                path.starts_with(&skip[..skip.len() - 1])
            } else {
                path == skip
            }
        }) {
            debug!(path = %path, "Skipping excluded path");
            continue;
        }

        let methods_obj = match methods.as_object() {
            Some(m) => m,
            None => continue,
        };

        for (method_str, operation) in methods_obj {
            let method = method_str.to_uppercase();
            if !["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"]
                .contains(&method.as_str())
            {
                continue;
            }

            let operation_id = operation
                .get("operationId")
                .and_then(|v| v.as_str())
                .map(String::from);

            let summary = operation
                .get("summary")
                .and_then(|v| v.as_str())
                .map(String::from);

            let tags: Vec<String> = operation
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let requires_auth = operation.get("security").is_some();

            routes.push(GenRoute {
                method: method.clone(),
                path: path.clone(),
                operation_id: operation_id.clone(),
                summary: summary.clone(),
                tags: tags.clone(),
                requires_auth,
                skip_auth: false,
            });

            // Resolve scope
            let scope_name = scope::resolve_scope(
                &service.name,
                path,
                &method,
                operation_id.as_deref(),
                &service.scopes,
            );

            // Extract parameters
            let (path_params, query_params) = extract_parameters(spec, operation, path);

            // Extract request body type
            let request_body = extract_request_body_type(operation);

            // Extract response type
            let response_type = extract_response_type(operation);

            scoped_ops.push(ScopedOperation {
                scope: scope_name,
                method,
                path: path.clone(),
                operation_id,
                summary,
                tags,
                path_params,
                query_params,
                request_body,
                response_type,
                protocol: ProtocolKind::Http,
                channel: None,
            });
        }
    }

    Ok((routes, scoped_ops))
}

/// Extract parameters from an OpenAPI operation
fn extract_parameters(
    spec: &Value,
    operation: &Value,
    path: &str,
) -> (Vec<ParamInfo>, Vec<ParamInfo>) {
    let mut path_params = Vec::new();
    let mut query_params = Vec::new();

    // Collect parameters from operation and path-level
    let params = operation
        .get("parameters")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();

    for param in &params {
        let param = resolve_ref(spec, param);
        let name = param
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let location = param.get("in").and_then(|v| v.as_str()).unwrap_or_default();
        let required = param
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(location == "path");
        let param_type = param
            .get("schema")
            .and_then(|s| s.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("string")
            .to_string();
        let description = param
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);

        let info = ParamInfo {
            name,
            param_type,
            required,
            description,
        };

        match location {
            "path" => path_params.push(info),
            "query" => query_params.push(info),
            _ => {}
        }
    }

    // Also extract path params from the path template itself if not already present
    for segment in path.split('/') {
        if segment.starts_with('{') && segment.ends_with('}') {
            let name = &segment[1..segment.len() - 1];
            if !path_params.iter().any(|p| p.name == name) {
                path_params.push(ParamInfo {
                    name: name.to_string(),
                    param_type: "string".to_string(),
                    required: true,
                    description: None,
                });
            }
        }
    }

    (path_params, query_params)
}

/// Extract request body type reference
fn extract_request_body_type(operation: &Value) -> Option<String> {
    operation
        .get("requestBody")
        .and_then(|rb| rb.get("content"))
        .and_then(|c| c.get("application/json"))
        .and_then(|j| j.get("schema"))
        .and_then(|s| {
            s.get("$ref")
                .and_then(|r| r.as_str())
                .map(|r| ref_to_type_name(r))
        })
}

/// Extract response type reference (from 200/201 response)
fn extract_response_type(operation: &Value) -> Option<String> {
    let responses = operation.get("responses")?;

    // Try 200, then 201, then first 2xx
    let response = responses
        .get("200")
        .or_else(|| responses.get("201"))
        .or_else(|| {
            responses
                .as_object()
                .and_then(|obj| obj.iter().find(|(k, _)| k.starts_with('2')).map(|(_, v)| v))
        })?;

    response
        .get("content")
        .and_then(|c| c.get("application/json"))
        .and_then(|j| j.get("schema"))
        .and_then(|s| {
            // Direct $ref
            if let Some(r) = s.get("$ref").and_then(|r| r.as_str()) {
                return Some(ref_to_type_name(r));
            }
            // Array of $ref
            if s.get("type").and_then(|t| t.as_str()) == Some("array") {
                if let Some(r) = s
                    .get("items")
                    .and_then(|i| i.get("$ref"))
                    .and_then(|r| r.as_str())
                {
                    return Some(format!("{}[]", ref_to_type_name(r)));
                }
            }
            None
        })
}

/// Convert a $ref path to a type name
/// "#/components/schemas/User" → "User"
fn ref_to_type_name(ref_path: &str) -> String {
    ref_path.rsplit('/').next().unwrap_or(ref_path).to_string()
}

/// Resolve a JSON $ref if present
fn resolve_ref<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
    if let Some(ref_path) = value.get("$ref").and_then(|r| r.as_str()) {
        let parts: Vec<&str> = ref_path.trim_start_matches("#/").split('/').collect();
        let mut current = spec;
        for part in parts {
            current = current.get(part).unwrap_or(value);
        }
        current
    } else {
        value
    }
}

/// Extract routes from an AsyncAPI spec with full message type extraction
fn extract_asyncapi_routes(
    spec: &Value,
    service: &ServiceGenConfig,
) -> Result<(Vec<GenRoute>, Vec<ScopedOperation>, Vec<ChannelInfo>)> {
    let version = spec
        .get("asyncapi")
        .and_then(|v| v.as_str())
        .unwrap_or("2.0.0");

    let is_v3 = version.starts_with("3.");

    if is_v3 {
        extract_asyncapi_v3_routes(spec, service)
    } else {
        extract_asyncapi_v2_routes(spec, service)
    }
}

/// Extract from AsyncAPI 2.x spec
fn extract_asyncapi_v2_routes(
    spec: &Value,
    service: &ServiceGenConfig,
) -> Result<(Vec<GenRoute>, Vec<ScopedOperation>, Vec<ChannelInfo>)> {
    let channels_obj = spec
        .get("channels")
        .and_then(|c| c.as_object())
        .cloned()
        .unwrap_or_default();

    let mut routes = Vec::new();
    let mut scoped_ops = Vec::new();
    let mut channels = Vec::new();

    for (channel_name, definition) in &channels_obj {
        let description = definition
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Detect protocol: default WS, check for SSE extension
        let protocol = detect_channel_protocol(definition);
        let protocol_tag = match protocol {
            ProtocolKind::Sse => "sse",
            _ => "websocket",
        };

        let path = match protocol {
            ProtocolKind::Sse => format!("/events{channel_name}"),
            _ => format!("/ws{channel_name}"),
        };

        // Extract subscribe messages (client receives)
        let receive_messages = extract_v2_operation_messages(spec, definition.get("subscribe"));

        // Extract publish messages (client sends)
        let send_messages = extract_v2_operation_messages(spec, definition.get("publish"));

        // Extract WebSocket bindings
        let ws_bindings = extract_ws_bindings(spec, definition);

        // Build route
        let requires_auth = definition.get("security").is_some()
            || definition
                .get("subscribe")
                .and_then(|s| s.get("security"))
                .is_some();

        routes.push(GenRoute {
            method: "GET".to_string(),
            path: path.clone(),
            operation_id: None,
            summary: description.clone(),
            tags: vec![protocol_tag.to_string()],
            requires_auth,
            skip_auth: false,
        });

        let scope_name =
            scope::resolve_scope(&service.name, &path, "SUBSCRIBE", None, &service.scopes);

        // Build union type names for request/response
        let send_type = build_union_type_name(channel_name, "Send", &send_messages);
        let receive_type = build_union_type_name(channel_name, "Receive", &receive_messages);

        let channel_info = ChannelInfo {
            channel_name: channel_name.clone(),
            protocol: protocol.clone(),
            send_messages: send_messages.clone(),
            receive_messages: receive_messages.clone(),
            ws_bindings,
            description: description.clone(),
            path: path.clone(),
            scope: scope_name.clone(),
        };

        scoped_ops.push(ScopedOperation {
            scope: scope_name,
            method: "SUBSCRIBE".to_string(),
            path: path.clone(),
            operation_id: None,
            summary: description,
            tags: vec![protocol_tag.to_string()],
            path_params: Vec::new(),
            query_params: Vec::new(),
            request_body: send_type,
            response_type: receive_type,
            protocol,
            channel: Some(channel_info.clone()),
        });

        channels.push(channel_info);
    }

    Ok((routes, scoped_ops, channels))
}

/// Extract from AsyncAPI 3.x spec
fn extract_asyncapi_v3_routes(
    spec: &Value,
    service: &ServiceGenConfig,
) -> Result<(Vec<GenRoute>, Vec<ScopedOperation>, Vec<ChannelInfo>)> {
    let channels_obj = spec
        .get("channels")
        .and_then(|c| c.as_object())
        .cloned()
        .unwrap_or_default();

    let operations_obj = spec
        .get("operations")
        .and_then(|o| o.as_object())
        .cloned()
        .unwrap_or_default();

    let mut routes = Vec::new();
    let mut scoped_ops = Vec::new();
    let mut channels = Vec::new();

    // In v3, operations define direction and reference channels
    // Build a map of channel -> (send_messages, receive_messages) from operations
    let mut channel_sends: HashMap<String, Vec<ChannelMessageInfo>> = HashMap::new();
    let mut channel_receives: HashMap<String, Vec<ChannelMessageInfo>> = HashMap::new();

    for (_op_name, op_def) in &operations_obj {
        let action = op_def
            .get("action")
            .and_then(|a| a.as_str())
            .unwrap_or("receive");

        // Resolve channel reference
        let channel_ref = op_def
            .get("channel")
            .and_then(|c| c.get("$ref"))
            .and_then(|r| r.as_str())
            .and_then(|r| r.strip_prefix("#/channels/"))
            .map(String::from);

        let channel_key = match channel_ref {
            Some(k) => k,
            None => continue,
        };

        // Extract messages from the operation
        let messages = extract_v3_operation_messages(spec, op_def);

        match action {
            "send" => channel_sends
                .entry(channel_key)
                .or_default()
                .extend(messages),
            "receive" | _ => channel_receives
                .entry(channel_key)
                .or_default()
                .extend(messages),
        }
    }

    for (channel_name, definition) in &channels_obj {
        let description = definition
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from);

        let protocol = detect_channel_protocol(definition);
        let protocol_tag = match protocol {
            ProtocolKind::Sse => "sse",
            _ => "websocket",
        };

        let path = match protocol {
            ProtocolKind::Sse => format!("/events{channel_name}"),
            _ => format!("/ws{channel_name}"),
        };

        let send_messages = channel_sends.remove(channel_name).unwrap_or_default();
        let receive_messages = channel_receives.remove(channel_name).unwrap_or_default();

        // Also extract inline messages from the channel itself if operations didn't provide them
        let (send_messages, receive_messages) =
            if send_messages.is_empty() && receive_messages.is_empty() {
                let inline_msgs = extract_v3_channel_inline_messages(spec, definition);
                (Vec::new(), inline_msgs)
            } else {
                (send_messages, receive_messages)
            };

        let ws_bindings = extract_ws_bindings(spec, definition);

        routes.push(GenRoute {
            method: "GET".to_string(),
            path: path.clone(),
            operation_id: None,
            summary: description.clone(),
            tags: vec![protocol_tag.to_string()],
            requires_auth: false,
            skip_auth: false,
        });

        let scope_name =
            scope::resolve_scope(&service.name, &path, "SUBSCRIBE", None, &service.scopes);

        let send_type = build_union_type_name(channel_name, "Send", &send_messages);
        let receive_type = build_union_type_name(channel_name, "Receive", &receive_messages);

        let channel_info = ChannelInfo {
            channel_name: channel_name.clone(),
            protocol: protocol.clone(),
            send_messages: send_messages.clone(),
            receive_messages: receive_messages.clone(),
            ws_bindings,
            description: description.clone(),
            path: path.clone(),
            scope: scope_name.clone(),
        };

        scoped_ops.push(ScopedOperation {
            scope: scope_name,
            method: "SUBSCRIBE".to_string(),
            path: path.clone(),
            operation_id: None,
            summary: description,
            tags: vec![protocol_tag.to_string()],
            path_params: Vec::new(),
            query_params: Vec::new(),
            request_body: send_type,
            response_type: receive_type,
            protocol,
            channel: Some(channel_info.clone()),
        });

        channels.push(channel_info);
    }

    Ok((routes, scoped_ops, channels))
}

/// Detect protocol from channel definition
fn detect_channel_protocol(definition: &Value) -> ProtocolKind {
    // Check x-protocol extension
    if let Some(proto) = definition.get("x-protocol").and_then(|p| p.as_str()) {
        match proto.to_lowercase().as_str() {
            "sse" | "server-sent-events" => return ProtocolKind::Sse,
            "websocket" | "ws" => return ProtocolKind::WebSocket,
            _ => {}
        }
    }

    // Check bindings
    if let Some(bindings) = definition.get("bindings") {
        if bindings.get("http").is_some() {
            // HTTP bindings with event-stream content type suggests SSE
            if let Some(http_binding) = bindings.get("http") {
                if let Some(ct) = http_binding
                    .pointer("/message/headers/properties/content-type/const")
                    .or_else(|| http_binding.pointer("/message/contentType"))
                    .and_then(|v| v.as_str())
                {
                    if ct.contains("event-stream") {
                        return ProtocolKind::Sse;
                    }
                }
            }
        }
        if bindings.get("ws").is_some() {
            return ProtocolKind::WebSocket;
        }
    }

    // Check tags
    if let Some(tags) = definition.get("tags").and_then(|t| t.as_array()) {
        for tag in tags {
            if let Some(name) = tag.get("name").and_then(|n| n.as_str()) {
                if name.to_lowercase().contains("sse")
                    || name.to_lowercase().contains("event-stream")
                {
                    return ProtocolKind::Sse;
                }
            }
        }
    }

    ProtocolKind::WebSocket
}

/// Extract messages from an AsyncAPI 2.x operation (subscribe/publish)
fn extract_v2_operation_messages(
    spec: &Value,
    operation: Option<&Value>,
) -> Vec<ChannelMessageInfo> {
    let operation = match operation {
        Some(op) => op,
        None => return Vec::new(),
    };

    let message = match operation.get("message") {
        Some(m) => m,
        None => return Vec::new(),
    };

    // Handle oneOf (multiple message types)
    if let Some(one_of) = message.get("oneOf").and_then(|o| o.as_array()) {
        return one_of
            .iter()
            .filter_map(|msg| extract_single_message(spec, msg))
            .collect();
    }

    // Single message
    match extract_single_message(spec, message) {
        Some(info) => vec![info],
        None => Vec::new(),
    }
}

/// Extract messages from an AsyncAPI 3.x operation
fn extract_v3_operation_messages(spec: &Value, operation: &Value) -> Vec<ChannelMessageInfo> {
    let mut messages = Vec::new();

    // v3 operations have `messages` array
    if let Some(msgs) = operation.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            // Resolve $ref if present
            let resolved = if let Some(ref_path) = msg.get("$ref").and_then(|r| r.as_str()) {
                resolve_json_pointer(spec, ref_path)
            } else {
                Some(msg)
            };
            if let Some(resolved_msg) = resolved {
                if let Some(info) = extract_single_message(spec, resolved_msg) {
                    messages.push(info);
                }
            }
        }
    }

    // Also check single message reference
    if messages.is_empty() {
        if let Some(msg) = operation.get("message") {
            if let Some(info) = extract_single_message(spec, msg) {
                messages.push(info);
            }
        }
    }

    messages
}

/// Extract inline messages from a v3 channel definition
fn extract_v3_channel_inline_messages(spec: &Value, channel: &Value) -> Vec<ChannelMessageInfo> {
    let mut messages = Vec::new();

    if let Some(msgs) = channel.get("messages").and_then(|m| m.as_object()) {
        for (_key, msg) in msgs {
            let resolved = if let Some(ref_path) = msg.get("$ref").and_then(|r| r.as_str()) {
                resolve_json_pointer(spec, ref_path)
            } else {
                Some(msg)
            };
            if let Some(resolved_msg) = resolved {
                if let Some(info) = extract_single_message(spec, resolved_msg) {
                    messages.push(info);
                }
            }
        }
    }

    messages
}

/// Extract a single message definition into ChannelMessageInfo
fn extract_single_message(spec: &Value, message: &Value) -> Option<ChannelMessageInfo> {
    // Resolve $ref
    let resolved = if let Some(ref_path) = message.get("$ref").and_then(|r| r.as_str()) {
        resolve_json_pointer(spec, ref_path).unwrap_or(message)
    } else {
        message
    };

    let name = resolved
        .get("name")
        .or_else(|| resolved.get("messageId"))
        .or_else(|| resolved.get("x-message-name"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| {
            // Try to derive from $ref path
            message
                .get("$ref")
                .and_then(|r| r.as_str())
                .and_then(|r| r.rsplit('/').next())
                .map(String::from)
        })
        .unwrap_or_else(|| "Message".to_string());

    let description = resolved
        .get("description")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Extract payload type
    let payload_type = resolved.get("payload").and_then(|p| {
        // Direct $ref to a schema
        if let Some(r) = p.get("$ref").and_then(|r| r.as_str()) {
            return Some(ref_to_type_name(r));
        }
        // Inline schema with a title
        if let Some(title) = p.get("title").and_then(|t| t.as_str()) {
            return Some(title.to_string());
        }
        // Inline schema — use message name as type name
        if p.get("type").is_some() || p.get("properties").is_some() {
            return Some(pascal_case_simple(&name));
        }
        None
    });

    let headers_type = resolved
        .get("headers")
        .and_then(|h| h.get("$ref"))
        .and_then(|r| r.as_str())
        .map(|r| ref_to_type_name(r));

    Some(ChannelMessageInfo {
        name,
        payload_type,
        description,
        headers_type,
    })
}

/// Extract WebSocket bindings from a channel definition
fn extract_ws_bindings(_spec: &Value, definition: &Value) -> Option<WsChannelBindings> {
    let ws_binding = definition.get("bindings")?.get("ws")?;

    let mut query_params = Vec::new();
    let mut headers = Vec::new();

    // Query params from bindings
    if let Some(query) = ws_binding
        .get("query")
        .and_then(|q| q.get("properties"))
        .and_then(|p| p.as_object())
    {
        for (name, schema) in query {
            let param_type = schema
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("string")
                .to_string();
            let required = ws_binding
                .get("query")
                .and_then(|q| q.get("required"))
                .and_then(|r| r.as_array())
                .map(|arr| arr.iter().any(|v| v.as_str() == Some(name)))
                .unwrap_or(false);
            query_params.push(ParamInfo {
                name: name.clone(),
                param_type,
                required,
                description: schema
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(String::from),
            });
        }
    }

    // Headers from bindings
    if let Some(hdrs) = ws_binding
        .get("headers")
        .and_then(|h| h.get("properties"))
        .and_then(|p| p.as_object())
    {
        for (name, schema) in hdrs {
            headers.push(ParamInfo {
                name: name.clone(),
                param_type: "string".to_string(),
                required: false,
                description: schema
                    .get("description")
                    .and_then(|d| d.as_str())
                    .map(String::from),
            });
        }
    }

    if query_params.is_empty() && headers.is_empty() {
        return None;
    }

    Some(WsChannelBindings {
        query_params,
        headers,
    })
}

/// Resolve a JSON pointer (e.g., "#/components/messages/ChatMessage")
fn resolve_json_pointer<'a>(spec: &'a Value, pointer: &str) -> Option<&'a Value> {
    let path = pointer.trim_start_matches('#').trim_start_matches('/');
    let parts: Vec<&str> = path.split('/').collect();
    let mut current = spec;
    for part in parts {
        current = current.get(part)?;
    }
    Some(current)
}

/// Build a union type name from channel messages (e.g., "ChatSendMessage")
fn build_union_type_name(
    channel_name: &str,
    direction: &str,
    messages: &[ChannelMessageInfo],
) -> Option<String> {
    if messages.is_empty() {
        return None;
    }

    if messages.len() == 1 {
        // Single message — use its payload type directly
        return messages[0].payload_type.clone();
    }

    // Multiple messages — generate a union type name
    let clean_name = pascal_case_simple(
        channel_name
            .trim_start_matches('/')
            .replace('/', "_")
            .as_str(),
    );
    Some(format!("{clean_name}{direction}Message"))
}

/// Simple pascal case conversion
fn pascal_case_simple(s: &str) -> String {
    s.split(|c: char| c == '_' || c == '-' || c == '/' || c == '.')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let mut result = c.to_uppercase().to_string();
                    result.extend(chars);
                    result
                }
            }
        })
        .collect()
}

// ─── Config Fragment Writer ──────────────────────────────────────────────────

/// Write octopus config fragment files (routes + upstreams)
pub fn write_config_fragments(
    gen_config: &GenConfig,
    config_opts: &ConfigGenOptions,
    outputs: &[GenOutput],
) -> Result<()> {
    let output_dir = &gen_config.output_dir;
    std::fs::create_dir_all(output_dir)?;

    if config_opts.split {
        // Per-service files
        for output in outputs {
            write_service_routes(output_dir, &config_opts.format, output)?;
            write_service_upstream(output_dir, &config_opts.format, output)?;
        }
    } else {
        // Single merged file
        write_merged_config(output_dir, &config_opts.format, outputs)?;
    }

    info!(
        output_dir = %output_dir,
        split = config_opts.split,
        "Config fragments written"
    );

    Ok(())
}

fn write_service_routes(output_dir: &str, format: &str, output: &GenOutput) -> Result<()> {
    let prefix = output.prefix.as_deref().unwrap_or("");

    let routes: Vec<serde_json::Value> = output
        .routes
        .iter()
        .map(|r| {
            let mut route = serde_json::json!({
                "path": format!("{}{}", prefix, r.path),
                "methods": [&r.method],
                "upstream": &output.name,
            });

            if !prefix.is_empty() {
                route["strip_prefix"] = serde_json::json!(prefix);
            }

            if let Some(ref auth) = output.auth {
                if let Some(ref provider) = auth.provider {
                    route["auth_provider"] = serde_json::json!(provider);
                }
                if !auth.require_roles.is_empty() {
                    route["require_roles"] = serde_json::json!(auth.require_roles);
                }
                if !auth.require_scopes.is_empty() {
                    route["require_scopes"] = serde_json::json!(auth.require_scopes);
                }
                if auth.skip_auth {
                    route["skip_auth"] = serde_json::json!(true);
                }
            }

            // If route doesn't require auth per spec, skip_auth
            if !r.requires_auth && output.auth.is_some() {
                route["skip_auth"] = serde_json::json!(true);
            }

            route
        })
        .collect();

    let content = serde_json::json!({ "routes": routes });
    write_output_file(
        &format!("{}/{}.routes", output_dir, output.name),
        format,
        &content,
    )
}

fn write_service_upstream(output_dir: &str, format: &str, output: &GenOutput) -> Result<()> {
    let content = serde_json::json!({
        "upstreams": [{
            "name": &output.name,
            "lb_policy": &output.upstream.lb_policy,
            "instances": [{
                "id": format!("{}-1", output.name),
                "host": &output.upstream.host,
                "port": output.upstream.port,
            }],
        }]
    });

    write_output_file(
        &format!("{}/{}.upstream", output_dir, output.name),
        format,
        &content,
    )
}

fn write_merged_config(output_dir: &str, format: &str, outputs: &[GenOutput]) -> Result<()> {
    let mut all_routes = Vec::new();
    let mut all_upstreams = Vec::new();

    for output in outputs {
        let prefix = output.prefix.as_deref().unwrap_or("");

        for r in &output.routes {
            let mut route = serde_json::json!({
                "path": format!("{}{}", prefix, r.path),
                "methods": [&r.method],
                "upstream": &output.name,
            });
            if !prefix.is_empty() {
                route["strip_prefix"] = serde_json::json!(prefix);
            }
            if let Some(ref auth) = output.auth {
                if let Some(ref provider) = auth.provider {
                    route["auth_provider"] = serde_json::json!(provider);
                }
                if !auth.require_roles.is_empty() {
                    route["require_roles"] = serde_json::json!(auth.require_roles);
                }
            }
            all_routes.push(route);
        }

        all_upstreams.push(serde_json::json!({
            "name": &output.name,
            "lb_policy": &output.upstream.lb_policy,
            "instances": [{
                "id": format!("{}-1", output.name),
                "host": &output.upstream.host,
                "port": output.upstream.port,
            }],
        }));
    }

    let content = serde_json::json!({
        "routes": all_routes,
        "upstreams": all_upstreams,
    });

    write_output_file(&format!("{output_dir}/generated-config"), format, &content)
}

fn write_output_file(path_without_ext: &str, format: &str, content: &Value) -> Result<()> {
    let (path, serialized) = match format {
        "json" => (
            format!("{path_without_ext}.json"),
            serde_json::to_string_pretty(content)?,
        ),
        _ => (
            format!("{path_without_ext}.yaml"),
            serde_yaml::to_string(content)?,
        ),
    };

    std::fs::write(&path, &serialized).with_context(|| format!("Failed to write {path}"))?;

    info!(path = %path, "Written config fragment");
    Ok(())
}
