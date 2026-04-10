//! Load balancing strategies for upstream instance selection.
//!
//! Provides multiple load balancing algorithms:
//! - **Round Robin**: Cycles through healthy instances sequentially
//! - **Weighted Round Robin**: Distributes based on instance weights
//! - **Random**: Random selection among healthy instances
//! - **Least Connections**: Selects instance with fewest active connections
//! - **Consistent Hash (IP Hash)**: Deterministic selection based on a key (e.g., client IP)

use octopus_core::{LoadBalanceStrategy, UpstreamInstance};
use std::sync::atomic::{AtomicU64, Ordering};

/// Trait for load balancing algorithms.
///
/// Implementations must be thread-safe (`Send + Sync`) as the router
/// is shared across worker threads.
pub trait LoadBalancer: Send + Sync + std::fmt::Debug {
    /// Select an upstream instance from the given list.
    ///
    /// `instances` should be pre-filtered to only contain healthy instances.
    /// `key` is an optional hint (e.g., client IP) used by hash-based strategies.
    /// Returns the index of the selected instance, or `None` if the list is empty.
    fn select(&self, instances: &[&UpstreamInstance], key: &str) -> Option<usize>;
}

/// Create a load balancer for the given strategy.
pub fn new_load_balancer(strategy: LoadBalanceStrategy) -> Box<dyn LoadBalancer> {
    match strategy {
        LoadBalanceStrategy::RoundRobin => Box::new(RoundRobinLB::new()),
        LoadBalanceStrategy::WeightedRoundRobin => Box::new(WeightedRoundRobinLB::new()),
        LoadBalanceStrategy::Random => Box::new(RandomLB),
        LoadBalanceStrategy::LeastConnections => Box::new(LeastConnectionsLB),
        LoadBalanceStrategy::IpHash => Box::new(ConsistentHashLB),
    }
}

// ---------------------------------------------------------------------------
// Round Robin
// ---------------------------------------------------------------------------

/// Simple round-robin load balancer.
#[derive(Debug)]
pub struct RoundRobinLB {
    counter: AtomicU64,
}

impl RoundRobinLB {
    /// Create a new round-robin load balancer.
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }
}

impl LoadBalancer for RoundRobinLB {
    fn select(&self, instances: &[&UpstreamInstance], _key: &str) -> Option<usize> {
        if instances.is_empty() {
            return None;
        }
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) as usize % instances.len();
        Some(idx)
    }
}

// ---------------------------------------------------------------------------
// Weighted Round Robin
// ---------------------------------------------------------------------------

/// Weighted round-robin load balancer.
///
/// Distributes traffic proportionally to instance weights.
/// An instance with weight 3 receives 3x the traffic of weight 1.
#[derive(Debug)]
pub struct WeightedRoundRobinLB {
    counter: AtomicU64,
}

impl WeightedRoundRobinLB {
    /// Create a new weighted round-robin load balancer.
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }
}

impl LoadBalancer for WeightedRoundRobinLB {
    fn select(&self, instances: &[&UpstreamInstance], _key: &str) -> Option<usize> {
        if instances.is_empty() {
            return None;
        }

        let total_weight: u64 = instances.iter().map(|i| i.weight as u64).sum();
        if total_weight == 0 {
            // Fallback to simple round-robin if all weights are zero
            let idx = self.counter.fetch_add(1, Ordering::Relaxed) as usize % instances.len();
            return Some(idx);
        }

        let pos = self.counter.fetch_add(1, Ordering::Relaxed) % total_weight;
        let mut cumulative: u64 = 0;

        for (i, inst) in instances.iter().enumerate() {
            cumulative += inst.weight as u64;
            if pos < cumulative {
                return Some(i);
            }
        }

        // Should not reach here, but fallback to last instance
        Some(instances.len() - 1)
    }
}

// ---------------------------------------------------------------------------
// Random
// ---------------------------------------------------------------------------

/// Random load balancer.
#[derive(Debug)]
pub struct RandomLB;

impl LoadBalancer for RandomLB {
    fn select(&self, instances: &[&UpstreamInstance], _key: &str) -> Option<usize> {
        if instances.is_empty() {
            return None;
        }
        // Simple fast random using thread-local RNG
        Some(fastrand_index(instances.len()))
    }
}

/// Fast pseudo-random index using a simple xorshift on thread ID + counter.
/// Avoids pulling in `rand` crate for a simple use case.
fn fastrand_index(len: usize) -> usize {
    use std::cell::Cell;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    thread_local! {
        static STATE: Cell<u64> = Cell::new({
            let mut h = DefaultHasher::new();
            std::thread::current().id().hash(&mut h);
            let s = h.finish();
            if s == 0 { 1 } else { s }
        });
    }

    STATE.with(|state| {
        let mut s = state.get();
        // xorshift64
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        state.set(s);
        (s as usize) % len
    })
}

// ---------------------------------------------------------------------------
// Least Connections
// ---------------------------------------------------------------------------

/// Least-connections load balancer.
///
/// Selects the instance with the fewest active connections.
/// Ties are broken by selecting the first instance found.
#[derive(Debug)]
pub struct LeastConnectionsLB;

impl LoadBalancer for LeastConnectionsLB {
    fn select(&self, instances: &[&UpstreamInstance], _key: &str) -> Option<usize> {
        if instances.is_empty() {
            return None;
        }

        let mut min_conns = u32::MAX;
        let mut min_idx = 0;

        for (i, inst) in instances.iter().enumerate() {
            let conns = inst.active_connections();
            if conns < min_conns {
                min_conns = conns;
                min_idx = i;
            }
        }

        Some(min_idx)
    }
}

// ---------------------------------------------------------------------------
// Consistent Hash (IP Hash)
// ---------------------------------------------------------------------------

/// Consistent hash load balancer using FNV-1a.
///
/// Given the same key (e.g., client IP) and the same set of healthy instances,
/// always selects the same instance. Useful for session affinity.
#[derive(Debug)]
pub struct ConsistentHashLB;

impl LoadBalancer for ConsistentHashLB {
    fn select(&self, instances: &[&UpstreamInstance], key: &str) -> Option<usize> {
        if instances.is_empty() {
            return None;
        }

        if key.is_empty() {
            // Fallback to first instance when no key is provided
            return Some(0);
        }

        let hash = fnv1a_32(key.as_bytes());
        Some((hash as usize) % instances.len())
    }
}

/// FNV-1a 32-bit hash function.
fn fnv1a_32(data: &[u8]) -> u32 {
    const FNV_OFFSET: u32 = 2_166_136_261;
    const FNV_PRIME: u32 = 16_777_619;

    let mut hash = FNV_OFFSET;
    for &byte in data {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_core::UpstreamInstance;

    fn make_instances(count: usize) -> Vec<UpstreamInstance> {
        (0..count)
            .map(|i| UpstreamInstance::new(format!("inst-{i}"), "127.0.0.1", 8080 + i as u16))
            .collect()
    }

    fn make_weighted_instances(weights: &[u32]) -> Vec<UpstreamInstance> {
        weights
            .iter()
            .enumerate()
            .map(|(i, &w)| {
                let mut inst =
                    UpstreamInstance::new(format!("inst-{i}"), "127.0.0.1", 8080 + i as u16);
                inst.weight = w;
                inst
            })
            .collect()
    }

    fn refs(instances: &[UpstreamInstance]) -> Vec<&UpstreamInstance> {
        instances.iter().collect()
    }

    // ---- Round Robin ----

    #[test]
    fn test_round_robin_cycles_through_instances() {
        let lb = RoundRobinLB::new();
        let instances = make_instances(3);
        let r = refs(&instances);

        let mut selected = Vec::new();
        for _ in 0..6 {
            selected.push(lb.select(&r, "").unwrap());
        }
        assert_eq!(selected, vec![0, 1, 2, 0, 1, 2]);
    }

    #[test]
    fn test_round_robin_wraps_around() {
        let lb = RoundRobinLB::new();
        let instances = make_instances(2);
        let r = refs(&instances);

        for _ in 0..100 {
            let idx = lb.select(&r, "").unwrap();
            assert!(idx < 2);
        }
    }

    #[test]
    fn test_round_robin_empty_returns_none() {
        let lb = RoundRobinLB::new();
        let empty: Vec<&UpstreamInstance> = vec![];
        assert_eq!(lb.select(&empty, ""), None);
    }

    // ---- Weighted Round Robin ----

    #[test]
    fn test_weighted_rr_respects_weights() {
        let lb = WeightedRoundRobinLB::new();
        let instances = make_weighted_instances(&[3, 1]);
        let r = refs(&instances);

        let mut counts = [0u32; 2];
        // Total weight = 4, so over 400 iterations we expect ~300/100 distribution
        for _ in 0..400 {
            let idx = lb.select(&r, "").unwrap();
            counts[idx] += 1;
        }
        // inst-0 (weight 3) should get ~75%
        assert!(counts[0] == 300, "Expected 300, got {}", counts[0]);
        assert!(counts[1] == 100, "Expected 100, got {}", counts[1]);
    }

    #[test]
    fn test_weighted_rr_zero_weight_skipped() {
        let lb = WeightedRoundRobinLB::new();
        let instances = make_weighted_instances(&[0, 0, 0]);
        let r = refs(&instances);

        // All zero weights → fallback to simple round-robin
        let idx = lb.select(&r, "");
        assert!(idx.is_some());
    }

    #[test]
    fn test_weighted_rr_single_instance() {
        let lb = WeightedRoundRobinLB::new();
        let instances = make_weighted_instances(&[5]);
        let r = refs(&instances);

        for _ in 0..10 {
            assert_eq!(lb.select(&r, "").unwrap(), 0);
        }
    }

    // ---- Random ----

    #[test]
    fn test_random_selects_from_healthy() {
        let lb = RandomLB;
        let instances = make_instances(5);
        let r = refs(&instances);

        for _ in 0..100 {
            let idx = lb.select(&r, "").unwrap();
            assert!(idx < 5);
        }
    }

    #[test]
    fn test_random_empty_returns_none() {
        let lb = RandomLB;
        let empty: Vec<&UpstreamInstance> = vec![];
        assert_eq!(lb.select(&empty, ""), None);
    }

    // ---- Least Connections ----

    #[test]
    fn test_least_connections_picks_minimum() {
        let lb = LeastConnectionsLB;
        let instances = make_instances(3);
        // inst-0: 5 conns, inst-1: 2 conns, inst-2: 8 conns
        for _ in 0..5 {
            instances[0].increment_connections();
        }
        for _ in 0..2 {
            instances[1].increment_connections();
        }
        for _ in 0..8 {
            instances[2].increment_connections();
        }

        let r = refs(&instances);
        assert_eq!(lb.select(&r, "").unwrap(), 1); // inst-1 has fewest
    }

    #[test]
    fn test_least_connections_tie_breaking() {
        let lb = LeastConnectionsLB;
        let instances = make_instances(3);
        // All have 0 connections → first one wins

        let r = refs(&instances);
        assert_eq!(lb.select(&r, "").unwrap(), 0);
    }

    #[test]
    fn test_least_connections_empty_returns_none() {
        let lb = LeastConnectionsLB;
        let empty: Vec<&UpstreamInstance> = vec![];
        assert_eq!(lb.select(&empty, ""), None);
    }

    // ---- Consistent Hash ----

    #[test]
    fn test_consistent_hash_same_key_same_target() {
        let lb = ConsistentHashLB;
        let instances = make_instances(5);
        let r = refs(&instances);

        let idx1 = lb.select(&r, "192.168.1.100").unwrap();
        let idx2 = lb.select(&r, "192.168.1.100").unwrap();
        assert_eq!(idx1, idx2);
    }

    #[test]
    fn test_consistent_hash_different_keys_distribute() {
        let lb = ConsistentHashLB;
        let instances = make_instances(10);
        let r = refs(&instances);

        let mut seen = std::collections::HashSet::new();
        for i in 0..100 {
            let key = format!("10.0.0.{i}");
            let idx = lb.select(&r, &key).unwrap();
            seen.insert(idx);
        }
        // With 100 different IPs and 10 instances, we should hit multiple targets
        assert!(seen.len() > 1, "Hash should distribute across instances");
    }

    #[test]
    fn test_consistent_hash_empty_key_fallback() {
        let lb = ConsistentHashLB;
        let instances = make_instances(3);
        let r = refs(&instances);

        assert_eq!(lb.select(&r, "").unwrap(), 0);
    }

    #[test]
    fn test_consistent_hash_empty_returns_none() {
        let lb = ConsistentHashLB;
        let empty: Vec<&UpstreamInstance> = vec![];
        assert_eq!(lb.select(&empty, ""), None);
    }

    // ---- Factory ----

    #[test]
    fn test_factory_creates_correct_type() {
        let _ = new_load_balancer(LoadBalanceStrategy::RoundRobin);
        let _ = new_load_balancer(LoadBalanceStrategy::WeightedRoundRobin);
        let _ = new_load_balancer(LoadBalanceStrategy::Random);
        let _ = new_load_balancer(LoadBalanceStrategy::LeastConnections);
        let _ = new_load_balancer(LoadBalanceStrategy::IpHash);
    }

    // ---- Cross-strategy: empty ----

    #[test]
    fn test_all_strategies_empty_instances_returns_none() {
        let empty: Vec<&UpstreamInstance> = vec![];
        let strategies = vec![
            new_load_balancer(LoadBalanceStrategy::RoundRobin),
            new_load_balancer(LoadBalanceStrategy::WeightedRoundRobin),
            new_load_balancer(LoadBalanceStrategy::Random),
            new_load_balancer(LoadBalanceStrategy::LeastConnections),
            new_load_balancer(LoadBalanceStrategy::IpHash),
        ];
        for lb in &strategies {
            assert_eq!(lb.select(&empty, "key"), None);
        }
    }
}
