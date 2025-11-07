//! HTTP client for making requests to upstream services

use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use octopus_core::{Error, Result, UpstreamInstance};
use std::time::Duration;

/// Body type alias
pub type Body = Full<Bytes>;

/// HTTP client for upstream requests
#[derive(Debug, Clone)]
pub struct HttpClient {
    client: Client<hyper_util::client::legacy::connect::HttpConnector, Body>,
    timeout: Duration,
}

impl HttpClient {
    /// Create a new HTTP client
    pub fn new() -> Self {
        let connector = hyper_util::client::legacy::connect::HttpConnector::new();

        let client = Client::builder(TokioExecutor::new()).build(connector);

        Self {
            client,
            timeout: Duration::from_secs(30),
        }
    }

    /// Create a new HTTP client with custom timeout
    pub fn with_timeout(timeout: Duration) -> Self {
        let connector = hyper_util::client::legacy::connect::HttpConnector::new();

        let client = Client::builder(TokioExecutor::new()).build(connector);

        Self { client, timeout }
    }

    /// Send a request to an upstream instance
    pub async fn send(
        &self,
        req: Request<Body>,
        _upstream: &UpstreamInstance,
    ) -> Result<Response<Incoming>> {
        // Apply timeout
        let timeout = tokio::time::timeout(self.timeout, self.client.request(req));

        match timeout.await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(e)) => Err(Error::UpstreamConnection(e.to_string())),
            Err(_) => Err(Error::UpstreamTimeout),
        }
    }

    /// Get the configured timeout
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_client_new() {
        let client = HttpClient::new();
        assert_eq!(client.timeout(), Duration::from_secs(30));
    }

    #[test]
    fn test_http_client_with_timeout() {
        let client = HttpClient::with_timeout(Duration::from_secs(10));
        assert_eq!(client.timeout(), Duration::from_secs(10));
    }
}
