#![cfg(feature = "live-tests")]

use std::fs;

use ghost::web::{FetchOptions, fetch, fetch_with_crawl4ai};

const TEST_URL: &str = "https://all3dp.com/1/snapmaker-u1-reviewed-make-haste-not-waste/";

#[tokio::test]
async fn fetch_all3dp_article_saves_to_tmp() {
    let crawl4ai_url = std::env::var("CRAWL4AI_URL").ok();

    let options = FetchOptions {
        readability: true,
        ..Default::default()
    };

    let content = fetch(TEST_URL, &options, crawl4ai_url.as_deref())
        .await
        .expect("fetch should succeed");

    let out_dir = std::path::PathBuf::from("/tmp/ghost-web-fetch-test");
    fs::create_dir_all(&out_dir).expect("create output dir");

    let out_path = out_dir.join("all3dp_article.md");
    let mut output = String::new();
    output.push_str(&format!("# URL: {TEST_URL}\n"));
    output.push_str(&format!("# Word count: {}\n", content.word_count));
    output.push_str(&format!("# Truncated: {}\n", content.truncated));
    if let Some(title) = &content.title {
        output.push_str(&format!("# Title: {title}\n"));
    }
    output.push_str(&format!(
        "# crawl4ai_url: {}\n",
        crawl4ai_url.as_deref().unwrap_or("(none)")
    ));
    output.push_str("\n---\n\n");
    output.push_str(&content.text);

    fs::write(&out_path, &output).expect("write output file");
    eprintln!("Saved to: {}", out_path.display());
    eprintln!("Word count: {}", content.word_count);

    assert!(content.word_count > 0, "should have extracted some content");
}

/// PCMag returns 403 via plain reqwest — test that crawl4ai can fetch it.
///
/// Requires crawl4ai running locally. Set `CRAWL4AI_URL` env var (defaults
/// to `http://localhost:11235`).
///
/// ```sh
/// CRAWL4AI_URL=http://localhost:11235 cargo test --features live-tests fetch_pcmag -- --nocapture
/// ```
#[tokio::test]
async fn fetch_pcmag_via_crawl4ai() {
    let crawl4ai_url =
        std::env::var("CRAWL4AI_URL").unwrap_or_else(|_| "http://localhost:11235".to_string());

    let page_url = "https://www.pcmag.com/picks/the-best-3d-printers";

    let markdown = fetch_with_crawl4ai(&crawl4ai_url, page_url)
        .await
        .expect("crawl4ai should fetch PCMag successfully");

    let word_count = markdown.split_whitespace().count();

    let out_dir = std::path::PathBuf::from("/tmp/ghost-web-fetch-test");
    fs::create_dir_all(&out_dir).expect("create output dir");

    let out_path = out_dir.join("pcmag_3d_printers.md");
    let mut output = String::new();
    output.push_str(&format!("# URL: {page_url}\n"));
    output.push_str(&format!("# Word count: {word_count}\n"));
    output.push_str(&format!("# Fetched via: crawl4ai ({crawl4ai_url})\n"));
    output.push_str("\n---\n\n");
    output.push_str(&markdown);

    fs::write(&out_path, &output).expect("write output file");
    eprintln!("Saved to: {}", out_path.display());
    eprintln!("Word count: {word_count}");

    assert!(
        word_count > 100,
        "crawl4ai should extract substantial content (got {word_count} words)"
    );
}
