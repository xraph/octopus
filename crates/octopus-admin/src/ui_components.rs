//! UI component helpers using octopus-ui
//!
//! This module provides helper functions to generate HTML components using octopus-ui
//! that can be embedded in Askama templates.

use crate::models::{HealthCheckInfo, PluginInfo, RouteInfo};
use octopus_ui::components::card::*;
use octopus_ui::components::{Badge, Button, ButtonGroup};
use octopus_ui::core::{if_node, map, Classes, Node, Render, Size, Variant};
use octopus_ui::helpers::htmx::Htmx;
use octopus_ui::primitives::{Grid, HStack, VStack};

/// Generate a status badge HTML
pub fn status_badge(status: &str, variant: Variant) -> String {
    Badge::new(status).variant(variant).render_to_string()
}

/// Generate a health status badge
pub fn health_status_badge(status: &str) -> String {
    let variant = match status {
        "passing" | "healthy" => Variant::Default,
        "warning" => Variant::Secondary,
        "critical" | "failing" => Variant::Destructive,
        _ => Variant::Outline,
    };
    status_badge(status, variant)
}

/// Generate a method badge for HTTP methods
pub fn method_badge(method: &str) -> String {
    let variant = match method {
        "GET" => Variant::Default,
        "POST" => Variant::Secondary,
        "PUT" | "PATCH" => Variant::Secondary,
        "DELETE" => Variant::Destructive,
        _ => Variant::Outline,
    };
    Badge::new(method)
        .variant(variant)
        .class("font-mono")
        .render_to_string()
}

/// Generate a button HTML
pub fn action_button(text: &str, variant: Variant, size: Size) -> String {
    Button::with_text(text)
        .variant(variant)
        .size(size)
        .render_to_string()
}

/// Generate a button with HTMX attributes
pub fn htmx_button(text: &str, method: &str, url: &str, target: &str) -> String {
    let (method_attr_key, method_attr_val) = match method {
        "POST" => Htmx::hx_post(url),
        "PUT" => Htmx::hx_put(url),
        "DELETE" => Htmx::hx_delete(url),
        _ => Htmx::hx_get(url),
    };

    let (target_key, target_val) = Htmx::hx_target(target);
    let (swap_key, swap_val) = Htmx::hx_swap("outerHTML");

    Button::with_text(text)
        .variant(Variant::Default)
        .size(Size::SM)
        .attr(method_attr_key, method_attr_val)
        .attr(target_key, target_val)
        .attr(swap_key, swap_val)
        .render_to_string()
}

/// Generate a stats card
pub fn stats_card(title: &str, value: &str, description: &str, icon: Option<&str>) -> String {
    Card::new()
        .child(
            CardHeader::new()
                .class("flex flex-row items-center justify-between space-y-0 pb-2")
                .child(CardTitle::new(title).class("text-sm font-medium").render())
                .child(if_node(
                    icon.is_some(),
                    Node::element("div")
                        .attr("class", "text-muted-foreground")
                        .child(Node::raw(icon.unwrap_or(""))),
                ))
                .render(),
        )
        .child(
            CardContent::new()
                .child(
                    VStack::new()
                        .gap("1")
                        .child(
                            Node::element("div")
                                .attr("class", "text-2xl font-bold")
                                .child(Node::text(value)),
                        )
                        .child(
                            Node::element("p")
                                .attr("class", "text-xs text-muted-foreground")
                                .child(Node::text(description)),
                        )
                        .render(),
                )
                .render(),
        )
        .render_to_string()
}

/// Generate a route info card
pub fn route_card(route: &RouteInfo) -> String {
    Card::new()
        .child(
            CardHeader::new()
                .child(
                    HStack::new()
                        .gap("2")
                        .class("items-center")
                        .child(Node::raw(&method_badge(&route.method)))
                        .child(
                            CardTitle::new(&route.path)
                                .class("text-base font-mono")
                                .render(),
                        )
                        .child(Node::raw(&health_status_badge(if route.is_healthy {
                            "passing"
                        } else {
                            "failing"
                        })))
                        .render(),
                )
                .child(CardDescription::new(&route.upstream).render())
                .render(),
        )
        .child(
            CardContent::new()
                .child(
                    Grid::new()
                        .cols(3)
                        .gap("4")
                        .class("text-sm")
                        .child(
                            VStack::new()
                                .gap("1")
                                .child(
                                    Node::element("div")
                                        .attr("class", "text-muted-foreground")
                                        .child(Node::text("Requests")),
                                )
                                .child(
                                    Node::element("div")
                                        .attr("class", "font-medium")
                                        .child(Node::text(&route.request_count.to_string())),
                                )
                                .render(),
                        )
                        .child(
                            VStack::new()
                                .gap("1")
                                .child(
                                    Node::element("div")
                                        .attr("class", "text-muted-foreground")
                                        .child(Node::text("Avg Latency")),
                                )
                                .child(
                                    Node::element("div").attr("class", "font-medium").child(
                                        Node::text(&format!("{:.1}ms", route.avg_latency_ms)),
                                    ),
                                )
                                .render(),
                        )
                        .child(
                            VStack::new()
                                .gap("1")
                                .child(
                                    Node::element("div")
                                        .attr("class", "text-muted-foreground")
                                        .child(Node::text("Errors")),
                                )
                                .child(
                                    Node::element("div")
                                        .attr("class", "font-medium")
                                        .child(Node::text(&route.error_count.to_string())),
                                )
                                .render(),
                        )
                        .render(),
                )
                .render(),
        )
        .child(
            CardFooter::new()
                .child(
                    ButtonGroup::new()
                        .gap("2")
                        .child(Button::with_text("Details").size(Size::SM).render())
                        .child(
                            Button::with_text("Edit")
                                .variant(Variant::Outline)
                                .size(Size::SM)
                                .render(),
                        )
                        .render(),
                )
                .render(),
        )
        .render_to_string()
}

/// Generate health check card
pub fn health_check_card(check: &HealthCheckInfo) -> String {
    Card::new()
        .child(
            CardHeader::new()
                .child(
                    HStack::new()
                        .gap("2")
                        .class("items-center justify-between")
                        .child(CardTitle::new(&check.name).render())
                        .child(Node::raw(&health_status_badge(&check.status)))
                        .render(),
                )
                .child(
                    CardDescription::new(&format!("Last checked: {}", check.last_check)).render(),
                )
                .render(),
        )
        .child(
            CardContent::new()
                .child(
                    VStack::new()
                        .gap("2")
                        .child(if_node(
                            check.endpoint.is_some(),
                            Node::element("div")
                                .attr("class", "text-sm font-mono text-muted-foreground")
                                .child(Node::text(check.endpoint.as_deref().unwrap_or(""))),
                        ))
                        .child(
                            HStack::new()
                                .gap("4")
                                .class("text-sm")
                                .child(
                                    Node::element("div")
                                        .child(
                                            Node::element("span")
                                                .attr("class", "text-muted-foreground")
                                                .child(Node::text("Response time: ")),
                                        )
                                        .child(
                                            Node::element("span")
                                                .attr("class", "font-medium")
                                                .child(Node::text(&format!(
                                                    "{}ms",
                                                    check.response_time_ms
                                                ))),
                                        ),
                                )
                                .child(if_node(
                                    check.consecutive_failures > 0,
                                    Node::element("div").child(
                                        Node::element("span")
                                            .attr("class", "text-destructive")
                                            .child(Node::text(&format!(
                                                "{} consecutive failures",
                                                check.consecutive_failures
                                            ))),
                                    ),
                                ))
                                .render(),
                        )
                        .child(if_node(
                            check.message.is_some(),
                            Node::element("div")
                                .attr("class", "text-sm text-muted-foreground")
                                .child(Node::text(check.message.as_deref().unwrap_or(""))),
                        ))
                        .render(),
                )
                .render(),
        )
        .render_to_string()
}

/// Generate plugin card
pub fn plugin_card(plugin: &PluginInfo) -> String {
    Card::new()
        .child(
            CardHeader::new()
                .child(
                    HStack::new()
                        .gap("2")
                        .class("items-center justify-between")
                        .child(
                            VStack::new()
                                .gap("1")
                                .child(CardTitle::new(&plugin.name).render())
                                .child(CardDescription::new(&plugin.version).render())
                                .render(),
                        )
                        .child(Node::raw(&status_badge(
                            if plugin.enabled {
                                "Enabled"
                            } else {
                                "Disabled"
                            },
                            if plugin.enabled {
                                Variant::Default
                            } else {
                                Variant::Secondary
                            },
                        )))
                        .render(),
                )
                .render(),
        )
        .child(
            CardContent::new()
                .child(
                    VStack::new()
                        .gap("2")
                        .child(
                            Node::element("p")
                                .attr("class", "text-sm text-muted-foreground")
                                .child(Node::text(&plugin.description)),
                        )
                        .child(if_node(
                            plugin.author.is_some(),
                            Node::element("p")
                                .attr("class", "text-xs text-muted-foreground")
                                .child(Node::text(&format!(
                                    "By {}",
                                    plugin.author.as_deref().unwrap_or("Unknown")
                                ))),
                        ))
                        .render(),
                )
                .render(),
        )
        .child(
            CardFooter::new()
                .child(
                    ButtonGroup::new()
                        .gap("2")
                        .child(
                            Button::with_text(if plugin.enabled { "Disable" } else { "Enable" })
                                .variant(if plugin.enabled {
                                    Variant::Destructive
                                } else {
                                    Variant::Default
                                })
                                .size(Size::SM)
                                .render(),
                        )
                        .child(
                            Button::with_text("Configure")
                                .variant(Variant::Outline)
                                .size(Size::SM)
                                .disabled(!plugin.enabled)
                                .render(),
                        )
                        .child(if_node(
                            plugin.has_dashboard,
                            Button::with_text("Dashboard")
                                .variant(Variant::Link)
                                .size(Size::SM)
                                .render(),
                        ))
                        .render(),
                )
                .render(),
        )
        .render_to_string()
}

/// Generate a navigation item
pub fn nav_item(name: &str, path: &str, active: bool, icon: Option<&str>) -> String {
    let classes = Classes::new()
        .add("nav-link", true)
        .add(
            "flex items-center gap-3 rounded-lg px-3 py-2 transition-all",
            true,
        )
        .add("bg-muted text-primary", active)
        .add("text-muted-foreground hover:text-primary", !active)
        .build();

    Node::element("a")
        .attr("href", path)
        .attr("class", &classes)
        .child(if_node(icon.is_some(), Node::raw(icon.unwrap_or(""))))
        .child(Node::text(name))
        .render_to_string()
}

/// Generate an alert box
pub fn alert_box(title: &str, message: &str, variant: Variant) -> String {
    let alert_classes = match variant {
        Variant::Destructive => {
            "p-4 rounded-lg border border-destructive bg-destructive/10 text-destructive"
        }
        Variant::Default => "p-4 rounded-lg border border-primary bg-primary/10 text-primary",
        Variant::Secondary => "p-4 rounded-lg border border-secondary bg-secondary/10",
        _ => "p-4 rounded-lg border border-muted bg-muted/10",
    };

    VStack::new()
        .gap("2")
        .class(alert_classes)
        .child(
            Node::element("h4")
                .attr("class", "font-semibold text-sm")
                .child(Node::text(title)),
        )
        .child(
            Node::element("p")
                .attr("class", "text-sm")
                .child(Node::text(message)),
        )
        .render_to_string()
}

/// Generate a data table row for routes
pub fn route_table_row(route: &RouteInfo, index: usize) -> String {
    Node::element("tr")
        .attr("class", if index % 2 == 0 { "bg-muted/50" } else { "" })
        .child(
            Node::element("td")
                .attr("class", "px-4 py-3")
                .child(Node::raw(&method_badge(&route.method))),
        )
        .child(
            Node::element("td")
                .attr("class", "px-4 py-3 font-mono text-sm")
                .child(Node::text(&route.path)),
        )
        .child(
            Node::element("td")
                .attr("class", "px-4 py-3 text-sm")
                .child(Node::text(&route.upstream)),
        )
        .child(
            Node::element("td")
                .attr("class", "px-4 py-3 text-sm text-right")
                .child(Node::text(&route.request_count.to_string())),
        )
        .child(
            Node::element("td")
                .attr("class", "px-4 py-3 text-sm text-right")
                .child(Node::text(&format!("{:.1}ms", route.avg_latency_ms))),
        )
        .child(
            Node::element("td")
                .attr("class", "px-4 py-3")
                .child(Node::raw(&health_status_badge(if route.is_healthy {
                    "passing"
                } else {
                    "failing"
                }))),
        )
        .child(
            Node::element("td").attr("class", "px-4 py-3").child(
                ButtonGroup::new()
                    .gap("1")
                    .child(
                        Button::with_text("View")
                            .variant(Variant::Ghost)
                            .size(Size::SM)
                            .render(),
                    )
                    .child(
                        Button::with_text("Edit")
                            .variant(Variant::Ghost)
                            .size(Size::SM)
                            .render(),
                    )
                    .render(),
            ),
        )
        .render_to_string()
}

/// Generate complete routes list with table
pub fn routes_table(routes: &[RouteInfo]) -> String {
    Node::element("div")
        .attr("class", "rounded-md border")
        .child(
            Node::element("table")
                .attr("class", "w-full")
                .child(
                    Node::element("thead").child(
                        Node::element("tr")
                            .attr("class", "border-b bg-muted/50")
                            .child(
                                Node::element("th")
                                    .attr("class", "px-4 py-3 text-left text-sm font-medium")
                                    .child(Node::text("Method")),
                            )
                            .child(
                                Node::element("th")
                                    .attr("class", "px-4 py-3 text-left text-sm font-medium")
                                    .child(Node::text("Path")),
                            )
                            .child(
                                Node::element("th")
                                    .attr("class", "px-4 py-3 text-left text-sm font-medium")
                                    .child(Node::text("Upstream")),
                            )
                            .child(
                                Node::element("th")
                                    .attr("class", "px-4 py-3 text-right text-sm font-medium")
                                    .child(Node::text("Requests")),
                            )
                            .child(
                                Node::element("th")
                                    .attr("class", "px-4 py-3 text-right text-sm font-medium")
                                    .child(Node::text("Latency")),
                            )
                            .child(
                                Node::element("th")
                                    .attr("class", "px-4 py-3 text-left text-sm font-medium")
                                    .child(Node::text("Status")),
                            )
                            .child(
                                Node::element("th")
                                    .attr("class", "px-4 py-3 text-left text-sm font-medium")
                                    .child(Node::text("Actions")),
                            ),
                    ),
                )
                .child(
                    Node::element("tbody")
                        .child(map(routes, |route| Node::raw(&route_table_row(route, 0)))),
                ),
        )
        .render_to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_badge() {
        let html = status_badge("Active", Variant::Default);
        assert!(html.contains("Active"));
        assert!(html.contains("inline-flex"));
    }

    #[test]
    fn test_health_status_badge() {
        let html = health_status_badge("passing");
        assert!(html.contains("passing"));
    }

    #[test]
    fn test_method_badge() {
        let html = method_badge("GET");
        assert!(html.contains("GET"));
        assert!(html.contains("font-mono"));
    }

    #[test]
    fn test_nav_item() {
        let html = nav_item("Home", "/", true, None);
        assert!(html.contains("Home"));
        assert!(html.contains("href=\"/\""));
        assert!(html.contains("bg-muted"));
    }
}
