//! Response caching middleware
//!
//! Caches HTTP responses based on method, path, query, and configurable headers.
//! Supports in-memory storage with TTL-based expiration and FIFO eviction.

use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use http::header::HeaderMap;
use http::{Method, Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Body type alias
pub type Body = Full<Bytes>;

/// Caching configuration
#[derive(Debug, Clone)]
pub struct CachingConfig {
    /// Whether caching is enabled
    pub enabled: bool,
    /// Default TTL for cached responses
    pub default_ttl: Duration,
    /// Maximum number of entries in the cache
    pub max_entries: usize,
    /// HTTP methods eligible for caching
    pub cacheable_methods: Vec<Method>,
    /// Status code range eligible for caching (inclusive)
    pub cacheable_status_min: u16,
    /// Status code range eligible for caching (inclusive)
    pub cacheable_status_max: u16,
    /// Headers to include in cache key generation (Vary)
    pub vary_by_headers: Vec<String>,
}

impl Default for CachingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_ttl: Duration::from_secs(60),
            max_entries: 10_000,
            cacheable_methods: vec![Method::GET, Method::HEAD],
            cacheable_status_min: 200,
            cacheable_status_max: 399,
            vary_by_headers: Vec::new(),
        }
    }
}

/// A cached HTTP response
#[derive(Debug, Clone)]
pub struct CachedResponse {
    /// HTTP status code
    pub status: StatusCode,
    /// Response headers
    pub headers: HeaderMap,
    /// Response body
    pub body: Bytes,
    /// When this entry was cached
    pub cached_at: Instant,
    /// TTL for this entry
    pub ttl: Duration,
}

impl CachedResponse {
    /// Check if the cached response has expired
    pub fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }
}

/// Cache store trait for pluggable backends
#[async_trait]
pub trait CacheStore: Send + Sync + fmt::Debug {
    /// Get a cached response by key
    async fn get(&self, key: &str) -> Option<CachedResponse>;
    /// Store a response
    async fn set(&self, key: &str, resp: CachedResponse);
    /// Delete a cached response
    async fn delete(&self, key: &str);
    /// Get the number of entries
    async fn len(&self) -> usize;
    /// Returns `true` if the cache has no entries
    async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

/// In-memory cache store using DashMap
#[derive(Debug)]
pub struct InMemoryCacheStore {
    items: Arc<DashMap<String, CachedResponse>>,
    insertion_order: Arc<parking_lot::Mutex<VecDeque<String>>>,
    max_entries: usize,
}

impl InMemoryCacheStore {
    /// Create a new in-memory cache store
    pub fn new(max_entries: usize) -> Self {
        Self {
            items: Arc::new(DashMap::new()),
            insertion_order: Arc::new(parking_lot::Mutex::new(VecDeque::new())),
            max_entries,
        }
    }
}

#[async_trait]
impl CacheStore for InMemoryCacheStore {
    async fn get(&self, key: &str) -> Option<CachedResponse> {
        if let Some(entry) = self.items.get(key) {
            if entry.is_expired() {
                // Lazily remove expired entries
                drop(entry);
                self.items.remove(key);
                return None;
            }
            Some(entry.clone())
        } else {
            None
        }
    }

    async fn set(&self, key: &str, resp: CachedResponse) {
        // Evict if at capacity (FIFO)
        while self.items.len() >= self.max_entries {
            let mut order = self.insertion_order.lock();
            if let Some(oldest_key) = order.pop_front() {
                self.items.remove(&oldest_key);
            } else {
                break;
            }
        }

        self.items.insert(key.to_string(), resp);
        self.insertion_order.lock().push_back(key.to_string());
    }

    async fn delete(&self, key: &str) {
        self.items.remove(key);
    }

    async fn len(&self) -> usize {
        self.items.len()
    }
}

/// Response caching middleware
#[derive(Clone)]
pub struct Caching {
    config: CachingConfig,
    store: Arc<dyn CacheStore>,
}

impl Caching {
    /// Create a new Caching middleware with default in-memory store
    pub fn new() -> Self {
        Self::with_config(CachingConfig::default())
    }

    /// Create with custom config and default in-memory store
    pub fn with_config(config: CachingConfig) -> Self {
        let store = Arc::new(InMemoryCacheStore::new(config.max_entries));
        Self { config, store }
    }

    /// Create with custom config and store
    pub fn with_store(config: CachingConfig, store: Arc<dyn CacheStore>) -> Self {
        Self { config, store }
    }

    /// Generate a cache key from the request
    fn cache_key(&self, req: &Request<Body>) -> String {
        let mut hasher = Sha256::new();

        hasher.update(req.method().as_str().as_bytes());
        hasher.update(b"|");
        hasher.update(req.uri().path().as_bytes());
        hasher.update(b"|");
        if let Some(query) = req.uri().query() {
            hasher.update(query.as_bytes());
        }
        hasher.update(b"|");

        // Sort vary headers for consistent keys
        let mut vary_values: Vec<(String, String)> = self
            .config
            .vary_by_headers
            .iter()
            .filter_map(|name| {
                req.headers()
                    .get(name.as_str())
                    .and_then(|v| v.to_str().ok())
                    .map(|v| (name.to_lowercase(), v.to_string()))
            })
            .collect();
        vary_values.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, value) in &vary_values {
            hasher.update(name.as_bytes());
            hasher.update(b"=");
            hasher.update(value.as_bytes());
            hasher.update(b"&");
        }

        hex::encode(hasher.finalize())
    }

    /// Check if a method is cacheable
    fn is_cacheable_method(&self, method: &Method) -> bool {
        self.config.cacheable_methods.contains(method)
    }

    /// Check if a status is cacheable
    fn is_cacheable_status(&self, status: StatusCode) -> bool {
        let code = status.as_u16();
        code >= self.config.cacheable_status_min && code <= self.config.cacheable_status_max
    }

    /// Extract TTL from Cache-Control max-age or use default
    fn extract_ttl(&self, headers: &HeaderMap) -> Option<Duration> {
        if let Some(cc) = headers.get("cache-control") {
            if let Ok(cc_str) = cc.to_str() {
                let lower = cc_str.to_lowercase();
                // Don't cache if no-store or private
                if lower.contains("no-store") || lower.contains("private") {
                    return None;
                }
                // Don't cache if no-cache (must revalidate)
                if lower.contains("no-cache") {
                    return None;
                }
                // Extract max-age
                for part in lower.split(',') {
                    let part = part.trim();
                    if let Some(age_str) = part.strip_prefix("max-age=") {
                        if let Ok(secs) = age_str.trim().parse::<u64>() {
                            return Some(Duration::from_secs(secs));
                        }
                    }
                }
            }
        }
        Some(self.config.default_ttl)
    }

    /// Check if the request itself has Cache-Control: no-cache
    fn request_bypasses_cache(req: &Request<Body>) -> bool {
        if let Some(cc) = req.headers().get("cache-control") {
            if let Ok(cc_str) = cc.to_str() {
                let lower = cc_str.to_lowercase();
                return lower.contains("no-cache") || lower.contains("no-store");
            }
        }
        false
    }
}

impl Default for Caching {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Caching {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Caching")
            .field("enabled", &self.config.enabled)
            .field("default_ttl", &self.config.default_ttl)
            .field("max_entries", &self.config.max_entries)
            .finish()
    }
}

#[async_trait]
impl Middleware for Caching {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        if !self.config.enabled {
            return next.run(req).await;
        }

        // Only cache eligible methods
        if !self.is_cacheable_method(req.method()) {
            return next.run(req).await;
        }

        // Check if request explicitly bypasses cache
        if Self::request_bypasses_cache(&req) {
            let mut resp = next.run(req).await?;
            resp.headers_mut()
                .insert("X-Cache", http::header::HeaderValue::from_static("BYPASS"));
            return Ok(resp);
        }

        let key = self.cache_key(&req);

        // Try cache lookup
        if let Some(cached) = self.store.get(&key).await {
            // Build response from cache
            let mut builder = Response::builder().status(cached.status);
            for (name, value) in cached.headers.iter() {
                builder = builder.header(name, value);
            }
            let mut resp = builder
                .body(Full::new(cached.body))
                .expect("Failed to build cached response");
            resp.headers_mut()
                .insert("X-Cache", http::header::HeaderValue::from_static("HIT"));
            return Ok(resp);
        }

        // Cache miss — forward request
        let resp = next.run(req).await?;

        // Check if response is cacheable
        if self.is_cacheable_status(resp.status()) {
            if let Some(ttl) = self.extract_ttl(resp.headers()) {
                // Collect response body for caching
                use http_body_util::BodyExt;
                let status = resp.status();
                let headers = resp.headers().clone();
                let body_bytes = resp
                    .into_body()
                    .collect()
                    .await
                    .map(|c| c.to_bytes())
                    .unwrap_or_default();

                let cached = CachedResponse {
                    status,
                    headers: headers.clone(),
                    body: body_bytes.clone(),
                    cached_at: Instant::now(),
                    ttl,
                };
                self.store.set(&key, cached).await;

                // Rebuild response with MISS header
                let mut builder = Response::builder().status(status);
                for (name, value) in headers.iter() {
                    builder = builder.header(name, value);
                }
                let mut resp = builder
                    .body(Full::new(body_bytes))
                    .expect("Failed to build response");
                resp.headers_mut()
                    .insert("X-Cache", http::header::HeaderValue::from_static("MISS"));
                return Ok(resp);
            }
        }

        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octopus_core::Error;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Debug)]
    struct CountingHandler {
        call_count: Arc<AtomicU32>,
        status: StatusCode,
        cache_control: Option<String>,
    }

    impl CountingHandler {
        fn new() -> Self {
            Self {
                call_count: Arc::new(AtomicU32::new(0)),
                status: StatusCode::OK,
                cache_control: None,
            }
        }

        fn with_status(mut self, status: StatusCode) -> Self {
            self.status = status;
            self
        }

        fn with_cache_control(mut self, cc: &str) -> Self {
            self.cache_control = Some(cc.to_string());
            self
        }
    }

    #[async_trait]
    impl Middleware for CountingHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            let mut builder = Response::builder().status(self.status);
            if let Some(ref cc) = self.cache_control {
                builder = builder.header("Cache-Control", cc.as_str());
            }
            builder
                .body(Full::new(Bytes::from(format!("response-{count}"))))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    fn make_stack(caching: Caching, handler: CountingHandler) -> Arc<[Arc<dyn Middleware>]> {
        Arc::new([
            Arc::new(caching) as Arc<dyn Middleware>,
            Arc::new(handler) as Arc<dyn Middleware>,
        ])
    }

    fn get_req(path: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(path)
            .body(Body::from(""))
            .unwrap()
    }

    #[tokio::test]
    async fn test_cache_miss_forwards_request() {
        let handler = CountingHandler::new();
        let count = handler.call_count.clone();
        let stack = make_stack(Caching::new(), handler);

        let next = Next::new(stack);
        let resp = next.run(get_req("/test")).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("X-Cache").unwrap(), "MISS");
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_cache_hit_returns_cached_response() {
        let handler = CountingHandler::new();
        let count = handler.call_count.clone();
        let stack = make_stack(Caching::new(), handler);

        // First request → MISS
        let next = Next::new(stack.clone());
        let _ = next.run(get_req("/test")).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);

        // Second request → HIT (handler not called)
        let next = Next::new(stack);
        let resp = next.run(get_req("/test")).await.unwrap();
        assert_eq!(resp.headers().get("X-Cache").unwrap(), "HIT");
        assert_eq!(count.load(Ordering::SeqCst), 1); // Still 1, not called again
    }

    #[tokio::test]
    async fn test_cache_respects_no_store() {
        let handler = CountingHandler::new().with_cache_control("no-store");
        let count = handler.call_count.clone();
        let stack = make_stack(Caching::new(), handler);

        // First request
        let next = Next::new(stack.clone());
        let _ = next.run(get_req("/test")).await.unwrap();

        // Second request → should NOT be cached
        let next = Next::new(stack);
        let _ = next.run(get_req("/test")).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2); // Called twice
    }

    #[tokio::test]
    async fn test_cache_respects_private() {
        let handler = CountingHandler::new().with_cache_control("private");
        let count = handler.call_count.clone();
        let stack = make_stack(Caching::new(), handler);

        let next = Next::new(stack.clone());
        let _ = next.run(get_req("/test")).await.unwrap();

        let next = Next::new(stack);
        let _ = next.run(get_req("/test")).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_cache_respects_no_cache() {
        let handler = CountingHandler::new().with_cache_control("no-cache");
        let count = handler.call_count.clone();
        let stack = make_stack(Caching::new(), handler);

        let next = Next::new(stack.clone());
        let _ = next.run(get_req("/test")).await.unwrap();

        let next = Next::new(stack);
        let _ = next.run(get_req("/test")).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_cache_only_cacheable_methods() {
        let handler = CountingHandler::new();
        let count = handler.call_count.clone();
        let stack = make_stack(Caching::new(), handler);

        // POST request should not be cached
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/test")
            .body(Body::from(""))
            .unwrap();
        let _ = next.run(req).await.unwrap();

        let next = Next::new(stack);
        let req = Request::builder()
            .method("POST")
            .uri("/test")
            .body(Body::from(""))
            .unwrap();
        let _ = next.run(req).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_cache_only_cacheable_statuses() {
        let handler = CountingHandler::new().with_status(StatusCode::INTERNAL_SERVER_ERROR);
        let count = handler.call_count.clone();
        let stack = make_stack(Caching::new(), handler);

        let next = Next::new(stack.clone());
        let _ = next.run(get_req("/test")).await.unwrap();

        let next = Next::new(stack);
        let _ = next.run(get_req("/test")).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2); // 500 not cached
    }

    #[tokio::test]
    async fn test_cache_different_query_different_key() {
        let handler = CountingHandler::new();
        let count = handler.call_count.clone();
        let stack = make_stack(Caching::new(), handler);

        let next = Next::new(stack.clone());
        let _ = next.run(get_req("/test?a=1")).await.unwrap();

        let next = Next::new(stack);
        let _ = next.run(get_req("/test?a=2")).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2); // Different keys
    }

    #[tokio::test]
    async fn test_cache_vary_by_headers() {
        let config = CachingConfig {
            vary_by_headers: vec!["Accept".to_string()],
            ..Default::default()
        };
        let handler = CountingHandler::new();
        let count = handler.call_count.clone();
        let stack = make_stack(Caching::with_config(config), handler);

        // Request with Accept: json
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .method("GET")
            .uri("/test")
            .header("Accept", "application/json")
            .body(Body::from(""))
            .unwrap();
        let _ = next.run(req).await.unwrap();

        // Same path, different Accept → different key
        let next = Next::new(stack);
        let req = Request::builder()
            .method("GET")
            .uri("/test")
            .header("Accept", "text/html")
            .body(Body::from(""))
            .unwrap();
        let _ = next.run(req).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_cache_eviction_at_max_entries() {
        let config = CachingConfig {
            max_entries: 2,
            ..Default::default()
        };
        let handler = CountingHandler::new();
        let stack = make_stack(Caching::with_config(config), handler);

        // Fill cache with 2 entries
        let next = Next::new(stack.clone());
        let _ = next.run(get_req("/a")).await.unwrap();
        let next = Next::new(stack.clone());
        let _ = next.run(get_req("/b")).await.unwrap();

        // Third entry evicts first
        let next = Next::new(stack.clone());
        let _ = next.run(get_req("/c")).await.unwrap();

        // /a should be evicted (MISS again)
        let next = Next::new(stack);
        let resp = next.run(get_req("/a")).await.unwrap();
        assert_eq!(resp.headers().get("X-Cache").unwrap(), "MISS");
    }

    #[tokio::test]
    async fn test_disabled_bypasses_cache() {
        let config = CachingConfig {
            enabled: false,
            ..Default::default()
        };
        let handler = CountingHandler::new();
        let count = handler.call_count.clone();
        let stack = make_stack(Caching::with_config(config), handler);

        let next = Next::new(stack.clone());
        let resp = next.run(get_req("/test")).await.unwrap();
        assert!(resp.headers().get("X-Cache").is_none());

        let next = Next::new(stack);
        let _ = next.run(get_req("/test")).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_request_no_cache_bypasses() {
        let handler = CountingHandler::new();
        let count = handler.call_count.clone();
        let stack = make_stack(Caching::new(), handler);

        // First request, populate cache
        let next = Next::new(stack.clone());
        let _ = next.run(get_req("/test")).await.unwrap();

        // Second request with Cache-Control: no-cache → bypass
        let next = Next::new(stack);
        let req = Request::builder()
            .method("GET")
            .uri("/test")
            .header("Cache-Control", "no-cache")
            .body(Body::from(""))
            .unwrap();
        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.headers().get("X-Cache").unwrap(), "BYPASS");
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_in_memory_store_concurrent_access() {
        let store = InMemoryCacheStore::new(100);
        let store = Arc::new(store);

        let mut handles = Vec::new();
        for i in 0..10 {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                let key = format!("key-{i}");
                let resp = CachedResponse {
                    status: StatusCode::OK,
                    headers: HeaderMap::new(),
                    body: Bytes::from(format!("body-{i}")),
                    cached_at: Instant::now(),
                    ttl: Duration::from_secs(60),
                };
                s.set(&key, resp).await;
                s.get(&key).await.unwrap()
            }));
        }

        let results: Vec<_> = futures::future::join_all(handles).await;
        assert!(results.iter().all(|r| r.is_ok()));
        assert_eq!(store.len().await, 10);
    }
}
