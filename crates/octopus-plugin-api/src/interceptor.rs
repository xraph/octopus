//! Request and response interceptor traits

use crate::context::{RequestContext, ResponseContext};
use crate::error::{PluginError, Result};
use crate::plugin::Plugin;
use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;

/// Body type alias
pub type Body = Full<Bytes>;

/// Request interceptor plugin
///
/// Intercepts requests before they are routed to upstreams.
#[async_trait]
pub trait RequestInterceptor: Plugin {
    /// Intercept request before routing
    ///
    /// Plugins can:
    /// - Modify the request (headers, body, path, etc.)
    /// - Continue to next interceptor
    /// - Return a response immediately (short-circuit)
    /// - Abort with an error
    async fn intercept_request(
        &self,
        req: &mut Request<Body>,
        ctx: &RequestContext,
    ) -> Result<InterceptorAction>;
}

/// Response interceptor plugin
///
/// Intercepts responses before they are returned to clients.
#[async_trait]
pub trait ResponseInterceptor: Plugin {
    /// Intercept response before returning to client
    ///
    /// Plugins can:
    /// - Modify the response (headers, body, status, etc.)
    /// - Continue to next interceptor
    /// - Return a different response
    /// - Abort with an error
    async fn intercept_response(
        &self,
        res: &mut Response<Body>,
        ctx: &ResponseContext,
    ) -> Result<InterceptorAction>;
}

/// Action to take after interception
#[derive(Debug)]
pub enum InterceptorAction {
    /// Continue to next interceptor/handler
    Continue,

    /// Stop processing and return this response (request interceptor only)
    Return(Response<Body>),

    /// Abort with error
    Abort(PluginError),
}

impl InterceptorAction {
    /// Check if action is continue
    pub fn is_continue(&self) -> bool {
        matches!(self, InterceptorAction::Continue)
    }

    /// Check if action is return
    pub fn is_return(&self) -> bool {
        matches!(self, InterceptorAction::Return(_))
    }

    /// Check if action is abort
    pub fn is_abort(&self) -> bool {
        matches!(self, InterceptorAction::Abort(_))
    }

    /// Convert to result, treating Abort as error
    pub fn into_result(self) -> Result<Option<Response<Body>>> {
        match self {
            InterceptorAction::Continue => Ok(None),
            InterceptorAction::Return(res) => Ok(Some(res)),
            InterceptorAction::Abort(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interceptor_action() {
        let action = InterceptorAction::Continue;
        assert!(action.is_continue());
        assert!(!action.is_return());
        assert!(!action.is_abort());

        let result = action.into_result();
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_interceptor_action_abort() {
        let action = InterceptorAction::Abort(PluginError::runtime("test"));
        assert!(action.is_abort());

        let result = action.into_result();
        assert!(result.is_err());
    }
}

