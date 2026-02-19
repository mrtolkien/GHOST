#![cfg(feature = "live-tests")]

use std::fs;
use std::path::PathBuf;

use ghost::web::{FetchOptions, fetch, fetch_with_crawl4ai};

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
// Comparison tests: fetch the same page with reqwest+readability vs crawl4ai
// ---------------------------------------------------------------------------

/// All3DP article — readability often returns boilerplate/nav only.
#[tokio::test]
async fn compare_all3dp_article() {
    let url = "https://all3dp.com/1/snapmaker-u1-reviewed-make-haste-not-waste/";
    eprintln!("\n=== All3DP Article ===");

    let reqwest_result = fetch(
        url,
        &FetchOptions {
            readability: true,
            ..Default::default()
        },
        None, // no crawl4ai fallback — test reqwest alone
    )
    .await
    .expect("reqwest fetch");
    save(
        "all3dp_article",
        url,
        "reqwest",
        reqwest_result.word_count,
        &reqwest_result.text,
    );

    let c4ai_md = fetch_with_crawl4ai(&crawl4ai_url(), url)
        .await
        .expect("crawl4ai fetch");
    let c4ai_words = c4ai_md.split_whitespace().count();
    save("all3dp_article", url, "crawl4ai", c4ai_words, &c4ai_md);

    eprintln!(
        "  reqwest: {} words, crawl4ai: {} words",
        reqwest_result.word_count, c4ai_words
    );

    // crawl4ai should extract the actual article content
    assert!(
        c4ai_words > reqwest_result.word_count,
        "crawl4ai should extract more content than reqwest+readability \
         (crawl4ai={c4ai_words}, reqwest={})",
        reqwest_result.word_count
    );
}

/// All3DP best-enclosed list — the page from the e2e research agent.
#[tokio::test]
async fn compare_all3dp_enclosed_list() {
    let url = "https://all3dp.com/1/best-enclosed-3d-printers/";
    eprintln!("\n=== All3DP Enclosed List ===");

    let reqwest_result = fetch(
        url,
        &FetchOptions {
            readability: true,
            ..Default::default()
        },
        None,
    )
    .await
    .expect("reqwest fetch");
    save(
        "all3dp_enclosed",
        url,
        "reqwest",
        reqwest_result.word_count,
        &reqwest_result.text,
    );

    let c4ai_md = fetch_with_crawl4ai(&crawl4ai_url(), url)
        .await
        .expect("crawl4ai fetch");
    let c4ai_words = c4ai_md.split_whitespace().count();
    save("all3dp_enclosed", url, "crawl4ai", c4ai_words, &c4ai_md);

    eprintln!(
        "  reqwest: {} words, crawl4ai: {} words",
        reqwest_result.word_count, c4ai_words
    );
}

/// Reddit thread — reqwest returns tons of nav/ad boilerplate.
#[tokio::test]
async fn compare_reddit_thread() {
    let url = "https://www.reddit.com/r/3Dprinting/comments/1ip98af/best_enclosed_fdm_3d_printer_to_start_with/";
    eprintln!("\n=== Reddit Thread ===");

    let reqwest_result = fetch(
        url,
        &FetchOptions {
            readability: true,
            ..Default::default()
        },
        None,
    )
    .await
    .expect("reqwest fetch");
    save(
        "reddit_thread",
        url,
        "reqwest",
        reqwest_result.word_count,
        &reqwest_result.text,
    );

    let c4ai_md = fetch_with_crawl4ai(&crawl4ai_url(), url)
        .await
        .expect("crawl4ai fetch");
    let c4ai_words = c4ai_md.split_whitespace().count();
    save("reddit_thread", url, "crawl4ai", c4ai_words, &c4ai_md);

    eprintln!(
        "  reqwest: {} words, crawl4ai: {} words",
        reqwest_result.word_count, c4ai_words
    );
}

/// Tom's Hardware review — typically works OK with readability, compare anyway.
#[tokio::test]
async fn compare_toms_hardware() {
    let url = "https://www.tomshardware.com/best-picks/best-3d-printers";
    eprintln!("\n=== Tom's Hardware ===");

    let reqwest_result = fetch(
        url,
        &FetchOptions {
            readability: true,
            ..Default::default()
        },
        None,
    )
    .await
    .expect("reqwest fetch");
    save(
        "toms_hardware",
        url,
        "reqwest",
        reqwest_result.word_count,
        &reqwest_result.text,
    );

    let c4ai_md = fetch_with_crawl4ai(&crawl4ai_url(), url)
        .await
        .expect("crawl4ai fetch");
    let c4ai_words = c4ai_md.split_whitespace().count();
    save("toms_hardware", url, "crawl4ai", c4ai_words, &c4ai_md);

    eprintln!(
        "  reqwest: {} words, crawl4ai: {} words",
        reqwest_result.word_count, c4ai_words
    );
}

/// PCMag — returns 403 via reqwest, crawl4ai should work.
#[tokio::test]
async fn compare_pcmag() {
    let url = "https://www.pcmag.com/picks/the-best-3d-printers";
    eprintln!("\n=== PCMag ===");

    let reqwest_result = fetch(
        url,
        &FetchOptions {
            readability: true,
            ..Default::default()
        },
        None,
    )
    .await;
    match &reqwest_result {
        Ok(c) => {
            save("pcmag", url, "reqwest", c.word_count, &c.text);
            eprintln!("  reqwest: {} words", c.word_count);
        }
        Err(e) => eprintln!("  reqwest: FAILED — {e}"),
    }

    let c4ai_md = fetch_with_crawl4ai(&crawl4ai_url(), url)
        .await
        .expect("crawl4ai should handle PCMag");
    let c4ai_words = c4ai_md.split_whitespace().count();
    save("pcmag", url, "crawl4ai", c4ai_words, &c4ai_md);
    eprintln!("  crawl4ai: {c4ai_words} words");

    assert!(
        c4ai_words > 500,
        "crawl4ai should extract substantial PCMag content (got {c4ai_words} words)"
    );
}

// ---------------------------------------------------------------------------
// Integration test: verify fetch() with crawl4ai_url upgrades bad extractions
// ---------------------------------------------------------------------------

/// Verify that fetch() with crawl4ai_url falls back automatically for All3DP.
#[tokio::test]
async fn fetch_with_fallback_all3dp() {
    let url = "https://all3dp.com/1/best-enclosed-3d-printers/";
    eprintln!("\n=== fetch() with fallback — All3DP ===");

    let result = fetch(
        url,
        &FetchOptions {
            readability: true,
            ..Default::default()
        },
        Some(&crawl4ai_url()),
    )
    .await
    .expect("fetch should succeed");

    save(
        "all3dp_enclosed_integrated",
        url,
        "fetch_with_fallback",
        result.word_count,
        &result.text,
    );

    eprintln!("  word count: {}", result.word_count);
    // With fallback, we should get substantial article content
    assert!(
        result.word_count > 200,
        "fetch with crawl4ai fallback should extract meaningful content (got {} words)",
        result.word_count
    );
}

/// Verify that fetch() with crawl4ai_url falls back for Reddit.
#[tokio::test]
async fn fetch_with_fallback_reddit() {
    let url = "https://www.reddit.com/r/3Dprinting/comments/1ip98af/best_enclosed_fdm_3d_printer_to_start_with/";
    eprintln!("\n=== fetch() with fallback — Reddit ===");

    let result = fetch(
        url,
        &FetchOptions {
            readability: true,
            ..Default::default()
        },
        Some(&crawl4ai_url()),
    )
    .await
    .expect("fetch should succeed");

    save(
        "reddit_thread_integrated",
        url,
        "fetch_with_fallback",
        result.word_count,
        &result.text,
    );

    eprintln!("  word count: {}", result.word_count);
    // Reddit reqwest gets ~59 words; with fallback should get actual discussion
    assert!(
        result.word_count > 200,
        "Reddit via fallback should have discussion content (got {} words)",
        result.word_count
    );
}

/// Verify that fetch() with crawl4ai_url falls back for PCMag (403).
#[tokio::test]
async fn fetch_with_fallback_pcmag() {
    let url = "https://www.pcmag.com/picks/the-best-3d-printers";
    eprintln!("\n=== fetch() with fallback — PCMag ===");

    let result = fetch(
        url,
        &FetchOptions {
            readability: true,
            ..Default::default()
        },
        Some(&crawl4ai_url()),
    )
    .await
    .expect("fetch with fallback should succeed for PCMag");

    save(
        "pcmag_integrated",
        url,
        "fetch_with_fallback",
        result.word_count,
        &result.text,
    );

    eprintln!("  word count: {}", result.word_count);
    assert!(
        result.word_count > 500,
        "PCMag via fallback should have substantial content (got {} words)",
        result.word_count
    );
}
