//! Authorization engine combining role/scope checks, Rhai scripts, and OPA

use crate::authzen::{AuthZenClient, Authorizer};
use crate::opa::{AuthzContext, AuthzDecision, OpaClient};
use crate::registry::Principal;
use octopus_config::types::{AuthzAction, AuthzConfig, AuthzEngine, AuthzRule};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;

/// Route-level authorization context
#[derive(Debug, Clone)]
pub struct RouteAuthzContext {
    /// Required roles (any match)
    pub require_roles: Vec<String>,
    /// Required scopes (all must match)
    pub require_scopes: Vec<String>,
    /// Custom Rhai authz rule
    pub custom_rule: Option<String>,
}

/// Authorization evaluator
pub struct AuthzEvaluator {
    engine: AuthzEngine,
    global_rules: Vec<AuthzRule>,
    opa: Option<Arc<OpaClient>>,
    authzen: Option<Arc<AuthZenClient>>,
    rhai_engine: rhai::Engine,
}

impl std::fmt::Debug for AuthzEvaluator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthzEvaluator")
            .field("engine", &self.engine)
            .field("global_rules_count", &self.global_rules.len())
            .field("opa", &self.opa.is_some())
            .field("authzen", &self.authzen.is_some())
            .finish()
    }
}

impl AuthzEvaluator {
    /// Create from config
    pub fn from_config(config: &AuthzConfig) -> anyhow::Result<Self> {
        let opa = if let Some(ref opa_config) = config.opa {
            Some(Arc::new(OpaClient::from_config(opa_config)?))
        } else {
            None
        };

        let authzen = if let Some(ref az_config) = config.authzen {
            Some(Arc::new(AuthZenClient::from_config(az_config)?))
        } else {
            None
        };

        // Create a single Rhai engine to reuse across evaluations
        let mut rhai_engine = rhai::Engine::new();
        rhai_engine.set_max_expr_depths(25, 10);
        rhai_engine.set_max_operations(10_000);
        rhai_engine.set_max_string_size(4096);

        Ok(Self {
            engine: config.engine.clone(),
            global_rules: config.global_rules.clone(),
            opa,
            authzen,
            rhai_engine,
        })
    }

    /// Build the shared authorization context passed to external PDPs (OPA/AuthZEN).
    fn build_context(
        &self,
        principal: &Principal,
        request_method: &str,
        request_path: &str,
        request_headers: &HashMap<String, String>,
        route_upstream: &str,
        route_metadata: &HashMap<String, String>,
    ) -> AuthzContext {
        AuthzContext {
            principal: principal.into(),
            request: crate::opa::AuthzRequest {
                method: request_method.to_string(),
                path: request_path.to_string(),
                headers: request_headers.clone(),
            },
            route: crate::opa::AuthzRoute {
                upstream: route_upstream.to_string(),
                path: request_path.to_string(),
                metadata: route_metadata.clone(),
            },
        }
    }

    /// Evaluate authorization for a request
    pub async fn evaluate(
        &self,
        principal: &Principal,
        route_ctx: &RouteAuthzContext,
        request_method: &str,
        request_path: &str,
        request_headers: &HashMap<String, String>,
        route_upstream: &str,
        route_metadata: &HashMap<String, String>,
    ) -> anyhow::Result<AuthzDecision> {
        // 1. Check required roles (fast, no engine needed)
        if !route_ctx.require_roles.is_empty() {
            let has_any = route_ctx
                .require_roles
                .iter()
                .any(|r| principal.roles.contains(r));
            if !has_any {
                return Ok(AuthzDecision::Deny(format!(
                    "Required role(s): {:?}, principal has: {:?}",
                    route_ctx.require_roles, principal.roles
                )));
            }
        }

        // 2. Check required scopes (all must match)
        if !route_ctx.require_scopes.is_empty() {
            let has_all = route_ctx
                .require_scopes
                .iter()
                .all(|s| principal.scopes.contains(s));
            if !has_all {
                return Ok(AuthzDecision::Deny(format!(
                    "Required scope(s): {:?}, principal has: {:?}",
                    route_ctx.require_scopes, principal.scopes
                )));
            }
        }

        // 3. Evaluate custom route-level rule
        if let Some(ref rule) = route_ctx.custom_rule {
            let decision =
                self.evaluate_rhai_rule_sync(rule, principal, request_method, request_path)?;
            if let AuthzDecision::Deny(_) = decision {
                return Ok(decision);
            }
        }

        // 3b. When AuthZEN is the primary engine it is authoritative for the
        // whole request: subject = principal, action = HTTP method, resource =
        // request path. (OpenID AuthZEN Authorization API 1.0.)
        if matches!(self.engine, AuthzEngine::AuthZen) {
            return if let Some(ref pdp) = self.authzen {
                let ctx = self.build_context(
                    principal,
                    request_method,
                    request_path,
                    request_headers,
                    route_upstream,
                    route_metadata,
                );
                pdp.evaluate(&ctx).await
            } else {
                warn!("authz engine is AuthZen but no PDP configured; denying");
                Ok(AuthzDecision::Deny("AuthZEN PDP not configured".to_string()))
            };
        }

        // 4. Evaluate global rules
        for global_rule in &self.global_rules {
            let decision = match global_rule.engine.as_ref().unwrap_or(&self.engine) {
                AuthzEngine::Opa | AuthzEngine::Both => {
                    if let Some(ref opa) = self.opa {
                        let ctx = self.build_context(
                            principal,
                            request_method,
                            request_path,
                            request_headers,
                            route_upstream,
                            route_metadata,
                        );
                        opa.evaluate(&ctx).await?
                    } else {
                        self.evaluate_rhai_rule_sync(
                            &global_rule.rule,
                            principal,
                            request_method,
                            request_path,
                        )?
                    }
                }
                AuthzEngine::AuthZen => {
                    if let Some(ref pdp) = self.authzen {
                        let ctx = self.build_context(
                            principal,
                            request_method,
                            request_path,
                            request_headers,
                            route_upstream,
                            route_metadata,
                        );
                        pdp.evaluate(&ctx).await?
                    } else {
                        self.evaluate_rhai_rule_sync(
                            &global_rule.rule,
                            principal,
                            request_method,
                            request_path,
                        )?
                    }
                }
                AuthzEngine::Rhai => self.evaluate_rhai_rule_sync(
                    &global_rule.rule,
                    principal,
                    request_method,
                    request_path,
                )?,
            };

            match (&global_rule.action, &decision) {
                (AuthzAction::Deny, AuthzDecision::Allow) => {
                    // Rule says "deny when expression is true" - expression returned allow (false)
                    // So don't deny
                }
                (AuthzAction::Deny, AuthzDecision::Deny(_)) => {
                    // Rule says "deny when true" - expression returned deny (true matched)
                    return Ok(AuthzDecision::Deny(format!(
                        "Denied by global rule '{}'",
                        global_rule.name
                    )));
                }
                (AuthzAction::Allow, AuthzDecision::Deny(_)) => {
                    // Rule says "allow when true" but expression returned false
                    return Ok(AuthzDecision::Deny(format!(
                        "Not allowed by rule '{}'",
                        global_rule.name
                    )));
                }
                _ => {} // Allow + Allow = continue
            }
        }

        Ok(AuthzDecision::Allow)
    }

    /// Evaluate a Rhai authorization rule using the shared engine
    fn evaluate_rhai_rule_sync(
        &self,
        rule: &str,
        principal: &Principal,
        request_method: &str,
        request_path: &str,
    ) -> anyhow::Result<AuthzDecision> {
        let mut scope = rhai::Scope::new();

        // Build principal map
        let mut principal_map = rhai::Map::new();
        principal_map.insert("id".into(), principal.id.clone().into());
        principal_map.insert("name".into(), principal.name.clone().into());
        let roles_array: rhai::Array = principal
            .roles
            .iter()
            .map(|r| rhai::Dynamic::from(r.clone()))
            .collect();
        principal_map.insert("roles".into(), roles_array.into());
        let scopes_array: rhai::Array = principal
            .scopes
            .iter()
            .map(|s| rhai::Dynamic::from(s.clone()))
            .collect();
        principal_map.insert("scopes".into(), scopes_array.into());

        // Build attributes map
        let mut attrs_map = rhai::Map::new();
        for (k, v) in &principal.attributes {
            if let Some(s) = v.as_str() {
                attrs_map.insert(k.clone().into(), s.to_string().into());
            } else {
                attrs_map.insert(k.clone().into(), v.to_string().into());
            }
        }
        principal_map.insert("attributes".into(), attrs_map.into());
        scope.push("principal", principal_map);

        // Build request map
        let mut request_map = rhai::Map::new();
        request_map.insert("method".into(), request_method.to_string().into());
        request_map.insert("path".into(), request_path.to_string().into());
        scope.push("request", request_map);

        // Push helper variables for simple role/scope checks
        let roles_vec = principal.roles.clone();
        let scopes_vec = principal.scopes.clone();
        scope.push("_roles", roles_vec);
        scope.push("_scopes", scopes_vec);

        // Wrap the rule with helper function definitions
        let wrapped_rule = format!(
            r"
            fn has_role(role) {{ _roles.contains(role) }}
            fn has_scope(s) {{ _scopes.contains(s) }}
            fn has_any_role(roles) {{ roles.some(|r| _roles.contains(r)) }}
            fn has_all_roles(roles) {{ roles.all(|r| _roles.contains(r)) }}
            {rule}
            "
        );

        match self
            .rhai_engine
            .eval_with_scope::<rhai::Dynamic>(&mut scope, &wrapped_rule)
        {
            Ok(result) => {
                let allowed = result.as_bool().unwrap_or(false);
                if allowed {
                    Ok(AuthzDecision::Allow)
                } else {
                    Ok(AuthzDecision::Deny(
                        "Authz rule evaluated to false".to_string(),
                    ))
                }
            }
            Err(e) => {
                warn!(rule = %rule, error = %e, "Rhai authz rule evaluation error");
                Ok(AuthzDecision::Deny(format!("Authz rule error: {e}")))
            }
        }
    }
}
