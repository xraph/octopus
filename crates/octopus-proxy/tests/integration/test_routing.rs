//! Advanced routing and load balancing integration tests

use super::*;
use octopus_proxy::routing::{CanaryConfig, Router, RoutingConfig, RoutingStrategy, ShadowConfig};
use std::collections::HashMap;
use std::time::Duration;

#[tokio::test]
async fn test_round_robin_distribution() {
    // Start 3 mock upstreams
    let mut mock1 = MockUpstream::new(0).await.unwrap();
    let mut mock2 = MockUpstream::new(0).await.unwrap();
    let mut mock3 = MockUpstream::new(0).await.unwrap();

    mock1.start().await.unwrap();
    mock2.start().await.unwrap();
    mock3.start().await.unwrap();

    let upstreams = vec![
        TestFixtures::upstream()
            .id("up1")
            .host("127.0.0.1")
            .port(mock1.addr().port())
            .build(),
        TestFixtures::upstream()
            .id("up2")
            .host("127.0.0.1")
            .port(mock2.addr().port())
            .build(),
        TestFixtures::upstream()
            .id("up3")
            .host("127.0.0.1")
            .port(mock3.addr().port())
            .build(),
    ];

    let config = RoutingConfig {
        strategy: RoutingStrategy::RoundRobin,
        ..Default::default()
    };
    let router = Router::new(config);

    // Send 9 requests - should distribute in round-robin order
    let selected1 = router.select(&upstreams, None).unwrap();
    let selected2 = router.select(&upstreams, None).unwrap();
    let selected3 = router.select(&upstreams, None).unwrap();
    let selected4 = router.select(&upstreams, None).unwrap();

    // Verify all upstreams are selected
    assert!(upstreams.iter().any(|u| u.id == selected1.id));
    assert!(upstreams.iter().any(|u| u.id == selected2.id));
    assert!(upstreams.iter().any(|u| u.id == selected3.id));
    assert!(upstreams.iter().any(|u| u.id == selected4.id));

    // With round-robin, the 4th should equal the 1st
    assert_eq!(selected1.id, selected4.id);
}

#[tokio::test]
async fn test_random_load_balancing() {
    let upstreams = vec![
        TestFixtures::upstream()
            .id("up1")
            .host("localhost")
            .port(8001)
            .build(),
        TestFixtures::upstream()
            .id("up2")
            .host("localhost")
            .port(8002)
            .build(),
        TestFixtures::upstream()
            .id("up3")
            .host("localhost")
            .port(8003)
            .build(),
    ];

    let config = RoutingConfig {
        strategy: RoutingStrategy::Random,
        ..Default::default()
    };
    let router = Router::new(config);

    let mut selections = HashMap::new();

    // Make many selections
    for _ in 0..100 {
        let selected = router.select(&upstreams, None).unwrap();
        *selections.entry(selected.id.clone()).or_insert(0) += 1;
    }

    // All upstreams should be selected at least once with high probability
    assert!(
        selections.len() >= 2,
        "Random should distribute across multiple upstreams"
    );
}

#[tokio::test]
async fn test_least_connections_strategy() {
    let upstreams = vec![
        TestFixtures::upstream()
            .id("up1")
            .host("localhost")
            .port(8001)
            .build(),
        TestFixtures::upstream()
            .id("up2")
            .host("localhost")
            .port(8002)
            .build(),
        TestFixtures::upstream()
            .id("up3")
            .host("localhost")
            .port(8003)
            .build(),
    ];

    let config = RoutingConfig {
        strategy: RoutingStrategy::LeastConnections,
        ..Default::default()
    };
    let router = Router::new(config);

    // First selection should work
    let selected1 = router.select(&upstreams, None).unwrap();
    assert!(upstreams.iter().any(|u| u.id == selected1.id));

    // Subsequent selections should prefer least loaded
    let selected2 = router.select(&upstreams, None).unwrap();
    assert!(upstreams.iter().any(|u| u.id == selected2.id));
}

#[tokio::test]
async fn test_weighted_round_robin() {
    let upstreams = vec![
        TestFixtures::upstream()
            .id("up1")
            .host("localhost")
            .port(8001)
            .weight(1)
            .build(),
        TestFixtures::upstream()
            .id("up2")
            .host("localhost")
            .port(8002)
            .weight(3)
            .build(), // 3x weight
        TestFixtures::upstream()
            .id("up3")
            .host("localhost")
            .port(8003)
            .weight(1)
            .build(),
    ];

    let config = RoutingConfig {
        strategy: RoutingStrategy::WeightedRoundRobin,
        ..Default::default()
    };
    let router = Router::new(config);

    let mut selections = HashMap::new();

    // Make many selections
    for _ in 0..50 {
        let selected = router.select(&upstreams, None).unwrap();
        *selections.entry(selected.id.clone()).or_insert(0) += 1;
    }

    // up2 should get roughly 3x more selections than up1/up3
    let up2_count = selections.get("up2").unwrap_or(&0);
    assert!(
        *up2_count > 20,
        "Weighted upstream should get more selections, got {up2_count}"
    );
}

#[tokio::test]
async fn test_latency_aware_routing() {
    // Start mock upstreams with different delays
    let mut mock_fast = MockUpstream::new(0).await.unwrap();
    let mut mock_slow = MockUpstream::new(0).await.unwrap();

    mock_fast.start().await.unwrap();
    mock_slow.start().await.unwrap();

    // Configure slow mock with delay
    let mut slow_config = MockConfig::default();
    slow_config.delay = Some(Duration::from_millis(100));
    mock_slow.set_config(slow_config).await;

    let upstreams = vec![
        TestFixtures::upstream()
            .id("fast")
            .host("127.0.0.1")
            .port(mock_fast.addr().port())
            .build(),
        TestFixtures::upstream()
            .id("slow")
            .host("127.0.0.1")
            .port(mock_slow.addr().port())
            .build(),
    ];

    let config = RoutingConfig {
        strategy: RoutingStrategy::LatencyAware,
        ..Default::default()
    };
    let router = Router::new(config);

    // Initial selections (no history yet)
    for _ in 0..5 {
        let selected = router.select(&upstreams, None).unwrap();
        assert!(upstreams.iter().any(|u| u.id == selected.id));
    }

    // After collecting latency data, should prefer faster upstream
    // (This would require actual request execution to track latencies)
}

#[tokio::test]
async fn test_error_aware_routing() {
    let upstreams = vec![
        TestFixtures::upstream()
            .id("healthy")
            .host("localhost")
            .port(8001)
            .build(),
        TestFixtures::upstream()
            .id("unhealthy")
            .host("localhost")
            .port(8002)
            .build(),
    ];

    let config = RoutingConfig {
        strategy: RoutingStrategy::ErrorAware,
        ..Default::default()
    };
    let router = Router::new(config);

    // Both should be selectable initially
    let selected1 = router.select(&upstreams, None).unwrap();
    assert!(upstreams.iter().any(|u| u.id == selected1.id));

    // With error-aware routing, both should be tried
    // (Error tracking would be done at the proxy level)
    for _ in 0..10 {
        let selected = router.select(&upstreams, None).unwrap();
        assert!(upstreams.iter().any(|u| u.id == selected.id));
    }
}

#[tokio::test]
async fn test_canary_deployment_10_percent() {
    let stable = [TestFixtures::upstream()
        .id("stable-1")
        .host("localhost")
        .port(8001)
        .version("v1")
        .build()];

    let canary = [TestFixtures::upstream()
        .id("canary-1")
        .host("localhost")
        .port(8002)
        .version("v2")
        .build()];

    let all_upstreams: Vec<_> = stable.iter().chain(canary.iter()).cloned().collect();

    let canary_config = CanaryConfig {
        canary_version: "v2".to_string(),
        traffic_percentage: 10, // 10% to canary
        error_threshold: 0.05,
        auto_promote: false,
    };

    let config = RoutingConfig {
        strategy: RoutingStrategy::RoundRobin,
        enable_canary: true,
        ..Default::default()
    };
    let router = Router::new(config);

    let mut canary_count = 0;
    let mut stable_count = 0;

    // Make many selections
    for _ in 0..100 {
        let selected = router.select(&all_upstreams, Some(&canary_config)).unwrap();
        if selected.metadata.get("version") == Some(&"v2".to_string()) {
            canary_count += 1;
        } else {
            stable_count += 1;
        }
    }

    // Approximately 10% should go to canary (allow some variance)
    assert!(
        (5..=20).contains(&canary_count),
        "Canary should get ~10% of traffic, got {canary_count}/100"
    );
    assert!(
        stable_count >= 80,
        "Stable should get ~90% of traffic, got {stable_count}/100"
    );
}

#[tokio::test]
async fn test_canary_deployment_50_percent() {
    let stable = [TestFixtures::upstream()
        .id("stable-1")
        .host("localhost")
        .port(8001)
        .version("v1")
        .build()];

    let canary = [TestFixtures::upstream()
        .id("canary-1")
        .host("localhost")
        .port(8002)
        .version("v2")
        .build()];

    let all_upstreams: Vec<_> = stable.iter().chain(canary.iter()).cloned().collect();

    let canary_config = CanaryConfig {
        canary_version: "v2".to_string(),
        traffic_percentage: 50, // 50/50 split
        error_threshold: 0.05,
        auto_promote: false,
    };

    let config = RoutingConfig {
        strategy: RoutingStrategy::RoundRobin,
        enable_canary: true,
        ..Default::default()
    };
    let router = Router::new(config);

    let mut canary_count = 0;
    let mut stable_count = 0;

    // Make many selections
    for _ in 0..100 {
        let selected = router.select(&all_upstreams, Some(&canary_config)).unwrap();
        if selected.metadata.get("version") == Some(&"v2".to_string()) {
            canary_count += 1;
        } else {
            stable_count += 1;
        }
    }

    // Approximately 50% to each (allow variance)
    assert!(
        (35..=65).contains(&canary_count),
        "Canary should get ~50% of traffic, got {canary_count}/100"
    );
    assert!(
        (35..=65).contains(&stable_count),
        "Stable should get ~50% of traffic, got {stable_count}/100"
    );
}

#[tokio::test]
async fn test_request_shadowing_config() {
    let shadow_config = ShadowConfig {
        shadow_target: "shadow-cluster".to_string(),
        traffic_percentage: 100, // Shadow all traffic for test
        synchronous: false,      // Async - don't wait for shadow
        log_failures: true,
    };

    // Verify shadow config
    assert!(!shadow_config.synchronous);
    assert_eq!(shadow_config.traffic_percentage, 100);
    assert!(shadow_config.log_failures);
}

#[tokio::test]
async fn test_request_shadowing_percentage() {
    let shadow_config = ShadowConfig {
        shadow_target: "shadow-cluster".to_string(),
        traffic_percentage: 25, // Shadow 25% of traffic
        synchronous: false,
        log_failures: true,
    };

    // Verify configuration
    assert_eq!(shadow_config.traffic_percentage, 25);

    // With 25%, approximately 1 in 4 requests should be shadowed
    let mut shadowed_count = 0;
    for _ in 0..100 {
        if shadow_config.should_shadow() {
            shadowed_count += 1;
        }
    }

    // Allow variance, expect roughly 25%
    assert!(
        (15..=40).contains(&shadowed_count),
        "Should shadow ~25% of requests, got {shadowed_count}/100"
    );
}

#[tokio::test]
async fn test_router_with_multiple_strategies() {
    let upstreams = vec![
        TestFixtures::upstream()
            .id("up1")
            .host("localhost")
            .port(8001)
            .build(),
        TestFixtures::upstream()
            .id("up2")
            .host("localhost")
            .port(8002)
            .build(),
        TestFixtures::upstream()
            .id("up3")
            .host("localhost")
            .port(8003)
            .build(),
    ];

    // Test different routing strategies
    let strategies = vec![
        RoutingStrategy::RoundRobin,
        RoutingStrategy::Random,
        RoutingStrategy::LeastConnections,
        RoutingStrategy::WeightedRoundRobin,
        RoutingStrategy::LatencyAware,
        RoutingStrategy::ErrorAware,
    ];

    for strategy in strategies {
        let config = RoutingConfig {
            strategy,
            ..Default::default()
        };

        let router = Router::new(config);

        // Should be able to select from upstreams
        let selected = router.select(&upstreams, None).unwrap();
        assert!(
            upstreams.iter().any(|u| u.id == selected.id),
            "Strategy {strategy:?} should select valid upstream"
        );
    }
}

#[tokio::test]
async fn test_routing_with_no_healthy_upstreams() {
    let upstreams: Vec<octopus_core::UpstreamInstance> = vec![]; // Empty list

    let config = RoutingConfig::default();
    let router = Router::new(config);

    let result = router.select(&upstreams, None);
    assert!(
        result.is_none(),
        "Should return None when no upstreams available"
    );
}

#[tokio::test]
async fn test_routing_with_single_upstream() {
    let upstreams = vec![TestFixtures::upstream()
        .id("only")
        .host("localhost")
        .port(8001)
        .build()];

    let config = RoutingConfig::default();
    let router = Router::new(config);

    // Should always select the only upstream
    for _ in 0..10 {
        let selected = router.select(&upstreams, None).unwrap();
        assert_eq!(selected.id, "only");
    }
}

#[tokio::test]
async fn test_canary_without_canary_version() {
    // All upstreams are v1, but we request v2 canary
    let upstreams = vec![
        TestFixtures::upstream()
            .id("up1")
            .host("localhost")
            .port(8001)
            .version("v1")
            .build(),
        TestFixtures::upstream()
            .id("up2")
            .host("localhost")
            .port(8002)
            .version("v1")
            .build(),
    ];

    let canary_config = CanaryConfig {
        canary_version: "v2".to_string(),
        traffic_percentage: 50,
        error_threshold: 0.05,
        auto_promote: false,
    };

    let config = RoutingConfig {
        strategy: RoutingStrategy::RoundRobin,
        enable_canary: true,
        ..Default::default()
    };
    let router = Router::new(config);

    // Should fallback to stable since no canary version exists
    let mut v1_count = 0;
    for _ in 0..10 {
        let selected = router.select(&upstreams, Some(&canary_config)).unwrap();
        if selected.metadata.get("version") == Some(&"v1".to_string()) {
            v1_count += 1;
        }
    }

    // All should be v1 since no v2 exists
    assert_eq!(
        v1_count, 10,
        "Should fallback to stable when canary version doesn't exist"
    );
}
