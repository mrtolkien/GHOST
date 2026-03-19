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
use opentelemetry::trace::{Tracer, TracerProvider};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;

/// Verify that spans exported via OTLP HTTP reach the SigNoz stack.
///
/// The test creates a throwaway `SdkTracerProvider` with a unique service name,
/// emits one span, flushes via `shutdown()`, then queries ClickHouse directly
/// (exposed on port 8123) to confirm the span was ingested.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn otlp_spans_reach_signoz() {
    let service_name = format!("ghost-test-{}", ulid::Ulid::new());
    eprintln!("Service name for this test run: {service_name}");

    // Build a standalone provider pointing at the local collector.
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4318".to_owned());

    let resource = Resource::builder()
        .with_attributes([
            KeyValue::new("service.name", service_name.clone()),
            KeyValue::new("deployment.environment.name", "test"),
        ])
        .build();

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(&endpoint)
        .build()
        .expect("failed to build OTLP HTTP span exporter");

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    // Create a span using the OTel API directly.
    let tracer = provider.tracer("ghost-observability-test");
    let _span = tracer
        .span_builder("test-span")
        .with_attributes([KeyValue::new("test.marker", "observability-live")])
        .start(&tracer);
    drop(_span);

    // Shutdown flushes the batch exporter.
    provider
        .shutdown()
        .expect("provider shutdown (flush) failed");

    // Give the collector and ClickHouse a few seconds to ingest.
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    // Query ClickHouse directly for the test service name.
    let clickhouse_url = std::env::var("CLICKHOUSE_URL")
        .unwrap_or_else(|_| "http://localhost:8123".to_owned());

    let query = format!(
        "SELECT count() AS cnt \
         FROM signoz_traces.distributed_signoz_index_v3 \
         WHERE serviceName = '{service_name}' \
         FORMAT JSON"
    );

    let client = reqwest::Client::new();
    let resp = client
        .post(&clickhouse_url)
        .body(query.clone())
        .send()
        .await
        .expect("ClickHouse query failed");

    let body = resp.text().await.expect("failed to read ClickHouse response");
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
