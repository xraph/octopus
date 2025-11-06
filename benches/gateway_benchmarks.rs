// Performance benchmarks for Octopus API Gateway
//
// Run with: cargo bench --bench gateway_benchmarks

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use http::{Request, StatusCode};
use http_body_util::Full;
use bytes::Bytes;
use octopus_middleware::{JwtAuth, RateLimit, Compression};
use octopus_core::{Middleware, Next};
use std::sync::Arc;
use tokio::runtime::Runtime;

type Body = Full<Bytes>;

// Mock handler for benchmarking
#[derive(Debug, Clone)]
struct BenchHandler;

#[async_trait::async_trait]
impl Middleware for BenchHandler {
    async fn call(&self, _req: Request<Body>, _next: Next) -> octopus_core::Result<http::Response<Body>> {
        Ok(http::Response::builder()
            .status(StatusCode::OK)
            .body(Full::new(Bytes::from("benchmark response")))
            .unwrap())
    }
}

fn benchmark_jwt_auth(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    // Create valid test token
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde::{Serialize, Deserialize};
    use std::time::{SystemTime, UNIX_EPOCH};
    
    #[derive(Serialize, Deserialize)]
    struct Claims {
        sub: String,
        exp: usize,
    }
    
    let secret = "benchmark-secret-key-32-bytes-long";
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;
    
    let claims = Claims {
        sub: "bench-user".to_string(),
        exp: now + 3600,
    };
    
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();
    
    let jwt_auth = JwtAuth::new(secret);
    let handler = BenchHandler;
    
    let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([
        Arc::new(jwt_auth),
        Arc::new(handler),
    ]);
    
    let mut group = c.benchmark_group("jwt_authentication");
    group.throughput(Throughput::Elements(1));
    
    group.bench_function("jwt_validation", |b| {
        b.to_async(&rt).iter(|| async {
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .uri("/api/test")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::from(""))
                .unwrap();
            
            black_box(next.run(req).await.unwrap())
        });
    });
    
    group.finish();
}

fn benchmark_rate_limiting(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    let rate_limit = RateLimit::per_second(10000); // High limit for benchmarking
    let handler = BenchHandler;
    
    let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([
        Arc::new(rate_limit),
        Arc::new(handler),
    ]);
    
    let mut group = c.benchmark_group("rate_limiting");
    group.throughput(Throughput::Elements(1));
    
    group.bench_function("rate_limit_check", |b| {
        b.to_async(&rt).iter(|| async {
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .uri("/api/test")
                .body(Body::from(""))
                .unwrap();
            
            black_box(next.run(req).await.unwrap())
        });
    });
    
    group.finish();
}

fn benchmark_compression(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    let compression = Compression::new();
    let handler = BenchHandler;
    
    let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([
        Arc::new(compression),
        Arc::new(handler),
    ]);
    
    let mut group = c.benchmark_group("compression");
    
    // Test different payload sizes
    for size in [1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            size,
            |b, &size| {
                let payload = "x".repeat(size);
                b.to_async(&rt).iter(|| {
                    let payload = payload.clone();
                    async move {
                        let next = Next::new(stack.clone());
                        let req = Request::builder()
                            .uri("/api/test")
                            .header("Accept-Encoding", "gzip")
                            .body(Body::from(payload))
                            .unwrap();
                        
                        black_box(next.run(req).await.unwrap())
                    }
                });
            },
        );
    }
    
    group.finish();
}

fn benchmark_middleware_stack(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    // Full middleware stack
    let jwt_auth = JwtAuth::new("benchmark-secret-key");
    let rate_limit = RateLimit::per_second(10000);
    let compression = Compression::new();
    let handler = BenchHandler;
    
    let stack: Arc<[Arc<dyn Middleware>]> = Arc::new([
        Arc::new(jwt_auth),
        Arc::new(rate_limit),
        Arc::new(compression),
        Arc::new(handler),
    ]);
    
    // Create valid token
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde::{Serialize, Deserialize};
    use std::time::{SystemTime, UNIX_EPOCH};
    
    #[derive(Serialize, Deserialize)]
    struct Claims {
        sub: String,
        exp: usize,
    }
    
    let secret = "benchmark-secret-key";
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;
    
    let claims = Claims {
        sub: "bench-user".to_string(),
        exp: now + 3600,
    };
    
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap();
    
    let mut group = c.benchmark_group("full_middleware_stack");
    group.throughput(Throughput::Elements(1));
    
    group.bench_function("complete_request", |b| {
        b.to_async(&rt).iter(|| async {
            let next = Next::new(stack.clone());
            let req = Request::builder()
                .uri("/api/test")
                .header("Authorization", format!("Bearer {}", token))
                .header("Accept-Encoding", "gzip")
                .body(Body::from("test payload"))
                .unwrap();
            
            black_box(next.run(req).await.unwrap())
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    benchmark_jwt_auth,
    benchmark_rate_limiting,
    benchmark_compression,
    benchmark_middleware_stack
);
criterion_main!(benches);

