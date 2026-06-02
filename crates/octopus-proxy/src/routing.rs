//! Advanced routing strategies for load balancing and traffic management

use octopus_core::UpstreamInstance;
use rand::Rng;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::debug;

/// Routing strategy for selecting upstream instances
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingStrategy {
    /// Round-robin selection
    RoundRobin,
    /// Random selection
    Random,
    /// Least connections
    LeastConnections,
    /// Weighted round-robin
    WeightedRoundRobin,
    /// Latency-aware (adaptive)
    LatencyAware,
    /// Error-aware (adaptive)
    ErrorAware,
}

/// Routing configuration
#[derive(Debug, Clone)]
pub struct RoutingConfig {
    /// Primary routing strategy
    pub strategy: RoutingStrategy,

    /// Enable canary deployments
    pub enable_canary: bool,

    /// Enable request shadowing
    pub enable_shadowing: bool,

    /// Health check required for selection
    pub require_healthy: bool,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            strategy: RoutingStrategy::RoundRobin,
            enable_canary: false,
            enable_shadowing: false,
            require_healthy: true,
        }
    }
}

/// Router for selecting upstream instances
pub struct Router {
    config: RoutingConfig,
    round_robin_counter: AtomicU64,
    instance_stats: Arc<InstanceStatsMap>,
}

impl Router {
    /// Create a new router
    pub fn new(config: RoutingConfig) -> Self {
        Self {
            config,
            round_robin_counter: AtomicU64::new(0),
            instance_stats: Arc::new(InstanceStatsMap::new()),
        }
    }

    /// Select an upstream instance based on routing strategy
    pub fn select<'a>(
        &self,
        instances: &'a [UpstreamInstance],
        canary_config: Option<&CanaryConfig>,
    ) -> Option<&'a UpstreamInstance> {
        if instances.is_empty() {
            return None;
        }

        // All instances are available (health checking handled elsewhere)
        let available: Vec<&UpstreamInstance> = instances.iter().collect();

        if available.is_empty() {
            return None;
        }

        // Handle canary deployments
        if self.config.enable_canary {
            if let Some(canary) = canary_config {
                if let Some(instance) = self.select_with_canary(&available, canary) {
                    return Some(instance);
                }
            }
        }

        // Apply primary routing strategy
        self.select_by_strategy(&available)
    }

    /// Select by routing strategy
    fn select_by_strategy<'a>(
        &self,
        instances: &[&'a UpstreamInstance],
    ) -> Option<&'a UpstreamInstance> {
        match self.config.strategy {
            RoutingStrategy::RoundRobin => self.select_round_robin(instances),
            RoutingStrategy::Random => self.select_random(instances),
            RoutingStrategy::LeastConnections => self.select_least_connections(instances),
            RoutingStrategy::WeightedRoundRobin => self.select_weighted_round_robin(instances),
            RoutingStrategy::LatencyAware => self.select_latency_aware(instances),
            RoutingStrategy::ErrorAware => self.select_error_aware(instances),
        }
    }

    /// Round-robin selection
    fn select_round_robin<'a>(
        &self,
        instances: &[&'a UpstreamInstance],
    ) -> Option<&'a UpstreamInstance> {
        let counter = self.round_robin_counter.fetch_add(1, Ordering::Relaxed);
        let index = (counter as usize) % instances.len();
        instances.get(index).copied()
    }

    /// Random selection
    fn select_random<'a>(
        &self,
        instances: &[&'a UpstreamInstance],
    ) -> Option<&'a UpstreamInstance> {
        let mut rng = rand::thread_rng();
        let index = rng.gen_range(0..instances.len());
        instances.get(index).copied()
    }

    /// Least connections selection
    fn select_least_connections<'a>(
        &self,
        instances: &[&'a UpstreamInstance],
    ) -> Option<&'a UpstreamInstance> {
        instances
            .iter()
            .min_by_key(|instance| self.instance_stats.get_active_connections(&instance.id))
            .copied()
    }

    /// Weighted round-robin selection
    fn select_weighted_round_robin<'a>(
        &self,
        instances: &[&'a UpstreamInstance],
    ) -> Option<&'a UpstreamInstance> {
        // Calculate total weight
        let total_weight: u32 = instances
            .iter()
            .map(|i| if i.weight == 0 { 1 } else { i.weight })
            .sum();

        if total_weight == 0 {
            return self.select_round_robin(instances);
        }

        // Select based on weight
        let mut rng = rand::thread_rng();
        let mut target = rng.gen_range(0..total_weight);

        for instance in instances {
            let weight = if instance.weight == 0 {
                1
            } else {
                instance.weight
            };
            if target < weight {
                return Some(instance);
            }
            target -= weight;
        }

        instances.last().copied()
    }

    /// Latency-aware adaptive selection
    fn select_latency_aware<'a>(
        &self,
        instances: &[&'a UpstreamInstance],
    ) -> Option<&'a UpstreamInstance> {
        instances
            .iter()
            .min_by_key(|instance| self.instance_stats.get_avg_latency_ms(&instance.id))
            .copied()
    }

    /// Error-aware adaptive selection
    fn select_error_aware<'a>(
        &self,
        instances: &[&'a UpstreamInstance],
    ) -> Option<&'a UpstreamInstance> {
        instances
            .iter()
            .min_by_key(|instance| self.instance_stats.get_error_rate(&instance.id))
            .copied()
    }

    /// Select with canary deployment
    fn select_with_canary<'a>(
        &self,
        instances: &[&'a UpstreamInstance],
        canary: &CanaryConfig,
    ) -> Option<&'a UpstreamInstance> {
        // Separate canary and stable instances based on metadata
        let canary_instances: Vec<&UpstreamInstance> = instances
            .iter()
            .filter(|i| {
                i.metadata
                    .get("version")
                    .map(|v| v == &canary.canary_version)
                    .unwrap_or(false)
            })
            .copied()
            .collect();

        let stable_instances: Vec<&UpstreamInstance> = instances
            .iter()
            .filter(|i| {
                i.metadata
                    .get("version")
                    .map(|v| v != &canary.canary_version)
                    .unwrap_or(true)
            })
            .copied()
            .collect();

        if canary_instances.is_empty() {
            return self.select_by_strategy(&stable_instances);
        }

        // Determine if this request should go to canary
        let mut rng = rand::thread_rng();
        let roll: u32 = rng.gen_range(0..100);

        if roll < canary.traffic_percentage {
            debug!(
                roll = roll,
                percentage = canary.traffic_percentage,
                "Routing to canary"
            );
            self.select_by_strategy(&canary_instances)
        } else {
            self.select_by_strategy(&stable_instances)
        }
    }

    /// Record request metrics for adaptive routing
    pub fn record_request(&self, instance_id: &str, latency: Duration, is_error: bool) {
        self.instance_stats
            .record_request(instance_id, latency, is_error);
    }

    /// Increment active connections
    pub fn increment_connections(&self, instance_id: &str) {
        self.instance_stats.increment_connections(instance_id);
    }

    /// Decrement active connections
    pub fn decrement_connections(&self, instance_id: &str) {
        self.instance_stats.decrement_connections(instance_id);
    }

    /// Get configuration
    pub fn config(&self) -> &RoutingConfig {
        &self.config
    }
}

impl std::fmt::Debug for Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Router")
            .field("config", &self.config)
            .finish()
    }
}

/// Canary deployment configuration
#[derive(Debug, Clone)]
pub struct CanaryConfig {
    /// Version identifier for canary instances
    pub canary_version: String,

    /// Percentage of traffic to route to canary (0-100)
    pub traffic_percentage: u32,

    /// Automatically increase traffic if metrics are good
    pub auto_promote: bool,

    /// Error rate threshold for auto-rollback (0.0-1.0)
    pub error_threshold: f64,
}

impl CanaryConfig {
    /// Create a new canary configuration
    pub fn new(canary_version: String, traffic_percentage: u32) -> Self {
        Self {
            canary_version,
            traffic_percentage: traffic_percentage.min(100),
            auto_promote: false,
            error_threshold: 0.05, // 5% error rate
        }
    }

    /// Enable auto-promotion
    pub fn with_auto_promote(mut self) -> Self {
        self.auto_promote = true;
        self
    }

    /// Set error threshold
    pub fn with_error_threshold(mut self, threshold: f64) -> Self {
        self.error_threshold = threshold.clamp(0.0, 1.0);
        self
    }
}

/// Instance statistics for adaptive routing
struct InstanceStats {
    active_connections: AtomicU64,
    total_requests: AtomicU64,
    total_errors: AtomicU64,
    total_latency_ms: AtomicU64,
    last_updated: parking_lot::Mutex<Instant>,
}

impl InstanceStats {
    fn new() -> Self {
        Self {
            active_connections: AtomicU64::new(0),
            total_requests: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            total_latency_ms: AtomicU64::new(0),
            last_updated: parking_lot::Mutex::new(Instant::now()),
        }
    }

    fn record_request(&self, latency: Duration, is_error: bool) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ms
            .fetch_add(latency.as_millis() as u64, Ordering::Relaxed);

        if is_error {
            self.total_errors.fetch_add(1, Ordering::Relaxed);
        }

        *self.last_updated.lock() = Instant::now();
    }

    fn get_avg_latency_ms(&self) -> u64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return u64::MAX; // Penalize instances with no data
        }

        let latency = self.total_latency_ms.load(Ordering::Relaxed);
        latency / total
    }

    fn get_error_rate(&self) -> u64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return u64::MAX; // Penalize instances with no data
        }

        let errors = self.total_errors.load(Ordering::Relaxed);
        (errors * 1000) / total // Error rate * 1000 for precision
    }
}

/// Map of instance statistics
struct InstanceStatsMap {
    stats: parking_lot::RwLock<HashMap<String, Arc<InstanceStats>>>,
}

impl InstanceStatsMap {
    fn new() -> Self {
        Self {
            stats: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    fn get_or_create(&self, instance_id: &str) -> Arc<InstanceStats> {
        {
            let stats = self.stats.read();
            if let Some(instance_stats) = stats.get(instance_id) {
                return instance_stats.clone();
            }
        }

        let mut stats = self.stats.write();
        stats
            .entry(instance_id.to_string())
            .or_insert_with(|| Arc::new(InstanceStats::new()))
            .clone()
    }

    fn record_request(&self, instance_id: &str, latency: Duration, is_error: bool) {
        let stats = self.get_or_create(instance_id);
        stats.record_request(latency, is_error);
    }

    fn increment_connections(&self, instance_id: &str) {
        let stats = self.get_or_create(instance_id);
        stats.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    fn decrement_connections(&self, instance_id: &str) {
        let stats = self.get_or_create(instance_id);
        stats.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    fn get_active_connections(&self, instance_id: &str) -> u64 {
        self.stats
            .read()
            .get(instance_id)
            .map(|s| s.active_connections.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    fn get_avg_latency_ms(&self, instance_id: &str) -> u64 {
        self.stats
            .read()
            .get(instance_id)
            .map(|s| s.get_avg_latency_ms())
            .unwrap_or(u64::MAX)
    }

    fn get_error_rate(&self, instance_id: &str) -> u64 {
        self.stats
            .read()
            .get(instance_id)
            .map(|s| s.get_error_rate())
            .unwrap_or(u64::MAX)
    }
}

/// Request shadowing configuration
#[derive(Debug, Clone)]
pub struct ShadowConfig {
    /// Target instance/cluster for shadow traffic
    pub shadow_target: String,

    /// Percentage of traffic to shadow (0-100)
    pub traffic_percentage: u32,

    /// Whether to wait for shadow response
    pub synchronous: bool,

    /// Whether to log shadow failures
    pub log_failures: bool,
}

impl ShadowConfig {
    /// Create a new shadow configuration
    pub fn new(shadow_target: String, traffic_percentage: u32) -> Self {
        Self {
            shadow_target,
            traffic_percentage: traffic_percentage.min(100),
            synchronous: false,
            log_failures: true,
        }
    }

    /// Make shadowing synchronous
    pub fn synchronous(mut self) -> Self {
        self.synchronous = true;
        self
    }

    /// Check if request should be shadowed
    pub fn should_shadow(&self) -> bool {
        let mut rng = rand::thread_rng();
        let roll: u32 = rng.gen_range(0..100);
        roll < self.traffic_percentage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_instance(id: &str, _healthy: bool, weight: u32) -> UpstreamInstance {
        let mut instance = UpstreamInstance::new(id.to_string(), "localhost".to_string(), 8080);
        instance.weight = weight;
        instance
    }

    #[test]
    fn test_round_robin_selection() {
        let config = RoutingConfig {
            strategy: RoutingStrategy::RoundRobin,
            ..Default::default()
        };
        let router = Router::new(config);

        let instances = vec![
            create_test_instance("1", true, 1),
            create_test_instance("2", true, 1),
            create_test_instance("3", true, 1),
        ];

        let selected1 = router.select(&instances, None).unwrap();
        let selected2 = router.select(&instances, None).unwrap();
        let selected3 = router.select(&instances, None).unwrap();
        let selected4 = router.select(&instances, None).unwrap();

        assert_eq!(selected1.id, "1");
        assert_eq!(selected2.id, "2");
        assert_eq!(selected3.id, "3");
        assert_eq!(selected4.id, "1"); // Wraps around
    }

    #[test]
    fn test_random_selection() {
        let config = RoutingConfig {
            strategy: RoutingStrategy::Random,
            ..Default::default()
        };
        let router = Router::new(config);

        let instances = vec![
            create_test_instance("1", true, 1),
            create_test_instance("2", true, 1),
        ];

        // Should select one of the instances
        let selected = router.select(&instances, None).unwrap();
        assert!(selected.id == "1" || selected.id == "2");
    }

    #[test]
    fn test_weighted_selection() {
        let config = RoutingConfig {
            strategy: RoutingStrategy::WeightedRoundRobin,
            ..Default::default()
        };
        let router = Router::new(config);

        let instances = vec![
            create_test_instance("1", true, 10),
            create_test_instance("2", true, 1),
        ];

        // Should heavily favor instance 1
        let mut count1 = 0;
        let mut count2 = 0;

        for _ in 0..100 {
            let selected = router.select(&instances, None).unwrap();
            if selected.id == "1" {
                count1 += 1;
            } else {
                count2 += 1;
            }
        }

        assert!(count1 > count2);
    }

    #[test]
    fn test_canary_deployment() {
        let config = RoutingConfig {
            strategy: RoutingStrategy::RoundRobin,
            enable_canary: true,
            ..Default::default()
        };
        let router = Router::new(config);

        let mut stable = create_test_instance("stable", true, 1);
        stable
            .metadata
            .insert("version".to_string(), "v1".to_string());

        let mut canary = create_test_instance("canary", true, 1);
        canary
            .metadata
            .insert("version".to_string(), "v2".to_string());

        let instances = vec![stable, canary];

        let canary_config = CanaryConfig::new("v2".to_string(), 20); // 20% to canary

        // The split is a per-request Bernoulli(0.2) draw (gen_range(0..100) < 20),
        // so over N trials canary_count ~ Binomial(N, 0.2). A small N with tight
        // bounds flakes (~1% at N=100, ±2.5σ) regardless of platform. Use a large
        // N and ~±6σ bounds so the test is reliable everywhere (flake ~1e-10)
        // while still asserting a real ~20/80 split.
        let trials = 1000;
        let mut canary_count = 0;
        let mut stable_count = 0;

        for _ in 0..trials {
            let selected = router.select(&instances, Some(&canary_config)).unwrap();
            if selected.id == "canary" {
                canary_count += 1;
            } else {
                stable_count += 1;
            }
        }

        // Expected mean 200, σ ≈ 12.6; bounds below are ~±6σ.
        assert!(
            (120..=280).contains(&canary_count),
            "canary_count={canary_count} outside expected ~20% band"
        );
        assert!(
            (720..=880).contains(&stable_count),
            "stable_count={stable_count} outside expected ~80% band"
        );
    }

    #[test]
    fn test_instance_selection() {
        let config = RoutingConfig {
            strategy: RoutingStrategy::RoundRobin,
            require_healthy: true,
            ..Default::default()
        };
        let router = Router::new(config);

        let instances = vec![
            create_test_instance("1", true, 1),
            create_test_instance("2", true, 1),
        ];

        let selected = router.select(&instances, None).unwrap();
        assert!(!selected.id.is_empty());
    }

    #[test]
    fn test_shadow_config() {
        let config = ShadowConfig::new("shadow-cluster".to_string(), 10);

        assert_eq!(config.shadow_target, "shadow-cluster");
        assert_eq!(config.traffic_percentage, 10);
        assert!(!config.synchronous);
    }

    #[test]
    fn test_adaptive_routing_stats() {
        let config = RoutingConfig {
            strategy: RoutingStrategy::LatencyAware,
            ..Default::default()
        };
        let router = Router::new(config);

        // Record some stats
        router.record_request("1", Duration::from_millis(10), false);
        router.record_request("2", Duration::from_millis(50), false);

        let instances = vec![
            create_test_instance("1", true, 1),
            create_test_instance("2", true, 1),
        ];

        // Should select instance 1 (lower latency)
        let selected = router.select(&instances, None).unwrap();
        assert_eq!(selected.id, "1");
    }
}
