//! Pure Rust handlers using octopus-ui (no Askama templates)

use axum::{
    extract::State,
    response::{Html, IntoResponse},
};
use std::sync::Arc;

use crate::{handlers::AppState, models::RouteInfo, ui_components};
use octopus_ui::{
    core::{document, Node, Render},
    layouts::{admin_layout, admin_layout::icons, NavItem},
    primitives::{Grid, VStack},
};

/// Modern dashboard using pure Rust (no templates)
pub async fn octopus_ui_dashboard_handler(
    State(_state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Generate stats cards
    let stats_cards = format!(
        r#"<div class="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
            {}
            {}
            {}
            {}
        </div>"#,
        ui_components::stats_card(
            "Total Requests",
            "1,234,567",
            "+20.1% from last month",
            Some(icons::HOME)
        ),
        ui_components::stats_card(
            "Active Routes",
            "42",
            "+3 since last week",
            Some(icons::ROUTES)
        ),
        ui_components::stats_card(
            "Avg Latency",
            "45.6ms",
            "-5.2ms from yesterday",
            Some(
                r#"<svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" /></svg>"#
            )
        ),
        ui_components::stats_card(
            "Health Status",
            "Healthy",
            "All systems operational",
            Some(icons::HEALTH)
        ),
    );

    // Sample routes
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
            ..Default::default()
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
            ..Default::default()
        },
    ];

    let routes_table = ui_components::routes_table(&routes);

    // Build page content
    let content = VStack::new()
        .gap("6")
        .child(Node::raw(&stats_cards))
        .child(
            VStack::new()
                .gap("4")
                .child(
                    Node::element("div")
                        .attr("class", "flex items-center justify-between")
                        .child(
                            Node::element("h3")
                                .attr("class", "text-lg font-semibold")
                                .child(Node::text("Routes")),
                        )
                        .child(
                            Node::element("a")
                                .attr("href", "/admin/octopus-ui/routes")
                                .attr("class", "text-sm text-primary hover:underline")
                                .child(Node::text("View all →")),
                        ),
                )
                .child(Node::raw(&routes_table))
                .render(),
        )
        .render();

    // Build layout with navigation
    let layout = admin_layout("Dashboard")
        .nav_item(
            NavItem::new("Dashboard", "/admin/octopus-ui")
                .icon(icons::HOME.to_string())
                .active(true),
        )
        .nav_item(
            NavItem::new("Routes", "/admin/octopus-ui/routes").icon(icons::ROUTES.to_string()),
        )
        .nav_item(
            NavItem::new("Health", "/admin/octopus-ui/health").icon(icons::HEALTH.to_string()),
        )
        .nav_item(
            NavItem::new("Plugins", "/admin/octopus-ui/plugins").icon(icons::PLUGINS.to_string()),
        )
        .content(content)
        .build();

    // Build complete HTML document
    let page = document("Octopus Admin - Modern Dashboard")
        .html_class("h-full bg-background")
        .body_class("h-full")
        .stylesheet("/admin/static/output.css")
        .script("https://cdn.jsdelivr.net/npm/alpinejs@3.x.x/dist/cdn.min.js")
        .script("https://unpkg.com/htmx.org@1.9.10")
        .body(layout)
        .render_to_string();

    Html(page)
}

/// Routes page using pure Rust
pub async fn octopus_ui_routes_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
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
            ..Default::default()
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
            ..Default::default()
        },
    ];

    let routes_table = ui_components::routes_table(&routes);

    let routes_grid = routes
        .iter()
        .map(ui_components::route_card)
        .collect::<Vec<_>>()
        .join("\n");
    let routes_grid = format!(r#"<div class="grid gap-4 md:grid-cols-2">{routes_grid}</div>"#);

    let content = VStack::new()
        .gap("6")
        .child(
            VStack::new()
                .gap("4")
                .child(
                    Node::element("h3")
                        .attr("class", "text-lg font-semibold")
                        .child(Node::text("Table View")),
                )
                .child(Node::raw(&routes_table))
                .render(),
        )
        .child(
            VStack::new()
                .gap("4")
                .child(
                    Node::element("h3")
                        .attr("class", "text-lg font-semibold")
                        .child(Node::text("Grid View")),
                )
                .child(Node::raw(&routes_grid))
                .render(),
        )
        .render();

    let layout = admin_layout("Routes")
        .nav_item(NavItem::new("Dashboard", "/admin/octopus-ui").icon(icons::HOME.to_string()))
        .nav_item(
            NavItem::new("Routes", "/admin/octopus-ui/routes")
                .icon(icons::ROUTES.to_string())
                .active(true),
        )
        .nav_item(
            NavItem::new("Health", "/admin/octopus-ui/health").icon(icons::HEALTH.to_string()),
        )
        .nav_item(
            NavItem::new("Plugins", "/admin/octopus-ui/plugins").icon(icons::PLUGINS.to_string()),
        )
        .content(content)
        .build();

    let page = document("Routes - Octopus Admin")
        .html_class("h-full bg-background")
        .body_class("h-full")
        .stylesheet("/admin/static/output.css")
        .script("https://cdn.jsdelivr.net/npm/alpinejs@3.x.x/dist/cdn.min.js")
        .script("https://unpkg.com/htmx.org@1.9.10")
        .body(layout)
        .render_to_string();

    Html(page)
}

/// Health page using pure Rust
pub async fn octopus_ui_health_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let health_checks_html = r#"<p class="text-muted-foreground">Health checks will be displayed here once configured.</p>"#.to_string();
    let health_checks_grid =
        format!(r#"<div class="grid gap-4 md:grid-cols-2">{health_checks_html}</div>"#);

    let content = VStack::new()
        .gap("6")
        .child(
            Grid::new()
                .cols(3)
                .gap("4")
                .child(
                    Node::element("div")
                        .attr("class", "rounded-lg border p-4")
                        .child(
                            Node::element("div")
                                .attr("class", "text-sm text-muted-foreground")
                                .child(Node::text("24h Uptime")),
                        )
                        .child(
                            Node::element("div")
                                .attr("class", "text-2xl font-bold")
                                .child(Node::text("99.9%")),
                        ),
                )
                .child(
                    Node::element("div")
                        .attr("class", "rounded-lg border p-4")
                        .child(
                            Node::element("div")
                                .attr("class", "text-sm text-muted-foreground")
                                .child(Node::text("7d Uptime")),
                        )
                        .child(
                            Node::element("div")
                                .attr("class", "text-2xl font-bold")
                                .child(Node::text("99.8%")),
                        ),
                )
                .child(
                    Node::element("div")
                        .attr("class", "rounded-lg border p-4")
                        .child(
                            Node::element("div")
                                .attr("class", "text-sm text-muted-foreground")
                                .child(Node::text("30d Uptime")),
                        )
                        .child(
                            Node::element("div")
                                .attr("class", "text-2xl font-bold")
                                .child(Node::text("99.7%")),
                        ),
                )
                .render(),
        )
        .child(Node::raw(&health_checks_grid))
        .render();

    let layout = admin_layout("Health Checks")
        .nav_item(NavItem::new("Dashboard", "/admin/octopus-ui").icon(icons::HOME.to_string()))
        .nav_item(
            NavItem::new("Routes", "/admin/octopus-ui/routes").icon(icons::ROUTES.to_string()),
        )
        .nav_item(
            NavItem::new("Health", "/admin/octopus-ui/health")
                .icon(icons::HEALTH.to_string())
                .active(true),
        )
        .nav_item(
            NavItem::new("Plugins", "/admin/octopus-ui/plugins").icon(icons::PLUGINS.to_string()),
        )
        .content(content)
        .build();

    let page = document("Health - Octopus Admin")
        .html_class("h-full bg-background")
        .body_class("h-full")
        .stylesheet("/admin/static/output.css")
        .script("https://cdn.jsdelivr.net/npm/alpinejs@3.x.x/dist/cdn.min.js")
        .script("https://unpkg.com/htmx.org@1.9.10")
        .body(layout)
        .render_to_string();

    Html(page)
}

/// Plugins page using pure Rust
pub async fn octopus_ui_plugins_handler(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    let plugins_html = r#"<p class="text-muted-foreground">Plugins will be displayed here once the plugin system is configured.</p>"#.to_string();
    let plugins_grid =
        format!(r#"<div class="grid gap-4 md:grid-cols-2 lg:grid-cols-3">{plugins_html}</div>"#);

    let content = VStack::new()
        .gap("6")
        .child(Node::raw(&plugins_grid))
        .render();

    let layout = admin_layout("Plugins")
        .nav_item(NavItem::new("Dashboard", "/admin/octopus-ui").icon(icons::HOME.to_string()))
        .nav_item(
            NavItem::new("Routes", "/admin/octopus-ui/routes").icon(icons::ROUTES.to_string()),
        )
        .nav_item(
            NavItem::new("Health", "/admin/octopus-ui/health").icon(icons::HEALTH.to_string()),
        )
        .nav_item(
            NavItem::new("Plugins", "/admin/octopus-ui/plugins")
                .icon(icons::PLUGINS.to_string())
                .active(true),
        )
        .content(content)
        .build();

    let page = document("Plugins - Octopus Admin")
        .html_class("h-full bg-background")
        .body_class("h-full")
        .stylesheet("/admin/static/output.css")
        .script("https://cdn.jsdelivr.net/npm/alpinejs@3.x.x/dist/cdn.min.js")
        .script("https://unpkg.com/htmx.org@1.9.10")
        .body(layout)
        .render_to_string();

    Html(page)
}
