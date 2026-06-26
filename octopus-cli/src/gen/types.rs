//! Types for the generation pipeline

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Protocol Kind ──────────────────────────────────────────────────────────

/// Protocol kind for an operation or channel
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProtocolKind {
    #[default]
    Http,
    WebSocket,
    Sse,
}

// ─── Channel / Message Types ────────────────────────────────────────────────

/// Full channel semantics extracted from AsyncAPI
#[derive(Debug, Clone, Serialize)]
pub struct ChannelInfo {
    /// Original channel name from AsyncAPI (e.g., "/chat/messages")
    pub channel_name: String,
    /// Protocol for this channel
    pub protocol: ProtocolKind,
    /// Messages the client can send (AsyncAPI "publish" in 2.x, "send" in 3.x)
    pub send_messages: Vec<ChannelMessageInfo>,
    /// Messages the client can receive (AsyncAPI "subscribe" in 2.x, "receive" in 3.x)
    pub receive_messages: Vec<ChannelMessageInfo>,
    /// WebSocket-specific bindings (query params, headers)
    pub ws_bindings: Option<WsChannelBindings>,
    /// Description / summary
    pub description: Option<String>,
    /// Resolved path (e.g., "/ws/chat/messages")
    pub path: String,
    /// Scope name for namespace placement
    pub scope: String,
}

/// A single message type in a channel
#[derive(Debug, Clone, Serialize)]
pub struct ChannelMessageInfo {
    /// Message name/identifier
    pub name: String,
    /// Type reference for the message payload (matches types in OctopusSchema)
    pub payload_type: Option<String>,
    /// Description
    pub description: Option<String>,
    /// Message headers schema ref
    pub headers_type: Option<String>,
}

/// WebSocket-specific channel bindings from AsyncAPI
#[derive(Debug, Clone, Serialize)]
pub struct WsChannelBindings {
    /// Query parameters for the WS connection URL
    pub query_params: Vec<ParamInfo>,
    /// Headers for the WS handshake
    pub headers: Vec<ParamInfo>,
}

// ─── Gen Config (octopus-gen.yaml) ───────────────────────────────────────────

/// Root gen configuration
#[derive(Debug, Clone, Deserialize)]
pub struct GenConfig {
    /// Output directory for all generated files
    #[serde(default = "default_output_dir")]
    pub output_dir: String,

    /// Config fragment generation options
    #[serde(default)]
    pub config: Option<ConfigGenOptions>,

    /// Octopus Schema generation options
    #[serde(default)]
    pub schema: Option<SchemaGenOptions>,

    /// TypeScript client generation options
    #[serde(default)]
    pub client: Option<ClientGenOptions>,

    /// Services to generate from
    pub services: Vec<ServiceGenConfig>,
}

fn default_output_dir() -> String {
    "./generated".to_string()
}

/// Options for generating octopus config fragments
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigGenOptions {
    /// Output format
    #[serde(default = "default_yaml")]
    pub format: String,
    /// Split output per service (one file per service)
    #[serde(default = "default_true")]
    pub split: bool,
}

fn default_yaml() -> String {
    "yaml".to_string()
}
fn default_true() -> bool {
    true
}

/// Options for generating Octopus Schema
#[derive(Debug, Clone, Deserialize)]
pub struct SchemaGenOptions {
    /// Output file path (relative to output_dir or absolute)
    #[serde(default = "default_schema_output")]
    pub output: String,
}

fn default_schema_output() -> String {
    "octopus-schema.json".to_string()
}

/// Options for generating TypeScript client
#[derive(Debug, Clone, Deserialize)]
pub struct ClientGenOptions {
    /// Enable client generation
    #[serde(default)]
    pub enabled: bool,
    /// Output directory for TS files
    #[serde(default = "default_client_output_dir")]
    pub output_dir: String,
    /// NPM package name
    #[serde(default = "default_package_name")]
    pub package_name: String,
    /// Generate per-service standalone packages
    #[serde(default)]
    pub per_service_packages: bool,
    /// TanStack Query configuration
    #[serde(default)]
    pub tanstack_query: Option<TanStackQueryOptions>,
    /// Auth configuration for generated client
    #[serde(default)]
    pub auth: Option<ClientAuthOptions>,
    /// Type transformation options
    #[serde(default)]
    pub types: Option<TypeTransformOptions>,
    /// WebSocket client generation options
    #[serde(default)]
    pub websocket: Option<WebSocketClientOptions>,
    /// SSE client generation options
    #[serde(default)]
    pub sse: Option<SseClientOptions>,
}

/// WebSocket client generation options
#[derive(Debug, Clone, Deserialize)]
pub struct WebSocketClientOptions {
    /// Enable WS client generation
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Enable automatic reconnection in generated client
    #[serde(default = "default_true")]
    #[allow(dead_code)]
    pub reconnect: bool,
    /// Default ping interval in seconds
    #[serde(default = "default_ping_interval")]
    #[allow(dead_code)]
    pub ping_interval: u32,
}

fn default_ping_interval() -> u32 {
    30
}

/// SSE client generation options
#[derive(Debug, Clone, Deserialize)]
pub struct SseClientOptions {
    /// Enable SSE client generation
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Support POST SSE (fetch-based, not native EventSource)
    #[serde(default = "default_true")]
    #[allow(dead_code)]
    pub post_support: bool,
}

fn default_client_output_dir() -> String {
    "./generated/client".to_string()
}
fn default_package_name() -> String {
    "@octopus/api-client".to_string()
}

/// TanStack Query options
#[derive(Debug, Clone, Deserialize)]
pub struct TanStackQueryOptions {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// TanStack Query major version (4 or 5)
    #[serde(default = "default_query_version")]
    pub version: u32,
}

fn default_query_version() -> u32 {
    5
}

/// Client auth options
#[derive(Debug, Clone, Deserialize)]
pub struct ClientAuthOptions {
    /// Default auth strategy: "bearer", "api_key", "custom"
    #[serde(default = "default_bearer")]
    #[allow(dead_code)]
    pub default_strategy: String,
    /// Auth header name
    #[serde(default = "default_auth_header")]
    pub header: String,
    /// Auth header prefix (e.g., "Bearer ")
    #[serde(default = "default_auth_prefix")]
    pub prefix: String,
}

fn default_bearer() -> String {
    "bearer".to_string()
}
fn default_auth_header() -> String {
    "Authorization".to_string()
}
fn default_auth_prefix() -> String {
    "Bearer ".to_string()
}

/// Type transformation options
#[derive(Debug, Clone, Deserialize)]
pub struct TypeTransformOptions {
    /// Direct renames: old_name -> new_name
    #[serde(default)]
    pub rename: HashMap<String, String>,
    /// Regex-based transforms
    #[serde(default)]
    pub transforms: Vec<TypeTransform>,
}

/// Single type name transform rule
#[derive(Debug, Clone, Deserialize)]
pub struct TypeTransform {
    /// Regex pattern to match
    pub pattern: String,
    /// Replacement string
    pub replace: String,
}

// ─── Service Gen Config ──────────────────────────────────────────────────────

/// Configuration for a single service to generate from
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceGenConfig {
    /// Service name (used for file names and namespaces)
    pub name: String,
    /// Source type: "openapi", "asyncapi", "farp"
    #[serde(rename = "type")]
    pub service_type: String,
    /// HTTP endpoint for live spec/manifest
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Local file path or URL for spec
    #[serde(default)]
    pub spec: Option<String>,
    /// Route prefix (e.g., "/example")
    #[serde(default)]
    pub prefix: Option<String>,
    /// Upstream configuration
    pub upstream: UpstreamGenConfig,
    /// Auth configuration
    #[serde(default)]
    pub auth: Option<AuthGenConfig>,
    /// Paths to skip
    #[serde(default)]
    pub skip_paths: Vec<String>,
    /// Manual scope mappings (path -> { method -> scope_name })
    #[serde(default)]
    pub scopes: HashMap<String, HashMap<String, String>>,
}

/// Upstream config in gen file
#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamGenConfig {
    pub host: String,
    pub port: u16,
    #[serde(default = "default_lb_policy")]
    pub lb_policy: String,
}

fn default_lb_policy() -> String {
    "round_robin".to_string()
}

/// Auth config in gen file
#[derive(Debug, Clone, Deserialize)]
pub struct AuthGenConfig {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub require_roles: Vec<String>,
    #[serde(default)]
    pub require_scopes: Vec<String>,
    #[serde(default)]
    pub skip_auth: bool,
}

// ─── Intermediate Output (per-service processing result) ─────────────────────

/// Output from processing a single service
#[derive(Debug, Clone)]
pub struct GenOutput {
    /// Service name
    pub name: String,
    /// Generated routes
    pub routes: Vec<GenRoute>,
    /// Upstream config
    pub upstream: UpstreamGenConfig,
    /// Auth config
    pub auth: Option<AuthGenConfig>,
    /// Route prefix
    pub prefix: Option<String>,
    /// Raw OpenAPI spec (for type extraction)
    pub openapi_spec: Option<serde_json::Value>,
    /// Scope mappings: scope_name -> operation info
    pub scoped_operations: Vec<ScopedOperation>,
    /// Extracted channels (WS/SSE) from AsyncAPI
    pub channels: Vec<ChannelInfo>,
}

/// A single generated route
#[derive(Debug, Clone)]
pub struct GenRoute {
    pub method: String,
    pub path: String,
    #[allow(dead_code)]
    pub operation_id: Option<String>,
    #[allow(dead_code)]
    pub summary: Option<String>,
    #[allow(dead_code)]
    pub tags: Vec<String>,
    pub requires_auth: bool,
    #[allow(dead_code)]
    pub skip_auth: bool,
}

/// An operation with its scope path resolved
#[derive(Debug, Clone, Serialize)]
pub struct ScopedOperation {
    /// Dot-separated scope (e.g., "example.users.list")
    pub scope: String,
    /// HTTP method
    pub method: String,
    /// Path pattern (without prefix)
    pub path: String,
    /// Operation ID from spec
    pub operation_id: Option<String>,
    /// Summary
    pub summary: Option<String>,
    /// Tags
    pub tags: Vec<String>,
    /// Path parameters
    pub path_params: Vec<ParamInfo>,
    /// Query parameters
    pub query_params: Vec<ParamInfo>,
    /// Request body type ref
    pub request_body: Option<String>,
    /// Response type ref
    pub response_type: Option<String>,
    /// Protocol kind (Http, WebSocket, Sse)
    #[serde(default)]
    pub protocol: ProtocolKind,
    /// Channel info for WS/SSE operations (None for HTTP)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<ChannelInfo>,
}

/// Parameter information
#[derive(Debug, Clone, Serialize)]
pub struct ParamInfo {
    pub name: String,
    pub param_type: String,
    pub required: bool,
    pub description: Option<String>,
}

// ─── Octopus Schema Types ────────────────────────────────────────────────────

/// The Octopus Schema (.octopus.json) root
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctopusSchema {
    #[serde(rename = "$schema")]
    pub schema_url: String,
    pub version: String,
    pub generated_at: String,
    pub services: HashMap<String, OctopusServiceSchema>,
    pub types: HashMap<String, serde_json::Value>,
}

/// Per-service schema in octopus schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctopusServiceSchema {
    pub base_path: String,
    pub upstream: OctopusUpstream,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<OctopusAuthSchema>,
    pub operations: HashMap<String, OctopusOperation>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub channels: HashMap<String, OctopusChannel>,
}

/// Upstream info in schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctopusUpstream {
    pub host: String,
    pub port: u16,
}

/// Auth info in schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctopusAuthSchema {
    pub strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub require_roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub require_scopes: Vec<String>,
}

/// Single operation in schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctopusOperation {
    pub method: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<OctopusParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body: Option<OctopusTypeRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<OctopusTypeRef>,
}

/// Parameters grouped by location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctopusParams {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub path: HashMap<String, OctopusParamDef>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub query: HashMap<String, OctopusParamDef>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub header: HashMap<String, OctopusParamDef>,
}

/// A parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctopusParamDef {
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Type reference (either inline or $ref)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctopusTypeRef {
    #[serde(rename = "$ref", skip_serializing_if = "Option::is_none")]
    pub ref_path: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub inline_type: Option<String>,
}

// ─── Channel Schema Types ───────────────────────────────────────────────────

/// A WebSocket or SSE channel in the schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctopusChannel {
    /// Protocol: "websocket" or "sse"
    pub protocol: String,
    /// Path pattern for the channel endpoint
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub send_messages: Vec<OctopusMessageDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub receive_messages: Vec<OctopusMessageDef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bindings: Option<serde_json::Value>,
}

/// A typed message definition in a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctopusMessageDef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<OctopusTypeRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
