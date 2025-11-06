//! Mock implementations for testing

use crate::auth::{AuthProvider, AuthResult, Credentials, Principal};
use crate::context::{RequestContext, ResponseContext};
use crate::interceptor::{InterceptorAction, RequestInterceptor, ResponseInterceptor, Body};
use crate::plugin::{HealthStatus, Plugin};
use crate::PluginError;
use async_trait::async_trait;
use http::{Request, Response};
use std::sync::{Arc, Mutex};

/// Mock plugin for testing
#[derive(Debug, Clone)]
pub struct MockPlugin {
    name: String,
    version: String,
    init_calls: Arc<Mutex<Vec<serde_json::Value>>>,
    start_calls: Arc<Mutex<usize>>,
    stop_calls: Arc<Mutex<usize>>,
}

impl MockPlugin {
    /// Create a new mock plugin
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: "1.0.0".to_string(),
            init_calls: Arc::new(Mutex::new(Vec::new())),
            start_calls: Arc::new(Mutex::new(0)),
            stop_calls: Arc::new(Mutex::new(0)),
        }
    }

    /// Get number of init calls
    pub fn init_call_count(&self) -> usize {
        self.init_calls.lock().unwrap().len()
    }

    /// Get number of start calls
    pub fn start_call_count(&self) -> usize {
        *self.start_calls.lock().unwrap()
    }

    /// Get number of stop calls
    pub fn stop_call_count(&self) -> usize {
        *self.stop_calls.lock().unwrap()
    }
}

#[async_trait]
impl Plugin for MockPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn init(&mut self, config: serde_json::Value) -> Result<(), PluginError> {
        self.init_calls.lock().unwrap().push(config);
        Ok(())
    }

    async fn start(&mut self) -> Result<(), PluginError> {
        *self.start_calls.lock().unwrap() += 1;
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), PluginError> {
        *self.stop_calls.lock().unwrap() += 1;
        Ok(())
    }
}

/// Mock auth provider for testing
#[derive(Debug, Clone)]
pub struct MockAuthProvider {
    name: String,
    should_authenticate: bool,
    principal: Option<Principal>,
}

impl MockAuthProvider {
    /// Create a new mock auth provider that authenticates
    pub fn authenticated(name: impl Into<String>, principal: Principal) -> Self {
        Self {
            name: name.into(),
            should_authenticate: true,
            principal: Some(principal),
        }
    }

    /// Create a new mock auth provider that fails authentication
    pub fn unauthenticated(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            should_authenticate: false,
            principal: None,
        }
    }
}

#[async_trait]
impl Plugin for MockAuthProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    async fn init(&mut self, _config: serde_json::Value) -> Result<(), PluginError> {
        Ok(())
    }

    async fn start(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
}

#[async_trait]
impl AuthProvider for MockAuthProvider {
    async fn authenticate(&self, _req: &Request<Body>) -> Result<AuthResult, PluginError> {
        if self.should_authenticate {
            Ok(AuthResult::Authenticated(
                self.principal.clone().unwrap(),
            ))
        } else {
            Ok(AuthResult::Unauthenticated)
        }
    }

    async fn validate(&self, _credentials: &Credentials) -> Result<Principal, PluginError> {
        if self.should_authenticate {
            Ok(self.principal.clone().unwrap())
        } else {
            Err(PluginError::auth("Invalid credentials"))
        }
    }
}

/// Mock interceptor for testing
#[derive(Debug, Clone)]
pub struct MockInterceptor {
    name: String,
    request_action: InterceptorActionType,
    response_action: InterceptorActionType,
    request_calls: Arc<Mutex<usize>>,
    response_calls: Arc<Mutex<usize>>,
}

#[derive(Debug, Clone)]
pub enum InterceptorActionType {
    Continue,
    Abort,
}

impl MockInterceptor {
    /// Create a new mock interceptor that continues
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            request_action: InterceptorActionType::Continue,
            response_action: InterceptorActionType::Continue,
            request_calls: Arc::new(Mutex::new(0)),
            response_calls: Arc::new(Mutex::new(0)),
        }
    }

    /// Set request action to abort
    pub fn abort_requests(mut self) -> Self {
        self.request_action = InterceptorActionType::Abort;
        self
    }

    /// Set response action to abort
    pub fn abort_responses(mut self) -> Self {
        self.response_action = InterceptorActionType::Abort;
        self
    }

    /// Get number of request intercept calls
    pub fn request_call_count(&self) -> usize {
        *self.request_calls.lock().unwrap()
    }

    /// Get number of response intercept calls
    pub fn response_call_count(&self) -> usize {
        *self.response_calls.lock().unwrap()
    }
}

#[async_trait]
impl Plugin for MockInterceptor {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    async fn init(&mut self, _config: serde_json::Value) -> Result<(), PluginError> {
        Ok(())
    }

    async fn start(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
}

#[async_trait]
impl RequestInterceptor for MockInterceptor {
    async fn intercept_request(
        &self,
        _req: &mut Request<Body>,
        _ctx: &RequestContext,
    ) -> Result<InterceptorAction, PluginError> {
        *self.request_calls.lock().unwrap() += 1;

        match self.request_action {
            InterceptorActionType::Continue => Ok(InterceptorAction::Continue),
            InterceptorActionType::Abort => {
                Ok(InterceptorAction::Abort(PluginError::runtime("Aborted")))
            }
        }
    }
}

#[async_trait]
impl ResponseInterceptor for MockInterceptor {
    async fn intercept_response(
        &self,
        _res: &mut Response<Body>,
        _ctx: &ResponseContext,
    ) -> Result<InterceptorAction, PluginError> {
        *self.response_calls.lock().unwrap() += 1;

        match self.response_action {
            InterceptorActionType::Continue => Ok(InterceptorAction::Continue),
            InterceptorActionType::Abort => {
                Ok(InterceptorAction::Abort(PluginError::runtime("Aborted")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_plugin() {
        let mut plugin = MockPlugin::new("test");

        plugin.init(serde_json::json!({"key": "value"})).await.unwrap();
        assert_eq!(plugin.init_call_count(), 1);

        plugin.start().await.unwrap();
        assert_eq!(plugin.start_call_count(), 1);

        plugin.stop().await.unwrap();
        assert_eq!(plugin.stop_call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_auth_provider() {
        let principal = Principal::new("user123", "Test User");
        let provider = MockAuthProvider::authenticated("mock-auth", principal);

        let req = Request::builder().body(Body::default()).unwrap();
        let result = provider.authenticate(&req).await.unwrap();

        assert!(result.is_authenticated());
    }

    #[tokio::test]
    async fn test_mock_interceptor() {
        let interceptor = MockInterceptor::new("mock-interceptor");

        let mut req = Request::builder().body(Body::default()).unwrap();
        let ctx = RequestContext::new(
            "req-123".to_string(),
            "127.0.0.1:8080".parse().unwrap(),
        );

        let result = interceptor.intercept_request(&mut req, &ctx).await.unwrap();
        assert!(result.is_continue());
        assert_eq!(interceptor.request_call_count(), 1);
    }
}

