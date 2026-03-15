//! Live integration tests for the browser tool.
//!
//! Requires Chrome headless-shell running at ws://localhost:9222:
//!   docker compose up -d chrome
//!
//! Run with: cargo test --features live-tests -p ghost --test browser_live

#![cfg(feature = "live-tests")]

use ghost::web::browser::BrowserSession;

#[tokio::test]
async fn browser_navigate_and_snapshot() {
    let mut session = BrowserSession::connect("ws://localhost:9222")
        .await
        .expect("Chrome should be running at ws://localhost:9222");

    // Navigate to a simple page
    let (url, _title) = session
        .navigate("https://example.com")
        .await
        .expect("navigate should succeed");
    assert!(
        url.contains("example.com"),
        "final URL should contain example.com, got: {url}"
    );

    // Get accessibility snapshot
    let xml = session.snapshot(0).await.expect("snapshot should succeed");
    assert!(
        xml.contains("<heading"),
        "snapshot should contain a heading element"
    );
    assert!(
        xml.contains("Example Domain"),
        "snapshot should contain the page title text"
    );
    assert!(xml.contains("ref="), "snapshot should contain ref IDs");

    // Screenshot
    let dir = tempfile::tempdir().unwrap();
    let path = session
        .screenshot(dir.path())
        .await
        .expect("screenshot should succeed");
    assert!(path.exists(), "screenshot file should exist");
    let meta = tokio::fs::metadata(&path).await.unwrap();
    assert!(
        meta.len() > 1000,
        "screenshot should be >1KB, got {} bytes",
        meta.len()
    );
}

#[tokio::test]
async fn browser_ssrf_blocks_private_ips() {
    let mut session = BrowserSession::connect("ws://localhost:9222")
        .await
        .expect("Chrome should be running");

    let result = session.navigate("http://127.0.0.1:9222").await;
    assert!(result.is_err(), "navigating to localhost should be blocked");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not allowed"),
        "error should mention URL not allowed, got: {err}"
    );
}
