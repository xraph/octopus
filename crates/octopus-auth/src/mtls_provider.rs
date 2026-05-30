//! mTLS authentication provider - authenticates via client certificates

use crate::registry::{AuthProviderInstance, AuthRequest, AuthResult, Principal};
use async_trait::async_trait;
use octopus_config::types::MtlsProviderConfig;
use std::collections::HashMap;

/// mTLS auth provider
#[derive(Debug)]
pub struct MtlsProvider {
    name: String,
    require_client_cert: bool,
    extract_cn: bool,
    cn_to_roles: HashMap<String, Vec<String>>,
}

impl MtlsProvider {
    /// Create from config
    pub fn from_config(name: &str, config: &MtlsProviderConfig) -> Self {
        Self {
            name: name.to_string(),
            require_client_cert: config.require_client_cert,
            extract_cn: config.extract_cn_as_principal,
            cn_to_roles: config.cn_to_roles.clone(),
        }
    }

    /// Match CN against patterns in cn_to_roles
    fn roles_for_cn(&self, cn: &str) -> Vec<String> {
        let mut roles = Vec::new();
        for (pattern, pattern_roles) in &self.cn_to_roles {
            if pattern == "*" || pattern == cn || cn.ends_with(pattern) {
                roles.extend(pattern_roles.clone());
            }
        }
        roles
    }
}

#[async_trait]
impl AuthProviderInstance for MtlsProvider {
    async fn authenticate(&self, req: &AuthRequest<'_>) -> anyhow::Result<AuthResult> {
        match req.tls_client_cn {
            Some(cn) if self.extract_cn => {
                let roles = self.roles_for_cn(cn);
                Ok(AuthResult::Authenticated(Principal {
                    id: format!("mtls:{}", cn),
                    name: cn.to_string(),
                    roles,
                    scopes: vec![],
                    provider: self.name.clone(),
                    attributes: HashMap::new(),
                }))
            }
            Some(_) => {
                // Client cert present but not extracting CN
                Ok(AuthResult::Authenticated(Principal {
                    id: "mtls:verified".to_string(),
                    name: "Verified Client".to_string(),
                    roles: vec![],
                    scopes: vec![],
                    provider: self.name.clone(),
                    attributes: HashMap::new(),
                }))
            }
            None if self.require_client_cert => Ok(AuthResult::Failed(
                "Client certificate required".to_string(),
            )),
            None => Ok(AuthResult::Unauthenticated),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> &str {
        "mtls"
    }
}
