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
use opentelemetry::trace::{SpanId, SpanKind, Status, TraceId};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{SpanData, SpanExporter};

/// Verify that spans exported via OTLP HTTP reach the SigNoz stack.
///
/// Manually constructs a `SpanData`, exports it via the OTLP HTTP exporter
/// directly (no SDK batch processor), then queries ClickHouse to confirm
/// ingestion.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn otlp_spans_reach_signoz() {
    let service_name = format!("ghost-test-{}", ulid::Ulid::new());
    eprintln!("Service name for this test run: {service_name}");

    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4318".to_owned());

    let resource = Resource::builder()
        .with_attributes([
            KeyValue::new("service.name", service_name.clone()),
            KeyValue::new("deployment.environment.name", "test"),
        ])
        .build();

    // Set the env var so the SDK resolves the endpoint correctly (appends
    // /v1/traces). Using .with_endpoint() directly would NOT append the path.
    unsafe {
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", &endpoint);
    }
    let mut exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
        .expect("failed to build OTLP HTTP span exporter");

    exporter.set_resource(&resource);

    // Build a SpanData manually.
    let now = std::time::SystemTime::now();
    let span_data = SpanData {
        span_context: opentelemetry::trace::SpanContext::new(
            TraceId::from_hex("4bf92f3577b34da6a3ce929d0e0e4736").unwrap(),
            SpanId::from_hex("00f067aa0ba902b7").unwrap(),
            opentelemetry::trace::TraceFlags::SAMPLED,
            true,
            opentelemetry::trace::TraceState::default(),
        ),
        parent_span_id: SpanId::INVALID,
        parent_span_is_remote: false,
        span_kind: SpanKind::Internal,
        name: "test-span".into(),
        start_time: now,
        end_time: now + std::time::Duration::from_millis(42),
        attributes: vec![KeyValue::new("test.marker", "observability-live")],
        dropped_attributes_count: 0,
        events: opentelemetry_sdk::trace::SpanEvents::default(),
        links: opentelemetry_sdk::trace::SpanLinks::default(),
        status: Status::Ok,
        instrumentation_scope: opentelemetry::InstrumentationScope::builder("ghost-test").build(),
    };

    // Export directly (async, runs in the tokio runtime).
    let result = exporter.export(vec![span_data]).await;
    eprintln!("Export result: {result:?}");
    assert!(result.is_ok(), "OTLP export failed: {result:?}");

    // Shutdown the exporter.
    let _ = exporter.shutdown();

    // Give the collector + ClickHouse time to ingest.
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    // Query ClickHouse directly for the test service name.
    // In SigNoz v3 schema the column is resource_string_service$$name.
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
        .expect("ClickHouse query failed");

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
