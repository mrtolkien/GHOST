#![cfg(feature = "live-tests")]

use std::fs;
use std::path::PathBuf;

use ghost::web::{Crawl4aiOptions, FetchOptions, fetch, fetch_with_crawl4ai};

const OUT_DIR: &str = "/tmp/ghost-web-fetch-test";

fn crawl4ai_url() -> String {
    std::env::var("CRAWL4AI_URL").unwrap_or_else(|_| "http://localhost:11235".to_string())
}

fn save(name: &str, url: &str, method: &str, word_count: usize, text: &str) {
    let dir = PathBuf::from(OUT_DIR);
    fs::create_dir_all(&dir).expect("create output dir");
    let path = dir.join(format!("{name}_{method}.md"));

    let mut out = String::new();
    out.push_str(&format!("# URL: {url}\n"));
    out.push_str(&format!("# Method: {method}\n"));
    out.push_str(&format!("# Word count: {word_count}\n"));
    out.push_str("\n---\n\n");
    out.push_str(text);
    fs::write(&path, &out).expect("write output");
    eprintln!("  [{method}] {word_count} words → {}", path.display());
}

// ---------------------------------------------------------------------------
// Core crawl4ai functionality tests
// ---------------------------------------------------------------------------

/// Wikipedia — reliable, fast, validates basic extraction.
#[tokio::test]
async fn crawl4ai_wikipedia() {
    let url = "https://en.wikipedia.org/wiki/3D_printing";
    eprintln!("\n=== Wikipedia ===");

    let start = std::time::Instant::now();
    let result = fetch(url, &FetchOptions::default(), Some(&crawl4ai_url()))
        .await
        .expect("fetch should succeed");

    let elapsed = start.elapsed();
    save("wikipedia", url, "crawl4ai", result.word_count, &result.text);
    eprintln!(
        "  {} words in {:.1}s",
        result.word_count,
        elapsed.as_secs_f64()
    );

    assert!(
        result.word_count > 1000,
        "Wikipedia article should have substance (got {} words)",
        result.word_count
    );
    assert!(
        elapsed.as_secs() < 30,
        "should complete in < 30s (took {:.1}s)",
        elapsed.as_secs_f64()
    );
}

/// Reddit — JS-heavy, crawl4ai should extract discussion content.
#[tokio::test]
async fn crawl4ai_reddit() {
    let url = "https://www.reddit.com/r/3Dprinting/comments/1ip98af/best_enclosed_fdm_3d_printer_to_start_with/";
    eprintln!("\n=== Reddit Thread ===");

    let md = fetch_with_crawl4ai(&crawl4ai_url(), url, &Crawl4aiOptions::default())
        .await
        .expect("crawl4ai fetch");
    let words = md.split_whitespace().count();
    save("reddit_thread", url, "crawl4ai", words, &md);
    eprintln!("  {words} words");

    assert!(
        words > 200,
        "should extract discussion content (got {words} words)"
    );
}

/// Tom's Hardware — content-heavy review site.
#[tokio::test]
async fn crawl4ai_toms_hardware() {
    let url = "https://www.tomshardware.com/best-picks/best-3d-printers";
    eprintln!("\n=== Tom's Hardware ===");

    let md = fetch_with_crawl4ai(&crawl4ai_url(), url, &Crawl4aiOptions::default())
        .await
        .expect("crawl4ai fetch");
    let words = md.split_whitespace().count();
    save("toms_hardware", url, "crawl4ai", words, &md);
    eprintln!("  {words} words");

    assert!(
        words > 1000,
        "should extract review content (got {words} words)"
    );
}

/// GitHub issue — JS-rendered SPA.
#[tokio::test]
async fn crawl4ai_github_issue() {
    let url = "https://github.com/rust-lang/rust/issues/34511";
    eprintln!("\n=== GitHub Issue ===");

    let result = fetch(url, &FetchOptions::default(), Some(&crawl4ai_url()))
        .await
        .expect("fetch should succeed");

    save(
        "github_issue",
        url,
        "crawl4ai",
        result.word_count,
        &result.text,
    );
    eprintln!("  {} words", result.word_count);

    // GitHub SPA may render issue content or issue list — just verify
    // we got substantial content from the JS-rendered page
    assert!(
        result.word_count > 500,
        "should extract GitHub page content (got {} words)",
        result.word_count
    );
}

/// CSS selector should focus extraction on a specific DOM region.
#[tokio::test]
async fn crawl4ai_css_selector() {
    let url = "https://en.wikipedia.org/wiki/3D_printing";
    eprintln!("\n=== CSS Selector ===");

    let full = fetch_with_crawl4ai(&crawl4ai_url(), url, &Crawl4aiOptions::default())
        .await
        .expect("full fetch");
    let full_words = full.split_whitespace().count();

    let focused = fetch_with_crawl4ai(
        &crawl4ai_url(),
        url,
        &Crawl4aiOptions {
            css_selector: Some("#mw-content-text".into()),
            ..Default::default()
        },
    )
    .await
    .expect("focused fetch");
    let focused_words = focused.split_whitespace().count();

    save("wikipedia_full", url, "crawl4ai_full", full_words, &full);
    save(
        "wikipedia_focused",
        url,
        "crawl4ai_selector",
        focused_words,
        &focused,
    );
    eprintln!("  full: {full_words} words, focused: {focused_words} words");

    assert!(
        focused_words > 500,
        "focused should still have substance (got {focused_words} words)"
    );
}

// ---------------------------------------------------------------------------
// Integration: fetch() with HEAD routing + crawl4ai primary path
// ---------------------------------------------------------------------------

/// fetch() routes HTML to crawl4ai and extracts Reddit content.
#[tokio::test]
async fn fetch_integrated_reddit() {
    let url = "https://www.reddit.com/r/3Dprinting/comments/1ip98af/best_enclosed_fdm_3d_printer_to_start_with/";
    eprintln!("\n=== fetch() integrated — Reddit ===");

    let result = fetch(
        url,
        &FetchOptions::default(),
        Some(&crawl4ai_url()),
    )
    .await
    .expect("fetch should succeed");

    save(
        "reddit_integrated",
        url,
        "fetch_integrated",
        result.word_count,
        &result.text,
    );
    eprintln!("  word count: {}", result.word_count);

    assert!(
        result.word_count > 200,
        "Reddit via crawl4ai should have discussion content (got {} words)",
        result.word_count
    );
}

/// fetch() routes HTML to crawl4ai for PCMag (which blocks reqwest with 403).
#[tokio::test]
async fn fetch_integrated_pcmag() {
    let url = "https://www.pcmag.com/picks/the-best-3d-printers";
    eprintln!("\n=== fetch() integrated — PCMag ===");

    let result = fetch(
        url,
        &FetchOptions::default(),
        Some(&crawl4ai_url()),
    )
    .await
    .expect("fetch should succeed");

    save(
        "pcmag_integrated",
        url,
        "fetch_integrated",
        result.word_count,
        &result.text,
    );
    eprintln!("  word count: {}", result.word_count);

    assert!(
        result.word_count > 500,
        "PCMag should have substantial content (got {} words)",
        result.word_count
    );
}

// ---------------------------------------------------------------------------
// Fallback: when crawl4ai is unavailable, fetch() uses local extraction
// ---------------------------------------------------------------------------

/// When crawl4ai is down, fetch() falls back to local reqwest+readability.
#[tokio::test]
async fn fallback_to_local() {
    let url = "https://en.wikipedia.org/wiki/3D_printing";
    let bad_url = "http://localhost:1";
    eprintln!("\n=== Fallback to Local ===");

    let result = fetch(url, &FetchOptions::default(), Some(bad_url))
        .await
        .expect("should fall back to local extraction");

    save(
        "wikipedia_fallback",
        url,
        "local_fallback",
        result.word_count,
        &result.text,
    );
    eprintln!("  {} words (local extraction)", result.word_count);

    assert!(
        result.word_count > 500,
        "local fallback should produce content (got {} words)",
        result.word_count
    );
}
