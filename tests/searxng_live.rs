#![cfg(feature = "live-tests")]

use ghost::web::{SearxngSearchProvider, format_search_metadata};

fn searxng_url() -> String {
    std::env::var("SEARXNG_URL").unwrap_or_else(|_| "http://192.168.1.10:8888".to_string())
}

#[tokio::test]
async fn search_returns_results_with_metadata() {
    let provider = SearxngSearchProvider::new(&searxng_url(), 5).expect("build provider");
    let results = provider
        .search("rust programming language")
        .await
        .expect("search should succeed");

    assert!(!results.is_empty(), "should return at least one result");
    assert!(results.len() <= 5, "should respect max_results cap");

    for result in &results {
        assert!(!result.title.is_empty(), "every result needs a title");
        assert!(
            result.url.starts_with("http"),
            "URL should be absolute: {}",
            result.url
        );

        // SearXNG always populates engine metadata
        let engines = result.engines.as_ref().expect("engines should be Some");
        assert!(!engines.is_empty(), "at least one engine per result");

        assert!(
            result.score.is_some(),
            "score should be populated for SearXNG results"
        );
    }

    // Verify at least one result has a snippet
    assert!(
        results.iter().any(|r| r.snippet.is_some()),
        "at least one result should have a snippet"
    );
}

#[tokio::test]
async fn max_results_caps_output() {
    let provider = SearxngSearchProvider::new(&searxng_url(), 2).expect("build provider");
    let results = provider
        .search("linux kernel")
        .await
        .expect("search should succeed");

    assert!(
        results.len() <= 2,
        "should return at most 2 results, got {}",
        results.len()
    );
    assert!(!results.is_empty(), "should return at least one result");
}

#[tokio::test]
async fn metadata_formatting_matches_expected_shape() {
    let provider = SearxngSearchProvider::new(&searxng_url(), 3).expect("build provider");
    let results = provider
        .search("best 3d printers 2026")
        .await
        .expect("search should succeed");

    assert!(!results.is_empty());

    for result in &results {
        let meta = format_search_metadata(result);
        // Every SearXNG result has engines + score, so metadata should exist
        let meta = meta.expect("SearXNG results should have metadata");

        assert!(
            meta.contains("Sources:"),
            "metadata should contain Sources: — got: {meta}"
        );
        assert!(
            meta.contains("score:"),
            "metadata should contain score: — got: {meta}"
        );

        eprintln!("  {} — {}\n    {meta}", result.title, result.url);
    }
}

#[tokio::test]
async fn positions_pair_with_engines() {
    let provider = SearxngSearchProvider::new(&searxng_url(), 5).expect("build provider");
    let results = provider
        .search("surrealdb database")
        .await
        .expect("search should succeed");

    for result in &results {
        let engines = result.engines.as_ref().unwrap();
        if let Some(positions) = &result.positions {
            // When both are present, they should have matching lengths
            // (SearXNG pairs engine[i] with position[i])
            assert_eq!(
                engines.len(),
                positions.len(),
                "engines and positions should have same length for '{}'",
                result.title
            );
        }
    }
}

#[tokio::test]
async fn invalid_url_returns_error() {
    let provider = SearxngSearchProvider::new("http://127.0.0.1:1", 5).expect("build provider");
    let err = provider
        .search("test")
        .await
        .expect_err("should fail with unreachable URL");

    // Should be a connection/request error, not a panic
    let msg = err.to_string();
    assert!(
        msg.contains("request") || msg.contains("connect") || msg.contains("error"),
        "error should indicate connection failure: {msg}"
    );
}
