//! Middleware chain benchmarks for the Octopus API Gateway.
//!
//! Each group times one request through a middleware stack that ends in a
//! fixed handler, so the numbers are the middleware's own cost with no
//! network or upstream in the way.
//!
//! Run with: cargo bench -p octopus-middleware --bench gateway_benchmarks

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use http::{header, Request, Response, StatusCode};
use http_body_util::Full;
use jsonwebtoken::{encode, EncodingKey, Header};
use octopus_core::middleware::Body;
use octopus_middleware::{Compression, JwtAuth, Middleware, Next, RateLimit};
use serde::Serialize;
use std::hint::black_box;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;

const SECRET: &str = "benchmark-secret-key-32-bytes-long";

/// Terminal handler: answers every request with a fixed body.
#[derive(Debug, Clone)]
struct BenchHandler {
    body: Bytes,
}

impl BenchHandler {
    fn new(body: impl Into<Bytes>) -> Self {
        Self { body: body.into() }
    }
}

#[async_trait::async_trait]
impl Middleware for BenchHandler {
    async fn call(&self, _req: Request<Body>, _next: Next) -> octopus_core::Result<Response<Body>> {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Full::new(self.body.clone()))
            .expect("static response is valid"))
    }
}

type Stack = Arc<[Arc<dyn Middleware>]>;

fn stack(layers: Vec<Arc<dyn Middleware>>) -> Stack {
    layers.into()
}

fn request(token: Option<&str>, accept_encoding: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().uri("/api/test");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(encoding) = accept_encoding {
        builder = builder.header(header::ACCEPT_ENCODING, encoding);
    }
    builder
        .body(Body::default())
        .expect("static request is valid")
}

/// A token `JwtAuth::new(SECRET)` accepts for the next hour.
fn valid_token() -> String {
    #[derive(Serialize)]
    struct Claims {
        sub: String,
        exp: usize,
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after the epoch")
        .as_secs() as usize;
    let claims = Claims {
        sub: "bench-user".to_string(),
        exp: now + 3600,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(SECRET.as_bytes()),
    )
    .expect("token encodes")
}

/// Send one request before timing anything. A stack that answers 401 or 429
/// would otherwise get benchmarked as if it were the happy path.
fn assert_ok(rt: &Runtime, stack: &Stack, req: Request<Body>) -> Response<Body> {
    let response = rt
        .block_on(Next::new(stack.clone()).run(req))
        .expect("middleware chain runs");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "bench stack must succeed"
    );
    response
}

/// The handler alone, as a floor to read the other groups against.
fn benchmark_baseline(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let stack = stack(vec![Arc::new(BenchHandler::new("ok"))]);
    assert_ok(&rt, &stack, request(None, None));

    let mut group = c.benchmark_group("baseline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("handler_only", |b| {
        b.to_async(&rt)
            .iter(|| async { black_box(Next::new(stack.clone()).run(request(None, None)).await) });
    });
    group.finish();
}

fn benchmark_jwt_auth(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let token = valid_token();
    let stack = stack(vec![
        Arc::new(JwtAuth::new(SECRET)),
        Arc::new(BenchHandler::new("ok")),
    ]);
    assert_ok(&rt, &stack, request(Some(&token), None));

    let mut group = c.benchmark_group("jwt_authentication");
    group.throughput(Throughput::Elements(1));
    group.bench_function("jwt_validation", |b| {
        b.to_async(&rt).iter(|| async {
            black_box(
                Next::new(stack.clone())
                    .run(request(Some(&token), None))
                    .await,
            )
        });
    });
    group.finish();
}

fn benchmark_rate_limiting(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    // Criterion runs millions of iterations a second. Any realistic quota runs
    // dry in the first few milliseconds and the rest would time the 429 path,
    // so the quota is set out of reach to keep every check on the allow path.
    let stack = stack(vec![
        Arc::new(RateLimit::per_second(u32::MAX)),
        Arc::new(BenchHandler::new("ok")),
    ]);
    assert_ok(&rt, &stack, request(None, None));

    let mut group = c.benchmark_group("rate_limiting");
    group.throughput(Throughput::Elements(1));
    group.bench_function("rate_limit_check", |b| {
        b.to_async(&rt)
            .iter(|| async { black_box(Next::new(stack.clone()).run(request(None, None)).await) });
    });
    group.finish();
}

fn benchmark_compression(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("compression");

    // Compression works on the response, so the handler produces the payload.
    // Every size here clears the middleware's 1 KB minimum.
    for size in [1024usize, 10 * 1024, 100 * 1024] {
        let stack = stack(vec![
            Arc::new(Compression::new()),
            Arc::new(BenchHandler::new("x".repeat(size))),
        ]);
        let response = assert_ok(&rt, &stack, request(None, Some("gzip")));
        assert_eq!(
            response.headers().get(header::CONTENT_ENCODING),
            Some(&header::HeaderValue::from_static("gzip")),
            "{size} byte response should come back gzipped"
        );

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("gzip", size), &stack, |b, stack| {
            b.to_async(&rt).iter(|| async {
                black_box(
                    Next::new(stack.clone())
                        .run(request(None, Some("gzip")))
                        .await,
                )
            });
        });
    }
    group.finish();
}

fn benchmark_middleware_stack(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let token = valid_token();
    let stack = stack(vec![
        Arc::new(JwtAuth::new(SECRET)),
        Arc::new(RateLimit::per_second(u32::MAX)),
        Arc::new(Compression::new()),
        Arc::new(BenchHandler::new("x".repeat(4 * 1024))),
    ]);
    assert_ok(&rt, &stack, request(Some(&token), Some("gzip")));

    let mut group = c.benchmark_group("full_middleware_stack");
    group.throughput(Throughput::Elements(1));
    group.bench_function("complete_request", |b| {
        b.to_async(&rt).iter(|| async {
            black_box(
                Next::new(stack.clone())
                    .run(request(Some(&token), Some("gzip")))
                    .await,
            )
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    benchmark_baseline,
    benchmark_jwt_auth,
    benchmark_rate_limiting,
    benchmark_compression,
    benchmark_middleware_stack
);
criterion_main!(benches);
