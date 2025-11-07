//! Middleware trait and utilities

use crate::{Error, Result};
use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use std::fmt;
use std::sync::Arc;

/// Body type alias
pub type Body = Full<Bytes>;

/// Middleware trait for request/response processing
#[async_trait]
pub trait Middleware: Send + Sync + fmt::Debug {
    /// Process a request
    ///
    /// # Arguments
    ///
    /// * `req` - The incoming HTTP request
    /// * `next` - The next middleware/handler in the chain
    ///
    /// # Returns
    ///
    /// Returns the HTTP response or an error
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>>;
}

/// Type alias for the final handler function
pub type HandlerFn = Box<
    dyn Fn(
            Request<Body>,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response<Body>>> + Send>>
        + Send
        + Sync,
>;

/// Represents the next middleware/handler in the chain
pub struct Next {
    middleware_stack: Arc<[Arc<dyn Middleware>]>,
    index: usize,
    final_handler: Option<Arc<HandlerFn>>,
}

impl Next {
    /// Create a new Next from a middleware stack
    pub fn new(middleware_stack: Arc<[Arc<dyn Middleware>]>) -> Self {
        Self {
            middleware_stack,
            index: 0,
            final_handler: None,
        }
    }

    /// Create a new Next with a final handler
    pub fn with_handler(middleware_stack: Arc<[Arc<dyn Middleware>]>, handler: HandlerFn) -> Self {
        Self {
            middleware_stack,
            index: 0,
            final_handler: Some(Arc::new(handler)),
        }
    }

    /// Run the next middleware or final handler
    pub async fn run(self, req: Request<Body>) -> Result<Response<Body>> {
        if let Some(middleware) = self.middleware_stack.get(self.index) {
            let next = Self {
                middleware_stack: Arc::clone(&self.middleware_stack),
                index: self.index + 1,
                final_handler: self.final_handler.clone(),
            };
            middleware.call(req, next).await
        } else if let Some(handler) = self.final_handler {
            // Call the final handler
            handler(req).await
        } else {
            // Reached end of chain without handler
            Err(Error::Internal(
                "Middleware chain completed without handler".to_string(),
            ))
        }
    }
}

impl Clone for Next {
    fn clone(&self) -> Self {
        Self {
            middleware_stack: Arc::clone(&self.middleware_stack),
            index: self.index,
            final_handler: self.final_handler.clone(),
        }
    }
}

impl fmt::Debug for Next {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Next")
            .field("index", &self.index)
            .field("remaining", &(self.middleware_stack.len() - self.index))
            .finish()
    }
}

/// Helper macro for creating simple middleware
#[macro_export]
macro_rules! middleware_fn {
    ($name:ident, $func:expr) => {
        #[derive(Debug)]
        struct $name;

        #[async_trait]
        impl Middleware for $name {
            async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
                $func(req, next).await
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestMiddleware {
        name: String,
    }

    #[async_trait]
    impl Middleware for TestMiddleware {
        async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
            println!("Middleware: {}", self.name);
            next.run(req).await
        }
    }

    #[tokio::test]
    async fn test_middleware_chain() {
        let middleware1 = Arc::new(TestMiddleware {
            name: "first".to_string(),
        }) as Arc<dyn Middleware>;

        let middleware2 = Arc::new(TestMiddleware {
            name: "second".to_string(),
        }) as Arc<dyn Middleware>;

        let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([middleware1, middleware2]);
        let next = Next::new(stack);

        let req = Request::builder()
            .uri("/test")
            .body(Body::from("test"))
            .unwrap();

        let result = next.run(req).await;
        assert!(result.is_err()); // Should error at end of chain
    }
}
