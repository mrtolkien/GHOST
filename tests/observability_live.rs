#![cfg(feature = "live-tests-observability")]

//! Live tests for the OpenTelemetry observability pipeline.
//!
//! Prerequisites:
//!   docker compose -f docker-compose.signoz.yml up -d
//!
//! Run:
//!   OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 \
//!     cargo test --features live-tests-observability \
//!       --test observability_live -- --nocapture

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;

/// Verify that spans exported via OTLP HTTP reach the SigNoz stack.
///
/// Uses a standard `SdkTracerProvider` with the batch exporter (same as
/// production), creates one span, shuts down (flushes), then queries
/// ClickHouse to confirm ingestion.
#[tokio::test]
async fn otlp_spans_reach_signoz() {
    let service_name = format!("ghost-test-{}", ulid::Ulid::new());
    eprintln!("Service name for this test run: {service_name}");

    // Build a provider identical to what observability.rs does in production.
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
        .expect("failed to build OTLP HTTP span exporter");

    let resource = Resource::builder()
        .with_service_name(service_name.clone())
        .with_attribute(KeyValue::new("deployment.environment.name", "test"))
        .build();

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    // Create a span using the standard tracer API.
    let tracer = provider.tracer("ghost-observability-test");
    {
        use opentelemetry::trace::Tracer;
        tracer.in_span("test verify_otlp", |_cx| {
            // Span body — existence is the test.
        });
    }

    // Shutdown flushes the batch exporter.
    provider
        .shutdown()
        .expect("provider shutdown (flush) failed");

    // Give the collector + ClickHouse time to ingest.
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    // Query ClickHouse directly for the test service name.
    // SigNoz v3 schema column: resource_string_service$$name
    let clickhouse_url =
        std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_owned());

    let query = format!(
        "SELECT count() AS cnt \
         FROM signoz_traces.distributed_signoz_index_v3 \
         WHERE `resource_string_service$$name` = '{service_name}' \
         FORMAT JSON"
    );
    eprintln!("ClickHouse query: {query}");

    let client = reqwest::Client::new();
    let resp = client
        .post(&clickhouse_url)
        .body(query.clone())
        .send()
        .await
        .expect("ClickHouse query failed — is ClickHouse exposed on :8123?");

    let body = resp
        .text()
        .await
        .expect("failed to read ClickHouse response");
    eprintln!("ClickHouse response: {body}");

    let json: serde_json::Value =
        serde_json::from_str(&body).expect("ClickHouse response is not valid JSON");

    let count: u64 = json["data"][0]["cnt"]
        .as_str()
        .expect("missing cnt field in ClickHouse response")
        .parse()
        .expect("cnt is not a number");

    assert!(
        count > 0,
        "Expected at least one span for service '{service_name}' in ClickHouse, got 0. \
         Is the SigNoz stack running? (docker compose -f docker-compose.signoz.yml up -d)"
    );

    eprintln!("Found {count} span(s) for service '{service_name}' in ClickHouse.");
}

/// Verify that `init_for_live_tests()` works without `OTEL_EXPORTER_OTLP_ENDPOINT`.
///
/// This confirms the console-only code path doesn't panic or return an error
/// when no OTLP endpoint is configured.
#[tokio::test]
async fn console_only_mode_no_errors() {
    // Ensure the env var is NOT set for this test.
    // SAFETY: test initialisation, no concurrent tasks yet.
    unsafe {
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    }

    let result = ghost::observability::init_for_live_tests();
    assert!(
        result.is_ok(),
        "init_for_live_tests() should succeed without OTEL_EXPORTER_OTLP_ENDPOINT: {:?}",
        result.err()
    );
}
