export interface DashboardStats {
  total_requests: number;
  active_routes: number;
  avg_latency_ms: number;
  health_status: string;
  requests_per_second: number;
  error_rate: number;
  active_connections: number;
  cpu_usage: number;
  memory_usage: number;
}

export interface AnalyticsMetrics {
  timeframe: string;
  request_volume: TimeSeriesPoint[];
  latency_percentiles: LatencyPercentiles;
  error_breakdown: Record<string, number>;
  top_routes: RouteMetric[];
  status_code_distribution: Record<number, number>;
  traffic_by_method: Record<string, number>;
}

export interface TimeSeriesPoint {
  timestamp: string;
  value: number;
}

export interface LatencyPercentiles {
  p50: number;
  p90: number;
  p95: number;
  p99: number;
}

export interface RouteMetric {
  path: string;
  requests: number;
  avg_latency: number;
  error_rate: number;
}

export interface RouteInfo {
  id: string;
  path: string;
  method: string;
  upstream: string;
  request_count: number;
  is_healthy: boolean;
  avg_latency_ms: number;
  error_count: number;
  last_accessed: string | null;
}

export interface RouteConfig {
  id?: string;
  path: string;
  method: string;
  upstream: string;
  timeout_ms?: number;
  retry_count?: number;
  circuit_breaker?: CircuitBreakerConfig;
  rate_limit?: RateLimitConfig;
}

export interface CircuitBreakerConfig {
  failure_threshold: number;
  success_threshold: number;
  timeout_seconds: number;
}

export interface RateLimitConfig {
  requests_per_second: number;
  burst_size: number;
}

export interface HealthCheckInfo {
  name: string;
  status: string;
  response_time_ms: number;
  message: string | null;
  endpoint: string | null;
  last_check: string;
  consecutive_failures: number;
}

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string | null;
  enabled: boolean;
  has_dashboard: boolean;
  config: unknown;
}

export interface ActivityLogEntry {
  timestamp: string;
  level: string;
  message: string;
  details: string | null;
  source: string | null;
}

export interface LogQuery {
  level?: string;
  limit?: number;
  offset?: number;
  search?: string;
}

export interface SystemInfo {
  version: string;
  uptime_seconds: number;
  start_time: string;
  hostname: string;
  os: string;
  arch: string;
  num_cpus: number;
  total_memory: number;
}

export interface PerformanceMetrics {
  cpu_usage: number;
  memory_usage: number;
  memory_total: number;
  memory_available: number;
  goroutines: number;
  gc_count: number;
  gc_pause_ms: number;
}

export interface SecurityEvent {
  timestamp: string;
  event_type: string;
  severity: string;
  source_ip: string;
  details: string;
}

export interface ConfigItem {
  key: string;
  value: unknown;
  description: string | null;
  editable: boolean;
}

export interface WsMessage {
  msg_type: string;
  timestamp: string;
  data: unknown;
}

export interface UpstreamInfo {
  url: string;
  route_path: string;
  weight: number;
  healthy: boolean;
  active_connections: number;
}

export interface UpstreamClusterInfo {
  name: string;
  strategy: string;
  instance_count: number;
  healthy_count: number;
  instances: UpstreamInstanceInfo[];
}

export interface UpstreamInstanceInfo {
  id: string;
  address: string;
  port: number;
  url: string;
  weight: number;
  healthy: boolean;
  active_connections: number;
  avg_latency_ms: number;
  error_rate: number;
}

export interface ServiceInfo {
  name: string;
  version: string;
  address: string;
  port: number;
  route_count: number;
  healthy: boolean;
  source?: string;
  instance_count?: number;
  healthy_count?: number;
  schemas_count?: number;
  capabilities?: string[];
}

export interface FarpServiceInfo {
  name: string;
  version: string;
  instance_id: string | null;
  schemas_count: number;
  capabilities: string[];
  registered_at: string;
  updated_at: string;
}

export interface CircuitInfo {
  target_url: string;
  route_path: string;
  state: string;
  active_connections: number;
  failure_count: number;
}

// ── Upstream CRUD ──────────────────────────────────────────────────────────

export interface UpstreamInstanceConfig {
  id?: string;
  address: string;
  port: number;
  weight?: number;
}

export interface UpstreamConfig {
  name: string;
  strategy?: string;
  instances: UpstreamInstanceConfig[];
}

// ── TLS / certificates ─────────────────────────────────────────────────────

export interface TlsCertInfo {
  name: string;
  cert_file: string | null;
  key_file: string | null;
  sni_hosts: string[];
  subject_cn: string | null;
  sans: string[];
  issuer: string | null;
  not_before: string | null;
  not_after: string | null;
  days_until_expiry: number | null;
  status: string; // valid | expiring | expired | unknown
  min_tls_version: string | null;
  require_client_cert: boolean;
  source: string; // config | operator | manual
}

export interface TlsCertUpload {
  name: string;
  cert_pem: string;
  key_pem: string;
}

// ── Kubernetes CRD views ───────────────────────────────────────────────────

export interface K8sResourceSummary {
  name: string;
  namespace: string | null;
  kind: string;
  spec: unknown;
  created_at: string | null;
}

export interface K8sStatus {
  connected: boolean;
  feature_enabled: boolean;
  detail: string | null;
  counts: Record<string, number>;
}

// ── gRPC ───────────────────────────────────────────────────────────────────

export interface GrpcServiceEntry {
  service: string;
  upstream: string;
  enabled: boolean;
}

export interface GrpcConfigInfo {
  enabled: boolean;
  max_message_size?: number;
  enable_reflection?: boolean;
  enable_grpc_web?: boolean;
  deadline_propagation?: boolean;
  services: GrpcServiceEntry[];
}

// ── Auth providers / authorization config ──────────────────────────────────

export interface AuthProviderInfo {
  name: string;
  type: string; // jwt | oidc | api_key | forward_auth | mtls
  status: string;
}

export interface AuthConfigInfo {
  default_provider: string | null;
  global_enforce: boolean;
  skip_paths?: string[];
  token_cache_ttl_secs?: number;
  error_format?: string;
  authz_engine?: string;
  global_rules_count?: number;
  opa_configured?: boolean;
  providers_count: number;
}

// ── Admin session / login ──────────────────────────────────────────────────

export interface LoginRequest {
  username: string;
  password: string;
}

export interface LoginResponse {
  success: boolean;
  token: string | null;
  expires_at: string | null;
  message: string | null;
}

export interface MeResponse {
  authenticated: boolean;
  auth_required: boolean;
  username: string | null;
  role: string | null;
}
