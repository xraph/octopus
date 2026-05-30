//! Test fixtures and builders

use bytes::Bytes;
use http::{Method, Request, Uri};
use http_body_util::Full;
use octopus_core::upstream::UpstreamInstance;
use std::collections::HashMap;

/// Test fixture factory
pub struct TestFixtures;

impl TestFixtures {
    /// Create a simple request builder
    pub fn request() -> RequestBuilder {
        RequestBuilder::new()
    }

    /// Create an upstream instance builder
    pub fn upstream() -> UpstreamBuilder {
        UpstreamBuilder::new()
    }

    /// Create test body bytes
    pub fn body(size: usize) -> Bytes {
        Bytes::from(vec![b'x'; size])
    }

    /// Create random body bytes
    pub fn random_body(size: usize) -> Bytes {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let data: Vec<u8> = (0..size).map(|_| rng.gen()).collect();
        Bytes::from(data)
    }
}

/// Request builder for tests
pub struct RequestBuilder {
    method: Method,
    uri: String,
    headers: HashMap<String, String>,
    body: Option<Bytes>,
}

impl RequestBuilder {
    pub fn new() -> Self {
        Self {
            method: Method::GET,
            uri: "/".to_string(),
            headers: HashMap::new(),
            body: None,
        }
    }

    pub fn method(mut self, method: Method) -> Self {
        self.method = method;
        self
    }

    pub fn uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = uri.into();
        self
    }

    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    pub fn body(mut self, body: Bytes) -> Self {
        self.body = Some(body);
        self
    }

    pub fn build(self) -> Request<Full<Bytes>> {
        let mut req = Request::builder().method(self.method).uri(self.uri);

        for (k, v) in self.headers {
            req = req.header(k, v);
        }

        let body = self.body.unwrap_or_else(|| Bytes::new());
        req.body(Full::new(body)).unwrap()
    }
}

impl Default for RequestBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Upstream instance builder for tests
pub struct UpstreamBuilder {
    id: String,
    host: String,
    port: u16,
    protocol: String,
    weight: u32,
    metadata: HashMap<String, String>,
}

impl UpstreamBuilder {
    pub fn new() -> Self {
        Self {
            id: "test-upstream".to_string(),
            host: "localhost".to_string(),
            port: 8080,
            protocol: "http".to_string(),
            weight: 1,
            metadata: HashMap::new(),
        }
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn protocol(mut self, protocol: impl Into<String>) -> Self {
        self.protocol = protocol.into();
        self
    }

    pub fn weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.metadata.insert("version".to_string(), version.into());
        self
    }

    pub fn build(self) -> UpstreamInstance {
        // Address should just be the host, not host:port
        let mut instance = UpstreamInstance::new(&self.id, &self.host, self.port);
        instance.weight = self.weight;
        instance.metadata = self.metadata;
        instance
    }
}

impl Default for UpstreamBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_builder() {
        let req = TestFixtures::request()
            .method(Method::POST)
            .uri("/api/test")
            .header("Content-Type", "application/json")
            .body(Bytes::from("test"))
            .build();

        assert_eq!(req.method(), Method::POST);
        assert_eq!(req.uri().path(), "/api/test");
        assert_eq!(
            req.headers().get("content-type").unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_upstream_builder() {
        let upstream = TestFixtures::upstream()
            .id("test-1")
            .host("example.com")
            .port(9090)
            .weight(10)
            .version("v2")
            .build();

        assert_eq!(upstream.id, "test-1");
        assert_eq!(upstream.address, "example.com");
        assert_eq!(upstream.port, 9090);
        assert_eq!(upstream.weight, 10);
        assert_eq!(upstream.metadata.get("version"), Some(&"v2".to_string()));
    }

    #[test]
    fn test_body_generation() {
        let body = TestFixtures::body(100);
        assert_eq!(body.len(), 100);

        let random_body = TestFixtures::random_body(100);
        assert_eq!(random_body.len(), 100);
    }
}
