//! Demo showing how to use octopus-ui components in octopus-admin

use octopus_admin::models::{HealthCheckInfo, PluginInfo, RouteInfo};
use octopus_admin::ui_components;
use octopus_ui::core::{Node, Render, Size, Variant};
use octopus_ui::components::Button;
use octopus_ui::components::card::*;
use octopus_ui::primitives::{Grid, HStack, VStack};

fn main() {
    println!("=== Octopus UI Demo ===\n");

    // 1. Simple Badge
    println!("1. Status Badge:");
    let badge = ui_components::status_badge("Active", Variant::Default);
    println!("{}\n", badge);

    // 2. Method Badge
    println!("2. HTTP Method Badge:");
    let method_badge = ui_components::method_badge("POST");
    println!("{}\n", method_badge);

    // 3. Stats Card
    println!("3. Stats Card:");
    let stats_card = ui_components::stats_card(
        "Total Requests",
        "1,234,567",
        "+20.1% from last month",
        Some(r#"<svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 7h8m0 0v8m0-8l-8 8-4-4-6 6" /></svg>"#)
    );
    println!("{}\n", stats_card);

    // 4. Route Card
    println!("4. Route Card:");
    let route = RouteInfo {
        id: "route1".to_string(),
        path: "/api/users".to_string(),
        method: "GET".to_string(),
        upstream: "user-service:8080".to_string(),
        request_count: 45678,
        is_healthy: true,
        avg_latency_ms: 23.5,
        error_count: 12,
        last_accessed: Some("2024-01-15 10:30:00".to_string()),
    };
    let route_card = ui_components::route_card(&route);
    println!("{}\n", route_card);

    // 5. Health Check Card
    println!("5. Health Check Card:");
    let health_check = HealthCheckInfo {
        name: "PostgreSQL Database".to_string(),
        status: "passing".to_string(),
        response_time_ms: 12,
        message: Some("Connected successfully".to_string()),
        endpoint: Some("postgresql://localhost:5432/octopus".to_string()),
        last_check: "2024-01-15 10:30:00".to_string(),
        consecutive_failures: 0,
    };
    let health_card = ui_components::health_check_card(&health_check);
    println!("{}\n", health_card);

    // 6. Plugin Card
    println!("6. Plugin Card:");
    let plugin = PluginInfo {
        id: "auth-jwt".to_string(),
        name: "JWT Authentication".to_string(),
        version: "0.1.0".to_string(),
        description: "JWT token validation and authentication".to_string(),
        author: Some("Octopus Team".to_string()),
        enabled: true,
        has_dashboard: true,
        config: None,
    };
    let plugin_card = ui_components::plugin_card(&plugin);
    println!("{}\n", plugin_card);

    // 7. Navigation Item
    println!("7. Navigation Item:");
    let nav = ui_components::nav_item(
        "Dashboard",
        "/admin",
        true,
        Some(r#"<svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" /></svg>"#)
    );
    println!("{}\n", nav);

    // 8. Alert Box
    println!("8. Alert Box:");
    let alert = ui_components::alert_box(
        "Success",
        "Your changes have been saved successfully.",
        Variant::Default,
    );
    println!("{}\n", alert);

    // 9. HTMX Button
    println!("9. HTMX Button:");
    let htmx_btn = ui_components::htmx_button(
        "Refresh Stats",
        "GET",
        "/admin/api/stats",
        "#stats-container",
    );
    println!("{}\n", htmx_btn);

    // 10. Custom Component using octopus-ui primitives
    println!("10. Custom Dashboard Layout:");
    let dashboard = VStack::new()
        .gap("6")
        .class("p-6")
        .child(
            HStack::new()
                .gap("4")
                .class("items-center justify-between")
                .child(
                    Node::element("h1")
                        .attr("class", "text-3xl font-bold")
                        .child(Node::text("Dashboard"))
                )
                .child(
                    Button::with_text("Refresh")
                        .variant(Variant::Outline)
                        .size(Size::SM)
                        .render()
                )
                .render()
        )
        .child(
            Grid::new()
                .cols(3)
                .gap("4")
                .child(
                    Card::new()
                        .child(
                            CardHeader::new()
                                .child(CardTitle::new("Metric 1").render())
                                .render()
                        )
                        .child(
                            CardContent::new()
                                .child(
                                    Node::element("div")
                                        .attr("class", "text-2xl font-bold")
                                        .child(Node::text("1,234"))
                                )
                                .render()
                        )
                        .render()
                )
                .child(
                    Card::new()
                        .child(
                            CardHeader::new()
                                .child(CardTitle::new("Metric 2").render())
                                .render()
                        )
                        .child(
                            CardContent::new()
                                .child(
                                    Node::element("div")
                                        .attr("class", "text-2xl font-bold")
                                        .child(Node::text("5,678"))
                                )
                                .render()
                        )
                        .render()
                )
                .child(
                    Card::new()
                        .child(
                            CardHeader::new()
                                .child(CardTitle::new("Metric 3").render())
                                .render()
                        )
                        .child(
                            CardContent::new()
                                .child(
                                    Node::element("div")
                                        .attr("class", "text-2xl font-bold")
                                        .child(Node::text("9,012"))
                                )
                                .render()
                        )
                        .render()
                )
                .render()
        )
        .render_to_string();
    
    println!("{}\n", dashboard);

    // 11. Routes Table
    println!("11. Routes Table:");
    let routes = vec![
        RouteInfo {
            id: "route1".to_string(),
            path: "/api/users".to_string(),
            method: "GET".to_string(),
            upstream: "user-service:8080".to_string(),
            request_count: 45678,
            is_healthy: true,
            avg_latency_ms: 23.5,
            error_count: 12,
            last_accessed: Some("2024-01-15 10:30:00".to_string()),
        },
        RouteInfo {
            id: "route2".to_string(),
            path: "/api/products".to_string(),
            method: "POST".to_string(),
            upstream: "product-service:8080".to_string(),
            request_count: 23456,
            is_healthy: true,
            avg_latency_ms: 34.2,
            error_count: 8,
            last_accessed: Some("2024-01-15 10:25:00".to_string()),
        },
    ];
    let table = ui_components::routes_table(&routes);
    println!("{}\n", table);

    println!("=== Demo Complete ===");
}
