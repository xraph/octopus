//! Upstreams declared in config keep their `lb_policy` and instance weights
//! once the server registers them.

use octopus_core::LoadBalanceStrategy;
use octopus_runtime::ServerBuilder;

#[tokio::test]
async fn config_upstreams_keep_lb_policy_and_weights() {
    let yaml = r#"
gateway:
  listen: "127.0.0.1:0"
upstreams:
  - name: weighted
    lb_policy: weighted_round_robin
    instances:
      - {id: a, host: 127.0.0.1, port: 9001, weight: 3}
      - {id: b, host: 127.0.0.1, port: 9002}
  - name: least
    lb_policy: least_conn
    instances:
      - {id: c, host: 127.0.0.1, port: 9003}
  - name: plain
    instances:
      - {id: d, host: 127.0.0.1, port: 9004}
"#;
    let cfg = octopus_config::load_str(yaml, octopus_config::ConfigFormat::Yaml).unwrap();
    let server = ServerBuilder::new()
        .config(cfg)
        .enable_farp(false)
        .enable_plugins(false)
        .build()
        .await
        .unwrap();
    let router = server.router();

    let weighted = router.get_upstream("weighted").unwrap();
    assert_eq!(weighted.strategy, LoadBalanceStrategy::WeightedRoundRobin);
    let weights: Vec<u32> = weighted.instances.iter().map(|i| i.weight).collect();
    assert_eq!(weights, [3, 1]);

    assert_eq!(
        router.get_upstream("least").unwrap().strategy,
        LoadBalanceStrategy::LeastConnections
    );
    assert_eq!(
        router.get_upstream("plain").unwrap().strategy,
        LoadBalanceStrategy::RoundRobin
    );
}
