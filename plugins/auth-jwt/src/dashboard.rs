//! Dashboard view for JWT authentication plugin
//!
//! This module provides a Leptos-based dashboard view that displays
//! JWT token information and statistics.

use leptos::*;
use octopus_admin::prelude::*;
use serde::{Deserialize, Serialize};

/// JWT token information for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtTokenInfo {
    pub subject: String,
    pub issuer: String,
    pub expires_at: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

/// Stats for JWT authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtStats {
    pub total_tokens_issued: i32,
    pub active_tokens: i32,
    pub expired_tokens: i32,
    pub revoked_tokens: i32,
}

/// JWT authentication dashboard component
#[component]
pub fn JwtAuthDashboard() -> impl IntoView {
    // Fetch JWT stats
    let stats_resource = create_resource(
        || (),
        |_| async move {
            fetch_jwt_stats().await
        },
    );
    
    // Fetch recent tokens
    let tokens_resource = create_resource(
        || (),
        |_| async move {
            fetch_recent_tokens().await
        },
    );
    
    view! {
        <div>
            <h1 class="text-2xl font-bold mb-6 text-gray-800">"JWT Authentication"</h1>
            
            // Stats cards
            <div class="grid grid-cols-1 md:grid-cols-4 gap-6 mb-8">
                <Suspense fallback=|| view! { <div>"Loading..."</div> }>
                    {move || {
                        stats_resource.get().map(|result| {
                            match result {
                                Ok(stats) => view! {
                                    <>
                                        <StatCard 
                                            title="Total Issued"
                                            value=stats.total_tokens_issued
                                            icon="🎫"
                                        />
                                        <StatCard 
                                            title="Active"
                                            value=stats.active_tokens
                                            icon="✅"
                                        />
                                        <StatCard 
                                            title="Expired"
                                            value=stats.expired_tokens
                                            icon="⏰"
                                        />
                                        <StatCard 
                                            title="Revoked"
                                            value=stats.revoked_tokens
                                            icon="🚫"
                                        />
                                    </>
                                }.into_view(),
                                Err(e) => view! {
                                    <div class="col-span-4 bg-red-50 border border-red-200 rounded-lg p-4">
                                        <p class="text-red-800">"Error loading stats: " {e}</p>
                                    </div>
                                }.into_view(),
                            }
                        })
                    }}
                </Suspense>
            </div>
            
            // Recent tokens table
            <div class="bg-white rounded-lg shadow p-6">
                <h3 class="text-lg font-semibold mb-4 text-gray-800">"Recent Tokens"</h3>
                <Suspense fallback=|| view! { <div>"Loading tokens..."</div> }>
                    {move || {
                        tokens_resource.get().map(|result| {
                            match result {
                                Ok(tokens) => {
                                    if tokens.is_empty() {
                                        view! {
                                            <p class="text-gray-500 text-center py-4">"No tokens found"</p>
                                        }.into_view()
                                    } else {
                                        view! {
                                            <div class="space-y-3">
                                                <For
                                                    each=move || tokens.clone()
                                                    key=|token| token.subject.clone()
                                                    children=|token: JwtTokenInfo| {
                                                        view! {
                                                            <TokenCard token=token/>
                                                        }
                                                    }
                                                />
                                            </div>
                                        }.into_view()
                                    }
                                }
                                Err(e) => view! {
                                    <div class="bg-red-50 border border-red-200 rounded-lg p-4">
                                        <p class="text-red-800">"Error loading tokens: " {e}</p>
                                    </div>
                                }.into_view(),
                            }
                        })
                    }}
                </Suspense>
            </div>
        </div>
    }
}

#[component]
fn StatCard(title: &'static str, value: i32, icon: &'static str) -> impl IntoView {
    view! {
        <div class="bg-white rounded-lg shadow p-6">
            <div class="flex items-center justify-between">
                <div>
                    <p class="text-gray-500 text-sm mb-1">{title}</p>
                    <p class="text-3xl font-bold text-gray-800">{value}</p>
                </div>
                <div class="text-4xl">{icon}</div>
            </div>
        </div>
    }
}

#[component]
fn TokenCard(token: JwtTokenInfo) -> impl IntoView {
    view! {
        <div class="border border-gray-200 rounded-lg p-4 hover:bg-gray-50">
            <div class="flex items-center justify-between mb-2">
                <div class="flex items-center space-x-3">
                    <span class="text-2xl">"🔑"</span>
                    <div>
                        <p class="font-medium text-gray-800">{token.subject}</p>
                        <p class="text-sm text-gray-500">
                            "Issuer: " {token.issuer}
                        </p>
                    </div>
                </div>
                <div class="text-right">
                    <p class="text-sm text-gray-500">"Expires"</p>
                    <p class="text-sm font-medium text-gray-700">{token.expires_at}</p>
                </div>
            </div>
            <div class="flex flex-wrap gap-2 mt-3">
                <span class="text-xs font-medium">"Roles:"</span>
                <For
                    each=move || token.roles.clone()
                    key=|role| role.clone()
                    children=|role: String| {
                        view! {
                            <span class="px-2 py-1 bg-blue-100 text-blue-800 text-xs rounded-full">
                                {role}
                            </span>
                        }
                    }
                />
            </div>
            <div class="flex flex-wrap gap-2 mt-2">
                <span class="text-xs font-medium">"Permissions:"</span>
                <For
                    each=move || token.permissions.clone()
                    key=|perm| perm.clone()
                    children=|perm: String| {
                        view! {
                            <span class="px-2 py-1 bg-green-100 text-green-800 text-xs rounded-full">
                                {perm}
                            </span>
                        }
                    }
                />
            </div>
        </div>
    }
}

/// Fetch JWT statistics from the API
async fn fetch_jwt_stats() -> Result<JwtStats, String> {
    use gloo_net::http::Request;
    
    Request::get("/admin/api/auth/jwt/stats")
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("JSON parsing failed: {}", e))
}

/// Fetch recent JWT tokens from the API
async fn fetch_recent_tokens() -> Result<Vec<JwtTokenInfo>, String> {
    use gloo_net::http::Request;
    
    Request::get("/admin/api/auth/jwt/tokens")
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("JSON parsing failed: {}", e))
}

/// Register the JWT auth dashboard plugin
pub fn register_jwt_dashboard_plugin() -> Result<(), String> {
    use octopus_admin::register_plugin;
    
    let plugin = Box::new(JwtDashboardPlugin);
    register_plugin(plugin)
}

/// JWT dashboard plugin implementation
struct JwtDashboardPlugin;

impl DashboardPlugin for JwtDashboardPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "jwt-auth-dashboard".to_string(),
            name: "JWT Authentication Dashboard".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "Dashboard view for JWT authentication management".to_string(),
            author: Some("Octopus Team".to_string()),
        }
    }
    
    fn register_views(&self) -> Vec<DashboardView> {
        vec![
            DashboardView {
                id: "jwt-auth".to_string(),
                title: "JWT Auth".to_string(),
                icon: "🔑".to_string(),
                path: "/auth/jwt".to_string(),
                priority: 50,
                component: || view! { <JwtAuthDashboard/> },
            }
        ]
    }
    
    fn register_stats_cards(&self) -> Vec<StatsCard> {
        vec![
            StatsCard {
                id: "jwt-active-tokens".to_string(),
                title: "Active Tokens".to_string(),
                icon: "🎫".to_string(),
                priority: 40,
                fetch_value: || {
                    create_resource(|| (), |_| async move {
                        fetch_jwt_stats()
                            .await
                            .map(|stats| stats.active_tokens)
                    })
                },
            }
        ]
    }
    
    fn register_nav_items(&self) -> Vec<NavItem> {
        vec![
            NavItem {
                label: "JWT Auth".to_string(),
                icon: "🔑".to_string(),
                path: "/auth/jwt".to_string(),
                priority: 50,
            }
        ]
    }
}

