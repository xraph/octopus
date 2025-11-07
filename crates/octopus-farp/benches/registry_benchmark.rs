//! FARP Schema Registry Benchmarks
//!
//! Run with: cargo bench --package octopus-farp

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use octopus_farp::{
    SchemaDescriptor, SchemaEndpoints, SchemaLocation, SchemaManifest, SchemaRegistry, SchemaType,
};
use std::sync::Arc;

fn create_test_manifest(service_name: &str, instance_id: &str) -> SchemaManifest {
    let mut manifest = SchemaManifest::new(service_name, "1.0.0", instance_id);

    manifest.endpoints = SchemaEndpoints {
        health: "/health".to_string(),
        openapi: Some("/openapi.json".to_string()),
        ..Default::default()
    };

    manifest.add_capability("rest");

    // Add a schema
    let schema = SchemaDescriptor::new(
        SchemaType::OpenAPI,
        "3.1.0",
        SchemaLocation::http("http://localhost/openapi.json"),
        "application/json",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        1024,
    );
    manifest.add_schema(schema);
    manifest.update_checksum().unwrap();

    manifest
}

fn bench_register_service(c: &mut Criterion) {
    let registry = Arc::new(SchemaRegistry::new());

    c.bench_function("register_service", |b| {
        let mut counter = 0;
        b.iter(|| {
            let manifest = create_test_manifest("test-service", &format!("inst-{}", counter));
            registry.register_service(black_box(manifest)).unwrap();
            counter += 1;
        });
    });
}

fn bench_get_service(c: &mut Criterion) {
    let registry = Arc::new(SchemaRegistry::new());

    // Pre-populate with 100 services
    for i in 0..100 {
        let manifest = create_test_manifest(&format!("service-{}", i), &format!("inst-{}", i));
        registry.register_service(manifest).unwrap();
    }

    c.bench_function("get_service", |b| {
        b.iter(|| {
            let result = registry.get_service(black_box("service-50"));
            black_box(result);
        });
    });
}

fn bench_list_services(c: &mut Criterion) {
    let mut group = c.benchmark_group("list_services");

    for size in [10, 100, 1000].iter() {
        let registry = Arc::new(SchemaRegistry::new());

        // Pre-populate
        for i in 0..*size {
            let manifest = create_test_manifest(&format!("service-{}", i), &format!("inst-{}", i));
            registry.register_service(manifest).unwrap();
        }

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let services = registry.list_services();
                black_box(services);
            });
        });
    }
    group.finish();
}

fn bench_update_service(c: &mut Criterion) {
    let registry = Arc::new(SchemaRegistry::new());

    // Register initial service
    let manifest = create_test_manifest("test-service", "inst-1");
    registry.register_service(manifest).unwrap();

    c.bench_function("update_service", |b| {
        b.iter(|| {
            let mut manifest = create_test_manifest("test-service", "inst-1");
            manifest.service_version = "2.0.0".to_string();
            manifest.update_checksum().unwrap();
            registry.update_service(black_box(manifest)).unwrap();
        });
    });
}

fn bench_concurrent_registrations(c: &mut Criterion) {
    use std::thread;

    let mut group = c.benchmark_group("concurrent_registrations");

    for num_threads in [2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_threads),
            num_threads,
            |b, &num_threads| {
                b.iter(|| {
                    let registry = Arc::new(SchemaRegistry::new());
                    let mut handles = vec![];

                    for thread_id in 0..num_threads {
                        let registry = Arc::clone(&registry);
                        let handle = thread::spawn(move || {
                            for i in 0..10 {
                                let manifest = create_test_manifest(
                                    &format!("service-{}-{}", thread_id, i),
                                    &format!("inst-{}-{}", thread_id, i),
                                );
                                registry.register_service(manifest).unwrap();
                            }
                        });
                        handles.push(handle);
                    }

                    for handle in handles {
                        handle.join().unwrap();
                    }

                    black_box(registry);
                });
            },
        );
    }
    group.finish();
}

fn bench_rate_limiting(c: &mut Criterion) {
    // Test rate limiting overhead
    let registry = Arc::new(SchemaRegistry::with_rate_limit(1000)); // 1000/min = ~16/sec

    // Register initial service
    let manifest = create_test_manifest("test-service", "inst-1");
    registry.register_service(manifest).unwrap();

    c.bench_function("rate_limited_update", |b| {
        b.iter(|| {
            let mut manifest = create_test_manifest("test-service", "inst-1");
            manifest.service_version = format!("{}.0.0", fastrand::u32(..));
            manifest.update_checksum().unwrap();

            // This should succeed (under rate limit)
            let result = registry.update_service(black_box(manifest));
            black_box(result);
        });
    });
}

fn bench_manifest_checksum(c: &mut Criterion) {
    let mut manifest = create_test_manifest("test-service", "inst-1");

    // Add multiple schemas
    for i in 0..5 {
        let schema = SchemaDescriptor::new(
            SchemaType::OpenAPI,
            "3.1.0",
            SchemaLocation::http(&format!("http://localhost/api-{}.json", i)),
            "application/json",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            1024,
        );
        manifest.add_schema(schema);
    }

    c.bench_function("calculate_checksum", |b| {
        b.iter(|| {
            let mut m = manifest.clone();
            m.update_checksum().unwrap();
            black_box(m);
        });
    });
}

criterion_group!(
    benches,
    bench_register_service,
    bench_get_service,
    bench_list_services,
    bench_update_service,
    bench_concurrent_registrations,
    bench_rate_limiting,
    bench_manifest_checksum
);
criterion_main!(benches);
