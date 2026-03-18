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
use ghost::web::browser::BrowserManager;

#[tokio::test]
async fn browser_navigate_and_snapshot() {
    let configs = vec![ghost::config::BrowserConfig {
        name: "headless".to_string(),
        cdp_url: "ws://localhost:9222".to_string(),
        discovered: false,
    }];
    let mut mgr = BrowserManager::new(configs);

    // Navigate to a simple page
    let (url, _title) = mgr
        .navigate("https://example.com")
        .await
        .expect("navigate should succeed");
    assert!(
        url.contains("example.com"),
        "final URL should contain example.com, got: {url}"
    );

    // Get accessibility snapshot
    let xml = mgr.snapshot(0).await.expect("snapshot should succeed");
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
    let path = mgr
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
    let configs = vec![ghost::config::BrowserConfig {
        name: "headless".to_string(),
        cdp_url: "ws://localhost:9222".to_string(),
        discovered: false,
    }];
    let mut mgr = BrowserManager::new(configs);

    let result = mgr.navigate("http://127.0.0.1:9222").await;
    assert!(result.is_err(), "navigating to localhost should be blocked");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not allowed"),
        "error should mention URL not allowed, got: {err}"
    );
}

/// Helper: build a ToolContext with a browser configured.
fn browser_tool_ctx() -> (ToolContext, tempfile::TempDir) {
    let workspace = tempfile::tempdir().unwrap();
    let mut config = ghost::config::test_config(workspace.path());
    config.web.browsers = vec![ghost::config::BrowserConfig {
        name: "headless".to_string(),
        cdp_url: "ws://localhost:9222".to_string(),
        discovered: false,
    }];
    let ctx = ToolContext {
        workspace: workspace.path().to_path_buf(),
        cwd: workspace.path().to_path_buf(),
        db: sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
        config: Arc::new(config.clone()),
        session_id: "browser-test".to_string(),
        agent_runner: None,
        event_tx: None,
        channel_id: None,
        confirmation_tx: None,
        browser_manager: Arc::new(Mutex::new(BrowserManager::new(config.web.browsers.clone()))),
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

    // -- 1. navigate
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

    // -- 2. snapshot
    let result = browser_action(&ctx, json!({"action": "snapshot"})).await;
    assert!(result.contains("<<<EXTERNAL_UNTRUSTED_CONTENT>>>"));
    let xml = extract_snapshot_xml(&result);
    assert!(xml.contains("ref="));
    eprintln!(
        "  snapshot ok ({} chars, {} refs)",
        xml.len(),
        xml.matches("ref=").count()
    );

    // -- 3. scroll
    let result = browser_action(&ctx, json!({"action": "scroll", "direction": "down"})).await;
    assert!(serde_json::from_str::<serde_json::Value>(&result).unwrap()["ok"] == true);
    let result = browser_action(&ctx, json!({"action": "scroll", "direction": "up"})).await;
    assert!(serde_json::from_str::<serde_json::Value>(&result).unwrap()["ok"] == true);
    eprintln!("  scroll ok");

    // -- 4. screenshot
    let result = browser_action(&ctx, json!({"action": "screenshot"})).await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    let full_path = ws.path().join(parsed["path"].as_str().unwrap());
    assert!(full_path.exists());
    eprintln!("  screenshot ok");

    // -- 5. hover — hover over the first article link
    let result = browser_action(&ctx, json!({"action": "snapshot"})).await;
    let xml = extract_snapshot_xml(&result);
    let article_ref = find_ref_containing(xml, "Exorcising");
    let result = browser_action(&ctx, json!({"action": "hover", "ref": article_ref})).await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    eprintln!("  hover ok (ref={article_ref})");

    // -- 6. evaluate — run JS to get the page title
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

    // -- 7. resize — change viewport to mobile size
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

    // -- 8. wait — simple timeout wait
    let result = browser_action(&ctx, json!({"action": "wait", "timeout": 100})).await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    assert!(parsed["description"].as_str().unwrap().contains("100ms"));
    eprintln!("  wait ok");

    // -- 9. click — open the Search dialog
    let result = browser_action(&ctx, json!({"action": "snapshot"})).await;
    let xml = extract_snapshot_xml(&result);
    let search_ref = find_ref_containing(xml, "Search");
    let result = browser_action(&ctx, json!({"action": "click", "ref": search_ref})).await;
    assert!(serde_json::from_str::<serde_json::Value>(&result).unwrap()["ok"] == true);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    eprintln!("  click (Search) ok");

    // -- 10. type — enter a search query
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

    // -- 11. press — press Enter to submit search
    let result = browser_action(&ctx, json!({"action": "press", "key": "Enter"})).await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    assert!(parsed["description"].as_str().unwrap().contains("Enter"));
    eprintln!("  press (Enter) ok");

    // -- 12. press — press Escape to close search
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let result = browser_action(&ctx, json!({"action": "press", "key": "Escape"})).await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    eprintln!("  press (Escape) ok");

    // -- 13. stale ref -> error
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

// ---------------------------------------------------------------------------
// Multi-tab tests
// ---------------------------------------------------------------------------

/// Test multi-tab workflow: open, list, focus, close.
///
/// Exercises BrowserManager's tab management directly (not through the tool).
#[tokio::test]
async fn multi_tab_open_focus_close() {
    let configs = vec![ghost::config::BrowserConfig {
        name: "headless".to_string(),
        cdp_url: "ws://localhost:9222".to_string(),
        discovered: false,
    }];
    let mut mgr = BrowserManager::new(configs);

    // Navigate to a page (auto-creates browser connection + tab 1).
    let (url1, _) = mgr
        .navigate("https://example.com")
        .await
        .expect("navigate should succeed");
    assert!(url1.contains("example.com"));
    let tab1_id = mgr.active_tab_id().expect("should have active tab");
    eprintln!("  tab 1 (id={tab1_id}) opened: {url1}");

    // Open a second tab with a different URL.
    let _snapshot = mgr
        .open_tab(Some("https://httpbin.org/html"))
        .await
        .expect("open_tab should succeed");
    let tab2_id = mgr.active_tab_id().expect("should have active tab");
    assert_ne!(tab1_id, tab2_id, "second tab should get a new ID");
    eprintln!("  tab 2 (id={tab2_id}) opened");

    // List tabs — should have exactly 2.
    let tabs = mgr.list_tabs().await.expect("list_tabs should succeed");
    assert_eq!(tabs.len(), 2, "should have 2 tabs, got: {}", tabs.len());
    eprintln!(
        "  tabs: {:?}",
        tabs.iter()
            .map(|t| format!("tab {} @ {}", t.id, t.url))
            .collect::<Vec<_>>()
    );

    // Tab 2 should be active (was just opened).
    assert_eq!(mgr.active_tab_id(), Some(tab2_id));

    // Focus back to tab 1 — should return a snapshot.
    let snapshot = mgr
        .focus_tab(tab1_id)
        .await
        .expect("focus_tab should succeed");
    assert!(
        snapshot.contains("Example Domain"),
        "focusing tab 1 should show example.com content"
    );
    assert_eq!(mgr.active_tab_id(), Some(tab1_id));
    eprintln!("  focused tab 1, verified content");

    // Focus tab 2 — should show httpbin content.
    let snapshot = mgr
        .focus_tab(tab2_id)
        .await
        .expect("focus_tab should succeed");
    assert!(
        snapshot.contains("Herman Melville"),
        "focusing tab 2 should show httpbin/html content"
    );
    eprintln!("  focused tab 2, verified content");

    // Close tab 1.
    let msg = mgr
        .close_tab(tab1_id)
        .await
        .expect("close_tab should succeed");
    assert!(msg.contains(&tab1_id.to_string()));
    eprintln!("  closed tab 1");

    // Should have 1 tab remaining, tab 2 still active.
    let tabs = mgr.list_tabs().await.expect("list_tabs should succeed");
    assert_eq!(tabs.len(), 1, "should have 1 tab after close");
    assert_eq!(mgr.active_tab_id(), Some(tab2_id));
    eprintln!("  verified 1 tab remaining");

    // Close non-existent tab should error.
    let result = mgr.close_tab(tab1_id).await;
    assert!(result.is_err(), "closing already-closed tab should error");
    eprintln!("  closing non-existent tab errors correctly");
}

/// Test multi-tab through the Tool interface (same path GHOST uses).
#[tokio::test]
async fn multi_tab_tool_actions() {
    let (ctx, _ws) = browser_tool_ctx();

    // Navigate to first page.
    let result = browser_action(
        &ctx,
        json!({"action": "navigate", "url": "https://example.com"}),
    )
    .await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert!(parsed["url"].as_str().unwrap().contains("example.com"));
    // Verify browser/tab context fields are present.
    assert!(
        parsed.get("browser").is_some(),
        "output should contain browser field, got: {result}"
    );
    assert!(
        parsed.get("tab").is_some(),
        "output should contain tab field, got: {result}"
    );
    eprintln!("  navigate ok, browser/tab context present");

    // Open a second tab.
    let result = browser_action(
        &ctx,
        json!({"action": "open", "url": "https://httpbin.org/html"}),
    )
    .await;
    assert!(
        result.contains("EXTERNAL_UNTRUSTED_CONTENT"),
        "open should return snapshot"
    );
    eprintln!("  opened tab 2");

    // List tabs — should show 2.
    let result = browser_action(&ctx, json!({"action": "tabs"})).await;
    assert!(result.contains("Tab"), "tabs should list tabs");
    assert!(
        result.contains("[active]"),
        "should mark active tab: {result}"
    );
    // Count "Tab " occurrences to verify 2 tabs.
    let tab_count = result.matches("Tab ").count();
    assert_eq!(tab_count, 2, "should list 2 tabs, got: {result}");
    eprintln!("  tabs action shows 2 tabs");

    // Focus back to tab 1.
    let result = browser_action(&ctx, json!({"action": "focus", "tab": 1})).await;
    assert!(
        result.contains("Example Domain"),
        "focusing tab 1 should show example.com"
    );
    eprintln!("  focused tab 1 via tool");

    // Close tab 2.
    let result = browser_action(&ctx, json!({"action": "close", "tab": 2})).await;
    assert!(result.contains("ok"), "close should return ok");
    eprintln!("  closed tab 2 via tool");

    // Verify only 1 tab left.
    let result = browser_action(&ctx, json!({"action": "tabs"})).await;
    let tab_count = result.matches("Tab ").count();
    assert_eq!(tab_count, 1, "should have 1 tab after close: {result}");
    eprintln!("  verified 1 tab remaining");
}

/// Test that the 5-tab limit is enforced.
#[tokio::test]
async fn tab_limit_enforced() {
    let configs = vec![ghost::config::BrowserConfig {
        name: "headless".to_string(),
        cdp_url: "ws://localhost:9222".to_string(),
        discovered: false,
    }];
    let mut mgr = BrowserManager::new(configs);

    // Navigate creates tab 1.
    mgr.navigate("https://example.com")
        .await
        .expect("navigate should succeed");

    // Open tabs 2-5.
    for i in 2..=5 {
        mgr.open_tab(None)
            .await
            .unwrap_or_else(|e| panic!("open tab {i} should succeed: {e}"));
    }

    let tabs = mgr.list_tabs().await.unwrap();
    assert_eq!(tabs.len(), 5, "should have 5 tabs");
    eprintln!("  opened 5 tabs");

    // Tab 6 should fail with TabLimitReached.
    let result = mgr.open_tab(None).await;
    assert!(result.is_err(), "6th tab should be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("tab limit"),
        "error should mention tab limit, got: {err}"
    );
    eprintln!("  6th tab correctly rejected: {err}");
}

// ---------------------------------------------------------------------------
// Multi-browser tests (requires 2 Chrome instances)
// ---------------------------------------------------------------------------

/// Test multi-browser management: connect, switch, disconnect.
///
/// Requires two Chrome instances:
///   - ws://localhost:9222 (primary)
///   - ws://localhost:9223 (secondary)
///
/// Start both with:
///   docker compose up -d chrome chrome2
///
/// Only runs with `live-tests-multi-browser` feature.
#[cfg(feature = "live-tests-multi-browser")]
#[tokio::test]
async fn multi_browser_connect_and_switch() {
    let mut mgr = BrowserManager::new(vec![]);

    // Connect first browser.
    let info1 = mgr
        .connect_browser("primary", "ws://localhost:9222")
        .await
        .expect("connect primary should succeed");
    assert!(info1.connected, "primary should be connected");
    assert_eq!(
        mgr.active_browser_name(),
        Some("primary"),
        "primary should be active"
    );
    eprintln!("  connected primary: {:?}", info1);

    // Navigate on primary.
    mgr.navigate("https://example.com")
        .await
        .expect("navigate on primary should succeed");
    let url = mgr.current_url().await.unwrap_or_default();
    assert!(url.contains("example.com"));
    eprintln!("  navigated primary to example.com");

    // Connect second browser — becomes active.
    let info2 = mgr
        .connect_browser("secondary", "ws://localhost:9223")
        .await
        .expect("connect secondary should succeed");
    assert!(info2.connected, "secondary should be connected");
    assert_eq!(
        mgr.active_browser_name(),
        Some("secondary"),
        "secondary should now be active"
    );
    eprintln!("  connected secondary: {:?}", info2);

    // Navigate on secondary.
    mgr.navigate("https://httpbin.org/html")
        .await
        .expect("navigate on secondary should succeed");
    let url = mgr.current_url().await.unwrap_or_default();
    assert!(url.contains("httpbin.org"));
    eprintln!("  navigated secondary to httpbin.org");

    // List browsers — should show both.
    let browsers = mgr.list_browsers();
    assert_eq!(
        browsers.len(),
        2,
        "should have 2 browsers, got: {}",
        browsers.len()
    );
    assert!(
        browsers.iter().all(|b| b.connected),
        "both should be connected"
    );
    eprintln!("  listed 2 browsers, both connected");

    // Disconnect primary.
    mgr.disconnect_browser("primary")
        .await
        .expect("disconnect primary should succeed");
    let browsers = mgr.list_browsers();
    let primary = browsers.iter().find(|b| b.name == "primary").unwrap();
    assert!(!primary.connected, "primary should be disconnected");
    eprintln!("  disconnected primary");

    // Secondary still works.
    let snapshot = mgr
        .snapshot(0)
        .await
        .expect("snapshot on secondary should work");
    assert!(
        snapshot.contains("Herman Melville"),
        "secondary should still serve content"
    );
    eprintln!("  secondary still works after primary disconnect");
}

/// Test the browsers/connect/disconnect tool actions.
///
/// Requires two Chrome instances (see multi_browser_connect_and_switch).
#[cfg(feature = "live-tests-multi-browser")]
#[tokio::test]
async fn multi_browser_tool_actions() {
    let (ctx, _ws) = browser_tool_ctx();

    // Connect to a second browser via tool action.
    let result = browser_action(
        &ctx,
        json!({
            "action": "connect",
            "name": "secondary",
            "cdp_url": "ws://localhost:9223"
        }),
    )
    .await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["name"], "secondary");
    assert_eq!(parsed["connected"], true);
    eprintln!("  connected secondary via tool");

    // List browsers — should show the preconfigured headless + secondary.
    let result = browser_action(&ctx, json!({"action": "browsers"})).await;
    assert!(
        result.contains("headless") || result.contains("secondary"),
        "should list browsers: {result}"
    );
    eprintln!("  browsers action ok: {result}");

    // Navigate on secondary.
    let result = browser_action(
        &ctx,
        json!({"action": "navigate", "url": "https://httpbin.org/html"}),
    )
    .await;
    assert!(result.contains("httpbin.org"));
    eprintln!("  navigate on secondary ok");

    // Disconnect secondary.
    let result = browser_action(&ctx, json!({"action": "disconnect", "name": "secondary"})).await;
    assert!(
        result.contains("ok") || result.contains("Disconnected"),
        "disconnect should succeed: {result}"
    );
    eprintln!("  disconnected secondary via tool");
}

/// Test the upload action against a page with a file input.
///
/// Uses the-internet.herokuapp.com/upload which has a simple
/// `<input type="file">` + submit button. The CDP setFileInputFiles
/// command accepts any path regardless of whether Chrome can actually
/// read it, so this works even though the test workspace isn't the
/// real volume mount — it exercises the full tool path: ref resolution,
/// path staging, and the CDP call.
#[tokio::test]
async fn browser_upload_file() {
    let (ctx, _ws) = browser_tool_ctx();

    // Create a test file in workspace/uploads/
    let uploads_dir = _ws.path().join("uploads");
    tokio::fs::create_dir_all(&uploads_dir).await.unwrap();
    let test_file = uploads_dir.join("test-data.csv");
    tokio::fs::write(&test_file, "name,value\nfoo,42\n")
        .await
        .unwrap();

    // Navigate to the file upload test page.
    let result = browser_action(
        &ctx,
        json!({"action": "navigate", "url": "https://the-internet.herokuapp.com/upload"}),
    )
    .await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    eprintln!("  navigate to upload page ok");

    // Snapshot to find the file input element.
    let result = browser_action(&ctx, json!({"action": "snapshot"})).await;
    let xml = extract_snapshot_xml(&result);
    eprintln!("  upload page snapshot:\n{xml}");

    // File inputs appear as a button "Choose File" in Chrome's AX tree.
    let file_ref = find_ref_containing(xml, "Choose File");
    eprintln!("  found file input ref={file_ref}");

    // Upload the test file.
    let result = browser_action(
        &ctx,
        json!({"action": "upload", "ref": file_ref, "path": "uploads/test-data.csv"}),
    )
    .await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["ok"], true);
    assert!(
        parsed["description"].as_str().unwrap().contains("Uploaded"),
        "description should mention upload, got: {}",
        parsed["description"]
    );
    assert_eq!(parsed["path"], "uploads/test-data.csv");
    eprintln!("  upload ok");

    // Verify the file input reflects the uploaded filename via JS.
    let result = browser_action(
        &ctx,
        json!({"action": "evaluate", "expression": "document.getElementById('file-upload').value"}),
    )
    .await;
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let eval_text = parsed["result"].as_str().unwrap_or("");
    assert!(
        eval_text.contains("test-data.csv"),
        "file input value should contain filename, got: {eval_text}"
    );
    eprintln!("  file input value verified");
}

// ---------------------------------------------------------------------------
// Crawl4AI + shared Chrome session test
// ---------------------------------------------------------------------------

/// Test that Crawl4AI uses the same Chrome session as the browser tool.
///
/// Sets a cookie in Chrome via the browser tool, then fetches the same
/// site's cookie page via Crawl4AI (pointing at the same Chrome). If the
/// cookie is visible in Crawl4AI's output, the session is shared.
///
/// Requires:
///   - Chrome headless-shell on ws://localhost:9222 (docker compose up chrome)
///   - Crawl4AI on http://localhost:11235 (docker compose up crawl4ai)
///
/// Run with: cargo test --features live-tests-crawl4ai -p ghost --test browser_live
#[cfg(feature = "live-tests-crawl4ai")]
#[tokio::test]
async fn crawl4ai_shares_browser_session() {
    use ghost::web::browser::BrowserManager;

    let configs = vec![ghost::config::BrowserConfig {
        name: "headless".to_string(),
        cdp_url: "ws://localhost:9222".to_string(),
        discovered: false,
    }];
    let mut mgr = BrowserManager::new(configs);

    // 1. Navigate to httpbin and set a unique cookie via JS.
    let cookie_value = format!("ghost_test_{}", ulid::Ulid::new());
    mgr.navigate("https://httpbin.org/html")
        .await
        .expect("navigate should succeed");
    eprintln!("  navigated to httpbin.org/html");

    let js =
        format!("document.cookie = 'ghost_session={cookie_value}; path=/; domain=httpbin.org'");
    mgr.evaluate(&js)
        .await
        .expect("setting cookie should succeed");
    eprintln!("  set cookie: ghost_session={cookie_value}");

    // Verify the cookie is set in Chrome.
    let cookies = mgr
        .evaluate("document.cookie")
        .await
        .expect("reading cookie should succeed");
    assert!(
        cookies.contains(&cookie_value),
        "cookie should be set in Chrome, got: {cookies}"
    );
    eprintln!("  verified cookie in Chrome");

    // 2. Fetch httpbin.org/cookies via Crawl4AI through the same Chrome.
    //
    // Crawl4AI must run with network_mode: host so it can reach
    // localhost:9222 (Chrome) and Tailscale IPs.
    //
    // We call fetch_with_crawl4ai directly (not fetch()) because
    // httpbin.org/cookies returns JSON, and fetch() routes non-HTML
    // to plain reqwest which has no cookies.
    let crawl4ai_url = "http://localhost:11235";
    let cdp_url = "ws://localhost:9222";

    let c4ai_options = ghost::web::Crawl4aiOptions::default();
    let markdown = ghost::web::fetch_with_crawl4ai(
        crawl4ai_url,
        "https://httpbin.org/cookies",
        &c4ai_options,
        Some(cdp_url),
    )
    .await
    .expect("crawl4ai fetch should succeed");

    eprintln!(
        "  crawl4ai response ({} chars): {}",
        markdown.len(),
        &markdown[..markdown.len().min(500)]
    );

    // 3. The cookie should be visible in the fetched content.
    assert!(
        markdown.contains(&cookie_value),
        "Crawl4AI should see the cookie set via browser tool.\n\
         Expected cookie value: {cookie_value}\n\
         Crawl4AI output: {}",
        &markdown[..markdown.len().min(1000)]
    );
    eprintln!("  cookie visible in Crawl4AI output — shared session confirmed!");
}
