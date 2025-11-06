//! Prometheus metrics exporter

use crate::collector::MetricsCollector;
use std::fmt::Write;

/// Prometheus metrics exporter
pub struct PrometheusExporter;

impl PrometheusExporter {
    /// Export metrics in Prometheus text format
    pub fn export(collector: &MetricsCollector) -> String {
        let mut output = String::with_capacity(4096);

        // Add HELP and TYPE comments for each metric
        Self::write_header(&mut output);

        // Gateway-level metrics
        Self::write_gateway_metrics(&mut output, collector);

        // Per-route metrics
        Self::write_route_metrics(&mut output, collector);

        output
    }

    fn write_header(output: &mut String) {
        writeln!(
            output,
            "# HELP octopus_requests_total Total number of HTTP requests"
        )
        .unwrap();
        writeln!(output, "# TYPE octopus_requests_total counter").unwrap();

        writeln!(
            output,
            "# HELP octopus_requests_duration_seconds HTTP request duration in seconds"
        )
        .unwrap();
        writeln!(
            output,
            "# TYPE octopus_requests_duration_seconds histogram"
        )
        .unwrap();

        writeln!(
            output,
            "# HELP octopus_active_connections Current number of active connections"
        )
        .unwrap();
        writeln!(output, "# TYPE octopus_active_connections gauge").unwrap();

        writeln!(
            output,
            "# HELP octopus_route_requests_total Total number of requests per route"
        )
        .unwrap();
        writeln!(output, "# TYPE octopus_route_requests_total counter").unwrap();

        writeln!(
            output,
            "# HELP octopus_route_errors_total Total number of errors per route"
        )
        .unwrap();
        writeln!(output, "# TYPE octopus_route_errors_total counter").unwrap();

        writeln!(
            output,
            "# HELP octopus_route_latency_seconds Average latency per route in seconds"
        )
        .unwrap();
        writeln!(output, "# TYPE octopus_route_latency_seconds gauge").unwrap();
    }

    fn write_gateway_metrics(output: &mut String, collector: &MetricsCollector) {
        // Total requests
        writeln!(
            output,
            "octopus_requests_total {{}} {}",
            collector.total_requests()
        )
        .unwrap();

        // Active connections
        writeln!(
            output,
            "octopus_active_connections {{}} {}",
            collector.active_connections()
        )
        .unwrap();

        // Global latency histogram (simplified buckets)
        let avg_latency_ms = collector.global_avg_latency_ms();
        let avg_latency_sec = avg_latency_ms / 1000.0;

        // Simplified histogram representation
        writeln!(
            output,
            "octopus_requests_duration_seconds_sum {{}} {:.6}",
            avg_latency_sec * collector.total_requests() as f64
        )
        .unwrap();
        writeln!(
            output,
            "octopus_requests_duration_seconds_count {{}} {}",
            collector.total_requests()
        )
        .unwrap();

        // Histogram buckets (standard Prometheus buckets)
        let buckets = [
            0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
        ];

        let mut cumulative = 0u64;
        for bucket in buckets {
            // Estimate count based on average latency
            // In a real implementation, you'd track actual bucket counts
            if avg_latency_sec <= bucket {
                cumulative = collector.total_requests();
            }
            writeln!(
                output,
                "octopus_requests_duration_seconds_bucket{{le=\"{}\"}} {}",
                bucket, cumulative
            )
            .unwrap();
        }

        writeln!(
            output,
            "octopus_requests_duration_seconds_bucket{{le=\"+Inf\"}} {}",
            collector.total_requests()
        )
        .unwrap();
    }

    fn write_route_metrics(output: &mut String, collector: &MetricsCollector) {
        // Get all routes from the route_count map
        let route_count = collector.route_count();
        
        // For now, we'll just export basic per-route metrics
        // A more complete implementation would iterate over all routes
        // For the initial version, we can output placeholder metrics
        
        // Since we don't have a way to iterate all routes, we'll skip per-route metrics for now
        // This can be enhanced later by adding an API to list all routes
        
        writeln!(output, "# Per-route metrics (count: {})", route_count).unwrap();
    }

    #[allow(dead_code)]
    fn sanitize_label(label: &str) -> String {
        // Replace characters that might cause issues in Prometheus labels
        label
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_label() {
        assert_eq!(
            PrometheusExporter::sanitize_label(r#"test"path"#),
            r#"test\"path"#
        );
        assert_eq!(
            PrometheusExporter::sanitize_label("test\\path"),
            "test\\\\path"
        );
    }

    #[test]
    fn test_export_empty_metrics() {
        let collector = MetricsCollector::new();
        let output = PrometheusExporter::export(&collector);

        // Should contain headers
        assert!(output.contains("# HELP octopus_requests_total"));
        assert!(output.contains("# TYPE octopus_requests_total counter"));
        assert!(output.contains("octopus_requests_total {} 0"));
    }

    #[test]
    fn test_export_format() {
        let collector = MetricsCollector::new();
        let output = PrometheusExporter::export(&collector);

        // Verify Prometheus text format
        assert!(output.contains("# HELP"));
        assert!(output.contains("# TYPE"));
        assert!(output.contains("octopus_"));
    }
}

