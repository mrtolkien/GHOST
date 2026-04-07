mod common;

use std::path::Path;

use ghost::db;
use tempfile::TempDir;

const MALFORMED_REFERENCE_FIXTURE: &str = "tests/fixtures/db/reference_topic_malformed.db";
const MALFORMED_QUERY: &str = "mute compulsion main topic";
const EXPECTED_REFERENCE_PATH_FRAGMENT: &str = "books/mute-compulsion";

async fn fixture_database() -> (db::GhostDb, TempDir, TempDir) {
    let (config, workspace, config_dir) = common::test_config();
    let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(MALFORMED_REFERENCE_FIXTURE);
    std::fs::copy(&fixture_path, workspace.path().join("ghost.db")).expect("copy fixture db");

    let db = db::connect(&config.workspace, config.embeddings.dimension)
        .await
        .expect("connect fixture db");

    (db, workspace, config_dir)
}

#[tokio::test]
async fn malformed_reference_fts_falls_back_to_plain_search() {
    let (db, _workspace, _config_dir) = fixture_database().await;

    let hits = db::knowledge::search_references(&db, MALFORMED_QUERY, 10, None)
        .await
        .expect("fallback search should succeed");

    assert!(
        !hits.is_empty(),
        "fallback should still return relevant reference hits"
    );
    assert!(
        hits.iter().any(|hit| {
            hit.path
                .as_deref()
                .is_some_and(|path| path.contains(EXPECTED_REFERENCE_PATH_FRAGMENT))
        }),
        "expected a result from the imported book, got: {hits:#?}"
    );
}
