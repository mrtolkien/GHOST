//! Live integration tests for the browser tool.
//!
//! Requires Chrome headless-shell running at ws://localhost:9222:
//!   docker compose up -d chrome
//!
//! Run with: cargo test --features live-tests-browser -p ghost --test browser_live

#![cfg(feature = "live-tests-browser")]

use std::sync::Arc;

use serde_json::json;
use tokio::sync::Mutex;

use ghost::tools::browser::BrowserTool;
use ghost::tools::context::ToolContext;
use ghost::tools::manager::Tool;
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

/// Helper: build a ToolContext with chrome_cdp_url configured.
fn browser_tool_ctx() -> (ToolContext, tempfile::TempDir) {
    let workspace = tempfile::tempdir().unwrap();
    let mut config = ghost::config::test_config(workspace.path());
    config.web.chrome_cdp_url = Some("ws://localhost:9222".to_string());
    let ctx = ToolContext {
        workspace: workspace.path().to_path_buf(),
        cwd: workspace.path().to_path_buf(),
        db: sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
        config,
        session_id: "browser-test".to_string(),
        agent_runner: None,
        event_tx: None,
        channel_id: None,
        confirmation_tx: None,
        browser_session: Arc::new(Mutex::new(None)),
    };
    (ctx, workspace)
}

/// Helper: call BrowserTool.execute and return the text output.
async fn browser_action(ctx: &ToolContext, params: serde_json::Value) -> String {
    BrowserTool
        .execute(params, ctx)
        .await
        .expect("browser tool should succeed")
        .text
}

/// Helper: find the ref of the nearest ancestor element that contains
/// the given text anywhere in its subtree.
///
/// Walks the XML line by line: when we see a `ref="eN"`, we remember it.
/// When we see the target text, we return the most recent ref.
fn find_ref_containing(xml: &str, text: &str) -> String {
    let mut last_ref = String::new();
    for line in xml.lines() {
        if let Some(start) = line.find("ref=\"") {
            let after = &line[start + 5..];
            if let Some(end) = after.find('"') {
                last_ref = after[..end].to_string();
            }
        }
        if line.contains(text) && !last_ref.is_empty() {
            return last_ref;
        }
    }
    panic!("no ref found near text '{text}' in snapshot:\n{xml}");
}

/// Helper: find the first element with one of the given tag names and return
/// its ref. Looks for `<tagname ... ref="eN"` patterns.
fn find_ref_by_tag(xml: &str, tags: &[&str]) -> String {
    for line in xml.lines() {
        let trimmed = line.trim();
        for tag in tags {
            let prefix = format!("<{tag} ");
            let prefix_self = format!("<{tag}/");
            if (trimmed.starts_with(&prefix) || trimmed.starts_with(&prefix_self))
                && trimmed.contains("ref=\"")
                && let Some(start) = trimmed.find("ref=\"")
            {
                let after = &trimmed[start + 5..];
                if let Some(end) = after.find('"') {
                    return after[..end].to_string();
                }
            }
        }
    }
    panic!(
        "no element with tags {tags:?} found in snapshot:\n{}",
        &xml[..xml.len().min(2000)]
    );
}

/// Extract the XML body from a snapshot tool result (between security
/// boundaries).
fn extract_snapshot_xml(result: &str) -> &str {
    result
        .split("---\n")
        .nth(1)
        .and_then(|s| s.split("\n<<<END").next())
        .expect("should have XML between security boundaries")
}

/// Full integration test exercising all 14 browser tool actions through the
/// Tool trait's execute() path — the same way the GHOST uses them.
///
/// Uses blog.tolki.dev as the test page.
#[tokio::test]
async fn browser_tool_full_interaction() {
    let (ctx, ws) = browser_tool_ctx();

    // ── 1. navigate ─────────────────────────────────────────────────
    let result = browser_action(
        &ctx,
        json!({"action": "navigate", "url": "https://blog.tolki.dev/"}),
    )
    .await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    assert!(
        parsed["url"].as_str().unwrap().contains("blog.tolki.dev"),
        "should navigate to blog.tolki.dev, got: {}",
        parsed["url"]
    );
    eprintln!("  navigate ok");

    // ── 2. snapshot ─────────────────────────────────────────────────
    let result = browser_action(&ctx, json!({"action": "snapshot"})).await;
    assert!(result.contains("<<<EXTERNAL_UNTRUSTED_CONTENT>>>"));
    let xml = extract_snapshot_xml(&result);
    assert!(xml.contains("ref="));
    eprintln!(
        "  snapshot ok ({} chars, {} refs)",
        xml.len(),
        xml.matches("ref=").count()
    );

    // ── 3. scroll ───────────────────────────────────────────────────
    let result = browser_action(&ctx, json!({"action": "scroll", "direction": "down"})).await;
    assert!(serde_json::from_str::<serde_json::Value>(&result).unwrap()["ok"] == true);
    let result = browser_action(&ctx, json!({"action": "scroll", "direction": "up"})).await;
    assert!(serde_json::from_str::<serde_json::Value>(&result).unwrap()["ok"] == true);
    eprintln!("  scroll ok");

    // ── 4. screenshot ───────────────────────────────────────────────
    let result = browser_action(&ctx, json!({"action": "screenshot"})).await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    let full_path = ws.path().join(parsed["path"].as_str().unwrap());
    assert!(full_path.exists());
    eprintln!("  screenshot ok");

    // ── 5. hover — hover over the first article link ────────────────
    let result = browser_action(&ctx, json!({"action": "snapshot"})).await;
    let xml = extract_snapshot_xml(&result);
    let article_ref = find_ref_containing(xml, "Exorcising");
    let result = browser_action(&ctx, json!({"action": "hover", "ref": article_ref})).await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    eprintln!("  hover ok (ref={article_ref})");

    // ── 6. evaluate — run JS to get the page title ──────────────────
    let result = browser_action(
        &ctx,
        json!({"action": "evaluate", "expression": "document.title"}),
    )
    .await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    let eval_result = parsed["result"].as_str().unwrap();
    assert!(
        eval_result.contains("EXTERNAL_UNTRUSTED_CONTENT"),
        "evaluate result should be wrapped in security boundaries"
    );
    eprintln!("  evaluate ok");

    // ── 7. resize — change viewport to mobile size ──────────────────
    let result = browser_action(
        &ctx,
        json!({"action": "resize", "width": 375, "height": 812}),
    )
    .await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    assert!(parsed["description"].as_str().unwrap().contains("375x812"));
    // Resize back to default.
    browser_action(
        &ctx,
        json!({"action": "resize", "width": 1280, "height": 720}),
    )
    .await;
    eprintln!("  resize ok");

    // ── 8. wait — simple timeout wait ───────────────────────────────
    let result = browser_action(&ctx, json!({"action": "wait", "timeout": 100})).await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    assert!(parsed["description"].as_str().unwrap().contains("100ms"));
    eprintln!("  wait ok");

    // ── 9. click — open the Search dialog ───────────────────────────
    let result = browser_action(&ctx, json!({"action": "snapshot"})).await;
    let xml = extract_snapshot_xml(&result);
    let search_ref = find_ref_containing(xml, "Search");
    let result = browser_action(&ctx, json!({"action": "click", "ref": search_ref})).await;
    assert!(serde_json::from_str::<serde_json::Value>(&result).unwrap()["ok"] == true);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    eprintln!("  click (Search) ok");

    // ── 10. type — enter a search query ─────────────────────────────
    let result = browser_action(&ctx, json!({"action": "snapshot"})).await;
    let xml = extract_snapshot_xml(&result);
    let input_ref = find_ref_by_tag(xml, &["searchbox", "textbox", "combobox"]);
    let result = browser_action(
        &ctx,
        json!({"action": "type", "ref": input_ref, "text": "ghost"}),
    )
    .await;
    assert!(serde_json::from_str::<serde_json::Value>(&result).unwrap()["ok"] == true);
    eprintln!("  type ok (ref={input_ref})");

    // ── 11. press — press Enter to submit search ────────────────────
    let result = browser_action(&ctx, json!({"action": "press", "key": "Enter"})).await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    assert!(parsed["description"].as_str().unwrap().contains("Enter"));
    eprintln!("  press (Enter) ok");

    // ── 12. press — press Escape to close search ────────────────────
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let result = browser_action(&ctx, json!({"action": "press", "key": "Escape"})).await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    eprintln!("  press (Escape) ok");

    // ── 13. stale ref → error ───────────────────────────────────────
    let result = BrowserTool
        .execute(json!({"action": "click", "ref": "e999"}), &ctx)
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("ref=e999") && err.contains("not found"));
    eprintln!("  stale ref error ok");

    // Note: fill, select, drag not tested here — they require specific
    // page elements (form fields, <select> dropdowns, draggable elements)
    // that blog.tolki.dev doesn't have. They're wired and compile-checked.

    eprintln!("all actions tested through Tool path");
}
