//! Integration tests for mDNS discovery
//!
//! These tests verify that mDNS discovery works correctly despite
//! VPN interface errors on macOS.

#[cfg(feature = "mdns")]
mod mdns_integration_tests {
    use octopus_discovery::mdns::{MdnsConfig, MdnsDiscovery};
    use octopus_discovery::provider::DiscoveryProvider;
    use std::time::Duration;

    #[tokio::test]
    async fn test_mdns_config_defaults() {
        let config = MdnsConfig::default();
        
        assert_eq!(config.service_type, "_octopus._tcp");
        assert_eq!(config.domain, "local.");
        assert_eq!(config.watch_interval, Duration::from_secs(30));
        assert_eq!(config.query_timeout, Duration::from_secs(5));
        
        // IPv6 should be disabled on macOS to avoid VPN errors
        #[cfg(target_os = "macos")]
        assert_eq!(config.enable_ipv6, false);
        
        #[cfg(not(target_os = "macos"))]
        assert_eq!(config.enable_ipv6, true);
    }

    #[tokio::test]
    async fn test_mdns_config_full_service_name() {
        let config = MdnsConfig::default();
        assert_eq!(config.full_service_name(), "_octopus._tcp.local.");
        
        let custom = MdnsConfig::new("_myservice._tcp")
            .with_domain("example.local.");
        assert_eq!(custom.full_service_name(), "_myservice._tcp.example.local.");
    }

    #[tokio::test]
    async fn test_mdns_config_builder() {
        let config = MdnsConfig::new("_test._tcp")
            .with_domain("test.local.")
            .with_watch_interval(Duration::from_secs(60))
            .with_ipv6(false);
        
        assert_eq!(config.service_type, "_test._tcp");
        assert_eq!(config.domain, "test.local.");
        assert_eq!(config.watch_interval, Duration::from_secs(60));
        assert_eq!(config.enable_ipv6, false);
    }

    #[tokio::test]
    async fn test_mdns_discovery_creation() {
        let config = MdnsConfig::default();
        let discovery = MdnsDiscovery::new(config);
        
        assert_eq!(discovery.name(), "mdns");
    }

    #[tokio::test]
    async fn test_mdns_discover_services_no_services() {
        // This test will show VPN errors on macOS but should complete successfully
        let discovery = MdnsDiscovery::with_defaults();
        
        // Discovery should succeed even if no services are found
        let result = discovery.discover_services().await;
        
        // Should not return an error, even with VPN interface errors
        assert!(result.is_ok(), "Discovery should succeed despite VPN errors");
        
        let services = result.unwrap();
        // May or may not find services depending on network
        println!("Found {} services", services.len());
    }

    #[tokio::test]
    async fn test_mdns_discover_specific_service() {
        let discovery = MdnsDiscovery::with_defaults();
        
        // Should succeed even if service not found
        let result = discovery.discover_service("nonexistent-service").await;
        assert!(result.is_ok(), "Discovery should succeed even when service not found");
        
        let services = result.unwrap();
        // Should return empty list for nonexistent service
        assert_eq!(services.len(), 0, "Should return empty list for nonexistent service");
    }

    // Note: parse_txt_records and parse_endpoints are private methods
    // They are tested indirectly through discover_services

    #[tokio::test(flavor = "multi_thread")]
    async fn test_mdns_discovery_with_timeout() {
        // Test that discovery completes within reasonable time
        // even with VPN errors
        let discovery = MdnsDiscovery::with_defaults();
        
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            discovery.discover_services()
        ).await;
        
        let elapsed = start.elapsed();
        
        assert!(result.is_ok(), "Discovery should complete within 10 seconds");
        assert!(result.unwrap().is_ok(), "Discovery should succeed");
        
        println!("Discovery completed in {:?}", elapsed);
        
        // Should complete within query timeout + buffer
        assert!(elapsed < Duration::from_secs(10), "Discovery should be fast");
    }

    #[tokio::test]
    async fn test_mdns_ipv6_disabled_on_macos() {
        // Verify IPv6 is disabled on macOS to reduce VPN errors
        #[cfg(target_os = "macos")]
        {
            let config = MdnsConfig::default();
            assert_eq!(config.enable_ipv6, false, 
                "IPv6 should be disabled on macOS to avoid VPN tunnel errors");
        }
        
        #[cfg(not(target_os = "macos"))]
        {
            let config = MdnsConfig::default();
            assert_eq!(config.enable_ipv6, true,
                "IPv6 should be enabled on non-macOS platforms");
        }
    }

    #[tokio::test]
    async fn test_mdns_error_handling_graceful() {
        // Test that VPN interface errors don't crash the application
        let discovery = MdnsDiscovery::with_defaults();
        
        // Multiple discovery calls should all succeed
        for i in 0..3 {
            let result = discovery.discover_services().await;
            assert!(result.is_ok(), 
                "Discovery attempt {} should succeed despite VPN errors", i + 1);
        }
    }

    #[tokio::test]
    async fn test_mdns_multiple_sequential_discoveries() {
        // Test multiple sequential discoveries work correctly
        let discovery = MdnsDiscovery::with_defaults();
        
        // Run multiple discoveries sequentially
        for i in 0..3 {
            let result = discovery.discover_services().await;
            assert!(result.is_ok(), 
                "Sequential discovery {} should succeed despite VPN errors", i + 1);
        }
    }
}

#[cfg(not(feature = "mdns"))]
mod mdns_feature_disabled {
    #[test]
    fn test_mdns_feature_not_enabled() {
        // This test ensures the test suite still runs when mdns feature is disabled
        assert!(true, "mDNS feature is not enabled");
    }
}

