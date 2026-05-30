//! Request deduplication / idempotency middleware

use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use http::{Request, Response, StatusCode};
use http_body_util::Full;
use octopus_core::{Middleware, Next, Result};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Body type alias
pub type Body = Full<Bytes>;

/// A cached response entry
struct CachedEntry {
    status: u16,
    headers: Vec<(String, String)>,
    body: Bytes,
    created_at: Instant,
}

/// Deduplication configuration
#[derive(Debug, Clone)]
pub struct DeduplicationConfig {
    /// Header name containing the idempotency key (default: "Idempotency-Key")
    pub header_name: String,
    /// Time-to-live for cached entries (default: 24 hours)
    pub ttl: Duration,
    /// HTTP methods subject to deduplication (default: POST, PUT, PATCH)
    pub methods: Vec<String>,
    /// Maximum number of cached entries (default: 10000)
    pub max_entries: usize,
}

impl Default for DeduplicationConfig {
    fn default() -> Self {
        Self {
            header_name: "Idempotency-Key".to_string(),
            ttl: Duration::from_secs(24 * 60 * 60),
            methods: vec![
                "POST".to_string(),
                "PUT".to_string(),
                "PATCH".to_string(),
            ],
            max_entries: 10_000,
        }
    }
}

/// Request deduplication middleware
///
/// Caches responses keyed by an idempotency header so that duplicate
/// requests with the same key receive the cached response instead of
/// being processed again.
#[derive(Clone)]
pub struct Deduplication {
    config: DeduplicationConfig,
    cache: Arc<DashMap<String, CachedEntry>>,
    request_counter: Arc<AtomicU64>,
}

impl Deduplication {
    /// Create a new Deduplication middleware with default config
    pub fn new() -> Self {
        Self::with_config(DeduplicationConfig::default())
    }

    /// Create a new Deduplication middleware with custom config
    pub fn with_config(config: DeduplicationConfig) -> Self {
        Self {
            config,
            cache: Arc::new(DashMap::new()),
            request_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Check if the request method is subject to deduplication
    fn is_dedup_method(&self, method: &str) -> bool {
        self.config
            .methods
            .iter()
            .any(|m| m.eq_ignore_ascii_case(method))
    }

    /// Periodically evict expired entries.
    /// Runs cleanup every 100 requests to amortise the cost.
    fn maybe_evict(&self) {
        let count = self.request_counter.fetch_add(1, Ordering::Relaxed);
        if count % 100 != 0 {
            return;
        }

        let now = Instant::now();
        self.cache
            .retain(|_, entry| now.duration_since(entry.created_at) < self.config.ttl);

        // If still over capacity, remove oldest entries
        if self.cache.len() > self.config.max_entries {
            let mut entries: Vec<(String, Instant)> = self
                .cache
                .iter()
                .map(|e| (e.key().clone(), e.value().created_at))
                .collect();
            entries.sort_by_key(|(_, ts)| *ts);

            let to_remove = self.cache.len() - self.config.max_entries;
            for (key, _) in entries.into_iter().take(to_remove) {
                self.cache.remove(&key);
            }
        }
    }

    /// Build a response from a cached entry, including the replay header
    fn response_from_cache(entry: &CachedEntry) -> Response<Body> {
        let status = StatusCode::from_u16(entry.status).unwrap_or(StatusCode::OK);
        let mut builder = Response::builder()
            .status(status)
            .header("X-Idempotent-Replayed", "true");

        for (name, value) in &entry.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }

        builder
            .body(Full::new(entry.body.clone()))
            .expect("Failed to build cached response")
    }
}

impl Default for Deduplication {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Deduplication {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Deduplication")
            .field("header_name", &self.config.header_name)
            .field("ttl", &self.config.ttl)
            .field("methods", &self.config.methods)
            .field("max_entries", &self.config.max_entries)
            .field("cached_entries", &self.cache.len())
            .finish()
    }
}

#[async_trait]
impl Middleware for Deduplication {
    async fn call(&self, req: Request<Body>, next: Next) -> Result<Response<Body>> {
        // Only apply deduplication to configured methods
        if !self.is_dedup_method(req.method().as_str()) {
            return next.run(req).await;
        }

        // Extract idempotency key; if absent, pass through without dedup
        let key = match req
            .headers()
            .get(&self.config.header_name)
            .and_then(|v| v.to_str().ok())
        {
            Some(k) => k.to_string(),
            None => return next.run(req).await,
        };

        // Periodic cleanup
        self.maybe_evict();

        // Check cache for existing entry
        if let Some(entry) = self.cache.get(&key) {
            if entry.created_at.elapsed() < self.config.ttl {
                tracing::debug!(idempotency_key = %key, "Returning cached idempotent response");
                return Ok(Self::response_from_cache(&entry));
            }
            // Expired — drop the ref so we can remove below
            drop(entry);
            self.cache.remove(&key);
        }

        // Execute the actual request
        let response = next.run(req).await?;

        // Cache the response
        let status = response.status().as_u16();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|v| (name.to_string(), v.to_string()))
            })
            .collect();

        // Extract body bytes so we can clone them into the cache
        let (parts, body) = response.into_parts();
        let body_bytes = http_body_util::BodyExt::collect(body)
            .await
            .map(|buf| buf.to_bytes())
            .unwrap_or_default();

        self.cache.insert(
            key,
            CachedEntry {
                status,
                headers,
                body: body_bytes.clone(),
                created_at: Instant::now(),
            },
        );

        // Reconstruct the response
        let response = Response::from_parts(parts, Full::new(body_bytes));
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use octopus_core::Error;
    use std::sync::Arc;

    #[derive(Debug)]
    struct CountingHandler {
        call_count: Arc<AtomicU64>,
    }

    impl CountingHandler {
        fn new() -> (Self, Arc<AtomicU64>) {
            let count = Arc::new(AtomicU64::new(0));
            (
                Self {
                    call_count: count.clone(),
                },
                count,
            )
        }
    }

    #[async_trait]
    impl Middleware for CountingHandler {
        async fn call(&self, _req: Request<Body>, _next: Next) -> Result<Response<Body>> {
            let n = self.call_count.fetch_add(1, Ordering::Relaxed) + 1;
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(format!("response-{n}"))))
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    fn make_stack(
        dedup: Deduplication,
        handler: CountingHandler,
    ) -> Arc<[Arc<dyn Middleware>]> {
        Arc::new([
            Arc::new(dedup) as Arc<dyn Middleware>,
            Arc::new(handler) as Arc<dyn Middleware>,
        ])
    }

    #[tokio::test]
    async fn test_first_request_is_cached() {
        let dedup = Deduplication::new();
        let (handler, call_count) = CountingHandler::new();
        let stack = make_stack(dedup, handler);

        let next = Next::new(stack.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/submit")
            .header("Idempotency-Key", "key-1")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"response-1");
    }

    #[tokio::test]
    async fn test_duplicate_returns_cached_response() {
        let dedup = Deduplication::new();
        let (handler, call_count) = CountingHandler::new();
        let stack = make_stack(dedup, handler);

        // First request
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/submit")
            .header("Idempotency-Key", "key-dup")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"response-1");
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        // Duplicate request with same key
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/submit")
            .header("Idempotency-Key", "key-dup")
            .body(Body::from(""))
            .unwrap();

        let response = next.run(req).await.unwrap();
        assert_eq!(
            response.headers().get("X-Idempotent-Replayed").unwrap(),
            "true"
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"response-1");
        // Handler was NOT called again
        assert_eq!(call_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_different_keys_are_independent() {
        let dedup = Deduplication::new();
        let (handler, call_count) = CountingHandler::new();
        let stack = make_stack(dedup, handler);

        // Request with key-a
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/submit")
            .header("Idempotency-Key", "key-a")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"response-1");

        // Request with key-b
        let next = Next::new(stack.clone());
        let req = Request::builder()
            .method("POST")
            .uri("/submit")
            .header("Idempotency-Key", "key-b")
            .body(Body::from(""))
            .unwrap();
        let response = next.run(req).await.unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"response-2");

        // Both keys triggered separate handler calls
        assert_eq!(call_count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn test_get_requests_skip_dedup() {
        let dedup = Deduplication::new();
        let (handler, call_count) = CountingHandler::new();
        let stack = make_stack(dedup, handler);

        // GET requests are not deduplicated even with idempotency key
        for _ in 0..3 {
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .method("GET")
                .uri("/resource")
                .header("Idempotency-Key", "key-get")
                .body(Body::from(""))
                .unwrap();
            next.run(req).await.unwrap();
        }

        // Handler was called every time
        assert_eq!(call_count.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn test_no_key_header_skips_dedup() {
        let dedup = Deduplication::new();
        let (handler, call_count) = CountingHandler::new();
        let stack = make_stack(dedup, handler);

        // POST without idempotency key
        for _ in 0..2 {
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .method("POST")
                .uri("/submit")
                .body(Body::from(""))
                .unwrap();
            next.run(req).await.unwrap();
        }

        // Handler was called every time (no dedup without key)
        assert_eq!(call_count.load(Ordering::Relaxed), 2);
    }
}
